//! The UI thread's loop (brief M2 §5): the isolate's messages as they
//! arrive, and a frame at each vsync the framework asks for.
//!
//! A frame is the engine's sequence (`Animator::BeginFrame`, then
//! `RuntimeController::BeginFrame` and `Window::BeginFrame`): `_beginFrame`
//! with the vsync's time, the microtasks it queued, `_drawFrame`, and the
//! microtasks that queued. `ScheduleFrame` asks for one; it is served at the
//! first vsync tick from the request on, and never twice on one tick.
//!
//! Platform message replies dart:ui's natives queue are delivered between
//! steps, before anything else, with the microtasks they complete; any
//! still waiting when the loop returns are dropped. So are finished image
//! decodes, which arrive from the decode thread and wake the loop. The loop
//! does not end while a decode is outstanding.
//!
//! Vsync here is a fixed-interval clock: ticks `period` apart from the loop's
//! start. On KMS the flip event will take its place (stage 6).
//!
//! The VM is told when the loop expects to be idle, as `Animator` tells it:
//! when a frame is asked for, until the last frame's deadline; and when a
//! frame ends with none asked for, 51 ms on, if none has been asked for
//! since, for 100 ms. Not with less than a millisecond left, as
//! `RuntimeController::NotifyIdle`. The VM collects garbage then, and
//! empties its cache of heap pages. dart:ui's latency performance mode,
//! which skips them upstream, is not implemented here.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fl_dart_vm::{DartError, HandleScope, IsolateScope, MessageNotify};

/// `Animator`'s `kNotifyIdleTaskWaitTime`: a tad more than three frames at
/// 60 Hz with none asked for before the loop counts itself idle.
const IDLE_AFTER: Duration = Duration::from_millis(51);

/// How long an idle period after the last frame is reported to last.
const IDLE_FOR: Duration = Duration::from_millis(100);

/// `RuntimeController::NotifyIdle`: less than this before the deadline and
/// the VM is not told.
const IDLE_AT_LEAST: Duration = Duration::from_millis(1);

/// What the loop waits for: messages for the isolate, from any thread, and
/// frame requests. Cloning shares it.
#[derive(Clone, Default)]
pub struct Wake {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    pending: Mutex<Pending>,
    changed: Condvar,
}

#[derive(Default)]
struct Pending {
    /// Messages announced and not yet handled.
    messages: u64,
    /// When a frame was first asked for, if it has not been begun. Its tick
    /// is the first from then on: computed from now instead, every wake-up
    /// just after a tick would push the frame to the next one.
    frame: Option<Instant>,
    /// A reply arrived from another thread.
    replies: bool,
}

impl Wake {
    /// The isolate's [`fl_dart_vm::IsolateConfig::notify`].
    pub fn notify(&self) -> MessageNotify {
        let inner = Arc::clone(&self.inner);
        Arc::new(move || {
            inner.lock().messages += 1;
            inner.changed.notify_one();
        })
    }

    /// Wakes the loop for a reply queued on another thread: dart:ui's reply
    /// waker.
    pub fn reply_arrived(&self) {
        self.inner.lock().replies = true;
        self.inner.changed.notify_one();
    }

    /// A frame at the next vsync: what `ScheduleFrame` does.
    pub fn request_frame(&self) {
        let mut p = self.inner.lock();
        if p.frame.is_none() {
            p.frame = Some(Instant::now());
        }
        drop(p);
        self.inner.changed.notify_one();
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What a [`run`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The time each frame was begun with, in microseconds of the loop's
    /// clock: its vsync tick, not when the frame happened to start.
    pub frame_times_us: Vec<i64>,
}

/// Runs the isolate until `stop` says so, or until it can receive nothing
/// more and has no frame to draw. `stop` is asked between steps, so a
/// frame count it checks is never overshot by more than the step in hand.
///
/// Installs `wake` as dart:ui's frame scheduler for the process. The
/// isolate must have been made with [`crate::isolate_config`] for the same
/// `wake`, or its messages are never announced.
pub fn run(
    isolate: &mut IsolateScope<'_>,
    wake: &Wake,
    period: Duration,
    stop: impl FnMut() -> bool,
) -> Result<Report, DartError> {
    let report = run_steps(isolate, wake, period, stop);
    // Their isolate is not to be touched once this returns, whether the
    // loop ended or failed.
    fl_ui::discard_replies();
    report
}

/// Points fl-ui's frame requests and reply wakes at `wake`. Done when the
/// isolate is configured ([`crate::isolate_config`]), before `main` runs: a
/// frame `main` asks for before the loop starts is the loop's first, not
/// lost, as the framework, having asked, does not ask again until it is
/// begun.
pub(crate) fn connect(wake: &Wake) {
    let scheduler = wake.clone();
    fl_ui::set_frame_scheduler(move || scheduler.request_frame());
    let replies = wake.clone();
    fl_ui::set_reply_waker(move || replies.reply_arrived());
}

fn run_steps(
    isolate: &mut IsolateScope<'_>,
    wake: &Wake,
    period: Duration,
    mut stop: impl FnMut() -> bool,
) -> Result<Report, DartError> {
    connect(wake);
    let inner = &wake.inner;
    let clock = Clock::new(period);
    let mut last_tick = None;
    let mut report = Report::default();
    // `Animator`'s state for idle notifications: the last frame's deadline,
    // its tick plus a period, once one has been drawn; whether the frame
    // asked for has had its notification; and when the loop counts itself
    // idle, after a frame ended with none asked for.
    let mut deadline: Option<Instant> = None;
    let mut awaited = false;
    let mut idle_check: Option<Instant> = None;
    while !stop() {
        // Replies are queued by natives on this thread, during the step
        // before, so there is nothing to wait for: they go first.
        let step = if fl_ui::has_pending_replies() {
            Step::Replies
        } else {
            let mut p = inner.lock();
            loop {
                if std::mem::take(&mut p.replies) {
                    break Step::Replies;
                }
                if p.messages > 0 {
                    p.messages -= 1;
                    break Step::Message;
                }
                let Some(asked) = p.frame else {
                    break Step::Idle;
                };
                // `AwaitVSync`: once a frame has been drawn, asking for the
                // next tells the VM it is idle until the last one's deadline.
                if let (Some(d), false) = (deadline, awaited) {
                    awaited = true;
                    break Step::NotifyIdle(d);
                }
                let due = clock.tick_at_or_after(asked, last_tick);
                let now = Instant::now();
                if now >= due {
                    p.frame = None;
                    break Step::Frame(due);
                }
                // A message arriving meanwhile wakes this early.
                p = inner
                    .changed
                    .wait_timeout(p, due - now)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        };
        match step {
            Step::Message => isolate.with(|s| s.handle_message())?,
            Step::Replies => isolate.with(|s| {
                // SAFETY: inside `with`, this isolate is current with a scope
                // open, and it is the isolate whose natives queued them.
                unsafe { fl_ui::deliver_replies() }.map_err(DartError::Api)?;
                s.run_microtasks()
            })?,
            Step::Frame(tick) => {
                last_tick = Some(tick);
                let us = clock.micros(tick);
                report.frame_times_us.push(us);
                let n = report.frame_times_us.len() as i64;
                isolate.with(|s| frame(s, us, n))?;
                deadline = Some(tick + clock.period);
                awaited = false;
                // `EndFrame`: none asked for, so the loop may be idle soon.
                idle_check = inner
                    .lock()
                    .frame
                    .is_none()
                    .then(|| Instant::now() + IDLE_AFTER);
            }
            Step::NotifyIdle(until) => notify_idle(isolate, until),
            Step::Idle => {
                let live = isolate.with(|s| s.has_live_ports());
                let p = inner.lock();
                if p.messages > 0 || p.frame.is_some() || p.replies {
                    continue;
                }
                if !live && !fl_ui::awaiting_decodes() {
                    // Nothing can post to it, nothing is asked of it, and no
                    // decode is to answer.
                    break;
                }
                match idle_check {
                    Some(at) if Instant::now() >= at => {
                        // Still none asked for, and the last frame's
                        // deadline is past: idle for a while.
                        drop(p);
                        idle_check = None;
                        let now = Instant::now();
                        if deadline.is_none_or(|d| now > d) {
                            notify_idle(isolate, now + IDLE_FOR);
                        }
                    }
                    Some(at) => {
                        let wait = at.saturating_duration_since(Instant::now());
                        drop(
                            inner
                                .changed
                                .wait_timeout(p, wait)
                                .unwrap_or_else(|e| e.into_inner()),
                        );
                    }
                    None => drop(inner.changed.wait(p).unwrap_or_else(|e| e.into_inner())),
                }
            }
        }
    }
    Ok(report)
}

enum Step {
    Message,
    Replies,
    Frame(Instant),
    NotifyIdle(Instant),
    Idle,
}

/// Tells the VM the loop is idle until `until`, unless less than
/// [`IDLE_AT_LEAST`] is left.
fn notify_idle(isolate: &mut IsolateScope<'_>, until: Instant) {
    let left = until.saturating_duration_since(Instant::now());
    if left >= IDLE_AT_LEAST {
        isolate.with(|s| s.notify_idle(left));
    }
}

/// One frame, as the engine runs it.
fn frame(s: &mut HandleScope<'_>, time_us: i64, number: i64) -> Result<(), DartError> {
    let ui = s.lookup_library("dart:ui")?;
    let t = s.new_int(time_us);
    let n = s.new_int(number);
    s.invoke(ui, "_beginFrame", &[t, n])?;
    s.run_microtasks()?;
    s.invoke(ui, "_drawFrame", &[])?;
    s.run_microtasks()
}

/// Ticks `period` apart from its creation.
struct Clock {
    start: Instant,
    period: Duration,
}

impl Clock {
    fn new(period: Duration) -> Clock {
        Clock {
            start: Instant::now(),
            // A zero period would make every instant a tick, and a loop that
            // never waits. One microsecond is the clock's own resolution.
            period: period.max(Duration::from_micros(1)),
        }
    }

    /// The first tick at or after `t`, and after `last` if there was one.
    fn tick_at_or_after(&self, t: Instant, last: Option<Instant>) -> Instant {
        let since = t.saturating_duration_since(self.start).as_nanos();
        let p = self.period.as_nanos();
        let n = since.div_ceil(p);
        let mut tick = self.at(n);
        if let Some(last) = last {
            if tick <= last {
                tick = last + self.period;
            }
        }
        tick
    }

    fn at(&self, n: u128) -> Instant {
        let nanos = n.saturating_mul(self.period.as_nanos());
        self.start + Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }

    fn micros(&self, t: Instant) -> i64 {
        i64::try_from(t.saturating_duration_since(self.start).as_micros()).unwrap_or(i64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_on_the_grid_and_never_repeat() {
        let c = Clock::new(Duration::from_millis(10));
        let t0 = c.start;
        // Mid-period rounds up to the next tick.
        assert_eq!(
            c.tick_at_or_after(t0 + Duration::from_millis(13), None),
            t0 + Duration::from_millis(20)
        );
        // Exactly on a tick is that tick...
        assert_eq!(
            c.tick_at_or_after(t0 + Duration::from_millis(20), None),
            t0 + Duration::from_millis(20)
        );
        // ...unless that tick already had its frame.
        let last = Some(t0 + Duration::from_millis(20));
        assert_eq!(
            c.tick_at_or_after(t0 + Duration::from_millis(20), last),
            t0 + Duration::from_millis(30)
        );
        // A late request skips the ticks it missed rather than catching up.
        assert_eq!(
            c.tick_at_or_after(t0 + Duration::from_millis(55), last),
            t0 + Duration::from_millis(60)
        );
        assert_eq!(c.micros(t0 + Duration::from_millis(60)), 60_000);
    }

    #[test]
    fn a_zero_period_is_one_microsecond() {
        let c = Clock::new(Duration::ZERO);
        let t = c.tick_at_or_after(c.start, Some(c.start));
        assert_eq!(t - c.start, Duration::from_micros(1));
    }

    #[test]
    fn wake_counts_messages_and_one_frame_request() {
        let w = Wake::default();
        let n = w.notify();
        n();
        n();
        w.request_frame();
        w.request_frame();
        let p = w.inner.lock();
        assert_eq!(p.messages, 2);
        assert!(p.frame.is_some());
    }
}

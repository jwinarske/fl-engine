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
//! still waiting when the loop returns are dropped.
//!
//! Vsync here is a fixed-interval clock: ticks `period` apart from the loop's
//! start. On KMS the flip event will take its place (stage 6).

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fl_dart_vm::{DartError, HandleScope, IsolateScope, MessageNotify};

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
    mut stop: impl FnMut() -> bool,
) -> Result<Report, DartError> {
    let scheduler = wake.clone();
    fl_ui::set_frame_scheduler(move || scheduler.request_frame());
    let inner = &wake.inner;
    let clock = Clock::new(period);
    let mut last_tick = None;
    let mut report = Report::default();
    while !stop() {
        // Replies are queued by natives on this thread, during the step
        // before, so there is nothing to wait for: they go first.
        let step = if fl_ui::has_pending_replies() {
            Step::Replies
        } else {
            let mut p = inner.lock();
            loop {
                if p.messages > 0 {
                    p.messages -= 1;
                    break Step::Message;
                }
                let Some(asked) = p.frame else {
                    break Step::Idle;
                };
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
            }
            Step::Idle => {
                let live = isolate.with(|s| s.has_live_ports());
                let p = inner.lock();
                if p.messages > 0 || p.frame.is_some() {
                    continue;
                }
                if !live {
                    // Nothing can post to it and nothing is asked of it.
                    break;
                }
                drop(inner.changed.wait(p).unwrap_or_else(|e| e.into_inner()));
            }
        }
    }
    // Their isolate is not to be touched once this returns.
    fl_ui::discard_replies();
    Ok(report)
}

enum Step {
    Message,
    Replies,
    Frame(Instant),
    Idle,
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

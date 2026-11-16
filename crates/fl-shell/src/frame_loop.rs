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
//! Messages the engine sends the framework ([`Wake::send`], brief M4 §3) are
//! queued from any thread and dispatched one a step, each with the
//! microtasks it queued. A message's continuation is kept by its response
//! id and run when the framework responds, on the UI thread; one still
//! waiting when the loop returns is run with `None`, as is one sent after,
//! so a sender never waits forever.
//!
//! Pointer events ([`Wake::input`], brief M4 §2, §4) are queued from any
//! thread and dispatched in the packets they were delivered in, one for
//! each call of [`EventSink::pointer_events`], as the engine dispatches
//! one for each `FlutterEngineSendPointerEvent`: made `PointerData`, put
//! through the engine's converter, whose state is this thread's, and
//! given to `_dispatchPointerDataPacket`, then the microtasks that
//! queued. The framework resolves a gesture's arena in a microtask, so
//! where one packet ends and the next begins shows. Nothing asks for a
//! frame on their account: the framework does, if what they did shows.
//! Past [`INPUT_LIMIT`] queued, motion is dropped first.
//!
//! Key events (brief M4 §2, D2) come the same way, in their place among
//! the pointer events, and are never dropped. Each is sent as the Linux
//! embedder sends it: its `KeyData` on `flutter/keydata`, then its legacy
//! message on `flutter/keyevent`, each with the microtasks it queued. The
//! framework answers both; the key is handled if either answer says so
//! (`fl_keyboard_manager`), and counted ([`Wake::keys`]). An answer that
//! never comes is the loop's end, and not handled.
//!
//! A key the framework did not handle is offered to text input (brief M4
//! §2, `fl_view`'s redispatch): the input method first, then the handler's
//! own keys. What that tells the framework goes on `flutter/textinput`
//! ahead of the next key, so the framework sees each key's text before the
//! key after it.
//!
//! Vsync here is a fixed-interval clock: ticks `period` apart from when the
//! `Wake` was made, the frame clock's zero. On KMS the flip event will take its place (stage 6).
//!
//! The VM is told when the loop expects to be idle, as `Animator` tells it:
//! when a frame is asked for, until the last frame's deadline; and when a
//! frame ends with none asked for, 51 ms on, if none has been asked for
//! since, for 100 ms. Not with less than a millisecond left, as
//! `RuntimeController::NotifyIdle`. The VM collects garbage then, and
//! empties its cache of heap pages. dart:ui's latency performance mode,
//! which skips them upstream, is not implemented here.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use fl_dart_vm::{DartError, HandleScope, IsolateScope, MessageNotify};
use fl_input::xkb::XkbKey;
use fl_input::{EventSink, KeyEvent, PointerEvent};
use fl_ui::key::{KEY_DATA_CHANNEL, KEY_EVENT_CHANNEL};
use fl_ui::pointer::{Converter, PointerData};
use fl_ui::text_input::TEXT_INPUT_CHANNEL;

/// `Animator`'s `kNotifyIdleTaskWaitTime`: a tad more than three frames at
/// 60 Hz with none asked for before the loop counts itself idle.
const IDLE_AFTER: Duration = Duration::from_millis(51);

/// How long an idle period after the last frame is reported to last.
const IDLE_FOR: Duration = Duration::from_millis(100);

/// `RuntimeController::NotifyIdle`: less than this before the deadline and
/// the VM is not told.
const IDLE_AT_LEAST: Duration = Duration::from_millis(1);

/// One input event waiting for the loop.
enum Event {
    /// A pointer event, and the packet it was delivered in.
    Pointer(PointerEvent, u64),
    Key(KeyEvent),
}

impl Event {
    fn timestamp(&self) -> Duration {
        match self {
            Event::Pointer(e, _) => e.timestamp,
            Event::Key(e) => e.timestamp(),
        }
    }
}

/// Key events delivered, and what the framework said of them
/// ([`Wake::keys`]). One dispatched and not yet answered is in neither of
/// the last two.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyCounts {
    /// Delivered by a source.
    pub events: u64,
    /// The framework handled it, on either channel.
    pub handled: u64,
    /// Neither answer said so: the key was for whatever comes after the
    /// framework, text input first.
    pub unhandled: u64,
    /// Of the unhandled, those text input took: typed into a text field,
    /// or part of what is being composed.
    pub text: u64,
}

/// How many input events wait for the loop before motion is dropped
/// (brief M4 §15): a second of a 1,000 Hz mouse and ten fingers at 120 Hz,
/// far more than a loop that is running leaves waiting. Buttons, contacts
/// and signals are kept past it, so a pointer is never left down.
pub const INPUT_LIMIT: usize = 4096;

/// What the loop waits for: messages for the isolate, from any thread, and
/// frame requests. Cloning shares it.
#[derive(Clone, Default)]
pub struct Wake {
    inner: Arc<Inner>,
}

struct Inner {
    pending: Mutex<Pending>,
    changed: Condvar,
    /// Told when the loop goes idle, or ends: what [`EventSink::wait_idle`]
    /// waits on.
    idled: Condvar,
    /// The frame clock's zero.
    epoch: Instant,
    /// Continuations of dispatched messages, by response id.
    waiting: Mutex<Waiting>,
}

/// What a message's sender is answered through: the framework's response,
/// or `None` for a null response or no response at all.
type Respond = Box<dyn FnOnce(Option<Vec<u8>>) + Send>;

/// A message for the framework, not yet dispatched.
struct Outbound {
    channel: String,
    data: Option<Vec<u8>>,
    respond: Option<Respond>,
}

impl Default for Inner {
    fn default() -> Inner {
        Inner {
            pending: Mutex::default(),
            changed: Condvar::new(),
            idled: Condvar::new(),
            epoch: Instant::now(),
            waiting: Mutex::default(),
        }
    }
}

#[derive(Default)]
struct Waiting {
    by_id: HashMap<i64, Respond>,
    /// The last id given out. 0 is never one: it is a message that wants
    /// no response.
    last_id: i64,
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
    /// Messages for the framework, oldest first.
    outbound: VecDeque<Outbound>,
    /// The loop has returned: nothing more is dispatched.
    closed: bool,
    /// Pointer and key events, oldest first.
    input: VecDeque<Event>,
    /// Pointer packets delivered so far: the last one's number.
    packets: u64,
    /// Key events so far.
    keys: KeyCounts,
    /// Motion dropped with the queue at its limit.
    input_dropped: u64,
    /// [`Input`]s alive: sources that may still deliver.
    sources: usize,
    /// The loop is waiting with nothing to do. Cleared by whatever gives
    /// it something.
    idle: bool,
}

/// A source's way in to the loop ([`Wake::input`]). While one is alive the
/// loop does not end for want of something to do: the source may yet
/// deliver. Cloning shares it.
pub struct Input {
    inner: Arc<Inner>,
}

impl Clone for Input {
    fn clone(&self) -> Input {
        self.inner.lock().sources += 1;
        Input {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        let mut p = self.inner.lock();
        p.sources -= 1;
        drop(p);
        // The loop may have been waiting on this source alone.
        self.inner.changed.notify_one();
    }
}

impl EventSink for Input {
    fn now(&self) -> Duration {
        self.inner.epoch.elapsed()
    }

    fn pointer_events(&self, events: &[PointerEvent]) {
        let mut p = self.inner.lock();
        if p.closed {
            return;
        }
        p.packets += 1;
        let packet = p.packets;
        for e in events {
            if p.input.len() >= INPUT_LIMIT && e.is_motion() {
                p.input_dropped += 1;
            } else {
                p.input.push_back(Event::Pointer(*e, packet));
            }
        }
        p.idle = false;
        drop(p);
        self.inner.changed.notify_one();
    }

    fn key_event(&self, event: KeyEvent) {
        let mut p = self.inner.lock();
        if p.closed {
            return;
        }
        p.keys.events += 1;
        p.input.push_back(Event::Key(event));
        p.idle = false;
        drop(p);
        self.inner.changed.notify_one();
    }

    fn wait_idle(&self) -> bool {
        let mut p = self.inner.lock();
        while !p.idle && !p.closed {
            p = self.inner.idled.wait(p).unwrap_or_else(|e| e.into_inner());
        }
        !p.closed
    }

    fn is_closed(&self) -> bool {
        self.inner.lock().closed
    }
}

impl Wake {
    /// The isolate's [`fl_dart_vm::IsolateConfig::notify`].
    pub fn notify(&self) -> MessageNotify {
        let inner = Arc::clone(&self.inner);
        Arc::new(move || {
            let mut p = inner.lock();
            p.messages += 1;
            p.idle = false;
            drop(p);
            inner.changed.notify_one();
        })
    }

    /// A way in for an input source: what it delivers events through, from
    /// any thread, and what keeps the loop running while it may.
    pub fn input(&self) -> Input {
        self.inner.lock().sources += 1;
        Input {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Pointer motion dropped so far, with more than [`INPUT_LIMIT`] events
    /// waiting.
    pub fn input_dropped(&self) -> u64 {
        self.inner.lock().input_dropped
    }

    /// Key events delivered so far, and how the framework answered them.
    pub fn keys(&self) -> KeyCounts {
        self.inner.lock().keys
    }

    /// Wakes the loop for a reply queued on another thread: dart:ui's reply
    /// waker.
    pub fn reply_arrived(&self) {
        let mut p = self.inner.lock();
        p.replies = true;
        p.idle = false;
        drop(p);
        self.inner.changed.notify_one();
    }

    /// Sends the framework a message on `channel`, from any thread: the
    /// engine's `DispatchPlatformMessage`. `data` of `None` arrives as a
    /// null `ByteData`. `respond` is run once, with the framework's
    /// response (`None` for a null one), on the UI thread inside the
    /// framework's call, so it must not block; or with `None` if the loop
    /// returns first or already has, on the thread that finds that out.
    ///
    /// A channel with no listener keeps its messages until one is set
    /// (`ChannelBuffers`): one message by default, an older one answered
    /// with null as a newer one arrives.
    pub fn send(
        &self,
        channel: &str,
        data: Option<Vec<u8>>,
        respond: impl FnOnce(Option<Vec<u8>>) + Send + 'static,
    ) {
        self.push(channel, data, Some(Box::new(respond)));
    }

    /// [`Wake::send`] for a message whose response nobody wants.
    pub fn post(&self, channel: &str, data: Option<Vec<u8>>) {
        self.push(channel, data, None);
    }

    fn push(&self, channel: &str, data: Option<Vec<u8>>, respond: Option<Respond>) {
        let mut p = self.inner.lock();
        if p.closed {
            drop(p);
            if let Some(respond) = respond {
                respond(None);
            }
            return;
        }
        p.outbound.push_back(Outbound {
            channel: channel.to_owned(),
            data,
            respond,
        });
        p.idle = false;
        drop(p);
        self.inner.changed.notify_one();
    }

    /// The framework's response to the message dispatched with `id`. An id
    /// not waiting is ignored: unknown, or answered already.
    fn respond(&self, id: i64, data: Option<Vec<u8>>) {
        let respond = self.inner.waiting().by_id.remove(&id);
        if let Some(respond) = respond {
            respond(data);
        }
    }

    /// A frame at the next vsync: what `ScheduleFrame` does.
    pub fn request_frame(&self) {
        let mut p = self.inner.lock();
        if p.frame.is_none() {
            p.frame = Some(Instant::now());
        }
        p.idle = false;
        drop(p);
        self.inner.changed.notify_one();
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn waiting(&self) -> MutexGuard<'_, Waiting> {
        self.waiting.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keeps `respond` until the framework answers, and gives the id to
    /// dispatch its message with.
    fn await_response(&self, respond: Respond) -> i64 {
        let mut w = self.waiting();
        w.last_id += 1;
        let id = w.last_id;
        w.by_id.insert(id, respond);
        id
    }

    /// Ends dispatching: every message not yet answered, dispatched or
    /// not, is answered with `None`, oldest first, and so is any sent
    /// from now on.
    fn close(&self) {
        let queued = {
            let mut p = self.lock();
            p.closed = true;
            p.input.clear();
            std::mem::take(&mut p.outbound)
        };
        // A source waiting for the loop to go idle waits no longer.
        self.idled.notify_all();
        let mut dispatched: Vec<(i64, Respond)> = self.waiting().by_id.drain().collect();
        dispatched.sort_unstable_by_key(|(id, _)| *id);
        let dispatched = dispatched.into_iter().map(|(_, r)| r);
        for respond in dispatched.chain(queued.into_iter().filter_map(|m| m.respond)) {
            respond(None);
        }
    }
}

impl Drop for Inner {
    /// A `Wake` no loop ever ran for still answers what it was sent.
    fn drop(&mut self) {
        self.close();
    }
}

/// When the frame in hand was begun, before `_beginFrame`.
static FRAME_BEGUN: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// When the frame being built was begun: from this to the scene's `render`
/// is the frame's build time, as the engine's `FrameTiming` counts it,
/// `buildStart` to `buildFinish`. A frame sink reads it as the scene
/// arrives, before anything it does with the scene waits. `None` before
/// the first frame.
pub fn frame_begun() -> Option<Instant> {
    *FRAME_BEGUN.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a [`run`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The time each frame was begun with, in microseconds of the loop's
    /// clock: its vsync tick, not when the frame happened to start.
    pub frame_times_us: Vec<i64>,
    /// For each frame that input asked for, the time from the oldest event
    /// it shows to the end of its `_drawFrame`, the scene rendered, in
    /// microseconds: input latency on the UI thread (brief M4 §15).
    pub input_latency_us: Vec<u64>,
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
    fl_ui::set_response_sink(|_, _| {});
    wake.inner.close();
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
    let responses = wake.clone();
    fl_ui::set_response_sink(move |id, data| responses.respond(id, data));
    // A new isolate has been told of no key, and has no text field.
    fl_ui::key::reset_keyboard_state();
    fl_ui::text_input::reset();
}

fn run_steps(
    isolate: &mut IsolateScope<'_>,
    wake: &Wake,
    period: Duration,
    mut stop: impl FnMut() -> bool,
) -> Result<Report, DartError> {
    connect(wake);
    let inner = &wake.inner;
    let clock = Clock::new(inner.epoch, period);
    // The engine's `PointerDataPacketConverter`: which pointers there are,
    // where, and whether down. This thread's.
    let mut pointers = Converter::new();
    // The oldest event dispatched that a frame was asked for after and
    // has not been drawn since: what the next frame's latency is from.
    let mut undrawn_input: Option<Duration> = None;
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
                if let Some(m) = p.outbound.pop_front() {
                    break Step::Dispatch(m);
                }
                if !p.input.is_empty() {
                    break Step::Input(p.input.drain(..).collect());
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
            Step::Dispatch(m) => {
                let id = m.respond.map_or(0, |r| inner.await_response(r));
                isolate.with(|s| dispatch(s, &m.channel, m.data.as_deref(), id))?;
            }
            Step::Input(events) => {
                let oldest = events.iter().map(Event::timestamp).min();
                // Pointer events go in the packets they came in.
                let mut run = Vec::new();
                let mut run_packet = 0;
                let mut dispatched = false;
                for event in events {
                    match event {
                        Event::Pointer(e, packet) => {
                            if packet != run_packet {
                                dispatched |=
                                    dispatch_pointer_run(isolate, &mut pointers, &mut run)?;
                                run_packet = packet;
                            }
                            run.push(e);
                        }
                        Event::Key(e) => {
                            dispatch_pointer_run(isolate, &mut pointers, &mut run)?;
                            dispatch_key(isolate, inner, &e)?;
                            // What text input made of it, before the
                            // next key.
                            dispatch_outbound(isolate, inner)?;
                            dispatched = true;
                        }
                    }
                }
                dispatched |= dispatch_pointer_run(isolate, &mut pointers, &mut run)?;
                if dispatched && undrawn_input.is_none() && inner.lock().frame.is_some() {
                    undrawn_input = oldest;
                }
            }
            Step::Frame(tick) => {
                last_tick = Some(tick);
                let us = clock.micros(tick);
                report.frame_times_us.push(us);
                let n = report.frame_times_us.len() as i64;
                isolate.with(|s| frame(s, us, n))?;
                if let Some(since) = undrawn_input.take() {
                    let latency = inner.epoch.elapsed().saturating_sub(since);
                    report
                        .input_latency_us
                        .push(u64::try_from(latency.as_micros()).unwrap_or(u64::MAX));
                }
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
                let decoding = fl_ui::awaiting_decodes();
                let mut p = inner.lock();
                if p.messages > 0
                    || p.frame.is_some()
                    || p.replies
                    || !p.outbound.is_empty()
                    || !p.input.is_empty()
                {
                    continue;
                }
                if !live && !decoding && p.sources == 0 {
                    // Nothing can post to it, nothing is asked of it, no
                    // decode is to answer and no source is to deliver.
                    break;
                }
                if !decoding && !p.idle {
                    p.idle = true;
                    inner.idled.notify_all();
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
    Dispatch(Outbound),
    Input(Vec<Event>),
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

/// One message to the framework, as `PlatformConfiguration::
/// DispatchPlatformMessage` sends it, and the microtasks it queued.
fn dispatch(
    s: &mut HandleScope<'_>,
    channel: &str,
    data: Option<&[u8]>,
    response_id: i64,
) -> Result<(), DartError> {
    let ui = s.lookup_library("dart:ui")?;
    let name = s.new_string(channel)?;
    let data = match data {
        Some(d) => s.new_byte_data(d)?,
        None => s.null(),
    };
    let id = s.new_int(response_id);
    s.invoke(ui, "_dispatchPlatformMessage", &[name, data, id])?;
    s.run_microtasks()
}

/// Every message waiting for the framework, now, oldest first: what a
/// step would have dispatched one at a time before any more input.
fn dispatch_outbound(isolate: &mut IsolateScope<'_>, inner: &Arc<Inner>) -> Result<(), DartError> {
    loop {
        let Some(m) = inner.lock().outbound.pop_front() else {
            return Ok(());
        };
        let id = m.respond.map_or(0, |r| inner.await_response(r));
        isolate.with(|s| dispatch(s, &m.channel, m.data.as_deref(), id))?;
    }
}

/// The pointer events of `run`, emptied, as one packet: made `PointerData`,
/// put through the converter, and dispatched if anything is left. Whether
/// anything was.
fn dispatch_pointer_run(
    isolate: &mut IsolateScope<'_>,
    pointers: &mut Converter,
    run: &mut Vec<PointerEvent>,
) -> Result<bool, DartError> {
    if run.is_empty() {
        return Ok(false);
    }
    let packet: Vec<PointerData> = run.drain(..).map(|e| PointerData::from_event(&e)).collect();
    let mut records = Vec::with_capacity(packet.len() + 2);
    pointers.convert(&packet, |v| v == crate::IMPLICIT_VIEW_ID, &mut records);
    if records.is_empty() {
        return Ok(false);
    }
    let bytes = fl_ui::pointer::encode_packet(&records);
    isolate.with(|s| dispatch_pointers(s, &bytes))?;
    Ok(true)
}

/// What the framework says of one key event: handled if either of its
/// answers says so, counted once both are in, and offered to text input
/// if neither did.
struct KeyAnswer {
    inner: Weak<Inner>,
    /// Answers still to come, and whether one has said handled.
    state: Mutex<(u8, bool)>,
    /// The key, for text input.
    native: Option<XkbKey>,
}

impl KeyAnswer {
    fn new(inner: &Arc<Inner>, awaited: u8, native: Option<XkbKey>) -> Arc<KeyAnswer> {
        Arc::new(KeyAnswer {
            inner: Arc::downgrade(inner),
            state: Mutex::new((awaited, false)),
            native,
        })
    }

    /// One of the answers.
    fn answer(&self, handled: bool) {
        let done = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.0 = state.0.saturating_sub(1);
            state.1 |= handled;
            (state.0 == 0).then_some(state.1)
        };
        let (Some(handled), Some(inner)) = (done, self.inner.upgrade()) else {
            return;
        };
        if handled {
            inner.lock().keys.handled += 1;
            return;
        }
        // The loop has ended, and this is its unanswered keys being let
        // go: there is no framework to type into.
        {
            let mut p = inner.lock();
            if p.closed {
                p.keys.unhandled += 1;
                return;
            }
        }
        // `fl_view`'s redispatch: text input is offered the key. This is
        // the UI thread, inside the framework's response, as the handler's
        // state wants; its messages wait their turn like any other.
        let (taken, messages) = match &self.native {
            Some(key) => fl_ui::text_input::filter_key(key),
            None => (false, Vec::new()),
        };
        let mut p = inner.lock();
        p.keys.unhandled += 1;
        p.keys.text += u64::from(taken);
        if p.closed || messages.is_empty() {
            return;
        }
        for data in messages {
            p.outbound.push_back(Outbound {
                channel: TEXT_INPUT_CHANNEL.to_owned(),
                data: Some(data),
                respond: None,
            });
        }
        p.idle = false;
        drop(p);
        inner.changed.notify_one();
    }
}

/// One key event to the framework, as the Linux embedder sends it
/// (`fl_keyboard_manager_handle_event`): each `KeyData` on
/// `flutter/keydata`, then the legacy message on `flutter/keyevent`. The
/// answers awaited are those to the `KeyData` that is neither synthesized
/// nor empty and to the legacy message; with no such `KeyData` the event
/// is handled as it stands, as the embedder's responder says of a release
/// with no press.
fn dispatch_key(
    isolate: &mut IsolateScope<'_>,
    inner: &Arc<Inner>,
    event: &KeyEvent,
) -> Result<(), DartError> {
    let answered = event.answered();
    let answer = match answered {
        Some(_) => Some(KeyAnswer::new(
            inner,
            1 + u8::from(event.raw.is_some()),
            event.native,
        )),
        None => {
            inner.lock().keys.handled += 1;
            None
        }
    };
    for (i, data) in event.data.iter().enumerate() {
        let id = match (&answer, answered == Some(i)) {
            (Some(answer), true) => {
                let answer = Arc::clone(answer);
                inner.await_response(Box::new(move |r| {
                    answer.answer(fl_ui::key::key_data_handled(r.as_deref()));
                }))
            }
            _ => 0,
        };
        let packet = fl_ui::key::encode_packet(data);
        fl_ui::key::note_dispatched(data);
        isolate.with(|s| dispatch(s, KEY_DATA_CHANNEL, Some(&packet), id))?;
    }
    if let Some(raw) = &event.raw {
        let id = answer.map_or(0, |answer| {
            inner.await_response(Box::new(move |r| {
                answer.answer(fl_ui::key::key_event_handled(r.as_deref()));
            }))
        });
        isolate.with(|s| dispatch(s, KEY_EVENT_CHANNEL, Some(raw), id))?;
    }
    Ok(())
}

/// One pointer packet to the framework, as `PlatformConfiguration::
/// DispatchPointerDataPacket` sends it, and the microtasks it queued.
fn dispatch_pointers(s: &mut HandleScope<'_>, packet: &[u8]) -> Result<(), DartError> {
    let ui = s.lookup_library("dart:ui")?;
    let data = s.new_byte_data(packet)?;
    s.invoke(ui, "_dispatchPointerDataPacket", &[data])?;
    s.run_microtasks()
}

/// One frame, as the engine runs it.
fn frame(s: &mut HandleScope<'_>, time_us: i64, number: i64) -> Result<(), DartError> {
    *FRAME_BEGUN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    let ui = s.lookup_library("dart:ui")?;
    let t = s.new_int(time_us);
    let n = s.new_int(number);
    s.invoke(ui, "_beginFrame", &[t, n])?;
    s.run_microtasks()?;
    s.invoke(ui, "_drawFrame", &[])?;
    s.run_microtasks()
}

/// Ticks `period` apart from `start`.
struct Clock {
    start: Instant,
    period: Duration,
}

impl Clock {
    fn new(start: Instant, period: Duration) -> Clock {
        Clock {
            start,
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
        let c = Clock::new(Instant::now(), Duration::from_millis(10));
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
        let c = Clock::new(Instant::now(), Duration::ZERO);
        let t = c.tick_at_or_after(c.start, Some(c.start));
        assert_eq!(t - c.start, Duration::from_micros(1));
    }

    /// What each continuation was run with, by name, in order.
    type Log = Arc<Mutex<Vec<(&'static str, Option<Vec<u8>>)>>>;

    /// A continuation that records what it was run with, under `name`.
    fn record(log: &Log, name: &'static str) -> impl FnOnce(Option<Vec<u8>>) + Send + 'static {
        let log = Arc::clone(log);
        move |data| log.lock().unwrap().push((name, data))
    }

    #[test]
    fn a_response_runs_its_message_s_continuation_once() {
        let w = Wake::default();
        let log = Log::default();
        let a = w.inner.await_response(Box::new(record(&log, "a")));
        let b = w.inner.await_response(Box::new(record(&log, "b")));
        assert!(a != 0 && b != 0 && a != b);
        w.respond(b, Some(vec![1]));
        // Answered already, never given out, and the id of no response.
        w.respond(b, Some(vec![2]));
        w.respond(b + 1, None);
        w.respond(0, None);
        w.respond(a, None);
        assert_eq!(*log.lock().unwrap(), [("b", Some(vec![1])), ("a", None)]);
        assert!(w.inner.waiting().by_id.is_empty());
    }

    #[test]
    fn closing_answers_every_message_with_none() {
        let w = Wake::default();
        let log = Log::default();
        // Dispatched and waiting, then two still queued, one of them posted.
        let _ = w.inner.await_response(Box::new(record(&log, "dispatched")));
        w.send("c", Some(vec![1]), record(&log, "queued"));
        w.post("c", None);
        assert_eq!(w.inner.lock().outbound.len(), 2);
        w.inner.close();
        w.send("c", None, record(&log, "late"));
        assert_eq!(
            *log.lock().unwrap(),
            [("dispatched", None), ("queued", None), ("late", None)]
        );
        assert!(w.inner.lock().outbound.is_empty());
    }

    fn motion(x: f64) -> PointerEvent {
        use fl_input::{PointerDeviceKind, PointerPhase};
        PointerEvent::new(
            PointerPhase::Move,
            PointerDeviceKind::Touch,
            Duration::ZERO,
            x,
            0.0,
        )
    }

    fn pointer(e: &Event) -> Option<&PointerEvent> {
        match e {
            Event::Pointer(e, _) => Some(e),
            Event::Key(_) => None,
        }
    }

    fn key_down() -> KeyEvent {
        use fl_input::KeyData;
        KeyEvent {
            data: vec![KeyData {
                physical: 0x0007_0004,
                logical: 0x61,
                ..KeyData::default()
            }],
            raw: Some(b"{}".to_vec()),
            native: None,
        }
    }

    #[test]
    fn keys_are_queued_in_their_place_and_never_dropped() {
        let w = Wake::default();
        let input = w.input();
        let moves: Vec<PointerEvent> = (0..INPUT_LIMIT).map(|i| motion(i as f64)).collect();
        input.pointer_events(&moves);
        input.key_event(key_down());
        input.pointer_events(&[motion(1.0)]);
        input.key_event(key_down());
        let p = w.inner.lock();
        // The limit is passed by the keys alone; the move after it is gone.
        assert_eq!(p.input.len(), INPUT_LIMIT + 2);
        assert_eq!(p.input_dropped, 1);
        assert!(matches!(p.input.back(), Some(Event::Key(_))));
        assert_eq!(p.keys.events, 2);
    }

    /// `fl_keyboard_manager_test.cc`'s eight cases of who answered
    /// (`EngineHandledChannelNotHandledSync` and the rest): handled if
    /// either says so, in either order, and only once both have.
    #[test]
    fn a_key_is_handled_if_either_answer_says_so() {
        let w = Wake::default();
        for (engine, channel, engine_first) in [
            (true, false, true),
            (false, true, true),
            (true, true, true),
            (false, false, true),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, false, false),
        ] {
            let before = w.keys();
            let answer = KeyAnswer::new(&w.inner, 2, None);
            let (first, second) = if engine_first {
                (engine, channel)
            } else {
                (channel, engine)
            };
            answer.answer(first);
            assert_eq!(w.keys(), before, "one answer is not enough");
            answer.answer(second);
            let after = w.keys();
            let handled = engine || channel;
            assert_eq!(after.handled - before.handled, u64::from(handled));
            assert_eq!(after.unhandled - before.unhandled, u64::from(!handled));
        }
        // `EngineNoResponseChannelHandled`: with no answer from one, the
        // key is still waiting.
        let before = w.keys();
        let answer = KeyAnswer::new(&w.inner, 2, None);
        answer.answer(true);
        assert_eq!(w.keys(), before);
        // An answer after the wake is gone is counted nowhere, and harms
        // nothing.
        drop(w);
        answer.answer(false);
    }

    #[test]
    fn past_its_limit_the_input_queue_drops_motion_and_nothing_else() {
        use fl_input::PointerPhase;
        let w = Wake::default();
        let input = w.input();
        let moves: Vec<PointerEvent> = (0..INPUT_LIMIT + 10).map(|i| motion(i as f64)).collect();
        input.pointer_events(&moves);
        assert_eq!(w.inner.lock().input.len(), INPUT_LIMIT);
        assert_eq!(w.input_dropped(), 10);
        // The oldest are kept: the converter moves a pointer to its next
        // event, so what is lost is the way there.
        assert_eq!(
            w.inner.lock().input.back().and_then(pointer).map(|e| e.x),
            Some((INPUT_LIMIT - 1) as f64)
        );
        let up = PointerEvent {
            phase: PointerPhase::Up,
            ..motion(9.0)
        };
        input.pointer_events(&[motion(1.0), up, motion(2.0)]);
        assert_eq!(w.inner.lock().input.len(), INPUT_LIMIT + 1);
        assert_eq!(w.input_dropped(), 12);
        assert_eq!(
            w.inner
                .lock()
                .input
                .back()
                .and_then(pointer)
                .map(|e| e.phase),
            Some(PointerPhase::Up)
        );
    }

    /// A key the framework did not handle is offered to text input, and
    /// what it typed waits for the framework like any message (`fl_view`'s
    /// redispatch). The one test here that gives the handler a client: it
    /// is the process's.
    #[test]
    fn an_unhandled_key_is_offered_to_text_input() {
        let typed = |c: char| XkbKey {
            pressed: true,
            keyval: c as u32,
            unicode: c as u32,
            ..XkbKey::default()
        };
        let w = Wake::default();
        fl_ui::text_input::set_input_method(Box::new(fl_input::text::Direct));

        // No text field has the keyboard: unhandled, and that is all.
        KeyAnswer::new(&w.inner, 1, Some(typed('a'))).answer(false);
        assert_eq!((w.keys().unhandled, w.keys().text), (1, 0));
        assert!(w.inner.lock().outbound.is_empty());

        let call = br#"{"method":"TextInput.setClient","args":[5,{}]}"#;
        assert!(fl_ui::text_input::handle_call(Some(call)).is_some());
        // Handled by the framework: not text input's.
        KeyAnswer::new(&w.inner, 1, Some(typed('a'))).answer(true);
        assert_eq!((w.keys().handled, w.keys().text), (1, 0));
        // An event that is no key's, and a release.
        KeyAnswer::new(&w.inner, 1, None).answer(false);
        let release = XkbKey {
            pressed: false,
            ..typed('a')
        };
        KeyAnswer::new(&w.inner, 1, Some(release)).answer(false);
        assert_eq!((w.keys().unhandled, w.keys().text), (3, 0));
        assert!(w.inner.lock().outbound.is_empty());

        // Both answers in, neither handled: typed.
        let answer = KeyAnswer::new(&w.inner, 2, Some(typed('b')));
        answer.answer(false);
        assert!(w.inner.lock().outbound.is_empty(), "one answer to come");
        answer.answer(false);
        assert_eq!((w.keys().unhandled, w.keys().text), (4, 1));
        {
            let p = w.inner.lock();
            assert!(!p.idle);
            let sent: Vec<(&str, String)> = p
                .outbound
                .iter()
                .map(|m| {
                    let data = m.data.as_deref().unwrap_or_default();
                    (
                        m.channel.as_str(),
                        String::from_utf8_lossy(data).into_owned(),
                    )
                })
                .collect();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].0, "flutter/textinput");
            assert!(
                sent[0].1.starts_with(
                    r#"{"method":"TextInputClient.updateEditingState","args":[5,{"text":"b","#
                ),
                "{sent:?}"
            );
            assert!(p.outbound[0].respond.is_none());
        }

        // The loop ended: counted, and nothing queued for no one.
        w.inner.close();
        KeyAnswer::new(&w.inner, 1, Some(typed('c'))).answer(false);
        assert_eq!((w.keys().unhandled, w.keys().text), (5, 1));
        assert!(w.inner.lock().outbound.is_empty());
        fl_ui::text_input::reset();
    }

    /// Each delivery is a packet, as each `FlutterEngineSendPointerEvent`
    /// is: the queue keeps which events came together.
    #[test]
    fn pointer_events_keep_the_packets_they_came_in() {
        let w = Wake::default();
        let input = w.input();
        input.pointer_events(&[motion(1.0), motion(2.0)]);
        input.pointer_events(&[motion(3.0)]);
        input.key_event(key_down());
        input.pointer_events(&[]);
        input.pointer_events(&[motion(4.0)]);
        let p = w.inner.lock();
        let packets: Vec<Option<(f64, u64)>> = p
            .input
            .iter()
            .map(|e| match e {
                Event::Pointer(e, packet) => Some((e.x, *packet)),
                Event::Key(_) => None,
            })
            .collect();
        assert_eq!(
            packets,
            [
                Some((1.0, 1)),
                Some((2.0, 1)),
                Some((3.0, 2)),
                None,
                Some((4.0, 4))
            ]
        );
    }

    #[test]
    fn a_source_is_counted_while_it_lives_and_told_when_the_loop_ends() {
        let w = Wake::default();
        let a = w.input();
        let b = a.clone();
        assert_eq!(w.inner.lock().sources, 2);
        drop(a);
        assert_eq!(w.inner.lock().sources, 1);
        assert!(!b.is_closed());
        // Never idle, but ended: the wait is over, and says which.
        let waiter = std::thread::spawn({
            let b = b.clone();
            move || b.wait_idle()
        });
        w.inner.close();
        assert!(!waiter.join().unwrap_or(true));
        assert!(b.is_closed());
        b.pointer_events(&[motion(1.0)]);
        assert!(w.inner.lock().input.is_empty());
        drop(b);
        assert_eq!(w.inner.lock().sources, 0);
    }

    #[test]
    fn idle_ends_with_anything_to_do() {
        let w = Wake::default();
        let input = w.input();
        let idle = |w: &Wake| w.inner.lock().idle;
        let set = |w: &Wake| w.inner.lock().idle = true;
        set(&w);
        assert!(input.wait_idle());
        input.pointer_events(&[motion(1.0)]);
        assert!(!idle(&w));
        set(&w);
        w.request_frame();
        assert!(!idle(&w));
        set(&w);
        w.post("c", None);
        assert!(!idle(&w));
        set(&w);
        w.reply_arrived();
        assert!(!idle(&w));
        set(&w);
        (w.notify())();
        assert!(!idle(&w));
    }

    #[test]
    fn a_wake_never_run_answers_when_dropped() {
        let log = Log::default();
        let w = Wake::default();
        w.clone().send("c", None, record(&log, "queued"));
        assert!(log.lock().unwrap().is_empty());
        drop(w);
        assert_eq!(*log.lock().unwrap(), [("queued", None)]);
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

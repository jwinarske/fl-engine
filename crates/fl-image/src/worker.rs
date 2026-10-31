//! The decode thread (decision D5): one worker, as the engine's IO runner.
//!
//! A [`Job`] carries encoded bytes and an id; its [`Done`] goes to the sink
//! the worker was made with, on the worker's thread. Nothing here knows the
//! VM: the caller keeps whatever the id stands for (a callback, a persistent
//! handle) and resolves it when the result reaches its own thread.
//!
//! Jobs carry a tag, the isolate generation they belong to. [`Worker::retire`]
//! drops every job of a generation not yet started, and the caller drops
//! results it receives for one: so a result is never delivered to an isolate
//! that is gone (brief M3 §15).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::{decode, from_raw, Frame, Frames, ImageError, Limits, Raw};

/// What a job's bytes are.
#[derive(Debug, Clone)]
pub enum Source {
    /// An encoded image, its format from its first bytes.
    Encoded,
    /// Pixels, as [`Raw`] lays them out.
    Raw(Raw),
    /// An animation's next frame. The job's bytes, target and limits are
    /// not used: the frames hold their own, and frames are not resized.
    Next(Arc<Mutex<Frames>>),
}

/// One decode.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: u64,
    /// The isolate generation the job is for.
    pub tag: u64,
    pub bytes: Arc<[u8]>,
    pub source: Source,
    pub target: Option<(u32, u32)>,
    pub limits: Limits,
}

/// A finished job.
#[derive(Debug)]
pub struct Done {
    pub id: u64,
    pub tag: u64,
    pub result: Result<Frame, ImageError>,
}

/// The decode thread. Dropping it lets queued jobs finish and joins it.
pub struct Worker {
    tx: Option<Sender<Job>>,
    thread: Option<JoinHandle<()>>,
    /// Jobs tagged below this are dropped unstarted.
    live_from: Arc<AtomicU64>,
}

impl Worker {
    /// Starts the thread; each finished job goes to `sink`, there.
    pub fn spawn(sink: impl Fn(Done) + Send + 'static) -> std::io::Result<Worker> {
        let (tx, rx) = mpsc::channel::<Job>();
        let live_from = Arc::new(AtomicU64::new(0));
        let live = Arc::clone(&live_from);
        let thread = std::thread::Builder::new()
            .name("fl-image".into())
            .spawn(move || {
                for job in rx {
                    if job.tag < live.load(Ordering::Acquire) {
                        continue;
                    }
                    let result = catch_unwind(AssertUnwindSafe(|| match &job.source {
                        Source::Encoded => decode(&job.bytes, job.target, &job.limits),
                        Source::Raw(raw) => from_raw(&job.bytes, raw, job.target, &job.limits),
                        Source::Next(frames) => {
                            let mut f = frames.lock().unwrap_or_else(|e| {
                                // A panic mid-frame left the decoder there.
                                frames.clear_poison();
                                let mut f = e.into_inner();
                                f.restart();
                                f
                            });
                            f.next_frame()
                        }
                    }))
                    .unwrap_or_else(|p| {
                        let why = p
                            .downcast_ref::<&str>()
                            .map(|s| (*s).to_owned())
                            .or_else(|| p.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "a panic".to_owned());
                        Err(ImageError::Decode(format!("the decoder failed: {why}")))
                    });
                    // The bytes are the caller's; this job's share goes now,
                    // not when the next job replaces it.
                    let (id, tag) = (job.id, job.tag);
                    drop(job);
                    // A panicking sink must not end the thread for every
                    // job after it.
                    let _ = catch_unwind(AssertUnwindSafe(|| sink(Done { id, tag, result })));
                }
            })?;
        Ok(Worker {
            tx: Some(tx),
            thread: Some(thread),
            live_from,
        })
    }

    /// Queues `job`. False if the thread has gone.
    pub fn submit(&self, job: Job) -> bool {
        self.tx.as_ref().is_some_and(|tx| tx.send(job).is_ok())
    }

    /// Drops, unstarted and unanswered, every queued job tagged `tag` or
    /// lower. A job already decoding still finishes; its result carries the
    /// tag, for the caller to drop.
    pub fn retire(&self, tag: u64) {
        self.live_from
            .fetch_max(tag.saturating_add(1), Ordering::AcqRel);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

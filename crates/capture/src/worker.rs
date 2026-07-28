//! Background capture: a worker thread owning the `WindowServer` capture
//! connection, throttled per window, results delivered off-thread.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;

use crate::{Capturer, thumb};

/// At most one capture per window within this span.
const THROTTLE: Duration = Duration::from_millis(200);
/// Bound on queued requests; overflow is dropped — a refresh is best-effort.
const QUEUE_BOUND: usize = 256;

pub struct Worker {
    tx: Option<SyncSender<u32>>,
    thread: Option<JoinHandle<()>>,
    /// Set once the channel is found disconnected, so the notice is said once.
    dead: Cell<bool>,
}

impl Worker {
    /// Spawns the capture thread. `deliver` runs on that thread with each
    /// finished tile-sized image; hop back to the main thread inside it.
    /// `None` when the thread could not start — the caller then runs without
    /// previews rather than queueing into a channel nobody is reading.
    pub fn spawn(
        capturer: Capturer,
        deliver: impl Fn(u32, CFRetained<CGImage>) + Send + 'static,
    ) -> Option<Self> {
        let (tx, rx) = sync_channel(QUEUE_BOUND);
        let thread = std::thread::Builder::new()
            .name("capture".into())
            .spawn(move || run(&rx, &capturer, &deliver))
            .ok()?;
        Some(Self {
            tx: Some(tx),
            thread: Some(thread),
            dead: Cell::new(false),
        })
    }

    /// Asks for a fresh preview of `wid`. A full queue drops the request — a
    /// refresh is best-effort. A *disconnected* channel means the capture
    /// thread died, which is worth saying once rather than silently never
    /// producing another preview.
    pub fn request(&self, wid: u32) {
        let Some(tx) = &self.tx else { return };
        if let Err(TrySendError::Disconnected(_)) = tx.try_send(wid)
            && !self.dead.replace(true)
        {
            println!("capture: worker stopped — previews will not refresh");
        }
    }
}

/// Dropping drains in-flight work: the channel closes, the thread finishes its
/// backlog and exits, and the drop blocks until it has.
impl Drop for Worker {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(rx: &Receiver<u32>, capturer: &Capturer, deliver: &impl Fn(u32, CFRetained<CGImage>)) {
    let mut last: HashMap<u32, Instant> = HashMap::new();
    while let Ok(first) = rx.recv() {
        // Expired entries say nothing anymore; pruning keeps the map from
        // growing one entry per window id ever seen by this resident process.
        last.retain(|_, at| at.elapsed() < THROTTLE);
        // Coalesce the backlog so a burst of requests captures each window once.
        let mut batch = vec![first];
        while let Ok(wid) = rx.try_recv() {
            if !batch.contains(&wid) {
                batch.push(wid);
            }
        }
        for wid in batch {
            if last.get(&wid).is_some_and(|at| at.elapsed() < THROTTLE) {
                continue;
            }
            last.insert(wid, Instant::now());
            if let Some(image) = capturer.window_image(wid) {
                deliver(wid, thumb::thumbnail(image));
            }
        }
    }
}

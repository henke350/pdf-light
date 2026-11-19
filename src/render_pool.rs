//! Background page rendering on a pool of worker threads.
//!
//! The UI says which pages it wants (`set_wanted`); worker threads take requests
//! from a shared queue, render them, and send the results back over a channel.

use crate::document::open_pdf;
use crate::error::PdfError;
use crate::render::{RenderedPage, render_page};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

/// One page image the UI would like to have.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderRequest {
    /// Index of the page in the original file.
    pub page: usize,
    /// Pixels per PDF point.
    pub scale: f32,
    /// Thumbnails come after normal pages in the queue.
    pub thumbnail: bool,
}

/// A finished request: the image, or why it could not be made.
pub struct RenderResult {
    pub request: RenderRequest,
    pub result: Result<RenderedPage, PdfError>,
}

/// What the UI thread and the workers share, always behind the mutex.
#[derive(Default)]
struct Queue {
    /// Requests nobody has started yet, in order.
    waiting: VecDeque<RenderRequest>,
    /// Requests a worker has taken whose result the UI has not received yet.
    /// `set_wanted` skips these so a page is never rendered twice.
    outstanding: Vec<RenderRequest>,
    /// Set when the pool is dropped: workers exit instead of waiting.
    stop: bool,
}

/// `Mutex` lets one thread at a time change the queue; `Condvar` lets idle
/// workers sleep until `set_wanted` (or `Drop`) wakes them.
/// `Arc` shares this pair between the pool and every worker thread.
type Shared = Arc<(Mutex<Queue>, Condvar)>;

/// Renders pages in the background.
pub struct RenderPool {
    shared: Shared,
    results: Receiver<RenderResult>,
}

impl RenderPool {
    /// Starts `threads` workers (at least one) over the PDF in `bytes`. `wake`
    /// is called after each finished page so the UI knows to look at `try_recv`.
    pub fn new(bytes: Arc<Vec<u8>>, threads: usize, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let shared: Shared = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        // A channel: workers hold a `Sender` each, the pool keeps the `Receiver`.
        let (sender, results) = mpsc::channel();
        for _ in 0..threads.max(1) {
            let worker = Worker {
                bytes: Arc::clone(&bytes),
                shared: Arc::clone(&shared),
                results: sender.clone(),
                wake: Arc::clone(&wake),
            };
            // `move` hands `worker` to the new thread; dropping the `JoinHandle`
            // lets the thread run on its own (see `Drop` below).
            thread::spawn(move || worker.run());
        }
        Self { shared, results }
    }

    /// Replaces the queue with `wanted`: anything not started and no longer
    /// wanted is dropped. Pages go before thumbnails; otherwise order is kept.
    pub fn set_wanted(&self, wanted: Vec<RenderRequest>) {
        let (pages, thumbnails): (Vec<_>, Vec<_>) = wanted.into_iter().partition(|r| !r.thumbnail);
        let (mutex, workers) = &*self.shared;
        let mut queue = lock(mutex);
        let mut waiting = VecDeque::with_capacity(pages.len() + thumbnails.len());
        for request in pages.into_iter().chain(thumbnails) {
            if !queue.outstanding.contains(&request) && !waiting.contains(&request) {
                waiting.push_back(request);
            }
        }
        queue.waiting = waiting;
        workers.notify_all();
    }

    /// A finished page, if one is ready. Never blocks.
    pub fn try_recv(&self) -> Option<RenderResult> {
        let result = self.results.try_recv().ok()?;
        let mut queue = lock(&self.shared.0);
        if let Some(i) = queue.outstanding.iter().position(|r| *r == result.request) {
            queue.outstanding.swap_remove(i);
        }
        Some(result)
    }
}

/// `Drop` runs automatically when the pool goes away (e.g. another file is opened).
impl Drop for RenderPool {
    fn drop(&mut self) {
        let (mutex, workers) = &*self.shared;
        lock(mutex).stop = true;
        workers.notify_all();
        // No `join`: a worker in the middle of a page finishes it and exits on
        // its own, so the UI thread never waits for a render here.
    }
}

/// Everything one worker thread owns.
struct Worker {
    bytes: Arc<Vec<u8>>,
    shared: Shared,
    results: Sender<RenderResult>,
    /// `dyn Fn` is "any function or closure with this signature", chosen at run time;
    /// `Send + Sync` promise it is safe to call from other threads.
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Worker {
    fn run(self) {
        // hayro's `Pdf` cannot move between threads, so each worker parses its own.
        let pdf = open_pdf(&self.bytes);
        while let Some(request) = self.next_request() {
            let result = match &pdf {
                // A bug in a PDF library must not take the worker down with it:
                // `catch_unwind` turns a panic during rendering into an error.
                Ok(pdf) => catch_unwind(AssertUnwindSafe(|| {
                    render_page(pdf, request.page, request.scale)
                }))
                .unwrap_or(Err(PdfError::Invalid)),
                Err(error) => Err(same_error(error)),
            };
            if self.results.send(RenderResult { request, result }).is_err() {
                return; // The pool is gone.
            }
            (self.wake)();
        }
    }

    /// Waits for the next request; `None` means the pool was dropped.
    fn next_request(&self) -> Option<RenderRequest> {
        let (mutex, workers) = &*self.shared;
        let mut queue = lock(mutex);
        loop {
            if queue.stop {
                return None;
            }
            if let Some(request) = queue.waiting.pop_front() {
                queue.outstanding.push(request);
                return Some(request);
            }
            // Releases the lock while asleep and takes it back on waking.
            queue = workers.wait(queue).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Takes the lock even if another thread panicked while holding it: the queue
/// is only ever changed in small steps, so it is still usable.
fn lock(mutex: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `PdfError` can't be cloned (it may hold an `io::Error`), so rebuild the
/// errors `open_pdf` can return.
fn same_error(error: &PdfError) -> PdfError {
    match error {
        PdfError::Encrypted => PdfError::Encrypted,
        _ => PdfError::Invalid,
    }
}

//! Searching pages on a background thread, so the window never waits for
//! text extraction.
//!
//! Each search gets its own thread. It walks the pages it was given in order,
//! reads each page's text (from the cache, or by extracting it once), and
//! sends the matches of every page that has some back over a channel.

use crate::document::open_pdf;
use crate::search::{PageText, build_page_text, find_in_page, normalize_query};
use crate::text::{PageRect, extract_glyphs};
use hayro::hayro_syntax::Pdf;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

/// Page text already read, by original page index, kept for the document's
/// lifetime so a second search doesn't extract the same pages again.
/// `Mutex<HashMap>`: one thread at a time may look in or add to the map;
/// `Arc` shares it between the window and every search thread.
pub type TextCache = Arc<Mutex<HashMap<usize, Arc<PageText>>>>;

/// The matches on one page (only sent for pages with at least one).
#[derive(Debug, Clone, PartialEq)]
pub struct PageMatches {
    /// Index of the page in the original file.
    pub page: usize,
    /// One entry per match: its rects, one per line it covers.
    pub matches: Vec<Vec<PageRect>>,
}

/// A running search. Dropping it, or setting `cancel`, stops the thread
/// before its next page.
pub struct SearchHandle {
    /// Results arrive here page by page; once the search is done the thread
    /// hangs up, and `try_recv` reports `Disconnected`.
    pub results: Receiver<PageMatches>,
    /// An `AtomicBool` is a flag that several threads can read and set safely
    /// without a lock; the thread checks it before every page.
    pub cancel: Arc<AtomicBool>,
}

impl Drop for SearchHandle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Starts searching `pages` (original indices, searched in this order) for
/// `query`. `wake` is called after each page with matches and once at the end,
/// so the window knows to look at `results`.
pub fn spawn_search(
    bytes: Arc<Vec<u8>>,
    pages: Vec<usize>,
    query: String,
    cache: TextCache,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> SearchHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let (sender, results) = mpsc::channel();
    let search = Search {
        bytes,
        cache,
        cancel: Arc::clone(&cancel),
        results: sender,
        wake: Arc::clone(&wake),
    };
    thread::spawn(move || {
        search.run(&pages, &normalize_query(&query));
        // `search` (and with it the `Sender`) is gone now, so the window sees
        // `Disconnected` when it is woken: the search is finished.
        wake();
    });
    SearchHandle { results, cancel }
}

/// Everything one search thread owns.
struct Search {
    bytes: Arc<Vec<u8>>,
    cache: TextCache,
    cancel: Arc<AtomicBool>,
    results: Sender<PageMatches>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Search {
    /// Taking `self` by value means the `Search` is dropped when `run` returns.
    fn run(self, pages: &[usize], query: &[char]) {
        if query.is_empty() {
            return;
        }
        // Parsed only when a page isn't cached yet; hayro's `Pdf` can't move
        // between threads, so this thread parses its own.
        let mut pdf: Option<Pdf> = None;
        for &page in pages {
            // `Relaxed` is enough: the flag carries no other data with it.
            if self.cancel.load(Ordering::Relaxed) {
                return;
            }
            let Some(text) = self.page_text(&mut pdf, page) else {
                return; // The file can't be parsed: nothing to find.
            };
            let matches = find_in_page(&text, query);
            if matches.is_empty() {
                continue;
            }
            if self.results.send(PageMatches { page, matches }).is_err() {
                return; // The window dropped the handle.
            }
            (self.wake)();
        }
    }

    /// The text of `page`, from the cache or extracted (and then cached).
    /// `None` if the file can't be parsed.
    fn page_text(&self, pdf: &mut Option<Pdf>, page: usize) -> Option<Arc<PageText>> {
        // The lock is held only for the look-up, never while extracting.
        if let Some(text) = lock(&self.cache).get(&page) {
            return Some(Arc::clone(text));
        }
        if pdf.is_none() {
            *pdf = Some(open_pdf(&self.bytes).ok()?);
        }
        let pdf = pdf.as_ref()?;
        // As in the render pool, a panic inside the PDF library must not take
        // the thread down: such a page just has no text.
        let glyphs =
            catch_unwind(AssertUnwindSafe(|| extract_glyphs(pdf, page))).unwrap_or_default();
        let text = Arc::new(build_page_text(&glyphs));
        lock(&self.cache).insert(page, Arc::clone(&text));
        Some(text)
    }
}

/// Takes the lock even if another thread panicked while holding it: the map
/// is only changed by single inserts, so it is still usable.
fn lock(cache: &TextCache) -> std::sync::MutexGuard<'_, HashMap<usize, Arc<PageText>>> {
    cache.lock().unwrap_or_else(PoisonError::into_inner)
}

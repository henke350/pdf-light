//! Find in document: the running search, its matches, and which one is current.
//!
//! The search itself runs on a `search_worker` thread; this side only starts
//! and stops it and collects what it sends, so the window never waits.

use super::toolbar::FindStatus;
use eframe::egui;
use pdf_light::search::normalize_query;
use pdf_light::search_worker::{PageMatches, SearchHandle, TextCache, spawn_search};
use pdf_light::text::PageRect;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

/// How long the field must be still before a changed query is searched, in seconds.
pub const DEBOUNCE_SECS: f64 = 0.15;

/// Find for one open document. It lives in `OpenDocument`, so a newly opened
/// (or saved and reopened) file starts with a fresh text cache and search.
pub struct Find {
    cache: TextCache,
    /// The normalized query of the current search; empty when there is none.
    query: Vec<char>,
    /// The kept pages (in document order) the current search covers.
    pages: Vec<usize>,
    /// The field's text as last seen, and `ctx.input(|i| i.time)` when it changed.
    typed: String,
    typed_at: f64,
    /// `None` once the search has finished (or when there is none).
    handle: Option<SearchHandle>,
    matches: Matches,
    /// Scroll to the current match on the next `take_reveal`.
    reveal: bool,
    /// The search is for a new query: scroll to its first match when it
    /// arrives. (A restart after deleting pages leaves the view where it is.)
    reveal_first: bool,
}

impl Find {
    pub fn new() -> Self {
        Self {
            cache: TextCache::default(),
            query: Vec::new(),
            pages: Vec::new(),
            typed: String::new(),
            typed_at: 0.0,
            handle: None,
            matches: Matches::default(),
            reveal: false,
            reveal_first: false,
        }
    }

    /// Starts, restarts or clears the search so it matches the find field
    /// (`field`) and the kept pages (`kept`, original indices in document
    /// order). Called every frame, after anything that may delete pages.
    /// A new search starts at `current` (a position in `kept`) and wraps.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        field: &str,
        kept: &[usize],
        current: usize,
        bytes: &Arc<Vec<u8>>,
    ) {
        let now = ctx.input(|i| i.time);
        if field != self.typed {
            field.clone_into(&mut self.typed);
            self.typed_at = now;
        }
        let wanted = normalize_query(field);
        let searched = (self.query.as_slice(), self.pages.as_slice());
        match next_step(searched, (&wanted, kept), now - self.typed_at) {
            Step::Keep => {}
            Step::Clear => self.clear(),
            Step::Wait(secs) => ctx.request_repaint_after(Duration::from_secs_f64(secs)),
            Step::Start => {
                let repaint_ctx = ctx.clone();
                // Called from the search thread; `request_repaint` is thread-safe.
                let wake: Arc<dyn Fn() + Send + Sync> =
                    Arc::new(move || repaint_ctx.request_repaint());
                self.reveal_first = wanted != self.query;
                self.reveal = false;
                self.matches = Matches::default();
                // Replacing the handle drops the old one, which cancels its search.
                self.handle = Some(spawn_search(
                    Arc::clone(bytes),
                    search_order(kept, current),
                    field.to_owned(),
                    Arc::clone(&self.cache),
                    wake,
                ));
                self.query = wanted;
                kept.clone_into(&mut self.pages);
            }
        }
    }

    /// Carries the search over to a reopened file (after Save): starts it at
    /// once, without the debounce, and without scrolling to its first match.
    pub fn resume(
        &mut self,
        ctx: &egui::Context,
        field: &str,
        kept: &[usize],
        current: usize,
        bytes: &Arc<Vec<u8>>,
    ) {
        field.clone_into(&mut self.typed);
        // The same query with no pages searched yet: `update` restarts at once.
        self.query = normalize_query(field);
        self.update(ctx, field, kept, current, bytes);
    }

    /// Stops the search and forgets its matches (the text cache stays).
    fn clear(&mut self) {
        self.handle = None;
        self.query.clear();
        self.pages.clear();
        self.matches = Matches::default();
        self.reveal = false;
        self.reveal_first = false;
    }

    /// Takes the results that arrived since the last frame; never waits.
    pub fn receive(&mut self) {
        loop {
            let Some(handle) = &self.handle else {
                return;
            };
            // `try_recv` returns at once: a result, `Empty` (nothing yet) or
            // `Disconnected` (the search thread has finished and hung up).
            match handle.results.try_recv() {
                Ok(found) => {
                    let first = self.matches.current().is_none();
                    self.matches.push(found);
                    if first && self.reveal_first {
                        self.reveal = true;
                    }
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => self.handle = None,
            }
        }
    }

    /// What the toolbar shows after the field.
    pub fn status(&self) -> FindStatus {
        if self.query.is_empty() {
            FindStatus::Idle
        } else {
            self.matches.status(self.handle.is_some())
        }
    }

    pub fn matches(&self) -> &Matches {
        &self.matches
    }

    /// Makes the next (or previous) match current, wrapping, and scrolls to it.
    pub fn step(&mut self, forward: bool) {
        if forward {
            self.matches.next();
        } else {
            self.matches.previous();
        }
        self.reveal = true;
    }

    /// The match to scroll to, once: after a step, or a new query's first match.
    pub fn take_reveal(&mut self) -> Option<&Match> {
        // `mem::take` reads the flag and resets it to `false` in one go.
        if std::mem::take(&mut self.reveal) {
            self.matches.current()
        } else {
            None
        }
    }
}

/// One match: the page it is on (original index) and its rects, one per line.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub page: usize,
    pub rects: Vec<PageRect>,
}

/// The matches found so far, in the order the search found them (from the
/// page the search started on, wrapping), and which one is current. Matches
/// are only ever added at the end, so the current match, and its number in
/// "3 of 27", stay put while more results arrive.
#[derive(Debug, Default)]
pub struct Matches {
    list: Vec<Match>,
    /// Where each page's matches sit in `list` (a page arrives once, whole).
    by_page: HashMap<usize, Range<usize>>,
    /// Index into `list`; `Some` whenever `list` isn't empty.
    current: Option<usize>,
}

impl Matches {
    /// Adds one page's matches. The first match found becomes current: the
    /// search starts at the current page, so it is the first one at or after it.
    pub fn push(&mut self, found: PageMatches) {
        let start = self.list.len();
        let page = found.page;
        self.list
            .extend(found.matches.into_iter().map(|rects| Match { page, rects }));
        self.by_page.insert(page, start..self.list.len());
        if self.current.is_none() && !self.list.is_empty() {
            self.current = Some(0);
        }
    }

    /// Makes the next match current, wrapping to the first.
    pub fn next(&mut self) {
        if let Some(current) = self.current {
            self.current = Some((current + 1) % self.list.len());
        }
    }

    /// Makes the previous match current, wrapping to the last.
    pub fn previous(&mut self) {
        if let Some(current) = self.current {
            let len = self.list.len();
            self.current = Some((current + len - 1) % len);
        }
    }

    pub fn current(&self) -> Option<&Match> {
        self.current.and_then(|i| self.list.get(i))
    }

    /// "3 of 27" (counting as results arrive), "Searching…" or "No matches".
    pub fn status(&self, searching: bool) -> FindStatus {
        match self.current {
            Some(current) => FindStatus::Matches {
                current: current + 1,
                total: self.list.len(),
            },
            None if searching => FindStatus::Searching,
            None => FindStatus::NoMatches,
        }
    }

    /// The matches on `page` (original index), each with whether it is current.
    pub fn on_page(&self, page: usize) -> impl Iterator<Item = (&Match, bool)> {
        let range = self.by_page.get(&page).cloned().unwrap_or_default();
        let current = self.current;
        // `get` with a range gives `None` instead of panicking if it were out
        // of bounds; the empty slice is then the `Default`.
        let on_page = self.list.get(range.clone()).unwrap_or_default();
        on_page
            .iter()
            .zip(range)
            .map(move |(m, i)| (m, Some(i) == current))
    }
}

/// The kept pages starting at `start` (a position in `kept`) and wrapping
/// round; all of `kept` from the first page if `start` is past the end.
pub fn search_order(kept: &[usize], start: usize) -> Vec<usize> {
    let mut order = kept.to_vec();
    if start < order.len() {
        order.rotate_left(start);
    }
    order
}

/// What the search should do this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Nothing changed.
    Keep,
    /// The field is empty: stop and forget the search.
    Clear,
    /// Start (or restart) the search now.
    Start,
    /// The query changed; start once the field has been still this many more seconds.
    Wait(f64),
}

/// Decides `Step` from what was searched and what is wanted now, each as
/// (normalized query, kept pages). `since_typed` is how long ago the field
/// last changed, in seconds. A changed query waits for typing to pause; a
/// changed page list (delete, undo) restarts at once.
pub fn next_step(
    searched: (&[char], &[usize]),
    wanted: (&[char], &[usize]),
    since_typed: f64,
) -> Step {
    let (searched_query, searched_pages) = searched;
    let (query, pages) = wanted;
    if query.is_empty() {
        if searched_query.is_empty() {
            Step::Keep
        } else {
            Step::Clear
        }
    } else if query != searched_query {
        if since_typed >= DEBOUNCE_SECS {
            Step::Start
        } else {
            Step::Wait(DEBOUNCE_SECS - since_typed)
        }
    } else if pages != searched_pages {
        Step::Start
    } else {
        Step::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(y: f32) -> PageRect {
        PageRect {
            x0: 0.0,
            y0: y,
            x1: 10.0,
            y1: y + 10.0,
        }
    }

    /// Matches on `page`, one per entry of `ys` (the match's top).
    fn found(page: usize, ys: &[f32]) -> PageMatches {
        PageMatches {
            page,
            matches: ys.iter().map(|&y| vec![rect(y)]).collect(),
        }
    }

    /// (page, top) of the current match.
    fn current(matches: &Matches) -> Option<(usize, f32)> {
        matches.current().map(|m| (m.page, m.rects[0].y0))
    }

    #[test]
    fn search_order_should_start_at_the_current_page_and_wrap() {
        assert_eq!(search_order(&[0, 2, 3, 5], 2), vec![3, 5, 0, 2]);
    }

    #[test]
    fn search_order_should_start_at_the_first_page_when_past_the_end() {
        assert_eq!(search_order(&[0, 2, 3], 7), vec![0, 2, 3]);
    }

    #[test]
    fn matches_should_make_the_first_match_current() {
        let mut matches = Matches::default();
        matches.push(found(4, &[30.0, 50.0]));
        assert_eq!(current(&matches), Some((4, 30.0)));
    }

    #[test]
    fn matches_should_keep_the_current_match_as_results_arrive() {
        let mut matches = Matches::default();
        matches.push(found(4, &[30.0, 50.0]));
        matches.next();
        matches.push(found(6, &[10.0]));
        assert_eq!(
            (
                current(&matches),
                matches.status(true),
                matches.status(false)
            ),
            (
                Some((4, 50.0)),
                FindStatus::Matches {
                    current: 2,
                    total: 3
                },
                FindStatus::Matches {
                    current: 2,
                    total: 3
                }
            )
        );
    }

    #[test]
    fn matches_should_wrap_when_stepping() {
        let mut matches = Matches::default();
        matches.push(found(1, &[10.0]));
        matches.push(found(2, &[20.0]));
        matches.next();
        matches.next();
        let after_next = current(&matches);
        matches.previous();
        let after_previous = current(&matches);
        assert_eq!(
            (after_next, after_previous),
            (Some((1, 10.0)), Some((2, 20.0)))
        );
    }

    #[test]
    fn matches_should_report_searching_then_no_matches() {
        let mut matches = Matches::default();
        matches.next(); // Stepping with nothing found does nothing.
        assert_eq!(
            (
                matches.status(true),
                matches.status(false),
                current(&matches)
            ),
            (FindStatus::Searching, FindStatus::NoMatches, None)
        );
    }

    #[test]
    fn matches_should_list_a_pages_matches_and_mark_the_current_one() {
        let mut matches = Matches::default();
        matches.push(found(1, &[10.0]));
        matches.push(found(2, &[20.0, 40.0]));
        matches.next();
        matches.next();
        let on_two: Vec<(f32, bool)> = matches
            .on_page(2)
            .map(|(m, is_current)| (m.rects[0].y0, is_current))
            .collect();
        assert_eq!(
            (on_two, matches.on_page(3).count()),
            (vec![(20.0, false), (40.0, true)], 0)
        );
    }

    const HELLO: &[char] = &['h', 'e', 'l', 'l', 'o'];
    const HELP: &[char] = &['h', 'e', 'l', 'p'];
    const PAGES: &[usize] = &[0, 1, 2];

    #[test]
    fn next_step_should_keep_an_unchanged_search() {
        assert_eq!(next_step((HELLO, PAGES), (HELLO, PAGES), 0.0), Step::Keep);
    }

    #[test]
    fn next_step_should_wait_for_typing_to_pause() {
        assert_eq!(
            next_step((HELLO, PAGES), (HELP, PAGES), 0.05),
            Step::Wait(DEBOUNCE_SECS - 0.05)
        );
    }

    #[test]
    fn next_step_should_start_once_typing_paused() {
        assert_eq!(
            next_step((HELLO, PAGES), (HELP, PAGES), DEBOUNCE_SECS),
            Step::Start
        );
    }

    #[test]
    fn next_step_should_restart_at_once_when_pages_change() {
        assert_eq!(
            next_step((HELLO, PAGES), (HELLO, &[0, 2]), 0.0),
            Step::Start
        );
    }

    #[test]
    fn next_step_should_clear_when_the_field_is_emptied() {
        assert_eq!(
            [
                next_step((HELLO, PAGES), (&[], PAGES), 0.0),
                next_step((&[], PAGES), (&[], &[0]), 0.0),
            ],
            [Step::Clear, Step::Keep]
        );
    }
}

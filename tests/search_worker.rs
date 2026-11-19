//! Tests for searching pages on a background thread.

mod common;

use common::make_pdf;
use pdf_light::search::PageText;
use pdf_light::search_worker::{PageMatches, SearchHandle, TextCache, spawn_search};
use pdf_light::text::PageRect;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const FIXTURE: [&str; 3] = ["Page 1\nhello world", "Page 2\nhello", "Page 3"];

fn no_wake() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {})
}

fn search(pages: Vec<usize>, query: &str, cache: &TextCache) -> SearchHandle {
    spawn_search(
        Arc::new(make_pdf(&FIXTURE)),
        pages,
        query.to_owned(),
        Arc::clone(cache),
        no_wake(),
    )
}

/// Every result until the search finishes (the worker hangs up), giving up
/// after 10 seconds without one.
fn collect(handle: &SearchHandle) -> Vec<PageMatches> {
    let mut results = Vec::new();
    loop {
        match handle.results.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => results.push(result),
            Err(RecvTimeoutError::Disconnected) => return results,
            Err(RecvTimeoutError::Timeout) => panic!("search did not finish"),
        }
    }
}

/// (page, number of matches) for each result, in arrival order.
fn counts(results: &[PageMatches]) -> Vec<(usize, usize)> {
    results.iter().map(|r| (r.page, r.matches.len())).collect()
}

#[test]
fn spawn_search_should_find_matches_on_each_page() {
    let cache = TextCache::default();
    let results = collect(&search(vec![0, 1, 2], "hello", &cache));
    assert_eq!(counts(&results), vec![(0, 1), (1, 1)]);
}

#[test]
fn spawn_search_should_only_search_given_pages() {
    let cache = TextCache::default();
    let results = collect(&search(vec![1, 2], "hello", &cache));
    assert_eq!(counts(&results), vec![(1, 1)]);
}

#[test]
fn spawn_search_should_fill_text_cache() {
    let cache = TextCache::default();
    collect(&search(vec![0, 1, 2], "hello", &cache));
    let entries = cache.lock().map(|c| c.len()).unwrap_or_default();
    assert_eq!(entries, 3);
}

#[test]
fn spawn_search_should_walk_pages_in_the_given_order() {
    let cache = TextCache::default();
    let results = collect(&search(vec![1, 2, 0], "hello", &cache));
    assert_eq!(counts(&results), vec![(1, 1), (0, 1)]);
}

#[test]
fn spawn_search_should_use_cached_text_instead_of_extracting() {
    // Page 2 has no "hello", but the cache says it has: the cache must win.
    let rect = PageRect {
        x0: 1.0,
        y0: 2.0,
        x1: 3.0,
        y1: 4.0,
    };
    let text = PageText {
        chars: "hello".chars().collect(),
        rects: vec![rect; 5],
    };
    let cache: TextCache = Arc::new(Mutex::new([(2, Arc::new(text))].into()));
    let results = collect(&search(vec![2], "HELLO", &cache));
    assert_eq!(counts(&results), vec![(2, 1)]);
}

#[test]
fn spawn_search_should_send_nothing_for_an_empty_query() {
    let cache = TextCache::default();
    let results = collect(&search(vec![0, 1, 2], "   ", &cache));
    assert!(results.is_empty());
}

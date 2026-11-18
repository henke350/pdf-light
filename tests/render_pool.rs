//! Tests for rendering pages on background threads.

mod common;

use common::make_pdf;
use pdf_light::error::PdfError;
use pdf_light::render_pool::{RenderPool, RenderRequest, RenderResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn page(page: usize, scale: f32) -> RenderRequest {
    RenderRequest {
        page,
        scale,
        thumbnail: false,
    }
}

fn no_wake() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {})
}

/// Collects `count` results, giving up after 10 seconds.
fn collect(pool: &RenderPool, count: usize) -> Vec<RenderResult> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut results = Vec::new();
    while results.len() < count && Instant::now() < deadline {
        match pool.try_recv() {
            Some(result) => results.push(result),
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    results
}

#[test]
fn render_pool_should_return_every_wanted_page() {
    let pool = RenderPool::new(Arc::new(make_pdf(&["1", "2", "3"])), 2, no_wake());
    pool.set_wanted((0..3).map(|i| page(i, 0.5)).collect());

    let mut widths: Vec<(usize, u32)> = collect(&pool, 3)
        .into_iter()
        .filter_map(|r| r.result.ok().map(|img| (r.request.page, img.width)))
        .collect();
    widths.sort_unstable();
    assert_eq!(widths, vec![(0, 306), (1, 306), (2, 306)]);
}

#[test]
fn render_pool_should_render_pages_before_thumbnails() {
    let pool = RenderPool::new(Arc::new(make_pdf(&["1", "2", "3"])), 1, no_wake());
    let thumbnail = RenderRequest {
        page: 0,
        scale: 0.25,
        thumbnail: true,
    };
    pool.set_wanted(vec![thumbnail, page(1, 0.5), page(2, 0.5)]);

    let order: Vec<(usize, bool)> = collect(&pool, 3)
        .iter()
        .map(|r| (r.request.page, r.request.thumbnail))
        .collect();
    assert_eq!(order, vec![(1, false), (2, false), (0, true)]);
}

#[test]
fn render_pool_should_report_errors_and_keep_working() {
    let pool = RenderPool::new(Arc::new(make_pdf(&["1"])), 1, no_wake());
    pool.set_wanted(vec![page(0, 1000.0), page(0, 0.5)]);

    let results = collect(&pool, 2);
    let outcome: Vec<bool> = results.iter().map(|r| r.result.is_ok()).collect();
    assert_eq!(outcome, vec![false, true]);
}

#[test]
fn render_pool_should_fail_every_request_for_garbage_bytes() {
    let pool = RenderPool::new(Arc::new(b"not a pdf".to_vec()), 1, no_wake());
    pool.set_wanted(vec![page(0, 1.0)]);

    let results = collect(&pool, 1);
    assert!(matches!(
        results.first().map(|r| &r.result),
        Some(Err(PdfError::Invalid))
    ));
}

#[test]
fn render_pool_should_call_wake_for_each_result() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let pool = RenderPool::new(Arc::new(make_pdf(&["1", "2"])), 2, wake);
    pool.set_wanted(vec![page(0, 0.5), page(1, 0.5)]);

    collect(&pool, 2);
    // `wake` runs just after a result is sent, so give the last call a moment.
    let deadline = Instant::now() + Duration::from_secs(5);
    while wakes.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
}

#[test]
fn set_wanted_should_not_repeat_a_request_already_being_rendered() {
    let pool = RenderPool::new(Arc::new(make_pdf(&["1"])), 1, no_wake());
    pool.set_wanted(vec![page(0, 0.5)]);
    pool.set_wanted(vec![page(0, 0.5)]);

    let first = collect(&pool, 1);
    std::thread::sleep(Duration::from_millis(300));
    let extra = pool.try_recv().is_some();
    assert_eq!((first.len(), extra), (1, false));
}

#[test]
fn dropping_the_pool_should_not_hang() {
    let pool = RenderPool::new(Arc::new(make_pdf(&["1", "2"])), 2, no_wake());
    pool.set_wanted(vec![page(0, 0.5), page(1, 0.5)]);
    drop(pool);
}

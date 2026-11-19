//! Tests for extracting text and its position from pages.

mod common;

use common::make_pdf;
use pdf_light::document::open_pdf;
use pdf_light::search::build_page_text;
use pdf_light::text::extract_glyphs;
use std::sync::Arc;

fn pdf_for(pages: &[&str]) -> hayro::hayro_syntax::Pdf {
    open_pdf(&Arc::new(make_pdf(pages))).expect("fixture opens")
}

#[test]
fn extract_glyphs_should_read_fixture_text() {
    let pdf = pdf_for(&["Hello world"]);
    let text: String = build_page_text(&extract_glyphs(&pdf, 0))
        .chars
        .into_iter()
        .collect();
    assert_eq!(text, "hello world");
}

#[test]
fn extract_glyphs_should_place_text_near_top_left() {
    let pdf = pdf_for(&["Hello world"]);
    let glyphs = extract_glyphs(&pdf, 0);
    let first = glyphs.first().expect("has glyphs").rect;
    assert!((70.0..75.0).contains(&first.x0), "x0 = {}", first.x0);
    assert!((50.0..80.0).contains(&first.y0), "y0 = {}", first.y0);
}

#[test]
fn extract_glyphs_should_return_nothing_for_blank_page() {
    let pdf = pdf_for(&[""]);
    assert!(extract_glyphs(&pdf, 0).is_empty());
}

#[test]
fn extract_glyphs_should_return_nothing_for_out_of_range_page() {
    let pdf = pdf_for(&["a"]);
    assert!(extract_glyphs(&pdf, 5).is_empty());
}

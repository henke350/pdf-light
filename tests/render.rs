//! Tests for opening documents and rendering pages.

mod common;

use common::make_pdf;
use pdf_light::document::{self, open_pdf};
use pdf_light::error::PdfError;
use pdf_light::render::render_page;
use std::path::PathBuf;
use std::sync::Arc;

fn pdf_for(pages: &[&str]) -> hayro::hayro_syntax::Pdf {
    open_pdf(&Arc::new(make_pdf(pages))).expect("fixture opens")
}

#[test]
fn open_should_report_page_sizes() {
    let doc = document::from_bytes(PathBuf::from("x.pdf"), make_pdf(&["a", "b"])).expect("opens");
    assert_eq!(doc.page_sizes, vec![(612.0, 792.0); 2]);
}

#[test]
fn from_bytes_should_reject_garbage() {
    let result = document::from_bytes(PathBuf::from("x.pdf"), b"not a pdf".to_vec());
    assert!(matches!(result, Err(PdfError::Invalid)));
}

#[test]
fn from_bytes_should_reject_empty_file() {
    let result = document::from_bytes(PathBuf::from("x.pdf"), vec![]);
    assert!(matches!(result, Err(PdfError::Invalid)));
}

#[test]
fn render_page_should_scale_pixels() {
    let pdf = pdf_for(&["Page 1"]);
    let page = render_page(&pdf, 0, 2.0).expect("renders");
    assert_eq!((page.width, page.height), (1224, 1584));
    assert_eq!(page.rgba.len(), 1224 * 1584 * 4);
}

#[test]
fn render_page_should_draw_text_dark_pixels() {
    let pdf = pdf_for(&["Page 1"]);
    let page = render_page(&pdf, 0, 1.0).expect("renders");
    assert!(page.rgba.as_chunks::<4>().0.iter().any(|px| px[0] < 128));
    assert_eq!(page.rgba[0..4], [255, 255, 255, 255]);
}

#[test]
fn render_page_should_refuse_oversized_page() {
    let pdf = pdf_for(&["Page 1"]);
    let result = render_page(&pdf, 0, 100.0);
    assert!(matches!(
        result,
        Err(PdfError::TooLarge {
            width: 61200,
            height: 79200
        })
    ));
}

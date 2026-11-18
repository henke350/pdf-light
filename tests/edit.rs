//! Tests for saving a PDF with some pages removed.

mod common;

use common::make_pdf;
use pdf_light::document::open_pdf;
use pdf_light::edit::save_with_pages;
use pdf_light::error::PdfError;
use pdf_light::search::build_page_text;
use pdf_light::text::extract_glyphs;
use std::path::PathBuf;
use std::sync::Arc;

/// A unique, empty folder for one test; removed when dropped (even if the test fails).
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pdf-light-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir is created");
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn five_pages() -> Vec<u8> {
    make_pdf(&["Page 1", "Page 2", "Page 3", "Page 4", "Page 5"])
}

fn files_in(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("dir is readable")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn save_with_pages_should_keep_only_listed_pages() {
    let dir = TempDir::new("keep");
    let target = dir.0.join("out.pdf");
    save_with_pages(&five_pages(), &[0, 2, 4], 5, &target).expect("save works");

    let pdf = open_pdf(&Arc::new(std::fs::read(&target).expect("read back"))).expect("opens");
    assert_eq!(pdf.pages().len(), 3);
    let text: String = build_page_text(&extract_glyphs(&pdf, 1))
        .chars
        .into_iter()
        .collect();
    assert_eq!(text, "page 3");
    assert_eq!(files_in(&dir.0), ["out.pdf"]);
}

#[test]
fn save_with_pages_should_replace_existing_file() {
    let dir = TempDir::new("replace");
    let target = dir.0.join("out.pdf");
    std::fs::write(&target, b"old contents").expect("write old file");
    save_with_pages(&five_pages(), &[1, 3], 5, &target).expect("save works");

    let pdf = open_pdf(&Arc::new(std::fs::read(&target).expect("read back"))).expect("opens");
    assert_eq!(pdf.pages().len(), 2);
}

#[test]
fn save_with_pages_should_fail_cleanly_for_unparseable_input() {
    let dir = TempDir::new("garbage");
    let target = dir.0.join("out.pdf");
    let result = save_with_pages(b"garbage", &[0], 1, &target);
    assert!(matches!(result, Err(PdfError::Save(_))));
    assert!(files_in(&dir.0).is_empty());
}

#[test]
fn save_with_pages_should_fail_cleanly_for_missing_folder() {
    let dir = TempDir::new("missing");
    let target = dir.0.join("no-such-folder").join("out.pdf");
    let result = save_with_pages(&five_pages(), &[0], 5, &target);
    assert!(matches!(result, Err(PdfError::Io(_))));
    assert!(files_in(&dir.0).is_empty());
}

#[test]
fn save_with_pages_should_refuse_when_page_counts_disagree() {
    let dir = TempDir::new("count");
    let target = dir.0.join("out.pdf");
    // The viewer thinks there are 4 pages, but lopdf finds 5: don't guess which went.
    let result = save_with_pages(&five_pages(), &[0, 1], 4, &target);
    assert!(matches!(result, Err(PdfError::Save(_))));
    assert!(files_in(&dir.0).is_empty());
}

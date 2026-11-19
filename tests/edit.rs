//! Tests for saving a PDF with some pages removed.

mod common;

use common::make_pdf;
use pdf_light::document::open_pdf;
use pdf_light::edit::save_with_pages;
use pdf_light::error::PdfError;
use pdf_light::pages::KeptPage;
use pdf_light::rotation::Rotation;
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

/// Pages `indices` of the file, none of them turned.
fn upright(indices: &[usize]) -> Vec<KeptPage> {
    indices
        .iter()
        .map(|&index| KeptPage::upright(index))
        .collect()
}

/// The page sizes hayro reads from the PDF at `path` (with `/Rotate` applied).
fn sizes_in(path: &std::path::Path) -> Vec<(f32, f32)> {
    let pdf = open_pdf(&Arc::new(std::fs::read(path).expect("read back"))).expect("opens");
    pdf.pages().iter().map(|p| p.render_dimensions()).collect()
}

fn turned(index: usize, quarter_turns: i32) -> KeptPage {
    KeptPage {
        index,
        rotation: Rotation::quarter_turns(quarter_turns),
    }
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
    save_with_pages(&five_pages(), &upright(&[0, 2, 4]), 5, &target).expect("save works");

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
    save_with_pages(&five_pages(), &upright(&[1, 3]), 5, &target).expect("save works");

    let pdf = open_pdf(&Arc::new(std::fs::read(&target).expect("read back"))).expect("opens");
    assert_eq!(pdf.pages().len(), 2);
}

#[test]
fn save_with_pages_should_fail_cleanly_for_unparseable_input() {
    let dir = TempDir::new("garbage");
    let target = dir.0.join("out.pdf");
    let result = save_with_pages(b"garbage", &upright(&[0]), 1, &target);
    assert!(matches!(result, Err(PdfError::Save(_))));
    assert!(files_in(&dir.0).is_empty());
}

#[test]
fn save_with_pages_should_fail_cleanly_for_missing_folder() {
    let dir = TempDir::new("missing");
    let target = dir.0.join("no-such-folder").join("out.pdf");
    let result = save_with_pages(&five_pages(), &upright(&[0]), 5, &target);
    assert!(matches!(result, Err(PdfError::Io(_))));
    assert!(files_in(&dir.0).is_empty());
}

#[test]
fn save_with_pages_should_refuse_when_page_counts_disagree() {
    let dir = TempDir::new("count");
    let target = dir.0.join("out.pdf");
    // The viewer thinks there are 4 pages, but lopdf finds 5: don't guess which went.
    let result = save_with_pages(&five_pages(), &upright(&[0, 1]), 4, &target);
    assert!(matches!(result, Err(PdfError::Save(_))));
    assert!(files_in(&dir.0).is_empty());
}

#[test]
fn save_with_pages_should_write_turned_pages() {
    let dir = TempDir::new("turn");
    let target = dir.0.join("out.pdf");
    let kept = [turned(0, 1), KeptPage::upright(1), turned(2, 2)];
    save_with_pages(&five_pages(), &kept, 5, &target).expect("save works");
    // Turned a quarter, the first page lies on its side; half a turn keeps the shape.
    assert_eq!(
        sizes_in(&target),
        [(792.0, 612.0), (612.0, 792.0), (612.0, 792.0)]
    );
}

#[test]
fn save_with_pages_should_add_to_an_inherited_rotation() {
    // Every page inherits /Rotate 90 from the page tree's root.
    let mut doc = lopdf::Document::load_mem(&five_pages()).expect("loads");
    let root = doc
        .catalog()
        .and_then(|catalog| catalog.get(b"Pages"))
        .and_then(lopdf::Object::as_reference)
        .expect("page tree");
    doc.get_dictionary_mut(root)
        .expect("page tree dictionary")
        .set("Rotate", 90);
    let mut rotated = Vec::new();
    doc.save_to(&mut rotated).expect("saves");

    let dir = TempDir::new("inherit");
    let target = dir.0.join("out.pdf");
    let kept = [turned(0, 1), KeptPage::upright(1)];
    save_with_pages(&rotated, &kept, 5, &target).expect("save works");
    // 90 + 90 = 180 keeps the first page upright-shaped; the second stays at 90.
    assert_eq!(sizes_in(&target), [(612.0, 792.0), (792.0, 612.0)]);
}

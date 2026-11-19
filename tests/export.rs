//! Tests for exporting pages to PNG and TIFF.

mod common;

use common::make_pdf;
use pdf_light::document::open_pdf;
use pdf_light::error::PdfError;
use pdf_light::export::{ExportOptions, ImageFormat, existing_outputs, export_pages};
use pdf_light::pages::KeptPage;
use pdf_light::rotation::Rotation;
use std::fs::{self, File};
use std::io::BufReader;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use tiff::decoder::Decoder;

/// A temp folder unique to one test; removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pdf-light-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir is created");
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn opts(dir: &TempDir, format: ImageFormat, dpi: u32, multipage_tiff: bool) -> ExportOptions {
    ExportOptions {
        format,
        dpi,
        multipage_tiff,
        folder: dir.0.clone(),
        base_name: "doc".to_string(),
    }
}

fn file_count(dir: &TempDir) -> usize {
    fs::read_dir(&dir.0).expect("dir lists").count()
}

#[test]
fn export_png_should_match_dpi() {
    let dir = TempDir::new("png");
    let pdf = open_pdf(&Arc::new(make_pdf(&["a"]))).expect("opens");
    let options = opts(&dir, ImageFormat::Png, 144, false);
    let paths = export_pages(
        &pdf,
        &[(0, KeptPage::upright(0))],
        pdf.pages().len(),
        &options,
        &|_, _| true,
    )
    .expect("exports");
    assert_eq!(paths.len(), 1);
    let image = image::open(&paths[0]).expect("png decodes");
    assert_eq!((image.width(), image.height()), (1224, 1584));
}

#[test]
fn export_tiff_should_match_dpi() {
    let dir = TempDir::new("tiff");
    let pdf = open_pdf(&Arc::new(make_pdf(&["a"]))).expect("opens");
    let options = opts(&dir, ImageFormat::Tiff, 144, false);
    let paths = export_pages(
        &pdf,
        &[(0, KeptPage::upright(0))],
        pdf.pages().len(),
        &options,
        &|_, _| true,
    )
    .expect("exports");
    let file = BufReader::new(File::open(&paths[0]).expect("opens"));
    let mut decoder = Decoder::new(file).expect("tiff decodes");
    assert_eq!(decoder.dimensions().expect("size"), (1224, 1584));
}

#[test]
fn export_multipage_tiff_should_hold_every_page() {
    let dir = TempDir::new("multitiff");
    let pdf = open_pdf(&Arc::new(make_pdf(&["a", "b", "c"]))).expect("opens");
    let pages = [
        (0, KeptPage::upright(0)),
        (1, KeptPage::upright(1)),
        (2, KeptPage::upright(2)),
    ];
    let options = opts(&dir, ImageFormat::Tiff, 72, true);
    let paths =
        export_pages(&pdf, &pages, pdf.pages().len(), &options, &|_, _| true).expect("exports");
    assert_eq!(paths.len(), 1);
    let file = BufReader::new(File::open(&paths[0]).expect("opens"));
    let mut decoder = Decoder::new(file).expect("tiff decodes");
    let mut images = 1;
    while decoder.more_images() {
        decoder.next_image().expect("next image");
        images += 1;
    }
    assert_eq!(images, 3);
}

#[test]
fn export_should_remove_partial_files_when_cancelled() {
    let dir = TempDir::new("cancel");
    let pdf = open_pdf(&Arc::new(make_pdf(&["a", "b", "c"]))).expect("opens");
    let pages = [
        (0, KeptPage::upright(0)),
        (1, KeptPage::upright(1)),
        (2, KeptPage::upright(2)),
    ];
    let options = opts(&dir, ImageFormat::Png, 72, false);
    // Returns `false` once the first page is done.
    let result = export_pages(&pdf, &pages, pdf.pages().len(), &options, &|done, _| {
        done < 1
    });
    assert!(matches!(result, Err(PdfError::Cancelled)));
    assert_eq!(file_count(&dir), 0);
}

/// Runs an export whose progress callback panics after the first page, the way a
/// bug in the PDF library would unwind through `export_pages`.
fn export_that_panics(options: &ExportOptions) {
    let pdf = open_pdf(&Arc::new(make_pdf(&["a", "b", "c"]))).expect("opens");
    let pages = [
        (0, KeptPage::upright(0)),
        (1, KeptPage::upright(1)),
        (2, KeptPage::upright(2)),
    ];
    let result = catch_unwind(AssertUnwindSafe(|| {
        export_pages(&pdf, &pages, 3, options, &|_, _| panic!("test panic"))
    }));
    assert!(result.is_err(), "the panic reaches the caller");
}

#[test]
fn export_should_remove_partial_files_when_a_page_panics() {
    let dir = TempDir::new("panic-png");
    export_that_panics(&opts(&dir, ImageFormat::Png, 72, false));
    assert_eq!(file_count(&dir), 0);
}

#[test]
fn export_should_remove_a_partial_multipage_tiff_when_a_page_panics() {
    let dir = TempDir::new("panic-tiff");
    export_that_panics(&opts(&dir, ImageFormat::Tiff, 72, true));
    assert_eq!(file_count(&dir), 0);
}

#[test]
fn export_should_refuse_oversized_pages() {
    let dir = TempDir::new("toolarge");
    let pdf = open_pdf(&Arc::new(make_pdf(&["a", "b"]))).expect("opens");
    let options = opts(&dir, ImageFormat::Png, 7000, false);
    let result = export_pages(
        &pdf,
        &[(0, KeptPage::upright(0)), (1, KeptPage::upright(1))],
        pdf.pages().len(),
        &options,
        &|_, _| true,
    );
    assert!(matches!(result, Err(PdfError::TooLarge { .. })));
    assert_eq!(file_count(&dir), 0);
}

#[test]
fn existing_outputs_should_list_only_files_that_exist() {
    let dir = TempDir::new("existing");
    let present = dir.0.join("here.png");
    let absent = dir.0.join("gone.png");
    fs::write(&present, b"x").expect("writes");
    assert_eq!(existing_outputs(&[present.clone(), absent]), vec![present]);
}

#[test]
fn export_should_number_files_from_the_given_page_count() {
    // 1200 original pages with 800 kept: the dialog numbers by the 800, so the
    // library must too, or the overwrite prompt would check other names.
    let dir = TempDir::new("numbering");
    let pdf = open_pdf(&Arc::new(make_pdf(&["a"]))).expect("opens");
    let options = opts(&dir, ImageFormat::Png, 72, false);
    let expected = pdf_light::export::output_paths(&options, &[0], 800);
    let paths = export_pages(
        &pdf,
        &[(0, KeptPage::upright(1100))],
        800,
        &options,
        &|_, _| true,
    );
    // Original index 1100 doesn't exist in the one-page fixture, so use index 0.
    assert!(paths.is_err());
    let paths = export_pages(
        &pdf,
        &[(0, KeptPage::upright(0))],
        800,
        &options,
        &|_, _| true,
    )
    .expect("exports");
    assert_eq!(paths, expected);
    assert_eq!(existing_outputs(&expected), expected);
    assert!(paths[0].ends_with("doc_p001.png"));
    // With 1200 pages the same position would get four digits.
    let wide = pdf_light::export::output_paths(&options, &[0], 1200);
    assert!(wide[0].ends_with("doc_p0001.png"));
}

#[test]
fn export_png_should_turn_rotated_pages() {
    let dir = TempDir::new("png-turned");
    let pdf = open_pdf(&Arc::new(make_pdf(&["a"]))).expect("opens");
    let options = opts(&dir, ImageFormat::Png, 72, false);
    let page = KeptPage {
        index: 0,
        rotation: Rotation::quarter_turns(1),
    };
    let paths = export_pages(&pdf, &[(0, page)], 1, &options, &|_, _| true).expect("exports");
    let image = image::open(&paths[0]).expect("png decodes");
    assert_eq!((image.width(), image.height()), (792, 612));
}

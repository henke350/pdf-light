//! Opening PDF files and reading basic facts about them.

use crate::error::PdfError;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// An opened PDF: where it came from, its raw bytes, and each page's size.
pub struct Document {
    pub path: PathBuf,
    /// `Arc` is a shared, reference-counted pointer: worker threads can each
    /// hold a handle to the same bytes without copying them.
    pub bytes: Arc<Vec<u8>>,
    /// Page sizes in points (1/72 inch), with page rotation already applied.
    pub page_sizes: Vec<(f32, f32)>,
}

/// Reads the file at `path` and opens it as a PDF.
pub fn open(path: &Path) -> Result<Document, PdfError> {
    // `?` returns early with the error; `#[from]` converts io::Error to PdfError.
    let bytes = std::fs::read(path)?;
    from_bytes(path.to_path_buf(), bytes)
}

/// Opens a PDF from bytes already in memory; `path` is only remembered, not read.
pub fn from_bytes(path: PathBuf, bytes: Vec<u8>) -> Result<Document, PdfError> {
    let bytes = Arc::new(bytes);
    let pdf = open_pdf(&bytes)?;
    let page_sizes = pdf.pages().iter().map(|p| p.render_dimensions()).collect();
    Ok(Document {
        path,
        bytes,
        page_sizes,
    })
}

/// Parses the bytes with hayro. Each worker thread calls this once, because
/// hayro's render cache cannot be shared between threads.
pub fn open_pdf(bytes: &Arc<Vec<u8>>) -> Result<Pdf, PdfError> {
    Pdf::new(Arc::clone(bytes)).map_err(|e| match e {
        LoadPdfError::Decryption(_) => PdfError::Encrypted,
        LoadPdfError::Invalid => PdfError::Invalid,
    })
}

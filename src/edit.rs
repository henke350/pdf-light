//! Writing a copy of the PDF with some pages removed.

use crate::error::PdfError;
use std::path::{Path, PathBuf};

/// Removes a leftover temp file when dropped, unless `disarm` was called.
/// `Drop` runs automatically when a value goes out of scope, even on early `?` returns,
/// so every error path cleans up without repeating the code.
struct TempGuard {
    path: PathBuf,
    armed: bool,
}

impl TempGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Saves `original` to `target`, keeping only the pages in `kept` (0-based indices).
/// `page_count` is how many pages the viewer (hayro) found in `original`.
///
/// Writes a temp file next to `target`, flushes it to disk and renames it into
/// place, so the target is never left half-written.
pub fn save_with_pages(
    original: &[u8],
    kept: &[usize],
    page_count: usize,
    target: &Path,
) -> Result<(), PdfError> {
    let save_err = |e: lopdf::Error| PdfError::Save(e.to_string());

    let mut doc = lopdf::Document::load_mem(original).map_err(save_err)?;
    // The indices in `kept` come from hayro's reading of the file. If lopdf sees a
    // different number of pages, they may not mean the same pages here, so stop.
    if doc.get_pages().len() != page_count {
        return Err(PdfError::Save(
            "the PDF's page structure couldn't be read consistently, so it wasn't saved".into(),
        ));
    }
    // lopdf numbers pages from 1, in page order.
    let to_delete: Vec<u32> = doc
        .get_pages()
        .keys()
        .copied()
        .filter(|number| !kept.contains(&(*number as usize - 1)))
        .collect();
    doc.delete_pages(&to_delete);
    doc.prune_objects();

    let file_name = target
        .file_name()
        .ok_or_else(|| PdfError::Save("the save location has no file name".into()))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(".pdf-light.tmp");
    let temp_path = target.with_file_name(temp_name);

    let mut guard = TempGuard {
        path: temp_path.clone(),
        armed: true,
    };
    let file = doc.save(&temp_path)?; // writing fails with an io::Error
    // Make sure the bytes are on the disk before the rename makes them the
    // target; otherwise a power cut could leave an empty or partial file.
    file.sync_all()?;
    drop(file); // Close the file before renaming it.
    std::fs::rename(&temp_path, target)?;
    guard.disarm();
    Ok(())
}

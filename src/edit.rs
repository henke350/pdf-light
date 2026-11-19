//! Writing a copy of the PDF with some pages removed and some turned.

use crate::error::PdfError;
use crate::pages::KeptPage;
use lopdf::{Dictionary, Object, ObjectId};
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

/// Saves `original` to `target`, keeping only the pages in `kept` and turning
/// each by its `rotation` (added to the page's own `/Rotate`).
/// `page_count` is how many pages the viewer (hayro) found in `original`.
///
/// Writes a temp file next to `target`, flushes it to disk and renames it into
/// place, so the target is never left half-written.
pub fn save_with_pages(
    original: &[u8],
    kept: &[KeptPage],
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
    let pages = doc.get_pages();
    for page in kept.iter().filter(|page| page.rotation.degrees() != 0) {
        let id = u32::try_from(page.index + 1)
            .ok()
            .and_then(|number| pages.get(&number))
            .ok_or_else(|| PdfError::Save("a page to turn wasn't found in the file".into()))?;
        let degrees = (page_rotation(&doc, *id) + page.rotation.degrees()) % 360;
        doc.get_dictionary_mut(*id)
            .map_err(save_err)?
            .set("Rotate", Object::Integer(degrees));
    }
    let to_delete: Vec<u32> = pages
        .keys()
        .copied()
        .filter(|number| !kept.iter().any(|page| page.index == *number as usize - 1))
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

/// How far the page `id` is turned in the file: its `/Rotate`, or the nearest
/// parent's (pages inherit it), as 0, 90, 180 or 270. A value that isn't a
/// multiple of 90 counts as 0, the same as the viewer (hayro) reads it.
fn page_rotation(doc: &lopdf::Document, id: ObjectId) -> i64 {
    // How deep to follow `/Parent`: a guard against a loop in a broken file.
    const MAX_DEPTH: usize = 64;
    let mut dict: Option<&Dictionary> = doc.get_dictionary(id).ok();
    for _ in 0..MAX_DEPTH {
        let Some(current) = dict else {
            break;
        };
        if let Ok(rotate) = current.get(b"Rotate") {
            // Some writers store it as a real number such as 90.0.
            let degrees = rotate
                .as_i64()
                .ok()
                .or_else(|| rotate.as_float().ok().map(|f| f.round() as i64))
                .unwrap_or(0)
                .rem_euclid(360);
            return if degrees % 90 == 0 { degrees } else { 0 };
        }
        dict = current
            .get(b"Parent")
            .and_then(Object::as_reference)
            .and_then(|parent| doc.get_dictionary(parent))
            .ok();
    }
    0
}

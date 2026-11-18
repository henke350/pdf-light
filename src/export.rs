//! Exporting pages as PNG or TIFF images.

use crate::error::PdfError;
use crate::render::{self, pixel_size};
use hayro::hayro_syntax::Pdf;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::PathBuf;
use tiff::encoder::{Compression, TiffEncoder, colortype};

/// Above this many pixels per page the UI should warn before exporting.
pub const WARN_PIXELS: u64 = 100_000_000;

/// The image formats we can write. An `enum` is a type that is exactly one of its variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Tiff,
}

/// Everything the owner chose in the export dialog.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub format: ImageFormat,
    pub dpi: u32,
    /// For TIFF only: put every page in one file.
    pub multipage_tiff: bool,
    pub folder: PathBuf,
    pub base_name: String,
}

/// Parses a page range like `"1-3, 7"` (1-based) into ascending, unique 0-based positions.
/// The error text is meant to be shown to the owner.
pub fn parse_page_range(input: &str, page_count: usize) -> Result<Vec<usize>, String> {
    // A `BTreeSet` keeps its items sorted and drops duplicates for us.
    let mut positions = BTreeSet::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("Enter pages like 1-3, 7.".to_string());
        }
        let (start, end) = match part.split_once('-') {
            Some((a, b)) => (parse_number(a)?, parse_number(b)?),
            None => {
                let n = parse_number(part)?;
                (n, n)
            }
        };
        if start < 1 || end > page_count {
            return Err(format!("Pages must be between 1 and {page_count}."));
        }
        if start > end {
            return Err(format!("\"{part}\" runs backwards."));
        }
        positions.extend(start - 1..end);
    }
    Ok(positions.into_iter().collect())
}

fn parse_number(text: &str) -> Result<usize, String> {
    let text = text.trim();
    text.parse::<usize>()
        .map_err(|_| format!("\"{text}\" isn't a page number."))
}

/// The files an export would write for `positions` (0-based) of a `page_count`-page document.
pub fn output_paths(opts: &ExportOptions, positions: &[usize], page_count: usize) -> Vec<PathBuf> {
    let extension = match opts.format {
        ImageFormat::Png => "png",
        ImageFormat::Tiff => "tif",
    };
    if opts.format == ImageFormat::Tiff && opts.multipage_tiff {
        return vec![opts.folder.join(format!("{}.{extension}", opts.base_name))];
    }
    let width = page_count.to_string().len().max(3);
    positions
        .iter()
        .map(|&position| {
            let number = position + 1;
            opts.folder
                .join(format!("{}_p{number:0width$}.{extension}", opts.base_name))
        })
        .collect()
}

/// The paths from `paths` that already exist (an export would overwrite them).
pub fn existing_outputs(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter(|path| path.exists()).cloned().collect()
}

/// Exports `pages` (pairs of position and original page index) and returns the files written.
/// `page_count` is the page count of the document as the owner sees it (after deletions);
/// it decides the file-name padding, the same as the dialog's preview uses.
///
/// `progress(done, total)` runs after each page; returning `false` cancels the export,
/// removes every file this run wrote and gives `Err(Cancelled)`. `&dyn Fn(..) -> bool`
/// is a borrowed callback: any closure taking two numbers and returning a bool fits.
pub fn export_pages(
    pdf: &Pdf,
    pages: &[(usize, usize)],
    page_count: usize,
    opts: &ExportOptions,
    progress: &dyn Fn(usize, usize) -> bool,
) -> Result<Vec<PathBuf>, PdfError> {
    let scale = opts.dpi as f32 / 72.0;
    let all_pages = pdf.pages();

    // Check every page's size first, so a too-big page writes nothing at all.
    for &(_, index) in pages {
        let page = all_pages.get(index).ok_or(PdfError::Invalid)?;
        let (width, height) = pixel_size(page.render_dimensions(), scale);
        render::check_size(width, height)?;
    }

    let positions: Vec<usize> = pages.iter().map(|&(position, _)| position).collect();
    let paths = output_paths(opts, &positions, page_count);

    let mut written = Vec::new();
    match write_all(pdf, pages, opts, scale, &paths, &mut written, progress) {
        Ok(()) => Ok(written),
        Err(error) => {
            // Best effort: a file we can't remove is not worth hiding the real error.
            for path in &written {
                let _ = fs::remove_file(path);
            }
            Err(error)
        }
    }
}

/// Renders and writes every page, recording each created file in `written`.
fn write_all(
    pdf: &Pdf,
    pages: &[(usize, usize)],
    opts: &ExportOptions,
    scale: f32,
    paths: &[PathBuf],
    written: &mut Vec<PathBuf>,
    progress: &dyn Fn(usize, usize) -> bool,
) -> Result<(), PdfError> {
    let total = pages.len();
    let multipage = opts.format == ImageFormat::Tiff && opts.multipage_tiff;

    // The encoder lives in an `Option` so single-file formats can leave it empty.
    let mut encoder = None;
    if multipage && let Some(path) = paths.first() {
        let file = File::create(path)?;
        written.push(path.clone());
        let new = TiffEncoder::new(BufWriter::new(file))
            .map_err(|e| PdfError::Image(e.to_string()))?
            .with_compression(Compression::Lzw);
        encoder = Some(new);
    }

    for (done, &(_, index)) in pages.iter().enumerate() {
        let rendered = render::render_page(pdf, index, scale)?;
        // Drop the alpha channel: pages are opaque white-backed, so RGB loses nothing.
        let rgb: Vec<u8> = rendered
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| [px[0], px[1], px[2]])
            .collect();

        if let Some(encoder) = encoder.as_mut() {
            encoder
                .write_image::<colortype::RGB8>(rendered.width, rendered.height, &rgb)
                .map_err(|e| PdfError::Image(e.to_string()))?;
        } else {
            let path = paths.get(done).ok_or(PdfError::Invalid)?;
            written.push(path.clone());
            match opts.format {
                ImageFormat::Png => {
                    let image = image::RgbImage::from_raw(rendered.width, rendered.height, rgb)
                        .ok_or_else(|| PdfError::Image("bad pixel buffer".to_string()))?;
                    image
                        .save(path)
                        .map_err(|e| PdfError::Image(e.to_string()))?;
                }
                ImageFormat::Tiff => {
                    let file = BufWriter::new(File::create(path)?);
                    TiffEncoder::new(file)
                        .map_err(|e| PdfError::Image(e.to_string()))?
                        .with_compression(Compression::Lzw)
                        .write_image::<colortype::RGB8>(rendered.width, rendered.height, &rgb)
                        .map_err(|e| PdfError::Image(e.to_string()))?;
                }
            }
        }

        if !progress(done + 1, total) {
            return Err(PdfError::Cancelled);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(format: ImageFormat, multipage_tiff: bool) -> ExportOptions {
        ExportOptions {
            format,
            dpi: 150,
            multipage_tiff,
            folder: PathBuf::from("out"),
            base_name: "report".to_string(),
        }
    }

    #[test]
    fn parse_page_range_should_accept_ranges_and_singles() {
        assert_eq!(parse_page_range("1-3, 7", 10), Ok(vec![0, 1, 2, 6]));
    }

    #[test]
    fn parse_page_range_should_deduplicate() {
        assert_eq!(parse_page_range("2,2,1-2", 5), Ok(vec![0, 1]));
    }

    #[test]
    fn parse_page_range_should_reject_bad_input() {
        for input in ["", "0", "3-1", "11", "a"] {
            assert!(parse_page_range(input, 10).is_err(), "{input:?}");
        }
    }

    #[test]
    fn output_paths_should_pad_to_three_digits() {
        let paths = output_paths(&opts(ImageFormat::Png, false), &[0, 11], 20);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("out").join("report_p001.png"),
                PathBuf::from("out").join("report_p012.png"),
            ]
        );
    }

    #[test]
    fn output_paths_should_widen_padding_for_large_documents() {
        let paths = output_paths(&opts(ImageFormat::Png, false), &[0], 1200);
        assert_eq!(paths, vec![PathBuf::from("out").join("report_p0001.png")]);
    }

    #[test]
    fn output_paths_should_give_one_file_for_multipage_tiff() {
        let paths = output_paths(&opts(ImageFormat::Tiff, true), &[0, 1, 2], 3);
        assert_eq!(paths, vec![PathBuf::from("out").join("report.tif")]);
    }
}

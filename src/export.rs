//! Exporting pages as PNG or TIFF images.

use crate::error::PdfError;
use crate::render::{self, pixel_size};
use hayro::hayro_syntax::Pdf;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
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
/// removes every file this run wrote and gives `Err(Cancelled)`. Any other error, or a
/// panic, removes them too. `&dyn Fn(..) -> bool` is a borrowed callback: any closure
/// taking two numbers and returning a bool fits.
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

    let mut written = WrittenFiles::default();
    write_all(pdf, pages, opts, scale, &paths, &mut written, progress)?;
    Ok(written.keep())
}

/// The files an export has created so far. Unless `keep` is called, dropping it
/// removes them: after an error, a cancel, or even a panic unwinding through
/// `export_pages` (`Drop` runs then too), so no half-finished files stay behind.
#[derive(Default)]
struct WrittenFiles {
    paths: Vec<PathBuf>,
    kept: bool,
}

impl WrittenFiles {
    fn push(&mut self, path: &Path) {
        self.paths.push(path.to_path_buf());
    }

    /// The export finished: keep the files and hand back their paths.
    fn keep(mut self) -> Vec<PathBuf> {
        self.kept = true;
        // `mem::take` moves the list out and leaves an empty one in its place.
        std::mem::take(&mut self.paths)
    }
}

impl Drop for WrittenFiles {
    fn drop(&mut self) {
        if !self.kept {
            // Best effort: a file we can't remove is not worth hiding the real error.
            for path in &self.paths {
                let _ = fs::remove_file(path);
            }
        }
    }
}

/// One rendered page as RGB bytes, ready for an image encoder.
struct RgbPage {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

fn render_rgb(pdf: &Pdf, index: usize, scale: f32) -> Result<RgbPage, PdfError> {
    let rendered = render::render_page(pdf, index, scale)?;
    // Drop the alpha channel: pages are opaque white-backed, so RGB loses nothing.
    let rgb = rendered
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| [px[0], px[1], px[2]])
        .collect();
    Ok(RgbPage {
        width: rendered.width,
        height: rendered.height,
        rgb,
    })
}

fn image_error(error: impl std::fmt::Display) -> PdfError {
    PdfError::Image(error.to_string())
}

/// Renders and writes every page, recording each created file in `written`.
fn write_all(
    pdf: &Pdf,
    pages: &[(usize, usize)],
    opts: &ExportOptions,
    scale: f32,
    paths: &[PathBuf],
    written: &mut WrittenFiles,
    progress: &dyn Fn(usize, usize) -> bool,
) -> Result<(), PdfError> {
    let total = pages.len();
    // Called after each page; the owner may have pressed Cancel meanwhile.
    let report = |done: usize| {
        if progress(done, total) {
            Ok(())
        } else {
            Err(PdfError::Cancelled)
        }
    };

    if opts.format == ImageFormat::Tiff && opts.multipage_tiff {
        let path = paths.first().ok_or(PdfError::Invalid)?;
        let file = File::create(path)?;
        written.push(path);
        return write_tiff(file, |encoder| {
            for (done, &(_, index)) in pages.iter().enumerate() {
                write_tiff_page(encoder, &render_rgb(pdf, index, scale)?)?;
                report(done + 1)?;
            }
            Ok(())
        });
    }

    for (done, &(_, index)) in pages.iter().enumerate() {
        let page = render_rgb(pdf, index, scale)?;
        let path = paths.get(done).ok_or(PdfError::Invalid)?;
        written.push(path);
        match opts.format {
            ImageFormat::Png => {
                let image = image::RgbImage::from_raw(page.width, page.height, page.rgb)
                    .ok_or_else(|| PdfError::Image("bad pixel buffer".to_string()))?;
                image.save(path).map_err(image_error)?;
            }
            ImageFormat::Tiff => {
                write_tiff(File::create(path)?, |encoder| {
                    write_tiff_page(encoder, &page)
                })?;
            }
        }
        report(done + 1)?;
    }
    Ok(())
}

/// Writes a TIFF to `writer`: `write_pages` adds the pages through the encoder,
/// then the last buffered bytes are flushed.
///
/// `W: Write + Seek` means any writer that can also jump around (a `File`, or
/// an in-memory `Cursor` in the tests); the tiff encoder needs both.
fn write_tiff<W: Write + Seek>(
    writer: W,
    write_pages: impl FnOnce(&mut TiffEncoder<&mut BufWriter<W>>) -> Result<(), PdfError>,
) -> Result<(), PdfError> {
    let mut buffered = BufWriter::new(writer);
    let mut encoder = TiffEncoder::new(&mut buffered)
        .map_err(image_error)?
        .with_compression(Compression::Lzw);
    write_pages(&mut encoder)?;
    // `encoder` borrowed `buffered`, but it isn't used any more, so the borrow is
    // over and `buffered` is ours again. The tiff encoder never flushes, and a
    // `BufWriter` that is just dropped ignores write errors, so without this a
    // full disk would leave a cut-off file that looks like a success.
    buffered.flush()?;
    Ok(())
}

fn write_tiff_page<W: Write + Seek>(
    encoder: &mut TiffEncoder<W>,
    page: &RgbPage,
) -> Result<(), PdfError> {
    encoder
        .write_image::<colortype::RGB8>(page.width, page.height, &page.rgb)
        .map_err(image_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor, SeekFrom};

    /// A writer with room for only `room` bytes, like a nearly full disk.
    struct NearlyFullDisk {
        inner: Cursor<Vec<u8>>,
        room: u64,
    }

    impl Write for NearlyFullDisk {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.inner.position() + buf.len() as u64 > self.room {
                return Err(io::Error::other("disk full"));
            }
            self.inner.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Seek for NearlyFullDisk {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    fn white_page() -> RgbPage {
        RgbPage {
            width: 8,
            height: 8,
            rgb: vec![255; 8 * 8 * 3],
        }
    }

    /// Writes one white page as a TIFF onto a disk with `room` bytes.
    fn tiff_on_disk(room: u64) -> Result<u64, PdfError> {
        let mut disk = NearlyFullDisk {
            inner: Cursor::new(Vec::new()),
            room,
        };
        write_tiff(&mut disk, |encoder| write_tiff_page(encoder, &white_page()))?;
        Ok(disk.inner.get_ref().len() as u64)
    }

    #[test]
    fn write_tiff_should_report_a_disk_that_fills_up_at_the_end() {
        let size = tiff_on_disk(u64::MAX).expect("fits on a big disk");
        assert!(tiff_on_disk(size - 1).is_err());
    }

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

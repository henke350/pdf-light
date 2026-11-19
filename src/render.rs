//! Rendering one PDF page to an RGBA pixel buffer.

use crate::error::PdfError;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

/// Longest allowed side: hayro's canvas size is a `u16`, so more would wrap silently.
pub const MAX_SIDE_PX: u64 = 65_535;
/// Largest allowed pixel count (about 1 GB of RGBA).
pub const MAX_PIXELS: u64 = 250_000_000;

/// A rendered page: straight RGBA8 on a white background.
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The pixel size of a page of `page_size` points at `scale`, rounded down.
pub fn pixel_size(page_size: (f32, f32), scale: f32) -> (u64, u64) {
    // `as u64` saturates, so negative or NaN values become 0 rather than wrapping.
    (
        (page_size.0 * scale).floor() as u64,
        (page_size.1 * scale).floor() as u64,
    )
}

/// Fails with `TooLarge` when a `width` × `height` canvas is over the limits.
pub(crate) fn check_size(width: u64, height: u64) -> Result<(), PdfError> {
    if width > MAX_SIDE_PX || height > MAX_SIDE_PX || width.saturating_mul(height) > MAX_PIXELS {
        return Err(PdfError::TooLarge { width, height });
    }
    Ok(())
}

/// Renders page `index` at `scale` (1.0 = 72 dpi). Refuses sizes that are too big.
pub fn render_page(pdf: &Pdf, index: usize, scale: f32) -> Result<RenderedPage, PdfError> {
    let page = pdf.pages().get(index).ok_or(PdfError::Invalid)?;
    let (width, height) = pixel_size(page.render_dimensions(), scale);
    // Check before rendering: hayro would silently truncate oversized canvases.
    check_size(width, height)?;

    // A fresh cache per call: `RenderCache` is not `Send`, so it can't be shared.
    let cache = RenderCache::new();
    let pixmap = hayro::render(
        page,
        &cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        },
    );
    // On an opaque white background, premultiplied alpha equals straight alpha.
    Ok(RenderedPage {
        width: u32::from(pixmap.width()),
        height: u32::from(pixmap.height()),
        rgba: pixmap.data_as_u8_slice().to_vec(),
    })
}

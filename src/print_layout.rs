//! Where a page goes on the printer's paper, and the pixel work to get it
//! there. Pure: no Windows calls, so it is tested like any other library code.

use crate::render::{MAX_PIXELS, MAX_SIDE_PX};

/// Pages are never rendered finer than this for printing.
const MAX_RENDER_DPI: u32 = 600;

/// Where one page lands on the paper, in printer pixels, inside the printable area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Turn the page 90° (a landscape page on portrait paper, or the other way round).
    pub rotate: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Places a page of `page_size_pt` points on a printable area of `printable_px`
/// printer pixels at `printer_dpi`: turned when the orientations differ, shrunk
/// to fit (never enlarged past 100 %), and centred.
pub fn place(
    page_size_pt: (f32, f32),
    printable_px: (i32, i32),
    printer_dpi: (i32, i32),
) -> Placement {
    let (page_w, page_h) = page_size_pt;
    let (area_w, area_h) = printable_px;
    // A square page or square paper has no orientation, so it's never turned.
    let rotate = (page_w > page_h && area_w < area_h) || (page_w < page_h && area_w > area_h);
    let (page_w, page_h) = if rotate {
        (page_h, page_w)
    } else {
        (page_w, page_h)
    };
    // The page at 100 %, in printer pixels.
    let full_w = page_w * printer_dpi.0 as f32 / 72.0;
    let full_h = page_h * printer_dpi.1 as f32 / 72.0;
    // Shrink to fit, never enlarge.
    let scale = (area_w as f32 / full_w)
        .min(area_h as f32 / full_h)
        .min(1.0);
    // `as i32` saturates, so a NaN or negative size becomes 0 instead of wrapping.
    let width = ((full_w * scale).round() as i32).clamp(0, area_w.max(0));
    let height = ((full_h * scale).round() as i32).clamp(0, area_h.max(0));
    Placement {
        rotate,
        x: (area_w - width) / 2,
        y: (area_h - height) / 2,
        width,
        height,
    }
}

/// The resolution pages are rendered at for a printer of `printer_dpi`: the
/// printer's own, capped at 600 DPI (more is slow and no one can see it).
pub fn render_dpi(printer_dpi: i32) -> u32 {
    // `clamp` keeps a broken driver's 0 or negative DPI from becoming a zero-size render.
    printer_dpi.clamp(1, MAX_RENDER_DPI as i32).unsigned_abs()
}

/// The scale (1.0 = 72 DPI) to render a page of `page_size_pt` at for `placement`:
/// sharp enough for the printer, never finer than `render_dpi` on paper, and
/// within the renderer's size limits.
pub fn render_scale(
    page_size_pt: (f32, f32),
    placement: &Placement,
    printer_dpi: (i32, i32),
) -> f32 {
    let (page_w, page_h) = page_size_pt;
    // The placement is on the paper; the page is rendered unturned.
    let (target_w, target_h) = if placement.rotate {
        (placement.height, placement.width)
    } else {
        (placement.width, placement.height)
    };
    // Printer pixels per point, as placed.
    let device = (target_w as f32 / page_w).min(target_h as f32 / page_h);
    // Rendering finer than `render_dpi` on paper only costs time and memory.
    let dpi = printer_dpi.0.max(printer_dpi.1);
    let mut scale = device * (render_dpi(dpi) as f32 / dpi.max(1) as f32).min(1.0);
    // Within the renderer's limits: shrink rather than refuse a huge poster.
    let (width, height) = (page_w * scale, page_h * scale);
    let pixels = width * height;
    if pixels > MAX_PIXELS as f32 {
        scale *= (MAX_PIXELS as f32 / pixels).sqrt() * 0.999;
    }
    let longest = page_w.max(page_h) * scale;
    if longest > MAX_SIDE_PX as f32 {
        scale *= MAX_SIDE_PX as f32 / longest * 0.999;
    }
    // A broken page size (0 or NaN) gives a scale the renderer turns into an error.
    if scale.is_finite() { scale } else { 0.0 }
}

/// A 24-bit device-independent bitmap (DIB), the format GDI prints: blue,
/// green, red per pixel, rows top-down, each row padded to a multiple of 4 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dib {
    pub width: u32,
    pub height: u32,
    /// Bytes per row, including the padding.
    pub stride: usize,
    pub bgr: Vec<u8>,
}

/// Converts straight RGBA (on white, so alpha is ignored) to a DIB, turned 90°
/// anticlockwise when `rotate` (the page's top ends up on the paper's left edge).
/// `None` when `rgba` doesn't hold `width` × `height` pixels.
pub fn bgr_dib(rgba: &[u8], width: u32, height: u32, rotate: bool) -> Option<Dib> {
    let (w, h) = (width as usize, height as usize);
    if rgba.len() != w.checked_mul(h)?.checked_mul(4)? {
        return None;
    }
    let (out_w, out_h) = if rotate { (h, w) } else { (w, h) };
    // Each row is 3 bytes per pixel, rounded up to a multiple of 4.
    let stride = (out_w * 3).div_ceil(4) * 4;
    let mut bgr = vec![0; stride * out_h];
    // `chunks_exact_mut` hands out one output row at a time, so no index can run past the end.
    for (y, row) in bgr.chunks_exact_mut(stride).enumerate() {
        for (x, pixel) in row
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .take(out_w)
            .enumerate()
        {
            // Turned anticlockwise, output (x, y) shows source (w - 1 - y, x).
            let (src_x, src_y) = if rotate { (w - 1 - y, x) } else { (x, y) };
            let at = (src_y * w + src_x) * 4;
            if let Some(&[r, g, b, _]) = rgba.get(at..at + 4) {
                *pixel = [b, g, r];
            }
        }
    }
    Some(Dib {
        width: u32::try_from(out_w).ok()?,
        height: u32::try_from(out_h).ok()?,
        stride,
        bgr,
    })
}

/// One strip of a DIB sent to the printer by itself; big bitmaps are sent in
/// strips because some printer drivers refuse a single huge one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    /// First DIB row in this strip, and how many rows it has.
    pub row: u32,
    pub rows: u32,
    /// Where the strip goes on the paper (top and height, in printer pixels).
    pub dest_y: i32,
    pub dest_height: i32,
}

/// Splits `height` DIB rows, drawn at `dest_y` with `dest_height` printer pixels,
/// into strips of at most `max_rows` rows. The strips meet exactly, so the
/// page has no gaps or overlaps.
pub fn bands(height: u32, dest_y: i32, dest_height: i32, max_rows: u32) -> Vec<Band> {
    // Where row `row` starts on the paper. Every strip uses the same formula for
    // its top and bottom, so neighbours share an edge. `i64` can't overflow here.
    let edge = |row: u32| {
        let offset = i64::from(row) * i64::from(dest_height) / i64::from(height.max(1));
        dest_y.saturating_add(i32::try_from(offset).unwrap_or(i32::MAX))
    };
    let step = max_rows.max(1);
    (0..height)
        .step_by(step as usize)
        .map(|row| {
            let rows = step.min(height - row);
            let (top, bottom) = (edge(row), edge(row + rows));
            Band {
                row,
                rows,
                dest_y: top,
                dest_height: bottom - top,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_should_fit_and_centre_portrait_on_portrait() {
        let placement = place((612.0, 792.0), (4800, 6000), (600, 600));
        assert!(!placement.rotate);
        assert!((placement.width - 4636).abs() <= 1, "{placement:?}");
        assert!((placement.height - 6000).abs() <= 1, "{placement:?}");
        assert_eq!(
            (placement.x, placement.y),
            ((4800 - placement.width) / 2, 0)
        );
    }

    #[test]
    fn place_should_rotate_landscape_page_on_portrait_paper() {
        assert!(place((792.0, 612.0), (4800, 6000), (600, 600)).rotate);
    }

    #[test]
    fn place_should_not_enlarge_small_pages() {
        assert_eq!(
            place((100.0, 100.0), (4800, 6000), (600, 600)),
            Placement {
                rotate: false,
                x: (4800 - 833) / 2,
                y: (6000 - 833) / 2,
                width: 833,
                height: 833,
            }
        );
    }

    #[test]
    fn render_dpi_should_cap_at_600() {
        assert_eq!([render_dpi(1200), render_dpi(300)], [600, 300]);
    }

    #[test]
    fn render_scale_should_match_the_placement_at_600_dpi() {
        let placement = place((612.0, 792.0), (4800, 6000), (600, 600));
        // 6000 px tall for 792 pt.
        let scale = render_scale((612.0, 792.0), &placement, (600, 600));
        assert!((scale - 6000.0 / 792.0).abs() < 0.01, "{scale}");
    }

    #[test]
    fn render_scale_should_cap_the_bitmap_at_600_dpi_on_paper() {
        // A 1200 DPI printer: the page is placed at 100 %, but rendered at 600 DPI.
        let placement = place((100.0, 100.0), (9600, 12000), (1200, 1200));
        let scale = render_scale((100.0, 100.0), &placement, (1200, 1200));
        assert!((scale - 600.0 / 72.0).abs() < 0.01, "{scale}");
    }

    #[test]
    fn render_scale_should_use_the_unturned_page_for_a_rotated_placement() {
        let placement = place((792.0, 612.0), (4800, 6000), (600, 600));
        // Turned, the page's 792 pt width runs down the paper's 6000 px (or less).
        let scale = render_scale((792.0, 612.0), &placement, (600, 600));
        let expected = f32::min(
            placement.height as f32 / 792.0,
            placement.width as f32 / 612.0,
        );
        assert!((scale - expected).abs() < 0.01, "{scale} vs {expected}");
    }

    #[test]
    fn render_scale_should_stay_within_the_render_limits() {
        // A huge poster on a huge plotter: shrink the render, don't fail.
        let placement = place((20_000.0, 20_000.0), (40_000, 40_000), (600, 600));
        let scale = render_scale((20_000.0, 20_000.0), &placement, (600, 600));
        let side = (20_000.0 * scale) as u64;
        assert!(side * side <= MAX_PIXELS && side <= MAX_SIDE_PX, "{side}");
    }

    /// 2 × 2 RGBA: red, green / blue, white (alpha ignored).
    const RGBA_2X2: [u8; 16] = [
        255, 0, 0, 255, 0, 255, 0, 255, //
        0, 0, 255, 255, 255, 255, 255, 255,
    ];

    #[test]
    fn bgr_dib_should_swap_channels_and_pad_rows_to_four_bytes() {
        let dib = bgr_dib(&RGBA_2X2, 2, 2, false);
        assert_eq!(
            dib,
            Some(Dib {
                width: 2,
                height: 2,
                stride: 8,
                bgr: vec![
                    0, 0, 255, 0, 255, 0, 0, 0, //
                    255, 0, 0, 255, 255, 255, 0, 0,
                ],
            })
        );
    }

    #[test]
    fn bgr_dib_should_turn_anticlockwise() {
        // 3 wide × 1 tall: A B C. Turned anticlockwise it is 1 wide × 3 tall:
        // C on top, A at the bottom.
        let rgba = [1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255];
        let dib = bgr_dib(&rgba, 3, 1, true);
        assert_eq!(
            dib,
            Some(Dib {
                width: 1,
                height: 3,
                stride: 4,
                bgr: vec![0, 0, 3, 0, 0, 0, 2, 0, 0, 0, 1, 0],
            })
        );
    }

    #[test]
    fn bgr_dib_should_turn_the_top_left_corner_to_the_bottom_left() {
        let dib = bgr_dib(&RGBA_2X2, 2, 2, true);
        // Red (top left) goes to the bottom left; green (top right) to the top left.
        let bgr = dib.map(|d| d.bgr).unwrap_or_default();
        assert_eq!(
            bgr,
            vec![
                0, 255, 0, 255, 255, 255, 0, 0, //
                0, 0, 255, 255, 0, 0, 0, 0,
            ]
        );
    }

    #[test]
    fn bgr_dib_should_refuse_a_buffer_of_the_wrong_size() {
        assert_eq!(bgr_dib(&RGBA_2X2, 3, 2, false), None);
    }

    #[test]
    fn bands_should_cover_every_row_and_meet_exactly() {
        let bands = bands(10, 100, 25, 4);
        assert_eq!(
            bands,
            vec![
                Band {
                    row: 0,
                    rows: 4,
                    dest_y: 100,
                    dest_height: 10
                },
                Band {
                    row: 4,
                    rows: 4,
                    dest_y: 110,
                    dest_height: 10
                },
                Band {
                    row: 8,
                    rows: 2,
                    dest_y: 120,
                    dest_height: 5
                },
            ]
        );
    }

    #[test]
    fn bands_should_send_a_small_bitmap_in_one_piece() {
        assert_eq!(
            bands(10, 0, 30, 100),
            vec![Band {
                row: 0,
                rows: 10,
                dest_y: 0,
                dest_height: 30
            }]
        );
    }
}

//! pdf-light's own icon, drawn in code: a red page with a folded corner and
//! "PDF" in white.
//!
//! The window icon is drawn from here at start-up. The exe's icon (what
//! Explorer and the taskbar show) is `assets/pdf-light.ico`, written from here
//! by `cargo run --example make_icon` and built into the exe by `build.rs`.

use crate::error::PdfError;
// `ImageEncoder` is the trait that provides the PNG encoder's `write_image`.
use image::ImageEncoder;

/// The sizes in `assets/pdf-light.ico`: what Windows asks for at the usual
/// display scales, from small list views up to large tiles.
pub const ICO_SIZES: [u32; 7] = [16, 20, 24, 32, 48, 64, 256];

/// The page's red, its folded corner (the paper's back), and the letters.
const RED: [u8; 3] = [212, 32, 39];
const FOLD: [u8; 3] = [246, 150, 150];
const WHITE: [u8; 3] = [255, 255, 255];

/// The page, as fractions of the icon (origin top-left, y down).
const PAGE_LEFT: f32 = 0.12;
const PAGE_RIGHT: f32 = 0.88;
const PAGE_TOP: f32 = 0.04;
const PAGE_BOTTOM: f32 = 0.96;
const CORNER_RADIUS: f32 = 0.06;
/// How far the folded corner reaches along the top and right edges.
const FOLD_SIZE: f32 = 0.26;

/// "PDF" on a grid of 3 × 5 cells per letter, one empty column between letters.
const LETTERS: [[&str; 5]; 3] = [
    ["###", "#.#", "###", "#..", "#.."],
    ["##.", "#.#", "#.#", "#.#", "##."],
    ["###", "#..", "##.", "#..", "#.."],
];
/// One grid cell's side, as a fraction of the icon.
const CELL: f32 = 0.062;
/// Where the text's top edge sits.
const TEXT_TOP: f32 = 0.48;

/// Samples per pixel along each axis; 4 × 4 samples give smooth edges.
const SUPERSAMPLE: u32 = 4;

/// The icon as straight (not premultiplied) RGBA, `size` × `size` pixels.
pub fn rgba(size: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    let samples = (SUPERSAMPLE * SUPERSAMPLE) as f32;
    for y in 0..size {
        for x in 0..size {
            // Add up each colour weighted by how many samples it covers.
            let mut sum = [0.0f32; 3];
            let mut covered = 0.0f32;
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let u = (x as f32 + (sx as f32 + 0.5) / SUPERSAMPLE as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SUPERSAMPLE as f32) / size as f32;
                    if let Some(colour) = colour_at(u, v) {
                        for (total, channel) in sum.iter_mut().zip(colour) {
                            *total += f32::from(channel);
                        }
                        covered += 1.0;
                    }
                }
            }
            if covered == 0.0 {
                pixels.extend([0, 0, 0, 0]);
            } else {
                // Straight alpha: the colour is the average of the covered samples.
                pixels.extend(sum.map(|total| (total / covered).round() as u8));
                pixels.push((covered / samples * 255.0).round() as u8);
            }
        }
    }
    pixels
}

/// The colour at the point `(u, v)` of the icon, or `None` where it's transparent.
fn colour_at(u: f32, v: f32) -> Option<[u8; 3]> {
    if !inside_rounded_page(u, v) {
        return None;
    }
    // Distance from the top-right corner along both edges: below `FOLD_SIZE`
    // the corner is cut away, just above it is the folded-over flap.
    let from_corner = (PAGE_RIGHT - u) + (v - PAGE_TOP);
    if from_corner < FOLD_SIZE {
        return None;
    }
    let in_flap = u >= PAGE_RIGHT - FOLD_SIZE && v <= PAGE_TOP + FOLD_SIZE;
    if in_flap {
        return Some(FOLD);
    }
    Some(if on_letter(u, v) { WHITE } else { RED })
}

/// Inside the page rectangle with its corners rounded (the folded corner is
/// cut away separately).
fn inside_rounded_page(u: f32, v: f32) -> bool {
    if !(PAGE_LEFT..=PAGE_RIGHT).contains(&u) || !(PAGE_TOP..=PAGE_BOTTOM).contains(&v) {
        return false;
    }
    // The nearest point of the rectangle shrunk by the radius; outside a
    // corner's circle means outside the rounded page.
    let cx = u.clamp(PAGE_LEFT + CORNER_RADIUS, PAGE_RIGHT - CORNER_RADIUS);
    let cy = v.clamp(PAGE_TOP + CORNER_RADIUS, PAGE_BOTTOM - CORNER_RADIUS);
    (u - cx).powi(2) + (v - cy).powi(2) <= CORNER_RADIUS.powi(2)
}

/// Whether `(u, v)` falls on a filled cell of the "PDF" letters.
fn on_letter(u: f32, v: f32) -> bool {
    // 3 letters of 3 columns, with 2 gaps between them.
    let columns = (LETTERS.len() * 4 - 1) as f32;
    let left = 0.5 - columns * CELL / 2.0;
    let (column, row) = ((u - left) / CELL, (v - TEXT_TOP) / CELL);
    if column < 0.0 || row < 0.0 {
        return false;
    }
    let (column, row) = (column as usize, row as usize);
    let (letter, x) = (column / 4, column % 4);
    LETTERS
        .get(letter)
        .and_then(|rows| rows.get(row))
        .and_then(|cells| cells.as_bytes().get(x))
        .is_some_and(|&cell| cell == b'#')
}

/// A Windows `.ico` file with the icon at each of `sizes` (1 to 256 pixels),
/// each stored as a PNG, which every Windows since Vista reads.
pub fn ico(sizes: &[u32]) -> Result<Vec<u8>, PdfError> {
    let image_error = |e: image::ImageError| PdfError::Image(e.to_string());
    let mut images = Vec::with_capacity(sizes.len());
    for &size in sizes {
        if !(1..=256).contains(&size) {
            return Err(PdfError::Image(format!("an icon can't be {size} pixels")));
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&rgba(size), size, size, image::ExtendedColorType::Rgba8)
            .map_err(image_error)?;
        images.push((size, png));
    }

    // The header (6 bytes), one 16-byte entry per image, then the images.
    let count = u16::try_from(images.len())
        .map_err(|_| PdfError::Image("too many icon sizes".to_owned()))?;
    let mut ico = Vec::new();
    ico.extend(0u16.to_le_bytes()); // reserved
    ico.extend(1u16.to_le_bytes()); // 1 = icon
    ico.extend(count.to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (size, png) in &images {
        // 256 doesn't fit in a byte, so the format writes it as 0.
        let side = if *size == 256 { 0 } else { *size as u8 };
        ico.extend([side, side, 0, 0]); // width, height, palette size, reserved
        ico.extend(1u16.to_le_bytes()); // colour planes
        ico.extend(32u16.to_le_bytes()); // bits per pixel
        ico.extend((png.len() as u32).to_le_bytes());
        ico.extend((offset as u32).to_le_bytes());
        offset += png.len();
    }
    for (_, png) in images {
        ico.extend(png);
    }
    Ok(ico)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RGBA of the pixel at `(x, y)` of a `size`-pixel icon.
    fn pixel(icon: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * size + x) * 4) as usize;
        [icon[at], icon[at + 1], icon[at + 2], icon[at + 3]]
    }

    #[test]
    fn rgba_should_be_size_squared_pixels() {
        assert_eq!(rgba(32).len(), 32 * 32 * 4);
    }

    #[test]
    fn rgba_should_draw_a_red_page_on_transparent() {
        let icon = rgba(64);
        assert_eq!(
            (pixel(&icon, 64, 0, 0)[3], pixel(&icon, 64, 12, 56)),
            (0, [RED[0], RED[1], RED[2], 255])
        );
    }

    #[test]
    fn rgba_should_cut_the_top_right_corner_and_fold_it() {
        let icon = rgba(64);
        // Right at the corner: cut away. A little further in: the flap.
        assert_eq!(
            (pixel(&icon, 64, 55, 3)[3], pixel(&icon, 64, 44, 14)),
            (0, [FOLD[0], FOLD[1], FOLD[2], 255])
        );
    }

    #[test]
    fn rgba_should_write_pdf_in_white() {
        // The first cell of "P" (top-left of the text).
        let size = 256;
        let x = ((0.5 - 11.0 * CELL / 2.0 + CELL / 2.0) * size as f32) as u32;
        let y = ((TEXT_TOP + CELL / 2.0) * size as f32) as u32;
        assert_eq!(pixel(&rgba(size), size, x, y), [255, 255, 255, 255]);
    }

    #[test]
    fn ico_should_list_every_size() {
        let ico = ico(&[16, 256]).expect("ico is made");
        // Count, then the first entry's width and the second's (256 is written as 0).
        assert_eq!((ico[4], ico[6], ico[6 + 16]), (2, 16, 0));
        // The first image starts right after the header and both entries.
        assert_eq!(&ico[38..42], b"\x89PNG");
    }

    #[test]
    fn assets_icon_should_match_the_drawing() {
        // If this fails, the drawing changed: run `cargo run --example make_icon`.
        let saved = include_bytes!("../assets/pdf-light.ico");
        let drawn = ico(&ICO_SIZES).expect("ico is made");
        assert!(
            saved[..] == drawn[..],
            "assets/pdf-light.ico is out of date"
        );
    }

    #[test]
    fn ico_should_refuse_sizes_the_format_cannot_hold() {
        assert!(ico(&[300]).is_err());
    }
}

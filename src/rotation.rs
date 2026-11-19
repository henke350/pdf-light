//! Turning pages by quarter turns: page sizes, positions on the page, and
//! rendered images.
//!
//! A `Rotation` is the turn the owner added on top of the page's own rotation
//! in the file (hayro already applies that one). Page images are always
//! rendered upright; the page view and the pages panel draw them turned, and
//! export, print and save apply the turn for real.

use crate::render::RenderedPage;
use crate::text::PageRect;

/// A clockwise turn of 0, 90, 180 or 270 degrees.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Rotation {
    /// Clockwise quarter turns, always 0 to 3.
    quarter_turns: u8,
}

impl Rotation {
    /// No turn at all.
    pub const NONE: Rotation = Rotation { quarter_turns: 0 };

    /// `turns` clockwise quarter turns; negative is anticlockwise.
    pub fn quarter_turns(turns: i32) -> Self {
        // `rem_euclid` keeps the result in 0..4, also for negative numbers.
        Self {
            quarter_turns: turns.rem_euclid(4) as u8,
        }
    }

    /// The turn in degrees: 0, 90, 180 or 270.
    pub fn degrees(self) -> i64 {
        i64::from(self.quarter_turns) * 90
    }

    /// This turn plus a quarter turn clockwise.
    pub fn clockwise(self) -> Self {
        Self::quarter_turns(i32::from(self.quarter_turns) + 1)
    }

    /// This turn plus a quarter turn anticlockwise.
    pub fn anticlockwise(self) -> Self {
        Self::quarter_turns(i32::from(self.quarter_turns) - 1)
    }

    /// Whether the page's width and height swap (a 90 or 270 degree turn).
    pub fn swaps_sides(self) -> bool {
        self.quarter_turns % 2 == 1
    }

    /// The size of a page of `size` (width, height) once turned.
    /// Turning twice by a swapping turn gives the original size back, so this
    /// also turns a shown size back into the upright one.
    pub fn size(self, (width, height): (f32, f32)) -> (f32, f32) {
        if self.swaps_sides() {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// Where the point `(u, v)` of an upright page ends up on the turned page.
    /// Both are fractions of the page (0 to 1), origin top-left, y down.
    pub fn fraction(self, (u, v): (f32, f32)) -> (f32, f32) {
        match self.quarter_turns {
            1 => (1.0 - v, u),
            2 => (1.0 - u, 1.0 - v),
            3 => (v, 1.0 - u),
            _ => (u, v),
        }
    }

    /// `rect` (points, on an upright page of `size` points) on the turned page.
    pub fn rect(self, rect: PageRect, (width, height): (f32, f32)) -> PageRect {
        let PageRect { x0, y0, x1, y1 } = rect;
        match self.quarter_turns {
            1 => PageRect {
                x0: height - y1,
                y0: x0,
                x1: height - y0,
                y1: x1,
            },
            2 => PageRect {
                x0: width - x1,
                y0: height - y1,
                x1: width - x0,
                y1: height - y0,
            },
            3 => PageRect {
                x0: y0,
                y0: width - x1,
                x1: y1,
                y1: width - x0,
            },
            _ => rect,
        }
    }

    /// The rendered `page` turned. Pixels are moved, not resampled, so nothing
    /// gets blurrier. A rendered page that isn't width × height × 4 bytes long
    /// is handed back unchanged.
    pub fn image(self, page: RenderedPage) -> RenderedPage {
        let (width, height) = (page.width as usize, page.height as usize);
        if self.quarter_turns == 0 || page.rgba.len() != width * height * 4 {
            return page;
        }
        let (out_width, out_height) = if self.swaps_sides() {
            (height, width)
        } else {
            (width, height)
        };
        let mut rgba = vec![0; page.rgba.len()];
        for out_y in 0..out_height {
            for out_x in 0..out_width {
                // The source pixel for this output pixel: `fraction` run backwards.
                let (x, y) = match self.quarter_turns {
                    1 => (out_y, height - 1 - out_x),
                    2 => (width - 1 - out_x, height - 1 - out_y),
                    _ => (width - 1 - out_y, out_x),
                };
                let from = (y * width + x) * 4;
                let to = (out_y * out_width + out_x) * 4;
                rgba[to..to + 4].copy_from_slice(&page.rgba[from..from + 4]);
            }
        }
        RenderedPage {
            width: out_width as u32,
            height: out_height as u32,
            rgba,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CW: Rotation = Rotation { quarter_turns: 1 };
    const HALF: Rotation = Rotation { quarter_turns: 2 };
    const CCW: Rotation = Rotation { quarter_turns: 3 };

    #[test]
    fn quarter_turns_should_wrap_both_ways() {
        assert_eq!(
            [
                Rotation::quarter_turns(5),
                Rotation::quarter_turns(-1),
                Rotation::NONE.anticlockwise(),
                CCW.clockwise()
            ],
            [CW, CCW, CCW, Rotation::NONE]
        );
    }

    #[test]
    fn degrees_should_count_clockwise() {
        assert_eq!(
            [CW.degrees(), HALF.degrees(), CCW.degrees()],
            [90, 180, 270]
        );
    }

    #[test]
    fn size_should_swap_only_for_quarter_turns() {
        assert_eq!(
            [CW.size((1.0, 2.0)), HALF.size((1.0, 2.0))],
            [(2.0, 1.0), (1.0, 2.0)]
        );
    }

    #[test]
    fn fraction_should_move_the_top_left_corner_clockwise() {
        // A clockwise turn takes the top-left corner to the top right, and so on.
        assert_eq!(
            [
                CW.fraction((0.0, 0.0)),
                HALF.fraction((0.0, 0.0)),
                CCW.fraction((0.0, 0.0))
            ],
            [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
        );
    }

    #[test]
    fn rect_should_agree_with_fraction() {
        // A 1 × 1 rect near the top-left of a 100 × 200 page.
        let rect = PageRect {
            x0: 10.0,
            y0: 20.0,
            x1: 11.0,
            y1: 21.0,
        };
        for rotation in [Rotation::NONE, CW, HALF, CCW] {
            let turned = rotation.rect(rect, (100.0, 200.0));
            let (w, h) = rotation.size((100.0, 200.0));
            // The rect's centre, as a fraction, must land where `fraction` says.
            let (u, v) = rotation.fraction((10.5 / 100.0, 20.5 / 200.0));
            let centre = ((turned.x0 + turned.x1) / 2.0, (turned.y0 + turned.y1) / 2.0);
            let off = (centre.0 - u * w).abs() + (centre.1 - v * h).abs();
            assert!(
                off < 1e-3,
                "rotation {rotation:?}: {centre:?} vs {:?}",
                (u * w, v * h)
            );
        }
    }

    /// A 3 × 2 image whose red channel numbers the pixels 0..6, row by row.
    fn numbered() -> RenderedPage {
        RenderedPage {
            width: 3,
            height: 2,
            rgba: (0..6).flat_map(|n| [n, 0, 0, 255]).collect(),
        }
    }

    fn reds(page: &RenderedPage) -> Vec<u8> {
        page.rgba.iter().step_by(4).copied().collect()
    }

    #[test]
    fn image_should_turn_pixels_clockwise() {
        // 0 1 2      3 0
        // 3 4 5  ->  4 1
        //            5 2
        let turned = CW.image(numbered());
        assert_eq!(
            (turned.width, turned.height, reds(&turned)),
            (2, 3, vec![3, 0, 4, 1, 5, 2])
        );
    }

    #[test]
    fn image_should_turn_pixels_half_way_and_anticlockwise() {
        let half = HALF.image(numbered());
        let ccw = CCW.image(numbered());
        assert_eq!(
            (reds(&half), (ccw.width, ccw.height), reds(&ccw)),
            (vec![5, 4, 3, 2, 1, 0], (2, 3), vec![2, 5, 1, 4, 0, 3])
        );
    }

    #[test]
    fn image_should_leave_a_malformed_buffer_alone() {
        let page = RenderedPage {
            width: 3,
            height: 3,
            rgba: vec![0; 4],
        };
        assert_eq!(CW.image(page).width, 3);
    }
}

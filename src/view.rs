//! Page layout and zoom math for the reading view, in logical pixels.
//!
//! No UI types here, so everything can be unit-tested.

use std::ops::Range;

/// Smallest zoom, in percent.
pub const MIN_ZOOM: f32 = 25.0;
/// Largest zoom, in percent.
pub const MAX_ZOOM: f32 = 800.0;

/// Zoom levels the + and − buttons step through, in percent.
const ZOOM_PRESETS: [f32; 18] = [
    25.0, 33.0, 50.0, 67.0, 75.0, 90.0, 100.0, 110.0, 125.0, 150.0, 175.0, 200.0, 250.0, 300.0,
    400.0, 500.0, 600.0, 800.0,
];

/// Logical pixels per PDF point at `zoom_percent`; 100 % = actual size on a 96-DPI screen.
pub fn points_to_px(zoom_percent: f32) -> f32 {
    zoom_percent / 100.0 * 96.0 / 72.0
}

/// Where each page sits when the pages are stacked top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    /// Top edge of each page.
    pub tops: Vec<f32>,
    /// Width and height of each page.
    pub sizes: Vec<(f32, f32)>,
    /// From the top of the first page to the bottom of the last.
    pub total_height: f32,
}

/// Stacks pages of `sizes` (points) vertically, `gap` pixels apart.
pub fn layout_pages(sizes: &[(f32, f32)], px_per_pt: f32, gap: f32) -> PageLayout {
    let mut tops = Vec::with_capacity(sizes.len());
    let mut px_sizes = Vec::with_capacity(sizes.len());
    let mut y = 0.0;
    for &(width, height) in sizes {
        tops.push(y);
        px_sizes.push((width * px_per_pt, height * px_per_pt));
        y += height * px_per_pt + gap;
    }
    // No gap after the last page.
    let total_height = if sizes.is_empty() { 0.0 } else { y - gap };
    PageLayout {
        tops,
        sizes: px_sizes,
        total_height,
    }
}

/// The pages that are at least partly inside `scroll_top .. scroll_top + viewport_height`.
pub fn visible_range(layout: &PageLayout, scroll_top: f32, viewport_height: f32) -> Range<usize> {
    let scroll_bottom = scroll_top + viewport_height;
    // `tops` is sorted, so a binary search finds the first page starting below the viewport.
    let end = layout.tops.partition_point(|&top| top < scroll_bottom);
    let start = layout.tops[..end]
        .iter()
        .zip(&layout.sizes)
        .take_while(|&(&top, &(_, height))| top + height <= scroll_top)
        .count();
    start..end
}

/// The zoom (percent) that makes a page `page_width_pt` wide fill `viewport_width`
/// minus `margin` pixels, clamped to `MIN_ZOOM..=MAX_ZOOM`.
pub fn fit_width_zoom(page_width_pt: f32, viewport_width: f32, margin: f32) -> f32 {
    if page_width_pt <= 0.0 || page_width_pt.is_nan() {
        return 100.0;
    }
    // The inverse of `points_to_px`, ordered so whole numbers stay exact.
    let zoom = (viewport_width - margin) * 72.0 / 96.0 / page_width_pt * 100.0;
    zoom.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// The next zoom preset above `zoom` (for Ctrl+plus and the + button).
pub fn zoom_in_step(zoom: f32) -> f32 {
    ZOOM_PRESETS
        .iter()
        .copied()
        .find(|&preset| preset > zoom + 0.5)
        .unwrap_or(MAX_ZOOM)
}

/// The next zoom preset below `zoom` (for Ctrl+minus and the − button).
pub fn zoom_out_step(zoom: f32) -> f32 {
    ZOOM_PRESETS
        .iter()
        .rev()
        .copied()
        .find(|&preset| preset < zoom - 0.5)
        .unwrap_or(MIN_ZOOM)
}

/// The page shown at the middle of the viewport (for "page N of M").
pub fn current_page(layout: &PageLayout, scroll_top: f32, viewport_height: f32) -> usize {
    page_at(layout, scroll_top + viewport_height / 2.0)
}

/// The last page whose top is at or above `y` (the first page if none is).
fn page_at(layout: &PageLayout, y: f32) -> usize {
    layout
        .tops
        .partition_point(|&top| top <= y)
        .saturating_sub(1)
}

/// Moves `y` (a position in `old`) to the same spot on the same page in `new`.
/// Used to keep the point under the cursor fixed while zooming.
pub fn map_y(old: &PageLayout, new: &PageLayout, y: f32) -> f32 {
    let page = page_at(old, y);
    // `zip` of two `Option`s is `Some` only when both pages exist.
    let (Some((&old_top, &(_, old_height))), Some((&new_top, &(_, new_height)))) = (
        old.tops.get(page).zip(old.sizes.get(page)),
        new.tops.get(page).zip(new.sizes.get(page)),
    ) else {
        return y;
    };
    if old_height <= 0.0 {
        return new_top;
    }
    new_top + (y - old_top) * new_height / old_height
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_pages() -> PageLayout {
        layout_pages(&[(100.0, 200.0), (100.0, 300.0)], 1.0, 10.0)
    }

    #[test]
    fn points_to_px_should_make_100_percent_96_dpi() {
        assert_eq!(points_to_px(100.0) * 72.0, 96.0);
    }

    #[test]
    fn layout_pages_should_stack_pages_with_gap() {
        let layout = two_pages();
        assert_eq!(
            (layout.tops, layout.total_height),
            (vec![0.0, 210.0], 510.0)
        );
    }

    #[test]
    fn layout_pages_should_scale_sizes() {
        let layout = layout_pages(&[(100.0, 200.0)], 2.0, 10.0);
        assert_eq!(layout.sizes, vec![(200.0, 400.0)]);
    }

    #[test]
    fn layout_pages_should_handle_no_pages() {
        assert_eq!(layout_pages(&[], 1.0, 10.0).total_height, 0.0);
    }

    #[test]
    fn visible_range_should_include_partly_visible_pages() {
        assert_eq!(visible_range(&two_pages(), 150.0, 100.0), 0..2);
    }

    #[test]
    fn visible_range_should_skip_pages_above_the_viewport() {
        assert_eq!(visible_range(&two_pages(), 205.0, 100.0), 1..2);
    }

    #[test]
    fn fit_width_zoom_should_fill_viewport() {
        let zoom = fit_width_zoom(612.0, 832.0, 16.0);
        assert!((zoom - 100.0).abs() < 1e-4, "zoom was {zoom}");
    }

    #[test]
    fn fit_width_zoom_should_clamp() {
        assert_eq!(fit_width_zoom(612.0, 10.0, 16.0), 25.0);
    }

    #[test]
    fn fit_width_zoom_should_survive_zero_width_page() {
        assert_eq!(fit_width_zoom(0.0, 832.0, 16.0), 100.0);
    }

    #[test]
    fn zoom_in_step_should_go_to_next_preset() {
        assert_eq!(zoom_in_step(100.0), 110.0);
    }

    #[test]
    fn zoom_in_step_should_round_up_between_presets() {
        assert_eq!(zoom_in_step(103.0), 110.0);
    }

    #[test]
    fn zoom_in_step_should_stop_at_max() {
        assert_eq!(zoom_in_step(MAX_ZOOM), MAX_ZOOM);
    }

    #[test]
    fn zoom_out_step_should_go_to_previous_preset() {
        assert_eq!(zoom_out_step(100.0), 90.0);
    }

    #[test]
    fn zoom_out_step_should_stop_at_min() {
        assert_eq!(zoom_out_step(MIN_ZOOM), MIN_ZOOM);
    }

    #[test]
    fn current_page_should_pick_page_at_viewport_middle() {
        assert_eq!(current_page(&two_pages(), 200.0, 100.0), 1);
    }

    #[test]
    fn current_page_should_start_at_first_page() {
        assert_eq!(current_page(&two_pages(), 0.0, 100.0), 0);
    }

    #[test]
    fn map_y_should_keep_spot_on_page_when_zooming() {
        let zoomed = layout_pages(&[(100.0, 200.0), (100.0, 300.0)], 2.0, 10.0);
        // 50 px into page 2 at 1 px/pt is 100 px into it at 2 px/pt.
        assert_eq!(map_y(&two_pages(), &zoomed, 260.0), 510.0);
    }
}

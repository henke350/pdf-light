//! Page layout and zoom math for the reading view, in logical pixels.
//!
//! No UI types here, so everything can be unit-tested.

use crate::pages::KeptPage;
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

/// The page to report as "page N of M". It is the page under a line that
/// moves from the top of the viewport (at the start of the document) to the
/// bottom (at the end), so the first and last pages are reached even when
/// several pages fit in the window.
pub fn current_page(layout: &PageLayout, scroll_top: f32, viewport_height: f32) -> usize {
    let scroll_range = layout.total_height - viewport_height;
    let progress = if scroll_range > 0.0 {
        (scroll_top / scroll_range).clamp(0.0, 1.0)
    } else {
        0.0
    };
    page_at(layout, scroll_top + viewport_height * progress)
}

/// The scroll position (as `current_page`'s `scroll_top`) for going to `page`:
/// the page's top `margin` pixels below the top of the view, moved only as far
/// as needed for `current_page` to report that page. (Its reference line can
/// otherwise fall just above the page early in a long document, or pages
/// below it when several pages fit in the view.) `None` if there is no such page.
pub fn scroll_top_for_page(
    layout: &PageLayout,
    page: usize,
    viewport_height: f32,
    margin: f32,
) -> Option<f32> {
    let (&top, &(_, height)) = layout.tops.get(page).zip(layout.sizes.get(page))?;
    let preferred = (top - margin).max(0.0);
    let scroll_range = layout.total_height - viewport_height;
    if scroll_range <= 0.0 {
        return Some(preferred); // Everything fits; there is nothing to scroll.
    }
    let preferred = preferred.min(scroll_range);
    // Within `0..=scroll_range`, `current_page`'s line sits at `scroll_top * scale`.
    let scale = layout.total_height / scroll_range;
    let line = preferred * scale;
    // Stay a little inside the page, so rounding can't tip the line into the gap.
    let inset = (height / 4.0).min(8.0);
    let (first, last) = (top + inset, top + height - inset);
    if (first..=last).contains(&line) {
        return Some(preferred);
    }
    Some((line.clamp(first, last) / scale).clamp(0.0, scroll_range))
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

/// Reads a zoom typed into the zoom box, such as "150", "150%" or " 75 % ".
/// The result is not clamped; `None` if it isn't a positive number.
pub fn parse_zoom(text: &str) -> Option<f32> {
    let number = text.trim().trim_end_matches('%').trim();
    // `.ok()` turns the parse `Result` into an `Option`, dropping the error.
    let zoom: f32 = number.parse().ok()?;
    (zoom.is_finite() && zoom > 0.0).then_some(zoom)
}

/// Largest page image the reading view asks for, in pixels (64 MB of RGBA).
pub const MAX_TEXTURE_PIXELS: f32 = 16_000_000.0;

/// The scale to render a page of `page_size` points at, given the `wanted`
/// scale: rounded down to a multiple of 1/64 (so nearby zooms share images) and
/// capped so the image stays within `max_side` pixels per side and
/// `MAX_TEXTURE_PIXELS` in total. At extreme zoom the page is then drawn
/// stretched rather than not at all.
pub fn render_scale(page_size: (f32, f32), wanted: f32, max_side: usize) -> f32 {
    let (width, height) = (page_size.0.max(1.0), page_size.1.max(1.0));
    let side_cap = max_side as f32 / width.max(height);
    let pixel_cap = (MAX_TEXTURE_PIXELS / (width * height)).sqrt();
    // Accepted for v1: near 800 % the cap makes pages slightly blurry (no tiling).
    let scale = wanted.min(side_cap).min(pixel_cap);
    (scale * 64.0).floor().max(1.0) / 64.0
}

/// The sizes of the kept pages as shown: in order, each turned the way the
/// owner turned it. `sizes` are the upright sizes of every page in the file.
pub fn kept_sizes(sizes: &[(f32, f32)], kept: &[KeptPage]) -> Vec<(f32, f32)> {
    // `filter_map` drops the `None`s that `get` returns for indices out of range.
    kept.iter()
        .filter_map(|page| Some(page.rotation.size(*sizes.get(page.index)?)))
        .collect()
}

/// A thumbnail is at most this many times as tall as it is wide.
const MAX_THUMBNAIL_ASPECT: f32 = 1.5;
/// Thumbnails render for widths rounded up to this step, so dragging the
/// panel's edge doesn't re-render them on every pixel.
const THUMBNAIL_WIDTH_STEP: f32 = 32.0;

/// A thumbnail's size for a page of `page_size` points in a column `width`
/// pixels wide: as wide as the column, unless the page is so tall that the
/// height limit makes it narrower.
pub fn thumbnail_size(page_size: (f32, f32), width: f32) -> (f32, f32) {
    let (page_width, page_height) = page_size;
    if !(page_width > 0.0 && page_height > 0.0) {
        // An empty or broken page: show an A4-shaped card.
        return (width, width * std::f32::consts::SQRT_2);
    }
    let scale = (width / page_width).min(width * MAX_THUMBNAIL_ASPECT / page_height);
    (page_width * scale, page_height * scale)
}

/// The render scale for a page's thumbnail in a column `width` logical pixels
/// wide, sharp on a screen with `pixels_per_point` physical pixels per logical one.
pub fn thumbnail_scale(page_size: (f32, f32), width: f32, pixels_per_point: f32) -> f32 {
    let stepped = (width / THUMBNAIL_WIDTH_STEP).ceil() * THUMBNAIL_WIDTH_STEP;
    let (thumbnail_width, _) = thumbnail_size(page_size, stepped);
    thumbnail_width * pixels_per_point / page_size.0.max(1.0)
}

/// The scroll position along one axis that shows `start..end` (content
/// coordinates) in a view `len` long now at `view_start`: unchanged if it is
/// already wholly in view, otherwise centred on it (its start shown first if
/// it is longer than the view). Not clamped to the content.
pub fn reveal(view_start: f32, len: f32, start: f32, end: f32) -> f32 {
    if start >= view_start && end <= view_start + len {
        view_start
    } else if end - start > len {
        start
    } else {
        (start + end) / 2.0 - len / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rotation::Rotation;

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
    fn current_page_should_follow_scrolling() {
        assert_eq!(current_page(&two_pages(), 200.0, 100.0), 1);
    }

    #[test]
    fn current_page_should_start_at_first_page() {
        assert_eq!(current_page(&two_pages(), 0.0, 100.0), 0);
    }

    #[test]
    fn current_page_should_reach_a_short_last_page_at_the_end() {
        let layout = layout_pages(&[(100.0, 100.0); 3], 1.0, 10.0);
        // 320 px of pages in a 300 px view, scrolled to the bottom.
        assert_eq!(current_page(&layout, 20.0, 300.0), 2);
    }

    /// `current_page` after going to `page` with `scroll_top_for_page`.
    fn page_after_going_to(layout: &PageLayout, page: usize, viewport_height: f32) -> usize {
        let scroll_top =
            scroll_top_for_page(layout, page, viewport_height, 8.0).expect("page exists");
        current_page(layout, scroll_top, viewport_height)
    }

    #[test]
    fn scroll_top_for_page_should_report_that_page_early_in_a_long_document() {
        // 2000 tall pages: near the start the reference line sits only a few
        // pixels below the top of the view.
        let layout = layout_pages(&[(1000.0, 1414.0); 2000], 1.0, 12.0);
        assert_eq!(page_after_going_to(&layout, 10, 900.0), 10);
    }

    #[test]
    fn scroll_top_for_page_should_report_that_page_at_low_zoom() {
        // Small pages, several in view: late in the document the reference
        // line sits far below the top of the view.
        let layout = layout_pages(&[(150.0, 212.0); 100], 1.0, 12.0);
        assert_eq!(page_after_going_to(&layout, 70, 900.0), 70);
    }

    #[test]
    fn scroll_top_for_page_should_put_page_at_top_when_that_already_works() {
        let layout = layout_pages(&[(100.0, 1000.0); 3], 1.0, 12.0);
        assert_eq!(scroll_top_for_page(&layout, 1, 300.0, 8.0), Some(1004.0));
    }

    #[test]
    fn scroll_top_for_page_should_reject_unknown_page() {
        assert_eq!(scroll_top_for_page(&two_pages(), 5, 100.0, 8.0), None);
    }

    #[test]
    fn parse_zoom_should_accept_percent_sign_and_spaces() {
        assert_eq!(parse_zoom(" 75 % "), Some(75.0));
    }

    #[test]
    fn parse_zoom_should_reject_text_and_non_positive_numbers() {
        assert_eq!(
            (parse_zoom("abc"), parse_zoom("0"), parse_zoom("NaN")),
            (None, None, None)
        );
    }

    #[test]
    fn render_scale_should_round_down_to_64ths() {
        assert_eq!(render_scale((612.0, 792.0), 1.34, 16384), 85.0 / 64.0);
    }

    #[test]
    fn render_scale_should_respect_max_side() {
        assert_eq!(render_scale((612.0, 792.0), 8.0, 2048), 165.0 / 64.0);
    }

    #[test]
    fn render_scale_should_respect_pixel_budget() {
        let scale = render_scale((612.0, 792.0), 8.0, 65536);
        assert!(612.0 * 792.0 * scale * scale <= MAX_TEXTURE_PIXELS);
    }

    #[test]
    fn render_scale_should_stay_positive_for_huge_pages() {
        assert_eq!(render_scale((1e9, 1e9), 1.0, 2048), 1.0 / 64.0);
    }

    #[test]
    fn map_y_should_keep_spot_on_page_when_zooming() {
        let zoomed = layout_pages(&[(100.0, 200.0), (100.0, 300.0)], 2.0, 10.0);
        // 50 px into page 2 at 1 px/pt is 100 px into it at 2 px/pt.
        assert_eq!(map_y(&two_pages(), &zoomed, 260.0), 510.0);
    }

    #[test]
    fn kept_sizes_should_follow_kept_order() {
        let sizes = [(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
        let kept = [KeptPage::upright(0), KeptPage::upright(2)];
        assert_eq!(kept_sizes(&sizes, &kept), vec![(1.0, 1.0), (3.0, 3.0)]);
    }

    #[test]
    fn kept_sizes_should_turn_rotated_pages() {
        let kept = [KeptPage {
            index: 0,
            rotation: Rotation::quarter_turns(1),
        }];
        assert_eq!(kept_sizes(&[(1.0, 2.0)], &kept), vec![(2.0, 1.0)]);
    }

    #[test]
    fn kept_sizes_should_skip_unknown_pages() {
        assert_eq!(kept_sizes(&[(1.0, 1.0)], &[KeptPage::upright(5)]), vec![]);
    }

    #[test]
    fn thumbnail_size_should_fill_width() {
        assert_eq!(thumbnail_size((200.0, 100.0), 100.0), (100.0, 50.0));
    }

    #[test]
    fn thumbnail_size_should_limit_very_tall_pages() {
        assert_eq!(thumbnail_size((100.0, 400.0), 100.0), (37.5, 150.0));
    }

    #[test]
    fn thumbnail_size_should_survive_empty_page() {
        assert_eq!(thumbnail_size((0.0, 0.0), 100.0).0, 100.0);
    }

    #[test]
    fn thumbnail_scale_should_render_at_screen_pixels() {
        // 100 px rounds up to 128 px; at 2 physical pixels per point that is 4 × 64 pt.
        assert_eq!(thumbnail_scale((64.0, 64.0), 100.0, 2.0), 4.0);
    }

    #[test]
    fn thumbnail_scale_should_ignore_small_width_changes() {
        let scale = |width| thumbnail_scale((612.0, 792.0), width, 1.0);
        assert_eq!(scale(130.0), scale(150.0));
    }
    #[test]
    fn reveal_should_not_move_when_the_target_is_in_view() {
        assert_eq!(reveal(100.0, 500.0, 150.0, 170.0), 100.0);
    }

    #[test]
    fn reveal_should_centre_a_target_out_of_view() {
        // A 20 px target at 1000 in a 500 px view: its middle (1010) goes to 250.
        assert_eq!(
            [
                reveal(100.0, 500.0, 1000.0, 1020.0),
                reveal(2000.0, 500.0, 1000.0, 1020.0)
            ],
            [760.0, 760.0]
        );
    }

    #[test]
    fn reveal_should_show_the_start_of_a_target_longer_than_the_view() {
        assert_eq!(reveal(0.0, 100.0, 300.0, 600.0), 300.0);
    }
}

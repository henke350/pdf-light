//! The pages panel: page thumbnails down the left side, where pages are
//! selected for deleting (and, later, exporting and printing).
//!
//! Like the page view, it lists the kept pages in order: positions count the
//! pages shown, `pages[position]` is the page's index in the original file.

use super::textures::{self, TextureCache, texture_key};
use super::theme::{self, Palette, SMALL_SIZE};
use super::toolbar::{self, ToolbarAction, ToolbarState};
use eframe::egui::{
    self, Align, Align2, FontId, Key, KeyboardShortcut, Layout, Modifiers, Pos2, Rect, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use pdf_light::render_pool::{RenderRequest, RenderResult};
use pdf_light::selection::Selection;
use pdf_light::view::{self, PageLayout};
use std::collections::HashSet;

/// The panel's width when the app starts, and the range its edge can be dragged in.
pub const DEFAULT_WIDTH: f32 = 176.0;
pub const MIN_WIDTH: f32 = 120.0;
pub const MAX_WIDTH: f32 = 320.0;

/// Space left and right of a thumbnail; the selection stripe sits in the left one.
const SIDE: f32 = 16.0;
/// Space inside a card above the thumbnail and below the page number.
const CARD_PAD: f32 = 6.0;
/// From the thumbnail's bottom to the page number.
const LABEL_GAP: f32 = 4.0;
const LABEL_HEIGHT: f32 = 14.0;
/// Space between cards.
const CARD_GAP: f32 = 2.0;
/// The selection stripe's width.
const STRIPE: f32 = 4.0;
/// The Marker tint behind a selected card.
const TINT: f32 = 0.18;
/// How far outside the thumbnail the current page's outline is drawn.
const OUTLINE_GAP: f32 = 3.0;
/// Most memory thumbnails may use before old ones are dropped.
const TEXTURE_BUDGET: usize = 64 * 1024 * 1024;
/// Thumbnails rendered beyond the visible ones, in each direction.
const AHEAD: usize = 2;

/// "Export page…" for one page, "Export 3 pages…" for several.
fn menu_label(verb: &str, count: usize) -> String {
    if count > 1 {
        format!("{verb} {count} pages…")
    } else {
        format!("{verb} page…")
    }
}

/// The quiet count at the bottom of the panel: "14 pages".
fn page_count_label(count: usize) -> String {
    if count == 1 {
        "1 page".to_owned()
    } else {
        format!("{count} pages")
    }
}

/// What the owner did in the panel this frame.
#[derive(Default)]
pub struct PanelOutput {
    /// Chosen from the right-click menu; the same actions as the toolbar's.
    pub actions: Vec<ToolbarAction>,
    /// A thumbnail was clicked: scroll the page view to this position.
    pub go_to: Option<usize>,
}

/// Thumbnails and scroll state for one open document.
pub struct PagesPanel {
    textures: TextureCache,
    /// Original pages whose thumbnail couldn't be made; not asked for again.
    failed: HashSet<usize>,
    /// The thumbnails this panel would like next; see `wanted`.
    wanted: Vec<RenderRequest>,
    /// The current page the list last made sure was in view.
    followed: Option<usize>,
    /// Last frame's visible part of the list, in list coordinates.
    viewport: Rect,
    frame: u64,
}

impl PagesPanel {
    pub fn new() -> Self {
        Self {
            textures: TextureCache::default(),
            failed: HashSet::new(),
            wanted: Vec::new(),
            followed: None,
            viewport: Rect::NOTHING,
            frame: 0,
        }
    }

    /// Turns a finished thumbnail render into a texture (or remembers that it failed).
    pub fn receive(&mut self, ctx: &egui::Context, result: RenderResult) {
        let RenderResult { request, result } = result;
        match textures::load(ctx, request, result, self.frame) {
            Some(entry) => self
                .textures
                .insert(texture_key(request.page, request.scale), entry),
            None => {
                self.failed.insert(request.page);
            }
        }
    }

    /// The thumbnail renders this panel would like next (original page indices).
    pub fn wanted(&self) -> &[RenderRequest] {
        &self.wanted
    }

    /// Draws the panel: the thumbnails of the kept pages (`sizes` in points,
    /// `pages` their original indices) with `current` outlined and `selection`
    /// marked, and the page count at the bottom. Clicks change `selection`.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        sizes: &[(f32, f32)],
        pages: &[usize],
        current: usize,
        selection: &mut Selection,
        state: &ToolbarState,
    ) -> PanelOutput {
        self.frame += 1;
        let palette = theme::palette(ui.ctx());
        let mut output = PanelOutput::default();
        // Bottom-up: the count goes in first, at the bottom; the list fills the rest.
        ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
            ui.add_space(6.0);
            ui.label(
                RichText::new(page_count_label(pages.len()))
                    .size(SMALL_SIZE)
                    .color(palette.quiet),
            );
            ui.add_space(2.0);
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                let list = List {
                    sizes,
                    pages,
                    current,
                    state,
                    palette,
                };
                self.list(ui, &list, selection, &mut output);
            });
        });
        self.textures.evict(TEXTURE_BUDGET, self.frame);
        output
    }

    /// The scrolling column of thumbnails.
    fn list(
        &mut self,
        ui: &mut Ui,
        list: &List<'_>,
        selection: &mut Selection,
        output: &mut PanelOutput,
    ) {
        let width = ui.available_width();
        let thumb_width = (width - 2.0 * SIDE).max(16.0);
        // Each card: padding, the thumbnail, the page number, padding.
        let cards: Vec<(f32, f32)> = list
            .sizes
            .iter()
            .map(|&size| {
                let (_, height) = view::thumbnail_size(size, thumb_width);
                let card = CARD_PAD + height + LABEL_GAP + LABEL_HEIGHT + CARD_PAD;
                (width, card)
            })
            .collect();
        let layout = view::layout_pages(&cards, 1.0, CARD_GAP);

        let mut area = ScrollArea::vertical()
            .id_salt("pages-panel")
            .auto_shrink(false);
        if let Some(offset) = self.follow(&layout, list.current) {
            area = area.vertical_scroll_offset(offset);
        }
        let output_area = area.show_viewport(ui, |ui, viewport| {
            let (rect, _) =
                ui.allocate_exact_size(vec2(width, layout.total_height), Sense::hover());
            let visible = view::visible_range(&layout, viewport.min.y, viewport.height());
            for position in visible.clone() {
                self.card(
                    ui,
                    rect.min,
                    &layout,
                    position,
                    thumb_width,
                    list,
                    selection,
                    output,
                );
            }
            self.request_thumbnails(ui.ctx(), list, visible, thumb_width);
        });
        self.viewport = Rect::from_min_size(
            output_area.state.clamped_offset().to_pos2(),
            output_area.inner_rect.size(),
        );
    }

    /// When the current page changes, a scroll offset that brings its
    /// thumbnail into view, if it isn't already. Otherwise the list stays
    /// where the owner scrolled it.
    fn follow(&mut self, layout: &PageLayout, current: usize) -> Option<f32> {
        // No viewport yet on the first frame: try again on the next one.
        if self.followed == Some(current) || self.viewport.height() <= 0.0 {
            return None;
        }
        self.followed = Some(current);
        let (&top, &(_, height)) = layout.tops.get(current).zip(layout.sizes.get(current))?;
        let in_view = top >= self.viewport.top() && top + height <= self.viewport.bottom();
        // Centre it in the list.
        (!in_view).then(|| (top - (self.viewport.height() - height) / 2.0).max(0.0))
    }

    /// Draws the card at `position` and handles clicks on it.
    #[expect(
        clippy::too_many_arguments,
        reason = "one card needs the list, its place in it, and the selection to change"
    )]
    fn card(
        &mut self,
        ui: &Ui,
        origin: Pos2,
        layout: &PageLayout,
        position: usize,
        thumb_width: f32,
        list: &List<'_>,
        selection: &mut Selection,
        output: &mut PanelOutput,
    ) {
        let (Some(&top), Some(&card_size), Some(&page), Some(&page_size)) = (
            layout.tops.get(position),
            layout.sizes.get(position),
            list.pages.get(position),
            list.sizes.get(position),
        ) else {
            return;
        };
        let palette = list.palette;
        let ppp = ui.ctx().pixels_per_point();
        let card = Rect::from_min_size(origin + vec2(0.0, top), card_size.into());
        let (width, height) = view::thumbnail_size(page_size, thumb_width);
        let thumb = snap(
            Rect::from_min_size(
                pos2(card.center().x - width / 2.0, card.top() + CARD_PAD),
                vec2(width, height),
            ),
            ppp,
        );

        let response = ui.interact(
            card,
            ui.scope_id().with(("thumbnail", position)),
            Sense::click(),
        );
        if response.clicked() {
            let modifiers = ui.input(|i| i.modifiers);
            selection.click(position, modifiers.command, modifiers.shift);
            // A plain click also goes to the page; Ctrl and Shift only change the selection.
            if !modifiers.command && !modifiers.shift {
                output.go_to = Some(position);
            }
        }
        // Right-clicking a page outside the selection selects it, so the menu acts on it.
        if response.secondary_clicked() && !selection.contains(position) {
            selection.click(position, false, false);
        }
        let count = selection.len();
        response.context_menu(|ui| {
            let state = ToolbarState {
                selected_count: count,
                ..list.state.clone()
            };
            context_menu(ui, count, &state, &mut output.actions);
        });

        let painter = ui.painter();
        if selection.contains(position) {
            // A highlighter swipe: Marker tint behind the card, a stripe on its left edge.
            painter.rect_filled(card, 0.0, palette.marker.gamma_multiply(TINT));
            let stripe = Rect::from_min_size(card.min, vec2(STRIPE, card.height()));
            painter.rect_filled(stripe, 0.0, palette.marker);
        } else if response.hovered() {
            painter.rect_filled(card, 0.0, palette.hover());
        }

        let scale = thumbnail_scale(ui.ctx(), page_size, thumb_width);
        if let Some(texture) = self.textures.best(page, scale, self.frame) {
            let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
            painter.image(texture.id(), thumb, uv, egui::Color32::WHITE);
        } else if self.failed.contains(&page) {
            painter.rect_filled(thumb, 0.0, palette.disabled());
        } else {
            // Not drawn yet: a plain card at the page's shape, so nothing jumps later.
            painter.rect_filled(thumb, 0.0, palette.panel);
        }
        painter.rect_stroke(
            thumb,
            0.0,
            Stroke::new(1.0, palette.quiet),
            StrokeKind::Outside,
        );
        if position == list.current {
            painter.rect_stroke(
                thumb.expand(OUTLINE_GAP),
                0.0,
                Stroke::new(1.0, palette.ink),
                StrokeKind::Outside,
            );
        }
        painter.text(
            pos2(card.center().x, thumb.bottom() + LABEL_GAP),
            Align2::CENTER_TOP,
            (position + 1).to_string(),
            FontId::proportional(SMALL_SIZE),
            palette.quiet,
        );
    }

    /// Lists the thumbnails in view, plus a few either side, that still need rendering.
    fn request_thumbnails(
        &mut self,
        ctx: &egui::Context,
        list: &List<'_>,
        visible: std::ops::Range<usize>,
        thumb_width: f32,
    ) {
        self.wanted.clear();
        let start = visible.start.saturating_sub(AHEAD);
        let end = (visible.end + AHEAD).min(list.pages.len());
        for position in start..end {
            let (Some(&page), Some(&size)) = (list.pages.get(position), list.sizes.get(position))
            else {
                continue;
            };
            let scale = thumbnail_scale(ctx, size, thumb_width);
            if !self.textures.contains(texture_key(page, scale)) && !self.failed.contains(&page) {
                self.wanted.push(RenderRequest {
                    page,
                    scale,
                    thumbnail: true,
                });
            }
        }
    }
}

/// What every card needs to know about the list, gathered in one place.
/// `'a` says these borrowed values all live at least as long as the `List`.
struct List<'a> {
    sizes: &'a [(f32, f32)],
    pages: &'a [usize],
    current: usize,
    state: &'a ToolbarState,
    palette: &'static Palette,
}

/// The right-click menu: the toolbar's Delete, Export and Print for the selected pages.
fn context_menu(ui: &mut Ui, count: usize, state: &ToolbarState, actions: &mut Vec<ToolbarAction>) {
    let items = [
        (ToolbarAction::DeleteSelected, toolbar::delete_label(count)),
        (ToolbarAction::Export, menu_label("Export", count)),
        (ToolbarAction::Print, menu_label("Print", count)),
    ];
    for (action, label) in items {
        let reason = toolbar::disabled_reason(action, state);
        let mut response = ui.add_enabled(reason.is_none(), egui::Button::new(label));
        if let Some(reason) = reason {
            response = response.on_disabled_hover_text(reason);
        }
        if response.clicked() {
            actions.push(action);
            ui.close();
        }
    }
}

/// Ctrl+A selects every page and Esc clears the selection. Call only while
/// the panel is shown and no dialog or text field has the keyboard.
pub fn selection_keys(ctx: &egui::Context, selection: &mut Selection, page_count: usize) {
    let (select_all, clear) = ctx.input_mut(|i| {
        (
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::A)),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    if select_all {
        selection.select_all(page_count);
    }
    if clear {
        selection.clear();
    }
}

/// The scale to render a thumbnail at, sharp on this screen and within the GPU's limits.
fn thumbnail_scale(ctx: &egui::Context, page_size: (f32, f32), thumb_width: f32) -> f32 {
    let wanted = view::thumbnail_scale(page_size, thumb_width, ctx.pixels_per_point());
    view::render_scale(page_size, wanted, ctx.input(|i| i.max_texture_side))
}

/// Rounds a rectangle to whole physical pixels so thumbnails stay sharp.
fn snap(rect: Rect, pixels_per_point: f32) -> Rect {
    let round = |v: f32| (v * pixels_per_point).round() / pixels_per_point;
    Rect::from_min_max(
        pos2(round(rect.min.x), round(rect.min.y)),
        pos2(round(rect.max.x), round(rect.max.y)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_label_should_count_pages() {
        assert_eq!(
            [menu_label("Export", 1), menu_label("Print", 3)],
            ["Export page…", "Print 3 pages…"]
        );
    }

    #[test]
    fn page_count_label_should_use_singular_for_one_page() {
        assert_eq!(
            [page_count_label(1), page_count_label(14)],
            ["1 page", "14 pages"]
        );
    }
}

//! The scrolling page view: layout, texture cache, drawing, and scroll/zoom input.
//!
//! The UI thread only says which pages it wants and turns finished images into
//! textures; the rendering itself happens in the `RenderPool`'s threads.
//!
//! The view shows the kept pages in order. `sizes` and positions count those
//! pages (0 = the first page shown); `pages[position]` is the page's index in
//! the original file, which is what textures and render requests use.

use super::icons::Icon;
use super::textures::{self, TextureCache, TextureKey, texture_key};
use super::theme::{self, LARGE_SIZE, TEXT_SIZE};
use super::toolbar::ToolButton;
use eframe::egui::{
    self, Align2, Color32, FontId, Key, Modifiers, Pos2, Rect, RichText, ScrollArea, Sense, Vec2,
    pos2, vec2,
};
use pdf_light::render_pool::{RenderRequest, RenderResult};
use pdf_light::view::{self, PageLayout};
use std::collections::HashSet;
use std::time::Duration;

/// Space between pages, in logical pixels.
const GAP: f32 = 12.0;
/// Space around the pages, in logical pixels.
const MARGIN: f32 = 8.0;
/// Most memory the page textures may use before old ones are dropped.
const TEXTURE_BUDGET: usize = 256 * 1024 * 1024;
/// While zooming, wait this long before asking for sharper images.
const ZOOM_SETTLE_SECS: f64 = 0.15;
/// How far an arrow key scrolls, in logical pixels.
const ARROW_STEP: f32 = 40.0;

/// Scroll position, zoom and page images for one open document.
pub struct PageView {
    /// Zoom in percent.
    zoom: f32,
    /// Keep the zoom at "fit width" as the window changes size.
    fit_width: bool,
    /// `ctx.input(|i| i.time)` of the last zoom change.
    zoom_changed_at: f64,
    /// Scroll position to apply on the next frame.
    scroll_to: Option<Vec2>,
    /// Last frame's scroll position and on-screen area.
    offset: Vec2,
    view_rect: Rect,
    current_page: usize,
    textures: TextureCache,
    /// Renders that failed; not asked for again.
    failed: HashSet<TextureKey>,
    /// The renders this view would like next; see `wanted`.
    wanted: Vec<RenderRequest>,
    frame: u64,
}

impl PageView {
    pub fn new() -> Self {
        Self {
            zoom: 100.0,
            fit_width: true,
            zoom_changed_at: f64::NEG_INFINITY,
            scroll_to: None,
            offset: Vec2::ZERO,
            view_rect: Rect::ZERO,
            current_page: 0,
            textures: TextureCache::default(),
            failed: HashSet::new(),
            wanted: Vec::new(),
            frame: 0,
        }
    }

    /// The page for "page N of M" (0-based); see `view::current_page`.
    pub fn current_page(&self) -> usize {
        self.current_page
    }

    /// Turns a finished page render into a texture (or remembers that it failed).
    pub fn receive(&mut self, ctx: &egui::Context, result: RenderResult) {
        let RenderResult { request, result } = result;
        let key = texture_key(request.page, request.scale);
        match textures::load(ctx, request, result, self.frame) {
            Some(entry) => self.textures.insert(key, entry),
            None => {
                self.failed.insert(key);
            }
        }
    }

    /// The page renders this view would like next (original page indices),
    /// in order: pages in view, then one after and one before.
    pub fn wanted(&self) -> &[RenderRequest] {
        &self.wanted
    }

    /// Forgets every page image, for when the page indices change meaning
    /// (a save reopens the file). Zoom and scroll position stay.
    pub fn reset_textures(&mut self) {
        self.textures = TextureCache::default();
        self.failed.clear();
        self.wanted.clear();
    }

    /// Scrolls so the page at `position` starts at the top of the view.
    pub fn scroll_to_page(&mut self, position: usize, sizes: &[(f32, f32)]) {
        let layout = view::layout_pages(sizes, view::points_to_px(self.zoom), GAP);
        if let Some(&top) = layout.tops.get(position) {
            let x = self.scroll_to.unwrap_or(self.offset).x;
            // The page sits `MARGIN` below its layout top, so that much desk stays above it.
            self.scroll_to = Some(vec2(x, top));
        }
    }

    /// Zoom in percent.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Whether the zoom follows the window width.
    pub fn fits_width(&self) -> bool {
        self.fit_width
    }

    /// Keeps the widest page as wide as the window from now on.
    pub fn fit_width(&mut self) {
        self.fit_width = true;
    }

    /// Zooms to `zoom` percent (clamped) around the middle of the view,
    /// and stops fitting the width.
    pub fn zoom_to(&mut self, zoom: f32, sizes: &[(f32, f32)], now: f64) {
        self.fit_width = false;
        self.set_zoom(zoom, self.view_rect.size() / 2.0, sizes, now);
    }

    /// Draws the kept pages (`sizes` in points, `pages` their original indices),
    /// handles scrolling and zooming, and works out which pages to render next.
    pub fn show(&mut self, ui: &mut egui::Ui, sizes: &[(f32, f32)], pages: &[usize]) {
        self.frame += 1;
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let available = ui.available_rect_before_wrap();
        if self.view_rect == Rect::ZERO {
            self.view_rect = available;
        }

        if self.fit_width {
            let widest = sizes.iter().map(|s| s.0).fold(0.0, f32::max);
            let zoom = view::fit_width_zoom(widest, available.width(), 2.0 * MARGIN);
            self.set_zoom(zoom, vec2(available.width() / 2.0, 0.0), sizes, now);
        }
        self.zoom_input(&ctx, sizes, now);

        let layout = view::layout_pages(sizes, view::points_to_px(self.zoom), GAP);
        let content = vec2(
            content_width(&layout, available.width()),
            layout.total_height + 2.0 * MARGIN,
        );
        self.scroll_keys(&ctx, content.y);

        let mut area = ScrollArea::both().id_salt("pages").auto_shrink(false);
        if let Some(offset) = self.scroll_to.take() {
            let max = (content - available.size()).max(Vec2::ZERO);
            area = area.scroll_offset(offset.clamp(Vec2::ZERO, max));
        }
        let output = area.show_viewport(ui, |ui, viewport| {
            let (rect, _) = ui.allocate_exact_size(content, Sense::hover());
            self.paint_pages(ui, rect.min, &layout, pages, viewport, content.x);
        });
        self.offset = output.state.clamped_offset();
        self.view_rect = output.inner_rect;
        self.current_page =
            view::current_page(&layout, self.offset.y - MARGIN, self.view_rect.height());

        self.request_pages(&ctx, sizes, pages, &layout, now);
        self.textures.evict(TEXTURE_BUDGET, self.frame);
    }

    /// Changes the zoom so the content point at `anchor` (relative to the
    /// view's top-left corner) stays under it.
    fn set_zoom(&mut self, zoom: f32, anchor: Vec2, sizes: &[(f32, f32)], now: f64) {
        let zoom = zoom.clamp(view::MIN_ZOOM, view::MAX_ZOOM);
        if (zoom - self.zoom).abs() < 0.01 {
            return;
        }
        let (old_px, new_px) = (view::points_to_px(self.zoom), view::points_to_px(zoom));
        let old = view::layout_pages(sizes, old_px, GAP);
        let new = view::layout_pages(sizes, new_px, GAP);
        let offset = self.scroll_to.unwrap_or(self.offset);

        let y = view::map_y(&old, &new, offset.y + anchor.y - MARGIN) + MARGIN - anchor.y;
        // Pages are centred, so measure x from the content's middle.
        let view_width = self.view_rect.width();
        let (old_mid, new_mid) = (
            content_width(&old, view_width) / 2.0,
            content_width(&new, view_width) / 2.0,
        );
        let x = new_mid + (offset.x + anchor.x - old_mid) / old_px * new_px - anchor.x;

        self.scroll_to = Some(vec2(x, y).max(Vec2::ZERO));
        self.zoom = zoom;
        self.zoom_changed_at = now;
    }

    /// Ctrl+wheel (or a touchpad pinch) over the pages zooms around the pointer.
    /// (Ctrl+plus/minus come through the toolbar as `ToolbarAction`s.)
    fn zoom_input(&mut self, ctx: &egui::Context, sizes: &[(f32, f32)], now: f64) {
        let (delta, pointer) = ctx.input(|i| (i.zoom_delta(), i.pointer.hover_pos()));
        let Some(pointer) = pointer.filter(|&p| self.view_rect.contains(p)) else {
            return;
        };
        if delta != 1.0 {
            self.fit_width = false;
            self.set_zoom(self.zoom * delta, pointer - self.view_rect.min, sizes, now);
        }
    }

    /// PgUp/PgDn, Home/End and the arrow keys.
    fn scroll_keys(&mut self, ctx: &egui::Context, content_height: f32) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let page_step = (self.view_rect.height() - ARROW_STEP).max(ARROW_STEP);
        let mut offset = self.scroll_to.unwrap_or(self.offset);
        let moved = ctx.input_mut(|i| {
            let mut moved = false;
            let steps = [
                (Key::PageDown, vec2(0.0, page_step)),
                (Key::PageUp, vec2(0.0, -page_step)),
                (Key::ArrowDown, vec2(0.0, ARROW_STEP)),
                (Key::ArrowUp, vec2(0.0, -ARROW_STEP)),
                (Key::ArrowRight, vec2(ARROW_STEP, 0.0)),
                (Key::ArrowLeft, vec2(-ARROW_STEP, 0.0)),
            ];
            for (key, step) in steps {
                // Count every press this frame (key repeat can deliver several at once).
                let presses = i.count_and_consume_key(Modifiers::NONE, key);
                if presses > 0 {
                    offset += step * presses as f32;
                    moved = true;
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Home) {
                offset.y = 0.0;
                moved = true;
            }
            if i.consume_key(Modifiers::NONE, Key::End) {
                offset.y = content_height;
                moved = true;
            }
            moved
        });
        if moved {
            self.scroll_to = Some(offset); // Clamped to the content in `show`.
        }
    }

    /// Paints the pages that intersect `viewport` (in content coordinates).
    fn paint_pages(
        &mut self,
        ui: &egui::Ui,
        origin: Pos2,
        layout: &PageLayout,
        pages: &[usize],
        viewport: Rect,
        content_width: f32,
    ) {
        let ppp = ui.ctx().pixels_per_point();
        let target_scale = view::points_to_px(self.zoom) * ppp;
        let painter = ui.painter();
        for position in view::visible_range(layout, viewport.min.y - MARGIN, viewport.height()) {
            let (Some(&(width, height)), Some(&top), Some(&page)) = (
                layout.sizes.get(position),
                layout.tops.get(position),
                pages.get(position),
            ) else {
                continue;
            };
            let min = origin + vec2((content_width - width) / 2.0, MARGIN + top);
            let rect = Rect::from_min_size(
                snap(min, ppp),
                snap(vec2(width, height).to_pos2(), ppp).to_vec2(),
            );
            painter.rect_filled(
                rect.translate(vec2(2.0, 2.0)),
                0.0,
                Color32::from_black_alpha(70),
            );

            // Stretch the closest cached image while a sharper one is on its way.
            if let Some(texture) = self.textures.best(page, target_scale, self.frame) {
                let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
                painter.image(texture.id(), rect, uv, Color32::WHITE);
            } else if self.failed.iter().any(|&(p, _)| p == page) {
                painter.rect_filled(rect, 0.0, Color32::from_gray(200));
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "Couldn't render this page",
                    FontId::proportional(16.0),
                    Color32::from_gray(60),
                );
            } else {
                painter.rect_filled(rect, 0.0, Color32::WHITE);
            }
        }
    }

    /// Lists the pages in view, then one after and one before, that still need rendering.
    fn request_pages(
        &mut self,
        ctx: &egui::Context,
        sizes: &[(f32, f32)],
        pages: &[usize],
        layout: &PageLayout,
        now: f64,
    ) {
        let settling = now - self.zoom_changed_at;
        let settled = settling >= ZOOM_SETTLE_SECS;
        if !settled {
            ctx.request_repaint_after(Duration::from_secs_f64(ZOOM_SETTLE_SECS - settling));
        }
        let wanted_scale = view::points_to_px(self.zoom) * ctx.pixels_per_point();
        let max_side = ctx.input(|i| i.max_texture_side);

        let visible = view::visible_range(layout, self.offset.y - MARGIN, self.view_rect.height());
        let next = (visible.end < sizes.len()).then_some(visible.end);
        let previous = visible.start.checked_sub(1);
        self.wanted.clear();
        for position in visible.chain(next).chain(previous) {
            let (Some(&size), Some(&page)) = (sizes.get(position), pages.get(position)) else {
                continue;
            };
            let scale = view::render_scale(size, wanted_scale, max_side);
            let key = texture_key(page, scale);
            let done = self.textures.contains(key) || self.failed.contains(&key);
            // While zooming, pages that already show something wait for the zoom to settle.
            let wait = !settled && self.textures.has_any(page);
            if !done && !wait {
                self.wanted.push(RenderRequest {
                    page,
                    scale,
                    thumbnail: false,
                });
            }
        }
    }
}

/// About how tall the empty state's three lines are, to centre them.
const EMPTY_STATE_HEIGHT: f32 = 100.0;

/// The desk with no file open: "Open a PDF to start reading", an Open button
/// and the quiet line "or drop a file here". Returns `true` when Open is clicked.
pub fn empty_state(ui: &mut egui::Ui) -> bool {
    let palette = theme::palette(ui.ctx());
    let text = palette.on_desk();
    ui.add_space(((ui.available_height() - EMPTY_STATE_HEIGHT) / 2.0).max(0.0));
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new("Open a PDF to start reading")
                .size(LARGE_SIZE)
                .color(text),
        );
        ui.add_space(14.0);
        let open = ui
            .add(ToolButton {
                icon: Icon::Open,
                label: Some("Open"),
                name: "Open",
                active: false,
                danger_on_hover: false,
                filled: true,
                palette,
            })
            .on_hover_text("Open a PDF (Ctrl+O)");
        ui.add_space(10.0);
        // Quiet on the desk: the desk's text colour, faded toward the desk.
        let quiet = palette.desk.lerp_to_gamma(text, 0.7);
        ui.label(
            RichText::new("or drop a file here")
                .size(TEXT_SIZE)
                .color(quiet),
        );
        open.clicked()
    })
    .inner
}

/// Content is as wide as the widest page plus margins, or the view if wider.
fn content_width(layout: &PageLayout, view_width: f32) -> f32 {
    let widest = layout.sizes.iter().map(|s| s.0).fold(0.0, f32::max);
    view_width.max(widest + 2.0 * MARGIN)
}

/// Rounds to whole physical pixels so page images stay sharp.
fn snap(p: Pos2, pixels_per_point: f32) -> Pos2 {
    let round = |v: f32| (v * pixels_per_point).round() / pixels_per_point;
    pos2(round(p.x), round(p.y))
}

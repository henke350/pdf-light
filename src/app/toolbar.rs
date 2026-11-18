//! The toolbar: what is possible right now (`ToolbarState`) goes in, the
//! actions the owner asked for (`ToolbarAction`s) come out. Keyboard shortcuts
//! produce the same actions, so a button and its shortcut can't behave differently.

use super::icons::{self, Icon};
use super::theme::{self, Palette, TEXT_SIZE};
use eframe::egui::{
    self, Align2, Color32, FontId, Galley, Id, Key, Margin, Modifiers, Rect, Response, RichText,
    Role, Sense, Stroke, StrokeKind, TextEdit, Ui, Widget, WidgetInfo, pos2, vec2,
};
use pdf_light::view;
use std::sync::Arc;

/// Something the owner asked for with a toolbar button or its shortcut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolbarAction {
    TogglePagesPanel,
    Open,
    Save,
    SaveAs,
    DeleteSelected,
    Undo,
    FindNext,
    FindPrevious,
    Export,
    Print,
    ZoomIn,
    ZoomOut,
    /// A zoom typed into the zoom box, in percent (not yet clamped).
    SetZoom(f32),
    FitWidth,
}

/// Where the search is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FindStatus {
    #[default]
    Idle,
    Searching,
    /// `current` counts from 1, as shown: "3 of 27".
    Matches {
        current: usize,
        total: usize,
    },
    NoMatches,
}

/// What the toolbar needs to know to draw itself and decide what is possible.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarState {
    /// The open file's name, e.g. "report.pdf"; `None` with no file open.
    pub file_name: Option<String>,
    pub is_modified: bool,
    pub can_undo: bool,
    pub selected_count: usize,
    pub page_count: usize,
    pub zoom_percent: f32,
    pub fit_width: bool,
    pub find: FindStatus,
    pub pages_panel_open: bool,
}

// `impl Default` says what `ToolbarState::default()` returns: here, no file open.
// (`#[derive(Default)]` would make the zoom 0 %, so it's written out.)
impl Default for ToolbarState {
    fn default() -> Self {
        Self {
            file_name: None,
            is_modified: false,
            can_undo: false,
            selected_count: 0,
            page_count: 0,
            zoom_percent: 100.0,
            fit_width: false,
            find: FindStatus::Idle,
            pages_panel_open: false,
        }
    }
}

/// Disabled tooltip for everything that needs an open file.
const OPEN_FIRST: &str = "Open a PDF first";

/// TEMPORARY: the tooltip for features whose task hasn't wired them yet. This is
/// the one place that keeps them disabled; Tasks 10–10b take their action out of
/// `not_built_yet`, and the last one deletes both.
const NOT_BUILT_YET: &str = "Not built yet";

/// TEMPORARY: see `NOT_BUILT_YET`.
fn not_built_yet(action: ToolbarAction) -> bool {
    matches!(action, ToolbarAction::Export | ToolbarAction::Print)
}

/// "Delete page" for one page, "Delete 3 pages" for several.
pub fn delete_label(selected: usize) -> String {
    if selected > 1 {
        format!("Delete {selected} pages")
    } else {
        "Delete page".to_owned()
    }
}

/// Why `action` isn't possible right now (the disabled button's tooltip), or
/// `None` when it is.
pub fn disabled_reason(action: ToolbarAction, state: &ToolbarState) -> Option<&'static str> {
    if action == ToolbarAction::Open {
        return None;
    }
    if state.file_name.is_none() {
        return Some(OPEN_FIRST);
    }
    let has_matches = matches!(state.find, FindStatus::Matches { total, .. } if total > 0);
    let reason = match action {
        ToolbarAction::Save if !state.is_modified => Some("No unsaved changes"),
        ToolbarAction::Undo if !state.can_undo => Some("Nothing to undo"),
        ToolbarAction::DeleteSelected if state.selected_count == 0 => {
            Some("Select pages in the sidebar first")
        }
        ToolbarAction::DeleteSelected if state.selected_count >= state.page_count => {
            Some("A PDF needs at least one page")
        }
        ToolbarAction::FindNext | ToolbarAction::FindPrevious if !has_matches => Some("No matches"),
        _ => None,
    };
    // The real reasons come first, so they stay right once the feature is built.
    reason.or_else(|| not_built_yet(action).then_some(NOT_BUILT_YET))
}

/// Why the find field can't be used right now, or `None` when it can.
fn find_field_reason(state: &ToolbarState) -> Option<&'static str> {
    state.file_name.is_none().then_some(OPEN_FIRST)
}

/// The icon on the button for `action`; `None` for actions without a button.
pub fn icon_for(action: ToolbarAction) -> Option<Icon> {
    let icon = match action {
        ToolbarAction::TogglePagesPanel => Icon::PagesPanel,
        ToolbarAction::Open => Icon::Open,
        ToolbarAction::Save => Icon::Save,
        ToolbarAction::SaveAs => Icon::SaveAs,
        ToolbarAction::DeleteSelected => Icon::DeletePage,
        ToolbarAction::Undo => Icon::Undo,
        ToolbarAction::FindNext => Icon::NextMatch,
        ToolbarAction::FindPrevious => Icon::PreviousMatch,
        ToolbarAction::Export => Icon::Export,
        ToolbarAction::Print => Icon::Print,
        ToolbarAction::ZoomIn => Icon::ZoomIn,
        ToolbarAction::ZoomOut => Icon::ZoomOut,
        ToolbarAction::FitWidth => Icon::FitWidth,
        // Typed into the zoom box; there is no button.
        ToolbarAction::SetZoom(_) => return None,
    };
    Some(icon)
}

/// The find field's preferred width, in logical pixels.
const FIND_WIDTH: f32 = 220.0;
/// The narrowest the find field gets before labels are dropped.
const FIND_MIN_WIDTH: f32 = 120.0;
/// Least space between the left groups and the right-aligned zoom group.
const ZOOM_GAP: f32 = 16.0;

/// How the toolbar fits the window (spec: shrink the find field to its
/// minimum, then drop labels, then wrap the zoom group to a second row).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fit {
    labels: bool,
    find_width: f32,
    zoom_wraps: bool,
}

/// The widths of the toolbar's two parts, measured one way (with or without labels).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Widths {
    /// The left groups, without the find field.
    left: f32,
    /// The right-aligned zoom group.
    zoom: f32,
}

/// Fits the toolbar into `available` pixels, given its widths with labels and
/// with icons only.
fn fit(available: f32, labelled: Widths, icons: Widths) -> Fit {
    // Room left for the find field when the zoom group shares the row.
    let room = |w: Widths| available - w.left - ZOOM_GAP - w.zoom;
    let (labels, zoom_wraps, field_room) = if room(labelled) >= FIND_MIN_WIDTH {
        (true, false, room(labelled))
    } else if room(icons) >= FIND_MIN_WIDTH {
        (false, false, room(icons))
    } else {
        (false, true, available - icons.left)
    };
    Fit {
        labels,
        find_width: field_room.clamp(FIND_MIN_WIDTH, FIND_WIDTH),
        zoom_wraps,
    }
}

/// The button's text label; `None` for icon-only buttons.
fn label(action: ToolbarAction, state: &ToolbarState) -> Option<String> {
    let text = match action {
        ToolbarAction::TogglePagesPanel => "Pages",
        ToolbarAction::Open => "Open",
        ToolbarAction::Save => "Save",
        ToolbarAction::SaveAs => "Save as",
        ToolbarAction::DeleteSelected => return Some(delete_label(state.selected_count)),
        ToolbarAction::Undo => "Undo",
        ToolbarAction::Export => "Export…",
        ToolbarAction::Print => "Print…",
        ToolbarAction::FitWidth => "Fit width",
        ToolbarAction::FindNext
        | ToolbarAction::FindPrevious
        | ToolbarAction::ZoomIn
        | ToolbarAction::ZoomOut
        | ToolbarAction::SetZoom(_) => return None,
    };
    Some(text.to_owned())
}

/// What the button does, plus its shortcut (the tooltip of an enabled button).
fn tooltip(action: ToolbarAction, state: &ToolbarState) -> String {
    let text = match action {
        ToolbarAction::TogglePagesPanel => "Show or hide the pages panel (Ctrl+B)",
        ToolbarAction::Open => "Open a PDF (Ctrl+O)",
        ToolbarAction::Save => {
            return match &state.file_name {
                Some(name) => format!("Save over {name} (Ctrl+S)"),
                None => "Save (Ctrl+S)".to_owned(),
            };
        }
        ToolbarAction::SaveAs => "Save a copy (Ctrl+Shift+S)",
        ToolbarAction::DeleteSelected => "Delete the selected pages (Delete)",
        ToolbarAction::Undo => "Undo the last delete (Ctrl+Z)",
        ToolbarAction::FindNext => "Next match (F3)",
        ToolbarAction::FindPrevious => "Previous match (Shift+F3)",
        ToolbarAction::Export => "Export pages as PNG or TIFF (Ctrl+E)",
        ToolbarAction::Print => "Print pages (Ctrl+P)",
        ToolbarAction::ZoomIn => "Zoom in (Ctrl++)",
        ToolbarAction::ZoomOut => "Zoom out (Ctrl+−)",
        ToolbarAction::SetZoom(_) => "Zoom",
        ToolbarAction::FitWidth => "Fit the page to the window width",
    };
    text.to_owned()
}

/// "125 %" for a zoom of 125.
fn zoom_label(zoom: f32) -> String {
    format!("{zoom:.0} %")
}

/// The text after the find field, and whether it is quiet (secondary).
fn find_status_text(find: FindStatus) -> Option<(String, bool)> {
    match find {
        FindStatus::Idle => None,
        FindStatus::Searching => Some(("Searching…".to_owned(), true)),
        FindStatus::Matches { current, total } => Some((format!("{current} of {total}"), false)),
        FindStatus::NoMatches => Some(("No matches".to_owned(), true)),
    }
}

/// Height of every toolbar control, in logical pixels.
const HEIGHT: f32 = 28.0;
/// Icons are drawn on a 16-unit grid at 1 px per unit.
const ICON_SIZE: f32 = 16.0;
/// Space from a button's left edge to its icon, and from the icon to the label.
const PAD: f32 = 6.0;
/// Space after a label, to the button's right edge.
const LABEL_END_PAD: f32 = 8.0;
/// Space between neighbouring controls.
const SPACING: f32 = 2.0;
/// A separator: a 1 px line with room either side.
const SEPARATOR_WIDTH: f32 = 13.0;
const ZOOM_BOX_WIDTH: f32 = 60.0;

/// One thing on the toolbar. The same list is first measured and then drawn,
/// so the measurement can't drift from what is on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Item {
    Button(ToolbarAction),
    Separator,
    FindField,
    /// "3 of 27", "No matches" or "Searching…".
    FindStatus,
    ZoomBox,
}

/// The right-aligned zoom group.
const ZOOM_ITEMS: [Item; 4] = [
    Item::Button(ToolbarAction::ZoomOut),
    Item::ZoomBox,
    Item::Button(ToolbarAction::ZoomIn),
    Item::Button(ToolbarAction::FitWidth),
];

/// The left groups in job order: pages panel · file · pages · find · output.
fn left_items(state: &ToolbarState) -> Vec<Item> {
    use ToolbarAction as A;
    let mut items = vec![
        Item::Button(A::TogglePagesPanel),
        Item::Separator,
        Item::Button(A::Open),
        Item::Button(A::Save),
        Item::Button(A::SaveAs),
        Item::Separator,
        Item::Button(A::DeleteSelected),
        Item::Button(A::Undo),
        Item::Separator,
        Item::FindField,
    ];
    // The ‹ › buttons appear with results.
    if matches!(state.find, FindStatus::Matches { .. }) {
        items.extend([Item::Button(A::FindPrevious), Item::Button(A::FindNext)]);
    }
    if find_status_text(state.find).is_some() {
        items.push(Item::FindStatus);
    }
    items.extend([
        Item::Separator,
        Item::Button(A::Export),
        Item::Button(A::Print),
    ]);
    items
}

/// Lays out `text` on one line at toolbar size; the colour is chosen when painting.
fn text_galley(ui: &Ui, text: String) -> Arc<Galley> {
    ui.painter()
        .layout_no_wrap(text, FontId::proportional(TEXT_SIZE), Color32::PLACEHOLDER)
}

/// A button's width, given its label's width (`None` for icon-only buttons).
fn button_width(label_width: Option<f32>) -> f32 {
    match label_width {
        Some(width) => PAD + ICON_SIZE + PAD + width + LABEL_END_PAD,
        None => HEIGHT,
    }
}

/// The width `items` take up in a row.
fn row_width(ui: &Ui, items: &[Item], state: &ToolbarState, labels: bool, find: f32) -> f32 {
    let widths: f32 = items
        .iter()
        .map(|&item| match item {
            Item::Button(action) => button_width(
                label(action, state)
                    .filter(|_| labels)
                    .map(|text| text_galley(ui, text).size().x),
            ),
            Item::Separator => SEPARATOR_WIDTH,
            Item::FindField => find,
            Item::FindStatus => {
                find_status_text(state.find).map_or(0.0, |(text, _)| text_galley(ui, text).size().x)
            }
            Item::ZoomBox => ZOOM_BOX_WIDTH,
        })
        .sum();
    widths + SPACING * items.len().saturating_sub(1) as f32
}

/// Draws the toolbar into `ui` and returns what the owner asked for this frame,
/// by click or by shortcut. `query` is the find field's text; Esc in the field
/// empties it, which ends the search.
pub fn show(ui: &mut Ui, state: &ToolbarState, query: &mut String) -> Vec<ToolbarAction> {
    let actions = shortcut_actions(ui.ctx(), state);
    let focus_find = ui.ctx().input_mut(|i| {
        i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::F))
    });
    ui.spacing_mut().item_spacing.x = SPACING;

    let left = left_items(state);
    let widths = |labels: bool| Widths {
        left: row_width(ui, &left, state, labels, 0.0),
        zoom: row_width(ui, &ZOOM_ITEMS, state, labels, 0.0),
    };
    let fit = fit(ui.available_width(), widths(true), widths(false));
    let zoom_width = widths(fit.labels).zoom;
    let mut toolbar = Toolbar {
        state,
        query,
        fit,
        focus_find,
        palette: theme::palette(ui.ctx()),
        actions,
    };
    ui.horizontal(|ui| {
        ui.set_min_height(HEIGHT);
        for &item in &left {
            toolbar.item(ui, item);
        }
        if !fit.zoom_wraps {
            // Push the zoom group to the right edge.
            ui.add_space((ui.available_width() - zoom_width).max(0.0));
            for item in ZOOM_ITEMS {
                toolbar.item(ui, item);
            }
        }
    });
    if fit.zoom_wraps {
        ui.horizontal(|ui| {
            ui.set_min_height(HEIGHT);
            ui.add_space((ui.available_width() - zoom_width).max(0.0));
            for item in ZOOM_ITEMS {
                toolbar.item(ui, item);
            }
        });
    }
    toolbar.actions
}

/// The actions asked for with the keyboard this frame, if they're possible.
fn shortcut_actions(ctx: &egui::Context, state: &ToolbarState) -> Vec<ToolbarAction> {
    use ToolbarAction as A;
    let ctrl = Modifiers::COMMAND;
    // Shift and Alt are ignored when a shortcut doesn't name them, so the more
    // specific shortcut comes first: Ctrl+Shift+S before Ctrl+S.
    let mut shortcuts = vec![
        (ctrl | Modifiers::SHIFT, Key::S, A::SaveAs),
        (ctrl, Key::S, A::Save),
        (ctrl, Key::O, A::Open),
        (ctrl, Key::E, A::Export),
        (ctrl, Key::P, A::Print),
        (ctrl, Key::B, A::TogglePagesPanel),
        (Modifiers::SHIFT, Key::F3, A::FindPrevious),
        (Modifiers::NONE, Key::F3, A::FindNext),
    ];
    // While typing in a field, these keys belong to the field.
    if !ctx.text_edit_focused() {
        shortcuts.extend([
            (ctrl, Key::Z, A::Undo),
            (Modifiers::NONE, Key::Delete, A::DeleteSelected),
            // Ctrl+= also zooms in, so Shift isn't needed on US keyboards.
            (ctrl, Key::Plus, A::ZoomIn),
            (ctrl, Key::Equals, A::ZoomIn),
            (ctrl, Key::Minus, A::ZoomOut),
        ]);
    }
    let mut actions = Vec::new();
    ctx.input_mut(|i| {
        for (modifiers, key, action) in shortcuts {
            // Count every press this frame (key repeat can deliver several at once).
            let presses = i.count_and_consume_key(modifiers, key);
            actions.extend(std::iter::repeat_n(action, presses));
        }
    });
    actions.retain(|&action| disabled_reason(action, state).is_none());
    actions
}

/// What drawing the toolbar needs, gathered so each item is one method call.
struct Toolbar<'a> {
    state: &'a ToolbarState,
    query: &'a mut String,
    fit: Fit,
    /// Ctrl+F was pressed this frame.
    focus_find: bool,
    palette: &'static Palette,
    actions: Vec<ToolbarAction>,
}

impl Toolbar<'_> {
    fn item(&mut self, ui: &mut Ui, item: Item) {
        match item {
            Item::Button(action) => self.button(ui, action),
            Item::Separator => separator(ui, self.palette),
            Item::FindField => self.find_field(ui),
            Item::FindStatus => {
                if let Some((text, quiet)) = find_status_text(self.state.find) {
                    let colour = if quiet {
                        self.palette.quiet
                    } else {
                        self.palette.ink
                    };
                    ui.label(RichText::new(text).color(colour));
                }
            }
            Item::ZoomBox => self.zoom_box(ui),
        }
    }

    fn button(&mut self, ui: &mut Ui, action: ToolbarAction) {
        let Some(icon) = icon_for(action) else {
            return;
        };
        let reason = disabled_reason(action, self.state);
        let label = label(action, self.state).filter(|_| self.fit.labels);
        let tooltip = tooltip(action, self.state);
        let active = reason.is_none()
            && match action {
                ToolbarAction::FitWidth => self.state.fit_width,
                ToolbarAction::TogglePagesPanel => self.state.pages_panel_open,
                _ => false,
            };
        let button = ToolButton {
            icon,
            label: label.as_deref(),
            name: label.as_deref().unwrap_or(&tooltip),
            active,
            danger_on_hover: action == ToolbarAction::DeleteSelected,
            filled: false,
            palette: self.palette,
        };
        let mut response = ui.add_enabled(reason.is_none(), button);
        response = response.on_hover_text(&tooltip);
        if let Some(reason) = reason {
            response = response.on_disabled_hover_text(reason);
        }
        if response.clicked() {
            self.actions.push(action);
        }
    }

    fn find_field(&mut self, ui: &mut Ui) {
        let reason = find_field_reason(self.state);
        let id = Id::unique("find-field");
        let edit = TextEdit::singleline(self.query)
            .id(id)
            .hint_text("Find in document")
            .desired_width(self.fit.find_width)
            // Room on the left for the magnifier.
            .margin(Margin {
                left: 26,
                right: 6,
                top: 4,
                bottom: 4,
            });
        let mut response = ui.add_enabled(reason.is_none(), edit);
        let icon_colour = if reason.is_none() {
            self.palette.quiet
        } else {
            self.palette.disabled()
        };
        let icon_rect = Rect::from_center_size(
            pos2(response.rect.left() + 13.0, response.rect.center().y),
            vec2(ICON_SIZE, ICON_SIZE),
        );
        icons::paint(ui.painter(), snap(ui, icon_rect), Icon::Find, icon_colour);
        response = response.on_hover_text("Find in document (Ctrl+F)");
        if let Some(reason) = reason {
            response.on_disabled_hover_text(reason);
            return;
        }
        if self.focus_find {
            response.request_focus();
        }
        // Enter finds the next match, Shift+Enter the previous one; the field
        // keeps the focus so Enter can be pressed again.
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            let action = if ui.input(|i| i.modifiers.shift) {
                ToolbarAction::FindPrevious
            } else {
                ToolbarAction::FindNext
            };
            if disabled_reason(action, self.state).is_none() {
                self.actions.push(action);
            }
            response.request_focus();
        }
        // egui drops the field's focus as soon as Esc is pressed, so the pages
        // panel would see the key next and clear its selection. Taking the key
        // here keeps it for the field: Esc closes the search, nothing else.
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            self.query.clear();
        }
    }

    fn zoom_box(&mut self, ui: &mut Ui) {
        let reason = disabled_reason(ToolbarAction::SetZoom(100.0), self.state);
        let id = Id::unique("zoom-box");
        let text_id = id.with("text");
        let current = zoom_label(self.state.zoom_percent);
        // While the owner types, the text lives in egui's memory between frames;
        // otherwise the box shows the current zoom.
        let mut text = if ui.memory(|m| m.has_focus(id)) {
            ui.data(|d| d.get_temp::<String>(text_id))
                .unwrap_or_else(|| current.clone())
        } else {
            current
        };
        let edit = TextEdit::singleline(&mut text)
            .id(id)
            .desired_width(ZOOM_BOX_WIDTH)
            .align(Align2::CENTER_CENTER)
            .margin(Margin::symmetric(4, 4));
        let mut response = ui.add_enabled(reason.is_none(), edit);
        if let Some(reason) = reason {
            response = response.on_disabled_hover_text(reason);
        }
        if response.lost_focus()
            && let Some(zoom) = view::parse_zoom(&text)
            // Clicking in and out again shouldn't switch off "Fit width".
            && (zoom - self.state.zoom_percent).abs() >= 0.5
        {
            self.actions.push(ToolbarAction::SetZoom(zoom));
        }
        ui.data_mut(|d| d.insert_temp(text_id, text));
    }
}

/// A thin Quiet line between groups: "different job".
fn separator(ui: &mut Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(SEPARATOR_WIDTH, HEIGHT), Sense::hover());
    let x = rect.center().x;
    let (top, bottom) = (rect.center().y - 9.0, rect.center().y + 9.0);
    ui.painter().line_segment(
        [pos2(x, top), pos2(x, bottom)],
        Stroke::new(1.0, palette.quiet),
    );
}

/// Moves `rect` to whole physical pixels so the icon's lines stay crisp.
fn snap(ui: &Ui, rect: Rect) -> Rect {
    let ppp = ui.ctx().pixels_per_point();
    let round = |v: f32| (v * ppp).round() / ppp;
    rect.translate(vec2(
        round(rect.min.x) - rect.min.x,
        round(rect.min.y) - rect.min.y,
    ))
}

/// A toolbar button: a 16 px icon plus an optional label, frameless until
/// hovered, with a focus ring, and a Marker underline while `active`.
pub struct ToolButton<'a> {
    pub icon: Icon,
    pub label: Option<&'a str>,
    /// What screen readers announce (the tooltip for icon-only buttons).
    pub name: &'a str,
    /// Draws a 2 px Marker underline ("Fit width" while fitting, for example).
    pub active: bool,
    /// Turns the icon and label Danger on hover (Delete).
    pub danger_on_hover: bool,
    /// Gives the button a Panel background, for buttons drawn on the desk.
    pub filled: bool,
    pub palette: &'a Palette,
}

// `Widget` is egui's trait for "something `ui.add` can draw"; implementing it
// lets `ui.add_enabled(enabled, button)` handle the disabled state for us.
impl Widget for ToolButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let galley = self.label.map(|text| text_galley(ui, text.to_owned()));
        let width = button_width(galley.as_ref().map(|g| g.size().x));
        let (rect, response) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::click());
        let enabled = ui.is_enabled();
        response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, self.name));
        if !ui.is_rect_visible(rect) {
            return response;
        }

        let palette = self.palette;
        let painter = ui.painter();
        let pressed = enabled && response.is_pointer_button_down_on();
        let hovered = enabled && response.hovered();
        let fill = if pressed {
            Some(palette.pressed())
        } else if hovered {
            Some(palette.hover())
        } else if self.filled {
            Some(palette.panel)
        } else {
            None
        };
        if let Some(fill) = fill {
            painter.rect_filled(rect, 4, fill);
        }

        let colour = if !enabled {
            palette.disabled()
        } else if self.danger_on_hover && hovered {
            palette.danger
        } else {
            palette.ink
        };
        let icon_left = if galley.is_some() {
            rect.left() + PAD
        } else {
            rect.center().x - ICON_SIZE / 2.0
        };
        let icon_rect = Rect::from_min_size(
            pos2(icon_left, rect.center().y - ICON_SIZE / 2.0),
            vec2(ICON_SIZE, ICON_SIZE),
        );
        let icon_rect = snap(ui, icon_rect);
        icons::paint(painter, icon_rect, self.icon, colour);
        if let Some(galley) = galley {
            // Segoe UI's line box sits a little low; 1 px up centres the letters on the icon.
            let top = rect.center().y - galley.size().y / 2.0 - 1.0;
            painter.galley(pos2(icon_rect.right() + PAD, top), galley, colour);
        }

        if self.active {
            let underline = Rect::from_min_max(
                pos2(rect.left() + PAD, rect.bottom() - 2.0),
                pos2(rect.right() - PAD, rect.bottom()),
            );
            painter.rect_filled(underline, 0, palette.marker);
        }
        if response.has_focus() {
            painter.rect_stroke(rect, 4, Stroke::new(1.0, palette.ink), StrokeKind::Inside);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_document() -> ToolbarState {
        ToolbarState::default()
    }

    fn document(selected_count: usize, page_count: usize) -> ToolbarState {
        ToolbarState {
            file_name: Some("report.pdf".to_owned()),
            selected_count,
            page_count,
            ..no_document()
        }
    }

    #[test]
    fn delete_label_should_count_pages() {
        assert_eq!(
            [delete_label(1), delete_label(3), delete_label(0)],
            ["Delete page", "Delete 3 pages", "Delete page"]
        );
    }

    #[test]
    fn disabled_reason_should_require_a_document() {
        let state = no_document();
        let needs_document = [
            ToolbarAction::Save,
            ToolbarAction::SaveAs,
            ToolbarAction::DeleteSelected,
            ToolbarAction::Export,
            ToolbarAction::FindNext,
            ToolbarAction::Print,
            ToolbarAction::TogglePagesPanel,
        ];
        let reasons: Vec<_> = needs_document
            .into_iter()
            .map(|action| disabled_reason(action, &state))
            .collect();
        assert_eq!(
            reasons,
            vec![Some("Open a PDF first"); needs_document.len()]
        );
    }

    #[test]
    fn disabled_reason_should_allow_open_without_a_document() {
        assert_eq!(disabled_reason(ToolbarAction::Open, &no_document()), None);
    }

    #[test]
    fn disabled_reason_should_explain_save_and_undo() {
        let state = document(0, 5);
        assert_eq!(
            (
                disabled_reason(ToolbarAction::Save, &state),
                disabled_reason(ToolbarAction::Undo, &state)
            ),
            (Some("No unsaved changes"), Some("Nothing to undo"))
        );
    }

    #[test]
    fn disabled_reason_should_guard_delete() {
        let reason = |selected, pages| {
            disabled_reason(ToolbarAction::DeleteSelected, &document(selected, pages))
        };
        assert_eq!(
            [reason(0, 5), reason(5, 5), reason(2, 5)],
            [
                Some("Select pages in the sidebar first"),
                Some("A PDF needs at least one page"),
                None
            ]
        );
    }

    #[test]
    fn disabled_reason_should_allow_page_editing_and_saving() {
        let state = ToolbarState {
            is_modified: true,
            can_undo: true,
            ..document(2, 5)
        };
        let actions = [
            ToolbarAction::TogglePagesPanel,
            ToolbarAction::Save,
            ToolbarAction::SaveAs,
            ToolbarAction::DeleteSelected,
            ToolbarAction::Undo,
        ];
        let reasons: Vec<_> = actions
            .into_iter()
            .map(|action| disabled_reason(action, &state))
            .collect();
        assert_eq!(reasons, vec![None; actions.len()]);
    }

    #[test]
    fn disabled_reason_should_allow_stepping_only_with_matches() {
        let with = |find| ToolbarState {
            find,
            ..document(0, 5)
        };
        let matches = with(FindStatus::Matches {
            current: 1,
            total: 3,
        });
        let reasons = [
            disabled_reason(ToolbarAction::FindNext, &matches),
            disabled_reason(ToolbarAction::FindPrevious, &matches),
            disabled_reason(ToolbarAction::FindNext, &with(FindStatus::NoMatches)),
            disabled_reason(ToolbarAction::FindPrevious, &with(FindStatus::Searching)),
        ];
        assert_eq!(
            reasons,
            [None, None, Some("No matches"), Some("No matches")]
        );
    }

    #[test]
    fn find_field_should_be_usable_once_a_document_is_open() {
        assert_eq!(
            (
                find_field_reason(&no_document()),
                find_field_reason(&document(0, 5))
            ),
            (Some("Open a PDF first"), None)
        );
    }

    #[test]
    fn disabled_reason_should_keep_save_as_without_changes() {
        assert_eq!(
            disabled_reason(ToolbarAction::SaveAs, &document(0, 5)),
            None
        );
    }

    const LABELLED: Widths = Widths {
        left: 600.0,
        zoom: 200.0,
    };
    const ICONS: Widths = Widths {
        left: 300.0,
        zoom: 150.0,
    };

    #[test]
    fn fit_should_keep_labels_and_full_find_field_when_wide() {
        assert_eq!(
            fit(1200.0, LABELLED, ICONS),
            Fit {
                labels: true,
                find_width: FIND_WIDTH,
                zoom_wraps: false
            }
        );
    }

    #[test]
    fn fit_should_shrink_find_field_before_dropping_labels() {
        // 1000 - 600 - 16 - 200 leaves 184 px for the field.
        assert_eq!(fit(1000.0, LABELLED, ICONS).find_width, 184.0);
    }

    #[test]
    fn fit_should_drop_labels_before_wrapping() {
        assert_eq!(
            fit(900.0, LABELLED, ICONS),
            Fit {
                labels: false,
                find_width: FIND_WIDTH,
                zoom_wraps: false
            }
        );
    }

    #[test]
    fn fit_should_wrap_zoom_when_icons_alone_do_not_fit() {
        assert_eq!(
            fit(500.0, LABELLED, ICONS),
            Fit {
                labels: false,
                find_width: 200.0,
                zoom_wraps: true
            }
        );
    }
}

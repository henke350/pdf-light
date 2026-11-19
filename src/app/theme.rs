//! Colours and type: the palette for light and dark (following Windows), and
//! Segoe UI loaded from `C:\Windows\Fonts` at start-up.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontFamily, FontId, Shadow, Stroke, TextStyle, Theme,
    ThemePreference, Visuals,
    epaint::text::{FontInsert, InsertFontFamily, VariationCoords},
};

/// The design's colour tokens (spec section "Palette").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Background around the pages.
    pub desk: Color32,
    /// Toolbar, sidebar, status bar, dialogs.
    pub panel: Color32,
    /// Text and focus ring.
    pub ink: Color32,
    /// Secondary text, separators; disabled at 50 % (see `disabled`).
    pub quiet: Color32,
    /// The highlighter yellow: search matches, active toggles, selection stripe.
    pub marker: Color32,
    /// "Delete" on hover, destructive confirm buttons.
    pub danger: Color32,
}

impl Palette {
    /// Disabled text and icons: Quiet at 50 % over the panel, pre-blended so it
    /// stays opaque (icons overlap their own strokes; see `icons::paint`).
    pub fn disabled(&self) -> Color32 {
        self.panel.lerp_to_gamma(self.quiet, 0.5)
    }

    /// Button background on hover: Panel shifted about 8 % toward Ink.
    pub fn hover(&self) -> Color32 {
        self.panel.lerp_to_gamma(self.ink, 0.08)
    }

    /// Button background while pressed.
    pub fn pressed(&self) -> Color32 {
        self.panel.lerp_to_gamma(self.ink, 0.14)
    }

    /// Text drawn straight on the desk (the empty state).
    pub fn on_desk(&self) -> Color32 {
        if self.desk == LIGHT.desk {
            LIGHT.panel
        } else {
            DARK.ink
        }
    }
}

// `Color32::from_rgb` is a `const fn`, so the palettes are compile-time constants.
pub const LIGHT: Palette = Palette {
    desk: Color32::from_rgb(0x5A, 0x65, 0x70),
    panel: Color32::from_rgb(0xEE, 0xF1, 0xF3),
    ink: Color32::from_rgb(0x1D, 0x25, 0x2C),
    quiet: Color32::from_rgb(0x66, 0x72, 0x7C),
    marker: Color32::from_rgb(0xFF, 0xD4, 0x3B),
    danger: Color32::from_rgb(0xB4, 0x23, 0x18),
};

pub const DARK: Palette = Palette {
    desk: Color32::from_rgb(0x2B, 0x32, 0x38),
    panel: Color32::from_rgb(0x1E, 0x23, 0x28),
    ink: Color32::from_rgb(0xE6, 0xEA, 0xED),
    quiet: Color32::from_rgb(0x9A, 0xA5, 0xAE),
    marker: Color32::from_rgb(0xF5, 0xC5, 0x18),
    danger: Color32::from_rgb(0xF0, 0x85, 0x7A),
};

/// Toolbar and dialog text size, in logical pixels.
pub const TEXT_SIZE: f32 = 14.0;
/// Status bar text size.
pub const SMALL_SIZE: f32 = 12.0;
/// The empty state's main line.
pub const LARGE_SIZE: f32 = 18.0;

/// The palette for the theme `ctx` is showing right now.
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        Theme::Light => &LIGHT,
        Theme::Dark => &DARK,
    }
}

/// Sets up colours (light and dark, following the Windows setting), text sizes and fonts.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(ThemePreference::System);
    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT, Visuals::light()));
    ctx.set_visuals_of(Theme::Dark, visuals(&DARK, Visuals::dark()));
    ctx.all_styles_mut(|style| {
        for (text_style, size) in [
            (TextStyle::Body, TEXT_SIZE),
            (TextStyle::Button, TEXT_SIZE),
            (TextStyle::Heading, LARGE_SIZE),
            (TextStyle::Small, SMALL_SIZE),
        ] {
            style
                .text_styles
                .insert(text_style, FontId::proportional(size));
        }
    });
    load_system_font(ctx);
}

/// egui's light or dark `base` visuals, recoloured with `palette`.
fn visuals(palette: &Palette, base: Visuals) -> Visuals {
    let mut visuals = base;
    visuals.panel_fill = palette.panel;
    visuals.window_fill = palette.panel;
    visuals.window_stroke = Stroke::new(1.0, palette.quiet);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.weak_text_color = Some(palette.quiet);
    visuals.hyperlink_color = palette.ink;
    visuals.error_fg_color = palette.danger;
    // The focus ring around text fields and focused buttons.
    visuals.selection.stroke = Stroke::new(1.0, palette.ink);
    // Selected text is highlighted like a marker swipe.
    visuals.selection.bg_fill = palette.marker.gamma_multiply(0.45);

    let widgets = &mut visuals.widgets;
    // Labels and separators.
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.ink);
    widgets.noninteractive.bg_stroke =
        Stroke::new(1.0, palette.panel.lerp_to_gamma(palette.quiet, 0.5));
    widgets.noninteractive.bg_fill = palette.panel;
    widgets.noninteractive.weak_bg_fill = palette.panel;
    // Buttons and fields: frameless until hovered.
    for (state, fill) in [
        (&mut widgets.inactive, palette.panel),
        (&mut widgets.hovered, palette.hover()),
        (&mut widgets.active, palette.pressed()),
        (&mut widgets.open, palette.hover()),
    ] {
        state.fg_stroke = Stroke::new(1.0, palette.ink);
        state.weak_bg_fill = fill;
        state.bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.corner_radius = CornerRadius::same(4);
        state.expansion = 0.0;
    }
    // Text fields get a thin Quiet outline so they read as fields.
    widgets.inactive.bg_stroke = Stroke::new(1.0, palette.panel.lerp_to_gamma(palette.quiet, 0.6));
    widgets.hovered.bg_stroke = Stroke::new(1.0, palette.quiet);
    visuals.text_edit_bg_color = Some(if visuals.dark_mode {
        palette.desk
    } else {
        Color32::WHITE
    });
    visuals.extreme_bg_color = visuals.text_edit_bg_color.unwrap_or(palette.panel);
    visuals
}

/// Uses Segoe UI Variable Text (Windows 11), else Segoe UI, else keeps egui's
/// built-in font. The file is read at start-up rather than embedded, so the
/// exe doesn't grow; if neither file can be read, the app carries on as is.
fn load_system_font(ctx: &egui::Context) {
    let candidates = [
        (r"C:\Windows\Fonts\SegUIVar.ttf", true),
        (r"C:\Windows\Fonts\segoeui.ttf", false),
    ];
    // `find_map` stops at the first file that reads; `.ok()?` skips the ones that don't.
    let Some((bytes, variable)) = candidates
        .into_iter()
        .find_map(|(path, variable)| Some((std::fs::read(path).ok()?, variable)))
    else {
        return;
    };
    let mut data = FontData::from_owned(bytes);
    if variable {
        // The "Text" optical size of the variable font, at regular weight.
        let mut tweak = data.tweak.clone();
        tweak.coords = VariationCoords::new([(b"opsz", 10.5), (b"wght", 400.0)]);
        data = data.tweak(tweak);
    }
    ctx.add_font(FontInsert::new(
        "segoe-ui",
        data,
        vec![InsertFontFamily {
            family: FontFamily::Proportional,
            priority: egui::epaint::text::FontPriority::Highest,
        }],
    ));
}

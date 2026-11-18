//! Small modal dialogs.

use eframe::egui;

/// A message for the error dialog.
pub struct ErrorMessage {
    pub title: String,
    pub text: String,
}

impl ErrorMessage {
    /// Builds a message from a title and an error; the error's text becomes a sentence.
    pub fn new(title: impl Into<String>, error: &impl std::fmt::Display) -> Self {
        Self {
            title: title.into(),
            text: sentence(&error.to_string()),
        }
    }
}

/// Shows `message` (if any) in a modal dialog; OK, Esc or a click outside closes it.
pub fn error_dialog(ctx: &egui::Context, message: &mut Option<ErrorMessage>) {
    let Some(current) = message.as_ref() else {
        return;
    };
    let response = egui::Modal::new(egui::Id::unique("error-dialog")).show(ctx, |ui| {
        ui.set_max_width(380.0);
        ui.heading(&current.title);
        ui.add_space(6.0);
        ui.label(&current.text);
        ui.add_space(10.0);
        ui.button("OK").clicked()
    });
    if response.inner || response.should_close() {
        *message = None;
    }
}

/// Capitalises the first letter and adds a full stop: "not a pdf" → "Not a pdf.".
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

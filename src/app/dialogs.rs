//! Small modal dialogs.

use super::theme;
use eframe::egui::{self, RichText};

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

/// The answer to the unsaved-changes dialog. An `enum` lists every possible
/// answer, so the code that acts on it has to handle each one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

/// "Save changes to report.pdf?" with Save / Discard / Cancel. Returns the
/// owner's answer in the frame they give it, `None` while the dialog stays
/// open. Esc or a click outside counts as Cancel.
pub fn unsaved_changes_dialog(ctx: &egui::Context, name: &str) -> Option<UnsavedChoice> {
    let danger = theme::palette(ctx).danger;
    let response = egui::Modal::new(egui::Id::unique("unsaved-dialog")).show(ctx, |ui| {
        ui.set_max_width(380.0);
        ui.heading(format!("Save changes to {name}?"));
        ui.add_space(6.0);
        ui.label("You deleted pages. If you don't save, the deletions are lost.");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                Some(UnsavedChoice::Save)
            } else if ui.button(RichText::new("Discard").color(danger)).clicked() {
                Some(UnsavedChoice::Discard)
            } else if ui.button("Cancel").clicked() {
                Some(UnsavedChoice::Cancel)
            } else {
                None
            }
        })
        .inner
    });
    if response.should_close() {
        return Some(UnsavedChoice::Cancel);
    }
    response.inner
}

/// "Overwrite report.pdf?" before the first Save over the open file. Returns
/// `Some(true)` for Overwrite, `Some(false)` for Cancel (or Esc, or a click
/// outside), `None` while the dialog stays open.
pub fn overwrite_dialog(ctx: &egui::Context, name: &str) -> Option<bool> {
    let danger = theme::palette(ctx).danger;
    let response = egui::Modal::new(egui::Id::unique("overwrite-dialog")).show(ctx, |ui| {
        ui.set_max_width(380.0);
        ui.heading(format!("Overwrite {name}?"));
        ui.add_space(6.0);
        ui.label("The deleted pages will be gone from the file itself. Use Save as to keep the original.");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Overwrite").color(danger)).clicked() {
                Some(true)
            } else if ui.button("Cancel").clicked() {
                Some(false)
            } else {
                None
            }
        })
        .inner
    });
    if response.should_close() {
        return Some(false);
    }
    response.inner
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

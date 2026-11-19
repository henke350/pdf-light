//! Entry point: starts the window and opens the file named on the command line, if any.

// In release builds this is a Windows GUI program, so no console window opens
// next to it. Debug builds keep the console for panic messages and logs.
// `cfg_attr(condition, attribute)` applies the attribute only when the condition holds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use eframe::egui;
use std::path::PathBuf;

/// The window icon's size in pixels; Windows scales it for the title bar and taskbar.
const WINDOW_ICON_SIZE: u32 = 64;

// `anyhow::Result` holds any error type, which suits the top of the program.
fn main() -> anyhow::Result<()> {
    // The first argument is the file to open, so Windows "Open with" works.
    // `absolute` makes "report.pdf" a full path from the current folder, so the
    // Save as folder and "is this the same file?" checks work; if it fails, the
    // path is used as given.
    let path = std::env::args_os().nth(1).map(|arg| {
        let path = PathBuf::from(arg);
        std::path::absolute(&path).unwrap_or(path)
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("pdf-light")
            // Wide enough for the toolbar with every label (narrower windows drop them).
            .with_inner_size([1180.0, 760.0])
            .with_drag_and_drop(true)
            // The title bar and taskbar icon; the exe's own icon comes from build.rs.
            .with_icon(egui::IconData {
                rgba: pdf_light::app_icon::rgba(WINDOW_ICON_SIZE),
                width: WINDOW_ICON_SIZE,
                height: WINDOW_ICON_SIZE,
            }),
        // OpenGL starts faster and makes a smaller exe than wgpu (see Cargo.toml).
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let started = eframe::run_native(
        "pdf-light",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, path)))),
    );
    if let Err(error) = &started {
        // Without a console an error returned from `main` is never seen, so
        // say it in a message box before giving up.
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("pdf-light")
            .set_description(format!(
                "pdf-light couldn't start: {error}. It needs a graphics driver with OpenGL 2 or newer."
            ))
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
    started.map_err(|e| anyhow::anyhow!("couldn't start the window: {e}"))
}

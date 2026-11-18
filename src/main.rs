//! Entry point: starts the window and opens the file named on the command line, if any.

mod app;

use eframe::egui;
use std::path::PathBuf;

// `anyhow::Result` holds any error type, which suits the top of the program.
fn main() -> anyhow::Result<()> {
    // The first argument is the file to open, so Windows "Open with" works.
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("pdf-light")
            .with_inner_size([900.0, 760.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "pdf-light",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, path)))),
    )
    .map_err(|e| anyhow::anyhow!("couldn't start the window: {e}"))
}

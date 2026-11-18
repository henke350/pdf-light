//! Entry point: opens an empty window titled "pdf-light".

use eframe::egui;

/// The application state. Empty for now; later tasks add fields here.
struct PdfLightApp;

impl eframe::App for PdfLightApp {
    // `&mut self` lets the method change the app's state; `ui` is called every frame.
    fn ui(&mut self, _ui: &mut egui::Ui, _frame: &mut eframe::Frame) {}
}

// `Result` means "this can fail". Returning it from `main` makes the program exit with an error code if the window fails to start.
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("pdf-light"),
        ..Default::default()
    };
    eframe::run_native(
        "pdf-light",
        options,
        Box::new(|_cc| Ok(Box::new(PdfLightApp))),
    )
}

//! The window: toolbar, page view, status bar and dialogs.

mod dialogs;
mod page_view;

use dialogs::{ErrorMessage, error_dialog};
use eframe::egui;
use page_view::PageView;
use pdf_light::document::{self, Document};
use pdf_light::render_pool::RenderPool;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// An open PDF and everything that belongs to it; replaced as a whole when
/// another file is opened (dropping the old pool stops its workers).
struct OpenDocument {
    document: Document,
    pool: RenderPool,
    view: PageView,
}

/// The application state.
pub struct App {
    open: Option<OpenDocument>,
    error: Option<ErrorMessage>,
    render_threads: usize,
}

impl App {
    /// Sets up the app and opens `path` if one was given on the command line.
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        // Ctrl+plus/minus zoom the page, not egui's own UI scale.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let cores = std::thread::available_parallelism().map_or(1, NonZero::get);
        let mut app = Self {
            open: None,
            error: None,
            // Leave one core for the UI, but always use at least two workers.
            render_threads: cores.saturating_sub(1).max(2),
        };
        if let Some(path) = path {
            app.open_file(&cc.egui_ctx, &path);
        }
        app
    }

    /// Opens `path`, replacing the current document, or shows why it can't.
    fn open_file(&mut self, ctx: &egui::Context, path: &Path) {
        match document::open(path) {
            Ok(document) => {
                let repaint_ctx = ctx.clone();
                // The pool calls this from its threads; `request_repaint` is thread-safe.
                let wake: Arc<dyn Fn() + Send + Sync> =
                    Arc::new(move || repaint_ctx.request_repaint());
                let pool = RenderPool::new(Arc::clone(&document.bytes), self.render_threads, wake);
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                    "{} - pdf-light",
                    file_name(path)
                )));
                self.open = Some(OpenDocument {
                    document,
                    pool,
                    view: PageView::new(),
                });
            }
            Err(error) => {
                let title = format!("Couldn't open {}", file_name(path));
                self.error = Some(ErrorMessage::new(title, &error));
            }
        }
    }

    /// Shows the Open dialog (it blocks until closed, like any modal dialog).
    fn pick_and_open(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .pick_file()
        {
            self.open_file(ctx, &path);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Open").on_hover_text("Ctrl+O").clicked() {
                let ctx = ui.ctx().clone();
                self.pick_and_open(&ctx);
            }
            if let Some(open) = &mut self.open {
                ui.separator();
                open.view.zoom_controls(ui, &open.document.page_sizes);
            }
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| match &self.open {
            Some(open) => {
                ui.label(file_name(&open.document.path));
                ui.separator();
                ui.label(format!(
                    "page {} of {}",
                    open.view.current_page() + 1,
                    open.document.page_sizes.len()
                ));
            }
            None => {
                ui.label("Open a PDF with Ctrl+O or drag one onto the window.");
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.open_file(&ctx, &path);
        }
        let open_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
        if ctx.input_mut(|i| i.consume_shortcut(&open_shortcut)) {
            self.pick_and_open(&ctx);
        }

        if let Some(open) = &mut self.open {
            // Turn finished renders into textures; this never waits.
            while let Some(result) = open.pool.try_recv() {
                open.view.receive(&ctx, result);
            }
        }

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(page_view::BACKGROUND))
            .show(ui, |ui| {
                if let Some(open) = &mut self.open {
                    open.view.show(ui, &open.document.page_sizes, &open.pool);
                }
            });

        error_dialog(&ctx, &mut self.error);
    }
}

/// The file name for titles and messages, e.g. "report.pdf".
fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

//! The window: toolbar, page view, status bar and dialogs.

mod dialogs;
mod icons;
mod page_view;
mod theme;
mod toolbar;

use dialogs::{ErrorMessage, error_dialog};
use eframe::egui::{self, Frame, Margin, RichText};
use page_view::PageView;
use pdf_light::document::{self, Document};
use pdf_light::render_pool::RenderPool;
use pdf_light::view;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use toolbar::{ToolbarAction, ToolbarState};

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
    /// The find field's text.
    find_query: String,
}

impl App {
    /// Sets up the app and opens `path` if one was given on the command line.
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        // Ctrl+plus/minus zoom the page, not egui's own UI scale.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        theme::apply(&cc.egui_ctx);
        let cores = std::thread::available_parallelism().map_or(1, NonZero::get);
        let mut app = Self {
            open: None,
            error: None,
            // Leave one core for the UI, but always use at least two workers.
            render_threads: cores.saturating_sub(1).max(2),
            find_query: String::new(),
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

    /// What the toolbar can offer right now.
    fn toolbar_state(&self) -> ToolbarState {
        let Some(open) = &self.open else {
            return ToolbarState::default();
        };
        // Selection, history, find and the pages panel are filled in by Tasks 8-10b.
        ToolbarState {
            file_name: Some(file_name(&open.document.path)),
            page_count: open.document.page_sizes.len(),
            zoom_percent: open.view.zoom(),
            fit_width: open.view.fits_width(),
            ..ToolbarState::default()
        }
    }

    /// Carries out a toolbar button, its shortcut, or the empty state's Open.
    fn act(&mut self, ctx: &egui::Context, action: ToolbarAction) {
        let now = ctx.input(|i| i.time);
        match action {
            ToolbarAction::Open => self.pick_and_open(ctx),
            ToolbarAction::ZoomIn
            | ToolbarAction::ZoomOut
            | ToolbarAction::SetZoom(_)
            | ToolbarAction::FitWidth => {
                let Some(open) = &mut self.open else {
                    return;
                };
                let (view, sizes) = (&mut open.view, &open.document.page_sizes);
                match action {
                    ToolbarAction::ZoomIn => {
                        view.zoom_to(view::zoom_in_step(view.zoom()), sizes, now);
                    }
                    ToolbarAction::ZoomOut => {
                        view.zoom_to(view::zoom_out_step(view.zoom()), sizes, now);
                    }
                    ToolbarAction::SetZoom(zoom) => view.zoom_to(zoom, sizes, now),
                    _ => view.fit_width(),
                }
            }
            // Drawn but disabled until Tasks 8-10b wire them.
            ToolbarAction::TogglePagesPanel
            | ToolbarAction::Save
            | ToolbarAction::SaveAs
            | ToolbarAction::DeleteSelected
            | ToolbarAction::Undo
            | ToolbarAction::FindNext
            | ToolbarAction::FindPrevious
            | ToolbarAction::Export
            | ToolbarAction::Print => {}
        }
    }

    /// File name ("(edited)" while there are unsaved changes) and "Page N of M".
    fn status_bar(&self, ui: &mut egui::Ui, state: &ToolbarState) {
        let palette = theme::palette(ui.ctx());
        ui.horizontal(|ui| {
            let (Some(open), Some(name)) = (&self.open, &state.file_name) else {
                return;
            };
            ui.label(
                RichText::new(file_status(name, state.is_modified))
                    .size(theme::SMALL_SIZE)
                    .color(palette.ink),
            );
            ui.add_space(24.0);
            ui.label(
                RichText::new(format!(
                    "Page {} of {}",
                    open.view.current_page() + 1,
                    state.page_count
                ))
                .size(theme::SMALL_SIZE)
                .color(palette.quiet),
            );
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
        if let Some(open) = &mut self.open {
            // Turn finished renders into textures; this never waits.
            while let Some(result) = open.pool.try_recv() {
                open.view.receive(&ctx, result);
            }
        }

        let palette = theme::palette(&ctx);
        let state = self.toolbar_state();
        let actions = egui::Panel::top("toolbar")
            .frame(
                Frame::NONE
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| toolbar::show(ui, &state, &mut self.find_query))
            .inner;
        // Acted on before the pages are drawn, so a zoom shows this frame.
        for action in actions {
            self.act(&ctx, action);
        }

        let state = self.toolbar_state();
        egui::Panel::bottom("status")
            .frame(
                Frame::NONE
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(10, 4)),
            )
            .show(ui, |ui| self.status_bar(ui, &state));
        let open_clicked = egui::CentralPanel::no_frame()
            .frame(Frame::NONE.fill(palette.desk))
            .show(ui, |ui| match &mut self.open {
                Some(open) => {
                    open.view.show(ui, &open.document.page_sizes, &open.pool);
                    false
                }
                None => page_view::empty_state(ui),
            })
            .inner;
        if open_clicked {
            self.act(&ctx, ToolbarAction::Open);
        }

        error_dialog(&ctx, &mut self.error);
    }
}

/// "report.pdf", or "report.pdf (edited)" while there are unsaved changes.
fn file_status(name: &str, modified: bool) -> String {
    if modified {
        format!("{name} (edited)")
    } else {
        name.to_owned()
    }
}

/// The file name for titles and messages, e.g. "report.pdf".
fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_status_should_mark_unsaved_changes() {
        assert_eq!(
            (
                file_status("report.pdf", true),
                file_status("report.pdf", false)
            ),
            ("report.pdf (edited)".to_owned(), "report.pdf".to_owned())
        );
    }
}

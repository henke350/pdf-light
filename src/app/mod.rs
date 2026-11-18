//! The window: toolbar, pages panel, page view, status bar and dialogs.

mod dialogs;
mod export_dialog;
mod find;
mod icons;
mod page_view;
mod sidebar;
mod textures;
mod theme;
mod toolbar;

use dialogs::{
    ErrorMessage, UnsavedChoice, error_dialog, overwrite_dialog, unsaved_changes_dialog,
};
use eframe::egui::{self, Align, Frame, Layout, Margin, RichText, ViewportCommand};
use export_dialog::{ExportDialog, Outcome};
use find::Find;
use page_view::PageView;
use pdf_light::document::{self, Document};
use pdf_light::edit;
use pdf_light::pages::PageList;
use pdf_light::render_pool::{RenderPool, RenderRequest};
use pdf_light::selection::Selection;
use pdf_light::view;
use sidebar::PagesPanel;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use toolbar::{ToolbarAction, ToolbarState};

/// How long a status message such as "Saved report.pdf" stays, in seconds.
const STATUS_SECS: f64 = 5.0;

/// An open PDF and everything that belongs to it; replaced as a whole when
/// another file is opened (dropping the old pool stops its workers).
struct OpenDocument {
    document: Document,
    /// Which original pages are still in the document, with undo.
    pages: PageList,
    /// Selected pages in the pages panel, as positions in `pages`.
    selection: Selection,
    pool: RenderPool,
    view: PageView,
    panel: PagesPanel,
    /// Find in document: the search, its matches and its text cache.
    find: Find,
    /// What was last handed to the pool, so it's only told about changes.
    requested: Vec<RenderRequest>,
    /// The owner answered "Overwrite" to "Overwrite report.pdf?" for this document.
    overwrite_confirmed: bool,
}

impl OpenDocument {
    fn name(&self) -> String {
        file_name(&self.document.path)
    }

    /// The sizes of the kept pages, in the order shown.
    fn sizes(&self) -> Vec<(f32, f32)> {
        view::kept_sizes(&self.document.page_sizes, self.pages.kept())
    }
}

/// A question waiting for the owner's answer; there is at most one.
/// Each variant is one kind of dialog, and can carry what it needs.
enum Dialog {
    /// "Overwrite report.pdf?" before the first Save over the open file.
    ConfirmOverwrite,
    /// "Save changes to report.pdf?", then carry on with what was asked for.
    Unsaved(Pending),
    /// The Export dialog, which also runs the export while it is open.
    Export(Box<ExportDialog>),
}

/// What the owner asked for when the unsaved-changes dialog stepped in.
enum Pending {
    Close,
    /// Open this file, or show the Open dialog when `None`.
    Open(Option<PathBuf>),
}

/// A short-lived message on the right of the status bar.
struct StatusMessage {
    text: String,
    /// `ctx.input(|i| i.time)` when it disappears.
    until: f64,
}

/// The application state.
pub struct App {
    open: Option<OpenDocument>,
    error: Option<ErrorMessage>,
    dialog: Option<Dialog>,
    status: Option<StatusMessage>,
    render_threads: usize,
    /// The find field's text.
    find_query: String,
    /// Whether the pages panel shows while a file is open; kept for the session.
    pages_panel_open: bool,
    /// The title last sent to the window, so it's only sent when it changes.
    title: String,
    /// The owner chose Discard (or saved) when closing: let the close through.
    allow_close: bool,
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
            dialog: None,
            status: None,
            // Leave one core for the UI, but always use at least two workers.
            render_threads: cores.saturating_sub(1).max(2),
            find_query: String::new(),
            pages_panel_open: true,
            title: "pdf-light".to_owned(),
            allow_close: false,
        };
        if let Some(path) = path {
            app.open_file(&cc.egui_ctx, &path);
        }
        app
    }

    /// Whether the open document has unsaved deletions.
    fn is_modified(&self) -> bool {
        self.open
            .as_ref()
            .is_some_and(|open| open.pages.is_modified())
    }

    /// Opens `path` (or asks for a file when `None`), first asking to save
    /// unsaved changes.
    fn request_open(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
        if self.is_modified() {
            self.dialog = Some(Dialog::Unsaved(Pending::Open(path)));
        } else {
            self.open_now(ctx, path);
        }
    }

    fn open_now(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
        match path {
            Some(path) => self.open_file(ctx, &path),
            None => self.pick_and_open(ctx),
        }
    }

    /// Opens `path`, replacing the current document, or shows why it can't.
    fn open_file(&mut self, ctx: &egui::Context, path: &Path) {
        match document::open(path) {
            Ok(document) => {
                // Another file: the old query means nothing here.
                self.find_query.clear();
                self.open = Some(self.start(ctx, document, PageView::new()));
            }
            Err(error) => {
                let title = format!("Couldn't open {}", file_name(path));
                self.error = Some(ErrorMessage::new(title, &error));
            }
        }
    }

    /// Everything that belongs to a freshly opened `document`: all pages kept,
    /// nothing selected, a new render pool and no search yet. `view` keeps its
    /// zoom and scroll.
    fn start(&self, ctx: &egui::Context, document: Document, view: PageView) -> OpenDocument {
        let repaint_ctx = ctx.clone();
        // The pool calls this from its threads; `request_repaint` is thread-safe.
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || repaint_ctx.request_repaint());
        let pool = RenderPool::new(Arc::clone(&document.bytes), self.render_threads, wake);
        OpenDocument {
            pages: PageList::new(document.page_sizes.len()),
            document,
            selection: Selection::default(),
            pool,
            view,
            panel: PagesPanel::new(),
            find: Find::new(),
            requested: Vec::new(),
            overwrite_confirmed: false,
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

    /// Save: over the open file, after "Overwrite report.pdf?" the first time.
    fn save(&mut self, ctx: &egui::Context) {
        let Some(open) = &self.open else {
            return;
        };
        if open.overwrite_confirmed {
            let path = open.document.path.clone();
            self.save_to(ctx, &path);
        } else {
            self.dialog = Some(Dialog::ConfirmOverwrite);
        }
    }

    /// Save as: asks where (suggesting "report-edited.pdf" next to the
    /// original), then saves there. Returns `true` when the file was saved.
    fn save_as(&mut self, ctx: &egui::Context) -> bool {
        let Some(open) = &self.open else {
            return false;
        };
        let path = &open.document.path;
        let mut dialog = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name(edited_file_name(path));
        if let Some(folder) = path.parent() {
            dialog = dialog.set_directory(folder);
        }
        let Some(mut target) = dialog.save_file() else {
            return false; // Cancelled.
        };
        if target.extension().is_none() {
            target.set_extension("pdf");
        }
        self.save_to(ctx, &target)
    }

    /// Writes the kept pages to `target` (atomically, see `edit::save_with_pages`),
    /// then reopens the saved file. Returns `true` when the file was saved.
    fn save_to(&mut self, ctx: &egui::Context, target: &Path) -> bool {
        let Some(open) = self.open.take() else {
            return false;
        };
        let name = file_name(target);
        if let Err(error) = edit::save_with_pages(&open.document.bytes, open.pages.kept(), target) {
            self.error = Some(ErrorMessage::new(format!("Couldn't save {name}"), &error));
            self.open = Some(open);
            return false;
        }
        // Reopen what was written: its pages are now the whole document, nothing is
        // unsaved, and the page indices change meaning, so the old images go.
        match document::open(target) {
            Ok(document) => {
                let same_file = target == open.document.path;
                let overwrite_confirmed = same_file && open.overwrite_confirmed;
                let mut view = open.view;
                view.reset_textures();
                let mut reopened = self.start(ctx, document, view);
                reopened.overwrite_confirmed = overwrite_confirmed;
                // The saved file holds the kept pages in the same order, so the
                // view's page positions and the query still apply.
                reopened.find.resume(
                    ctx,
                    &self.find_query,
                    reopened.pages.kept(),
                    reopened.view.current_page(),
                    &reopened.document.bytes,
                );
                self.open = Some(reopened);
            }
            Err(error) => {
                let title = format!("Saved {name}, but couldn't open it again");
                self.error = Some(ErrorMessage::new(title, &error));
                self.open = Some(open);
            }
        }
        self.show_status(ctx, format!("Saved {name}"));
        true
    }

    /// Deletes the selected pages (in memory; the file is untouched until saved).
    fn delete_selected(&mut self) {
        let Some(open) = &mut self.open else {
            return;
        };
        match open.pages.delete(&open.selection.positions()) {
            // Positions shift after a delete, so the old selection means nothing now.
            Ok(()) => open.selection.clear(),
            Err(error) => {
                self.error = Some(ErrorMessage::new("Couldn't delete the pages", &error));
            }
        }
    }

    /// Opens the Export dialog. A hidden panel's selection can't be seen, so it
    /// doesn't count (same rule as Delete).
    fn open_export(&mut self) {
        let Some(open) = &self.open else {
            return;
        };
        let selected = if self.pages_panel_open {
            open.selection.positions()
        } else {
            Vec::new()
        };
        self.dialog = Some(Dialog::Export(Box::new(ExportDialog::new(
            &open.document.path,
            Arc::clone(&open.document.bytes),
            open.pages.kept().to_vec(),
            open.sizes(),
            selected,
        ))));
    }

    fn show_status(&mut self, ctx: &egui::Context, text: String) {
        let now = ctx.input(|i| i.time);
        self.status = Some(StatusMessage {
            text,
            until: now + STATUS_SECS,
        });
    }

    /// What the toolbar can offer right now.
    fn toolbar_state(&self) -> ToolbarState {
        let Some(open) = &self.open else {
            return ToolbarState::default();
        };
        ToolbarState {
            file_name: Some(open.name()),
            is_modified: open.pages.is_modified(),
            // Every unsaved change is a delete that can be undone.
            can_undo: open.pages.is_modified(),
            // A hidden panel's selection can't be seen, so it can't be deleted either.
            selected_count: if self.pages_panel_open {
                open.selection.len()
            } else {
                0
            },
            page_count: open.pages.len(),
            zoom_percent: open.view.zoom(),
            fit_width: open.view.fits_width(),
            find: open.find.status(),
            pages_panel_open: self.pages_panel_open,
        }
    }

    /// Carries out a toolbar button, its shortcut, a right-click menu item,
    /// or the empty state's Open.
    fn act(&mut self, ctx: &egui::Context, action: ToolbarAction) {
        let now = ctx.input(|i| i.time);
        match action {
            ToolbarAction::Open => self.request_open(ctx, None),
            ToolbarAction::Save => self.save(ctx),
            ToolbarAction::SaveAs => {
                self.save_as(ctx);
            }
            ToolbarAction::TogglePagesPanel => self.pages_panel_open = !self.pages_panel_open,
            ToolbarAction::DeleteSelected => self.delete_selected(),
            ToolbarAction::Undo => {
                if let Some(open) = &mut self.open
                    && open.pages.undo()
                {
                    open.selection.clear();
                }
            }
            ToolbarAction::ZoomIn
            | ToolbarAction::ZoomOut
            | ToolbarAction::SetZoom(_)
            | ToolbarAction::FitWidth => {
                let Some(open) = &mut self.open else {
                    return;
                };
                let sizes = open.sizes();
                let view = &mut open.view;
                match action {
                    ToolbarAction::ZoomIn => {
                        view.zoom_to(view::zoom_in_step(view.zoom()), &sizes, now);
                    }
                    ToolbarAction::ZoomOut => {
                        view.zoom_to(view::zoom_out_step(view.zoom()), &sizes, now);
                    }
                    ToolbarAction::SetZoom(zoom) => view.zoom_to(zoom, &sizes, now),
                    _ => view.fit_width(),
                }
            }
            ToolbarAction::FindNext | ToolbarAction::FindPrevious => {
                if let Some(open) = &mut self.open {
                    open.find.step(action == ToolbarAction::FindNext);
                }
            }
            ToolbarAction::Export => self.open_export(),
            // Drawn but disabled until Task 10b wires it.
            ToolbarAction::Print => {}
        }
    }

    /// A close request (the window's X, Alt+F4) with unsaved changes is held
    /// back until the owner answers Save / Discard / Cancel.
    fn hold_close(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && self.is_modified() && !self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if !matches!(self.dialog, Some(Dialog::Unsaved(_))) {
                self.dialog = Some(Dialog::Unsaved(Pending::Close));
            }
        }
    }

    /// Shows the open question, if any, and acts on the answer.
    fn dialogs(&mut self, ctx: &egui::Context) {
        // `take` moves the dialog out of `self` so it can be matched by value;
        // a dialog still waiting for an answer is put back.
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let name = self
            .open
            .as_ref()
            .map(OpenDocument::name)
            .unwrap_or_default();
        match dialog {
            Dialog::ConfirmOverwrite => match overwrite_dialog(ctx, &name) {
                None => self.dialog = Some(Dialog::ConfirmOverwrite),
                Some(false) => {}
                Some(true) => {
                    if let Some(open) = &mut self.open {
                        open.overwrite_confirmed = true;
                        let path = open.document.path.clone();
                        self.save_to(ctx, &path);
                    }
                }
            },
            Dialog::Unsaved(pending) => match unsaved_changes_dialog(ctx, &name) {
                None => self.dialog = Some(Dialog::Unsaved(pending)),
                Some(UnsavedChoice::Cancel) => {}
                // Save goes through Save as, so the original is only ever
                // overwritten after "Overwrite report.pdf?".
                Some(UnsavedChoice::Save) => {
                    if self.save_as(ctx) {
                        self.carry_on(ctx, pending);
                    }
                }
                Some(UnsavedChoice::Discard) => self.carry_on(ctx, pending),
            },
            Dialog::Export(mut export) => match export.show(ctx) {
                Outcome::Open => self.dialog = Some(Dialog::Export(export)),
                Outcome::Closed => {}
                Outcome::Done(message) => self.show_status(ctx, message),
                Outcome::Cancelled => self.show_status(ctx, "Export cancelled".to_owned()),
                Outcome::Failed(error) => {
                    self.error = Some(ErrorMessage::new("Couldn't export the pages", &error));
                }
            },
        }
    }

    /// Does what was asked for before the unsaved-changes dialog stepped in.
    fn carry_on(&mut self, ctx: &egui::Context, pending: Pending) {
        match pending {
            Pending::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Pending::Open(path) => self.open_now(ctx, path),
        }
    }

    /// File name ("(edited)" while there are unsaved changes), "Page N of M",
    /// and on the right a short-lived message such as "Saved report.pdf".
    fn status_bar(&self, ui: &mut egui::Ui, state: &ToolbarState) {
        let palette = theme::palette(ui.ctx());
        let now = ui.input(|i| i.time);
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
            // Right after a delete the view still reports last frame's page.
            let page = (open.view.current_page() + 1).min(state.page_count);
            ui.label(
                RichText::new(format!("Page {page} of {}", state.page_count))
                    .size(theme::SMALL_SIZE)
                    .color(palette.quiet),
            );
            if let Some(status) = self.status.as_ref().filter(|s| now < s.until) {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(&status.text)
                            .size(theme::SMALL_SIZE)
                            .color(palette.ink),
                    );
                });
            }
        });
    }

    /// Keeps the window title in step: "report.pdf • — pdf-light" while modified.
    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self.open.as_ref().map(OpenDocument::name);
        let title = window_title(name.as_deref(), self.is_modified());
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Drops the status message once its time is up (and wakes up to do so).
    fn expire_status(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        match &self.status {
            Some(status) if now >= status.until => self.status = None,
            Some(status) => ctx.request_repaint_after(Duration::from_secs_f64(status.until - now)),
            None => {}
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.request_open(&ctx, Some(path));
        }
        self.hold_close(&ctx);
        if let Some(open) = &mut self.open {
            // Turn finished renders into textures; this never waits.
            while let Some(result) = open.pool.try_recv() {
                if result.request.thumbnail {
                    open.panel.receive(&ctx, result);
                } else {
                    open.view.receive(&ctx, result);
                }
            }
            open.find.receive();
        }

        let palette = theme::palette(&ctx);
        // While a dialog is open, keys and clicks behind it do nothing.
        let modal = self.dialog.is_some() || self.error.is_some();
        let state = self.toolbar_state();
        let mut actions = egui::Panel::top("toolbar")
            .frame(
                Frame::NONE
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| toolbar::show(ui, &state, &mut self.find_query))
            .inner;
        if modal {
            actions.clear();
        }
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

        let panel_open = self.pages_panel_open;
        let mut panel_actions = Vec::new();
        if panel_open && let Some(open) = &mut self.open {
            if !modal && !ctx.text_edit_focused() {
                sidebar::selection_keys(&ctx, &mut open.selection, open.pages.len());
            }
            let sizes = open.sizes();
            let current = open.view.current_page();
            let output = egui::Panel::left("pages-panel")
                .resizable(true)
                .default_size(sidebar::DEFAULT_WIDTH)
                .size_range(sidebar::MIN_WIDTH..=sidebar::MAX_WIDTH)
                .frame(Frame::NONE.fill(palette.panel))
                .show(ui, |ui| {
                    open.panel.show(
                        ui,
                        &sizes,
                        open.pages.kept(),
                        current,
                        &mut open.selection,
                        &state,
                    )
                })
                .inner;
            if let Some(position) = output.go_to {
                open.view.scroll_to_page(position, &sizes);
            }
            panel_actions = output.actions;
        }
        if !modal {
            for action in panel_actions {
                self.act(&ctx, action);
            }
        }

        if let Some(open) = &mut self.open {
            // After every action this frame, so a delete or undo restarts the search now.
            let current = open.view.current_page();
            let kept = open.pages.kept();
            open.find
                .update(&ctx, &self.find_query, kept, current, &open.document.bytes);
            // Copy the page and rect out, so `open.find` isn't borrowed any more.
            let target = open
                .find
                .take_reveal()
                .and_then(|found| Some((found.page, *found.rects.first()?)));
            if let Some((page, rect)) = target
                && let Some(position) = kept.iter().position(|&p| p == page)
            {
                let sizes = open.sizes();
                open.view.reveal(position, rect, &sizes);
            }
        }

        let open_clicked = egui::CentralPanel::no_frame()
            .frame(Frame::NONE.fill(palette.desk))
            .show(ui, |ui| match &mut self.open {
                Some(open) => {
                    let sizes = open.sizes();
                    open.view
                        .show(ui, &sizes, open.pages.kept(), open.find.matches());
                    false
                }
                None => page_view::empty_state(ui),
            })
            .inner;
        if open_clicked {
            self.act(&ctx, ToolbarAction::Open);
        }

        if let Some(open) = &mut self.open {
            // One queue for both: pages in view first, then thumbnails (the pool
            // sorts them that way too). Only changes are handed over.
            let mut wanted = open.view.wanted().to_vec();
            if panel_open {
                wanted.extend_from_slice(open.panel.wanted());
            }
            if wanted != open.requested {
                open.pool.set_wanted(wanted.clone());
                open.requested = wanted;
            }
        }

        self.dialogs(&ctx);
        error_dialog(&ctx, &mut self.error);
        self.update_title(&ctx);
        self.expire_status(&ctx);
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

/// The window title: "report.pdf — pdf-light", with a "•" while there are unsaved changes.
fn window_title(name: Option<&str>, modified: bool) -> String {
    match (name, modified) {
        (Some(name), true) => format!("{name} • — pdf-light"),
        (Some(name), false) => format!("{name} — pdf-light"),
        (None, _) => "pdf-light".to_owned(),
    }
}

/// The name Save as suggests: "report.pdf" → "report-edited.pdf".
fn edited_file_name(path: &Path) -> String {
    // `file_stem` is the name without its last extension: "v1.2 notes" for "v1.2 notes.PDF".
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    format!("{stem}-edited.pdf")
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

    #[test]
    fn window_title_should_mark_unsaved_changes() {
        assert_eq!(
            [
                window_title(Some("report.pdf"), true),
                window_title(Some("report.pdf"), false),
                window_title(None, false)
            ],
            [
                "report.pdf • — pdf-light",
                "report.pdf — pdf-light",
                "pdf-light"
            ]
        );
    }

    #[test]
    fn edited_file_name_should_add_suffix_before_extension() {
        assert_eq!(
            edited_file_name(Path::new(r"C:\docs\report.pdf")),
            "report-edited.pdf"
        );
    }

    #[test]
    fn edited_file_name_should_keep_dots_in_the_name() {
        assert_eq!(
            edited_file_name(Path::new("v1.2 notes.PDF")),
            "v1.2 notes-edited.pdf"
        );
    }
}

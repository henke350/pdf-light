//! The Export dialog (Ctrl+E): which pages, format, resolution and folder,
//! then a background export with progress and Cancel.
//!
//! Everything that is not drawing lives in plain functions on `Form` and
//! friends, so the tests can check the choices, the messages and when Export
//! is allowed without opening a window.

use super::theme;
use eframe::egui::{self, RichText};
use pdf_light::document;
use pdf_light::error::PdfError;
use pdf_light::export::{
    ExportOptions, ImageFormat, WARN_PIXELS, existing_outputs, export_pages, output_paths,
    parse_page_range,
};
use pdf_light::render::{MAX_PIXELS, MAX_SIDE_PX, pixel_size};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

/// The resolutions offered as buttons; "Custom" lets the owner type another.
const DPI_PRESETS: [u32; 4] = [72, 150, 300, 600];
const DPI_MIN: u32 = 1;
const DPI_MAX: u32 = 2400;

/// Which pages to export. Each variant is one choice; the enum makes
/// "none of them" impossible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagesChoice {
    Selected,
    All,
    Range,
}

/// A resolution button, or "Custom".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpiChoice {
    Preset(u32),
    Custom,
}

/// What the owner has chosen so far (the dialog's inputs, with no drawing).
#[derive(Debug, Clone)]
pub struct Form {
    pub pages: PagesChoice,
    pub range: String,
    pub format: ImageFormat,
    pub multipage_tiff: bool,
    pub dpi: DpiChoice,
    pub custom_dpi: String,
    pub folder: PathBuf,
    pub base_name: String,
}

impl Form {
    /// The starting choices: the selected pages if any (else all), PNG, 300 DPI,
    /// next to the PDF, named after the PDF.
    pub fn new(has_selection: bool, pdf_path: &Path) -> Self {
        Self {
            pages: if has_selection {
                PagesChoice::Selected
            } else {
                PagesChoice::All
            },
            range: String::new(),
            format: ImageFormat::Png,
            multipage_tiff: false,
            dpi: DpiChoice::Preset(300),
            custom_dpi: String::new(),
            folder: pdf_path.parent().map(Path::to_path_buf).unwrap_or_default(),
            base_name: pdf_path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }
}

/// What the dialog needs to know about the document, to check the form.
pub struct Facts<'a> {
    /// Selected positions in the current page order.
    pub selected: &'a [usize],
    /// Page sizes in points, one per current position.
    pub sizes: &'a [(f32, f32)],
}

/// Whether the pages are too big for the chosen resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeVerdict {
    Fine,
    /// Allowed, but slow and memory-hungry.
    Warn(String),
    /// Not allowed: Export stays disabled.
    Refuse(String),
}

/// The result of checking the form: what would be exported and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// 0-based positions to export (empty when the pages choice has an error).
    pub positions: Vec<usize>,
    pub dpi: Option<u32>,
    pub pages_error: Option<String>,
    pub dpi_error: Option<String>,
    pub name_error: Option<String>,
    pub size: SizeVerdict,
}

impl Check {
    pub fn can_export(&self) -> bool {
        !self.positions.is_empty()
            && self.dpi.is_some()
            && self.pages_error.is_none()
            && self.dpi_error.is_none()
            && self.name_error.is_none()
            && !matches!(self.size, SizeVerdict::Refuse(_))
    }
}

/// Checks `form` against the document.
pub fn check(form: &Form, facts: &Facts) -> Check {
    let page_count = facts.sizes.len();
    let (positions, pages_error) = match chosen_positions(form, facts.selected, page_count) {
        Ok(positions) => (positions, None),
        Err(message) => (Vec::new(), Some(message)),
    };
    let (dpi, dpi_error) = match chosen_dpi(form) {
        Ok(dpi) => (Some(dpi), None),
        Err(message) => (None, Some(message)),
    };
    let size = match dpi {
        Some(dpi) => size_verdict(facts.sizes, &positions, dpi),
        None => SizeVerdict::Fine,
    };
    Check {
        positions,
        dpi,
        pages_error,
        dpi_error,
        name_error: base_name_error(&form.base_name),
        size,
    }
}

/// The positions the Pages choice stands for, or a message for the owner.
fn chosen_positions(
    form: &Form,
    selected: &[usize],
    page_count: usize,
) -> Result<Vec<usize>, String> {
    match form.pages {
        PagesChoice::Selected if selected.is_empty() => {
            Err("Select pages in the sidebar first.".to_owned())
        }
        PagesChoice::Selected => {
            let mut positions = selected.to_vec();
            positions.sort_unstable();
            positions.dedup();
            Ok(positions)
        }
        PagesChoice::All => Ok((0..page_count).collect()),
        PagesChoice::Range => parse_page_range(&form.range, page_count),
    }
}

fn chosen_dpi(form: &Form) -> Result<u32, String> {
    match form.dpi {
        DpiChoice::Preset(dpi) => Ok(dpi),
        DpiChoice::Custom => parse_dpi(&form.custom_dpi),
    }
}

/// A custom resolution: a whole number from 1 to 2400.
pub fn parse_dpi(text: &str) -> Result<u32, String> {
    match text.trim().parse::<u32>() {
        Ok(dpi) if (DPI_MIN..=DPI_MAX).contains(&dpi) => Ok(dpi),
        _ => Err(format!(
            "Enter a resolution from {DPI_MIN} to {DPI_MAX} DPI."
        )),
    }
}

/// Windows doesn't allow these in file names.
const BAD_NAME_CHARS: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

fn base_name_error(name: &str) -> Option<String> {
    if name.trim().is_empty() {
        Some("Enter a file name.".to_owned())
    } else if name.contains(BAD_NAME_CHARS) {
        Some("A file name can't contain \\ / : * ? \" < > |".to_owned())
    } else {
        None
    }
}

/// Looks at the largest of the chosen pages at `dpi`.
pub fn size_verdict(sizes: &[(f32, f32)], positions: &[usize], dpi: u32) -> SizeVerdict {
    let scale = dpi as f32 / 72.0;
    // Of all chosen pages, keep the one that matters most: a page that is
    // over the hard limit beats one that isn't, then the one with most pixels.
    let mut worst: Option<(bool, u64, u64, u64)> = None;
    for &position in positions {
        let Some(&size) = sizes.get(position) else {
            continue;
        };
        let (width, height) = pixel_size(size, scale);
        let pixels = width.saturating_mul(height);
        let refused = width > MAX_SIDE_PX || height > MAX_SIDE_PX || pixels > MAX_PIXELS;
        let candidate = (refused, pixels, width, height);
        if worst.is_none_or(|w| (candidate.0, candidate.1) > (w.0, w.1)) {
            worst = Some(candidate);
        }
    }
    let Some((refused, pixels, width, height)) = worst else {
        return SizeVerdict::Fine;
    };
    if refused {
        SizeVerdict::Refuse(format!(
            "A page would be {width} × {height} px at {dpi} DPI, which is too large. Choose a lower resolution."
        ))
    } else if pixels > WARN_PIXELS {
        SizeVerdict::Warn(format!(
            "A page is {} megapixels ({width} × {height} px) at {dpi} DPI. Exporting may be slow and use a lot of memory.",
            pixels / 1_000_000
        ))
    } else {
        SizeVerdict::Fine
    }
}

/// The first file name, plus "… 12 files" when there are several.
pub fn preview_text(paths: &[PathBuf]) -> String {
    let Some(first) = paths.first() else {
        return String::new();
    };
    let name = first
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if paths.len() > 1 {
        format!("{name} … {} files", paths.len())
    } else {
        name
    }
}

/// "Overwrite 1 existing file?" / "Overwrite 3 existing files?"
pub fn overwrite_question(count: usize) -> String {
    let noun = if count == 1 { "file" } else { "files" };
    format!("Overwrite {count} existing {noun}?")
}

/// "Exporting page 3 of 12" while page `done + 1` is being worked on.
pub fn progress_text(done: usize, total: usize) -> String {
    format!("Exporting page {} of {total}", (done + 1).min(total))
}

/// The status-bar message after a finished export.
pub fn exported_message(count: usize, folder: &Path) -> String {
    let noun = if count == 1 { "page" } else { "pages" };
    format!("Exported {count} {noun} to {}", folder.display())
}

/// What the dialog tells the app after a frame.
pub enum Outcome {
    /// Still open: keep calling `show`.
    Open,
    /// The owner closed it without exporting.
    Closed,
    /// Finished; the text is the status-bar message.
    Done(String),
    Cancelled,
    Failed(PdfError),
}

/// A running export on its own thread.
struct Job {
    cancel: Arc<AtomicBool>,
    /// Pages finished so far; the thread writes it, the dialog reads it.
    done: Arc<AtomicUsize>,
    total: usize,
    folder: PathBuf,
    /// The thread sends its result here when it ends. A channel is a pipe
    /// between threads: one end sends, the other receives.
    result: mpsc::Receiver<Result<Vec<PathBuf>, PdfError>>,
}

enum Phase {
    Editing,
    /// "Overwrite N existing files?" with the number of files in the way.
    ConfirmOverwrite(usize),
    Running(Job),
}

/// What a click in the dialog asked for.
enum Click {
    Export,
    Overwrite,
    Back,
    Cancel,
    PickFolder,
}

/// The dialog's state; one exists while the dialog is open.
pub struct ExportDialog {
    /// One id for the dialog's whole life, so text fields keep their focus.
    id: egui::Id,
    form: Form,
    phase: Phase,
    // The document, copied in so the dialog doesn't borrow from the app.
    bytes: Arc<Vec<u8>>,
    /// Original page index for each current position.
    kept: Vec<usize>,
    sizes: Vec<(f32, f32)>,
    selected: Vec<usize>,
}

impl ExportDialog {
    pub fn new(
        pdf_path: &Path,
        bytes: Arc<Vec<u8>>,
        kept: Vec<usize>,
        sizes: Vec<(f32, f32)>,
        selected: Vec<usize>,
    ) -> Self {
        Self {
            id: egui::Id::unique("export-dialog"),
            form: Form::new(!selected.is_empty(), pdf_path),
            phase: Phase::Editing,
            bytes,
            kept,
            sizes,
            selected,
        }
    }

    /// Draws the dialog and reports what happened.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome {
        if let Some(outcome) = self.poll_job() {
            return outcome;
        }
        let check = check(
            &self.form,
            &Facts {
                selected: &self.selected,
                sizes: &self.sizes,
            },
        );
        let palette = theme::palette(ctx);
        let running = matches!(self.phase, Phase::Running(_));
        let response = egui::Modal::new(self.id).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading("Export pages");
            ui.add_space(8.0);
            match &self.phase {
                Phase::Running(job) => {
                    let done = job.done.load(Ordering::Relaxed);
                    running_body(ui, done, job.total)
                }
                Phase::ConfirmOverwrite(count) => confirm_body(ui, *count, palette.danger),
                Phase::Editing => self.form_body(ui, &check, palette.danger),
            }
        });
        let should_close = response.should_close();
        let click = response.inner;
        // Esc or a click outside closes the dialog, but not while exporting
        // (Cancel is the way out of that).
        if should_close && !running {
            return Outcome::Closed;
        }
        match click {
            Some(Click::Export) => self.begin(ctx, &check),
            Some(Click::Overwrite) => self.start(ctx, &check),
            Some(Click::Back) => self.phase = Phase::Editing,
            Some(Click::Cancel) => match &self.phase {
                Phase::Running(job) => job.cancel.store(true, Ordering::Relaxed),
                _ => return Outcome::Closed,
            },
            Some(Click::PickFolder) => self.pick_folder(),
            None => {}
        }
        Outcome::Open
    }

    /// Checks whether the background export has ended.
    fn poll_job(&self) -> Option<Outcome> {
        let Phase::Running(job) = &self.phase else {
            return None;
        };
        match job.result.try_recv() {
            Ok(Ok(_files)) => Some(Outcome::Done(exported_message(job.total, &job.folder))),
            Ok(Err(PdfError::Cancelled)) => Some(Outcome::Cancelled),
            Ok(Err(error)) => Some(Outcome::Failed(error)),
            Err(mpsc::TryRecvError::Empty) => None,
            // The thread ended without sending: it must have panicked.
            Err(mpsc::TryRecvError::Disconnected) => Some(Outcome::Failed(PdfError::Image(
                "the export stopped unexpectedly".to_owned(),
            ))),
        }
    }

    fn options(&self, dpi: u32) -> ExportOptions {
        ExportOptions {
            format: self.form.format,
            dpi,
            multipage_tiff: self.form.multipage_tiff,
            folder: self.form.folder.clone(),
            base_name: self.form.base_name.trim().to_owned(),
        }
    }

    /// Export was clicked: ask about files in the way, or start right away.
    fn begin(&mut self, ctx: &egui::Context, check: &Check) {
        let Some(dpi) = check.dpi.filter(|_| check.can_export()) else {
            return;
        };
        let paths = output_paths(&self.options(dpi), &check.positions, self.sizes.len());
        let existing = existing_outputs(&paths).len();
        if existing > 0 {
            self.phase = Phase::ConfirmOverwrite(existing);
        } else {
            self.start(ctx, check);
        }
    }

    /// Starts the export on a background thread (the UI thread never renders).
    fn start(&mut self, ctx: &egui::Context, check: &Check) {
        let Some(dpi) = check.dpi.filter(|_| check.can_export()) else {
            self.phase = Phase::Editing;
            return;
        };
        let options = self.options(dpi);
        let pages: Vec<(usize, usize)> = check
            .positions
            .iter()
            .filter_map(|&position| Some((position, *self.kept.get(position)?)))
            .collect();
        let total = pages.len();
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicUsize::new(0));
        let (sender, result) = mpsc::channel();
        let folder = options.folder.clone();

        // `move` hands the clones to the thread. Each thread opens its own
        // `Pdf`, because hayro's `Pdf` can't be shared between threads.
        let bytes = Arc::clone(&self.bytes);
        let (thread_cancel, thread_done, thread_ctx) =
            (Arc::clone(&cancel), Arc::clone(&done), ctx.clone());
        std::thread::spawn(move || {
            let progress = |finished: usize, _total: usize| {
                thread_done.store(finished, Ordering::Relaxed);
                thread_ctx.request_repaint();
                !thread_cancel.load(Ordering::Relaxed)
            };
            let outcome = document::open_pdf(&bytes)
                .and_then(|pdf| export_pages(&pdf, &pages, &options, &progress));
            // The dialog may be gone already; then nobody needs the result.
            let _ = sender.send(outcome);
            thread_ctx.request_repaint();
        });
        self.phase = Phase::Running(Job {
            cancel,
            done,
            total,
            folder,
            result,
        });
    }

    /// Shows the folder picker (it blocks until closed, like the Open dialog).
    fn pick_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if self.form.folder.is_dir() {
            dialog = dialog.set_directory(&self.form.folder);
        }
        if let Some(folder) = dialog.pick_folder() {
            self.form.folder = folder;
        }
    }

    /// The choices, the preview and the buttons.
    fn form_body(
        &mut self,
        ui: &mut egui::Ui,
        check: &Check,
        danger: egui::Color32,
    ) -> Option<Click> {
        let form = &mut self.form;
        let mut click = None;
        egui::Grid::new("export-grid")
            .num_columns(2)
            .spacing([14.0, 10.0])
            .show(ui, |ui| {
                ui.label("Pages");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let selected_label = format!("Selected ({})", self.selected.len());
                        ui.add_enabled_ui(!self.selected.is_empty(), |ui| {
                            ui.radio_value(&mut form.pages, PagesChoice::Selected, selected_label);
                        });
                        ui.radio_value(&mut form.pages, PagesChoice::All, "All");
                        ui.radio_value(&mut form.pages, PagesChoice::Range, "Range");
                    });
                    if form.pages == PagesChoice::Range {
                        ui.add(
                            egui::TextEdit::singleline(&mut form.range)
                                .hint_text("1-3, 7")
                                .desired_width(200.0),
                        );
                    }
                    if let Some(message) = &check.pages_error
                        && (form.pages != PagesChoice::Range || !form.range.is_empty())
                    {
                        error_text(ui, message, danger);
                    }
                });
                ui.end_row();

                ui.label("Format");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut form.format, ImageFormat::Png, "PNG");
                        ui.radio_value(&mut form.format, ImageFormat::Tiff, "TIFF");
                    });
                    ui.add_enabled(
                        form.format == ImageFormat::Tiff,
                        egui::Checkbox::new(&mut form.multipage_tiff, "One multi-page TIFF"),
                    );
                });
                ui.end_row();

                ui.label("Resolution");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        for preset in DPI_PRESETS {
                            ui.radio_value(
                                &mut form.dpi,
                                DpiChoice::Preset(preset),
                                preset.to_string(),
                            );
                        }
                        ui.radio_value(&mut form.dpi, DpiChoice::Custom, "Custom");
                    });
                    if form.dpi == DpiChoice::Custom {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut form.custom_dpi)
                                    .hint_text("1-2400")
                                    .desired_width(80.0),
                            );
                            ui.label("DPI");
                        });
                    }
                    if let Some(message) = &check.dpi_error
                        && (form.dpi != DpiChoice::Custom || !form.custom_dpi.is_empty())
                    {
                        error_text(ui, message, danger);
                    }
                });
                ui.end_row();

                ui.label("Folder");
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(form.folder.display().to_string()).truncate());
                    if ui.button("Change…").clicked() {
                        click = Some(Click::PickFolder);
                    }
                });
                ui.end_row();

                ui.label("File name");
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut form.base_name).desired_width(240.0));
                    if let Some(message) = &check.name_error {
                        error_text(ui, message, danger);
                    }
                });
                ui.end_row();
            });

        ui.add_space(8.0);
        let paths = match check.dpi {
            Some(dpi) => output_paths(&self.options(dpi), &check.positions, self.sizes.len()),
            None => Vec::new(),
        };
        let preview = preview_text(&paths);
        if !preview.is_empty() {
            ui.label(format!("Files: {preview}"));
        }
        match &check.size {
            SizeVerdict::Fine => {}
            SizeVerdict::Warn(message) => {
                ui.label(message);
            }
            SizeVerdict::Refuse(message) => error_text(ui, message, danger),
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(check.can_export(), egui::Button::new("Export"))
                .clicked()
            {
                click = Some(Click::Export);
            }
            if ui.button("Cancel").clicked() {
                click = Some(Click::Cancel);
            }
        });
        click
    }
}

fn error_text(ui: &mut egui::Ui, message: &str, danger: egui::Color32) {
    ui.label(RichText::new(message).size(theme::SMALL_SIZE).color(danger));
}

/// "Exporting page 3 of 12", the bar and Cancel.
fn running_body(ui: &mut egui::Ui, done: usize, total: usize) -> Option<Click> {
    ui.label(progress_text(done, total));
    ui.add_space(6.0);
    let fraction = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    ui.add(egui::ProgressBar::new(fraction));
    ui.add_space(10.0);
    ui.button("Cancel").clicked().then_some(Click::Cancel)
}

/// "Overwrite N existing files?" with Overwrite / Back.
fn confirm_body(ui: &mut egui::Ui, count: usize, danger: egui::Color32) -> Option<Click> {
    ui.label(overwrite_question(count));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new("Overwrite").color(danger))
            .clicked()
        {
            Some(Click::Overwrite)
        } else if ui.button("Back").clicked() {
            Some(Click::Back)
        } else {
            None
        }
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter_pages(count: usize) -> Vec<(f32, f32)> {
        vec![(612.0, 792.0); count]
    }

    fn form() -> Form {
        Form::new(false, Path::new(r"C:\docs\report.pdf"))
    }

    fn facts<'a>(selected: &'a [usize], sizes: &'a [(f32, f32)]) -> Facts<'a> {
        Facts { selected, sizes }
    }

    #[test]
    fn form_new_should_default_to_selected_pages_when_a_selection_exists() {
        let path = Path::new(r"C:\docs\report.pdf");
        assert_eq!(
            [Form::new(true, path).pages, Form::new(false, path).pages],
            [PagesChoice::Selected, PagesChoice::All]
        );
    }

    #[test]
    fn form_new_should_default_to_png_300_dpi_next_to_the_pdf() {
        let form = form();
        assert_eq!(form.format, ImageFormat::Png);
        assert_eq!(form.dpi, DpiChoice::Preset(300));
        assert!(!form.multipage_tiff);
        assert_eq!(form.folder, PathBuf::from(r"C:\docs"));
        assert_eq!(form.base_name, "report");
    }

    #[test]
    fn check_should_export_all_pages_by_default() {
        let sizes = letter_pages(3);
        let result = check(&form(), &facts(&[], &sizes));
        assert_eq!(result.positions, vec![0, 1, 2]);
        assert!(result.can_export());
    }

    #[test]
    fn check_should_use_the_selection_in_order() {
        let sizes = letter_pages(5);
        let mut form = form();
        form.pages = PagesChoice::Selected;
        let result = check(&form, &facts(&[3, 1], &sizes));
        assert_eq!(result.positions, vec![1, 3]);
    }

    #[test]
    fn check_should_refuse_selected_without_a_selection() {
        let sizes = letter_pages(5);
        let mut form = form();
        form.pages = PagesChoice::Selected;
        let result = check(&form, &facts(&[], &sizes));
        assert!(result.pages_error.is_some());
        assert!(!result.can_export());
    }

    #[test]
    fn check_should_report_a_bad_range_and_disable_export() {
        let sizes = letter_pages(5);
        let mut form = form();
        form.pages = PagesChoice::Range;
        form.range = "1-9".to_owned();
        let result = check(&form, &facts(&[], &sizes));
        assert_eq!(
            result.pages_error.as_deref(),
            Some("Pages must be between 1 and 5.")
        );
        assert!(!result.can_export());
    }

    #[test]
    fn check_should_turn_a_range_into_positions() {
        let sizes = letter_pages(10);
        let mut form = form();
        form.pages = PagesChoice::Range;
        form.range = "1-3, 7".to_owned();
        let result = check(&form, &facts(&[], &sizes));
        assert_eq!(result.positions, vec![0, 1, 2, 6]);
        assert!(result.can_export());
    }

    #[test]
    fn parse_dpi_should_accept_one_to_2400_only() {
        assert_eq!(parse_dpi(" 450 "), Ok(450));
        assert_eq!(parse_dpi("1"), Ok(1));
        assert_eq!(parse_dpi("2400"), Ok(2400));
        for bad in ["", "0", "2401", "abc", "-5", "1.5"] {
            assert!(parse_dpi(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn check_should_disable_export_for_a_bad_custom_dpi() {
        let sizes = letter_pages(2);
        let mut form = form();
        form.dpi = DpiChoice::Custom;
        form.custom_dpi = "5000".to_owned();
        let result = check(&form, &facts(&[], &sizes));
        assert!(result.dpi_error.is_some());
        assert!(!result.can_export());
    }

    #[test]
    fn check_should_disable_export_for_a_bad_file_name() {
        let sizes = letter_pages(2);
        for bad in ["", "  ", "a/b", "what?"] {
            let mut form = form();
            form.base_name = bad.to_owned();
            let result = check(&form, &facts(&[], &sizes));
            assert!(result.name_error.is_some(), "{bad:?}");
            assert!(!result.can_export(), "{bad:?}");
        }
    }

    #[test]
    fn size_verdict_should_be_fine_for_a_letter_page_at_300_dpi() {
        assert_eq!(size_verdict(&letter_pages(1), &[0], 300), SizeVerdict::Fine);
    }

    #[test]
    fn size_verdict_should_warn_above_100_megapixels() {
        // A0 (2384 × 3370 pt) at 300 DPI is about 7 × 10 thousand px = 140 MP.
        let verdict = size_verdict(&[(2384.0, 3370.0)], &[0], 300);
        assert!(matches!(verdict, SizeVerdict::Warn(_)), "{verdict:?}");
    }

    #[test]
    fn size_verdict_should_refuse_above_the_hard_limit() {
        // A0 at 600 DPI is about 560 MP, over the 250 MP limit.
        let verdict = size_verdict(&[(2384.0, 3370.0)], &[0], 600);
        assert!(matches!(verdict, SizeVerdict::Refuse(_)), "{verdict:?}");
    }

    #[test]
    fn size_verdict_should_refuse_a_side_over_the_limit() {
        // Thin and long: few pixels, but one side is over 65 535.
        let verdict = size_verdict(&[(10.0, 20000.0)], &[0], 600);
        assert!(matches!(verdict, SizeVerdict::Refuse(_)), "{verdict:?}");
    }

    #[test]
    fn size_verdict_should_only_look_at_the_chosen_pages() {
        let sizes = [(612.0, 792.0), (2384.0, 3370.0)];
        assert_eq!(size_verdict(&sizes, &[0], 600), SizeVerdict::Fine);
        assert!(matches!(
            size_verdict(&sizes, &[0, 1], 600),
            SizeVerdict::Refuse(_)
        ));
    }

    #[test]
    fn check_should_disable_export_when_pages_are_too_large() {
        let sizes = [(2384.0, 3370.0)];
        let mut form = form();
        form.dpi = DpiChoice::Preset(600);
        let result = check(&form, &facts(&[], &sizes));
        assert!(!result.can_export());
    }

    #[test]
    fn check_should_still_allow_export_with_a_warning() {
        let sizes = [(2384.0, 3370.0)];
        let result = check(&form(), &facts(&[], &sizes));
        assert!(matches!(result.size, SizeVerdict::Warn(_)));
        assert!(result.can_export());
    }

    #[test]
    fn preview_text_should_name_the_first_file_and_count_the_rest() {
        let one = vec![PathBuf::from("out").join("report_p001.png")];
        let many = vec![
            PathBuf::from("out").join("report_p001.png"),
            PathBuf::from("out").join("report_p002.png"),
            PathBuf::from("out").join("report_p003.png"),
        ];
        assert_eq!(
            [preview_text(&one), preview_text(&many), preview_text(&[])],
            ["report_p001.png", "report_p001.png … 3 files", ""]
        );
    }

    #[test]
    fn exported_message_should_say_page_or_pages_and_the_folder() {
        let folder = Path::new(r"C:\docs");
        assert_eq!(
            [exported_message(1, folder), exported_message(3, folder)],
            [
                r"Exported 1 page to C:\docs",
                r"Exported 3 pages to C:\docs"
            ]
        );
    }

    #[test]
    fn overwrite_question_should_count_the_files() {
        assert_eq!(
            [overwrite_question(1), overwrite_question(4)],
            ["Overwrite 1 existing file?", "Overwrite 4 existing files?"]
        );
    }

    #[test]
    fn progress_text_should_name_the_page_being_exported() {
        assert_eq!(
            [progress_text(2, 12), progress_text(12, 12)],
            ["Exporting page 3 of 12", "Exporting page 12 of 12"]
        );
    }
}

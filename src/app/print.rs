//! Printing through Windows' own print dialog and GDI. Windows only: `mod.rs`
//! includes this file with `#[cfg(windows)]`, which compiles code only when
//! building for Windows.
//!
//! The print dialog runs on the UI thread (it is modal, like the Open dialog).
//! Everything after it — rendering each page with hayro and sending it to the
//! printer as a bitmap — runs on one background thread, so the UI never renders.
//!
//! Windows APIs are C functions, so Rust can't check how they use memory;
//! each call sits in an `unsafe` block with a `// SAFETY:` comment saying why
//! it is sound. Raw handles are wrapped in small types whose `Drop` frees them,
//! so no early return or error can leak one ("RAII": the value owns the resource).

use eframe::egui;
use pdf_light::document;
use pdf_light::export::parse_page_range;
use pdf_light::pages::KeptPage;
use pdf_light::print_layout::{self, Dib, Placement, bands, bgr_dib};
use pdf_light::render;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
    Win32WindowHandle, WindowHandle,
};
use std::num::NonZeroIsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use thiserror::Error;
use windows::Win32::Foundation::{ERROR_CANCELLED, GlobalFree, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteDC, GetDeviceCaps, HALFTONE, HDC,
    HORZRES, LOGPIXELSX, LOGPIXELSY, SRCCOPY, SetBrushOrgEx, SetStretchBltMode, StretchDIBits,
    VERTRES,
};
use windows::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Controls::Dialogs::{
    PD_COLLATE, PD_HIDEPRINTTOFILE, PD_NOCURRENTPAGE, PD_NOSELECTION, PD_PAGENUMS, PD_RESULT_PRINT,
    PD_RETURNDC, PD_SELECTION, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGEXW, PRINTPAGERANGE,
    PrintDlgExW, START_PAGE_GENERAL,
};
use windows::core::{HRESULT, PCWSTR};

/// How many separate ranges the dialog's Pages box may hold ("1-3, 7, 9-12" is three).
const MAX_RANGES: usize = 64;
/// Bitmaps go to the printer in strips of about this many bytes.
const BAND_BYTES: usize = 8 * 1024 * 1024;
/// Longest the window waits on close for a cancelled print to stop.
const CLOSE_WAIT: Duration = Duration::from_secs(120);

/// The main window, which owns the print dialog and the "Save print output
/// as" box. Only valid while that window exists, which is the app's whole life.
#[derive(Debug, Clone, Copy)]
pub struct Owner(NonZeroIsize);

impl Owner {
    /// The window behind `window` (eframe's creation context or frame), if it is a Win32 window.
    pub fn of(window: &impl HasWindowHandle) -> Option<Self> {
        match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(Self(handle.hwnd)),
            _ => None,
        }
    }

    fn hwnd(self) -> HWND {
        HWND(self.0.get() as *mut std::ffi::c_void)
    }
}

// rfd's save dialog takes its parent window through these two traits.
impl HasWindowHandle for Owner {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(self.0));
        // SAFETY: `self.0` is the main window's HWND, read from eframe at
        // start-up. That window lives as long as the app, so it outlives this
        // borrow; it is only used on the UI thread, which owns the window.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for Owner {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

/// Which pages the owner picked in the print dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageChoice {
    All,
    /// The pages selected in the pages panel.
    Selection,
    /// Page ranges in the current numbering, 1-based and inclusive.
    Ranges(Vec<(u32, u32)>),
}

/// The 0-based positions (current numbering) `choice` stands for, in order.
/// `selected` is the pages panel's selection. The error is for the owner.
pub fn chosen_positions(
    choice: &PageChoice,
    page_count: usize,
    selected: &[usize],
) -> Result<Vec<usize>, String> {
    let positions = match choice {
        PageChoice::All => (0..page_count).collect(),
        PageChoice::Selection => {
            let mut positions: Vec<usize> = selected
                .iter()
                .copied()
                .filter(|&p| p < page_count)
                .collect();
            positions.sort_unstable();
            positions.dedup();
            positions
        }
        // The same rules as Export's page range, so "Pages" means the same in both.
        PageChoice::Ranges(ranges) => parse_page_range(&range_text(ranges), page_count)?,
    };
    if positions.is_empty() {
        return Err("there are no pages to print".to_owned());
    }
    Ok(positions)
}

/// The dialog's ranges as text, e.g. "1-3, 7".
fn range_text(ranges: &[(u32, u32)]) -> String {
    let parts: Vec<String> = ranges
        .iter()
        .map(|&(from, to)| {
            if from == to {
                from.to_string()
            } else {
                format!("{from}-{to}")
            }
        })
        .collect();
    parts.join(", ")
}

/// The order pages go to the printer, as indexes into the chosen pages:
/// collated copies print the whole set again (1 2 3 1 2 3), uncollated ones
/// repeat each page (1 1 2 2 3 3).
fn print_order(pages: usize, copies: u32, collate: bool) -> Vec<usize> {
    let copies = copies.max(1) as usize;
    if collate {
        (0..copies).flat_map(|_| 0..pages).collect()
    } else {
        (0..pages)
            .flat_map(|page| std::iter::repeat_n(page, copies))
            .collect()
    }
}

/// "Printing page 3 of 12" while page `done + 1` is being worked on.
pub fn progress_text(done: usize, total: usize) -> String {
    format!("Printing page {} of {total}", (done + 1).min(total))
}

/// The status-bar message after a finished print.
pub fn sent_message(count: usize, printer: &str) -> String {
    let noun = if count == 1 { "page" } else { "pages" };
    format!("Sent {count} {noun} to {printer}")
}

/// Why a print didn't finish.
#[derive(Debug, Error)]
pub enum PrintError {
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

/// A printer device context ("DC"): what GDI draws on. `Drop` deletes it.
pub struct PrinterDc(HDC);

impl Drop for PrinterDc {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: `self.0` is a DC this value owns (from PrintDlgExW or
            // CreateDCW) and nothing else deletes it; after this it is never used.
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
}

// SAFETY: a DC handle is a process-wide token, not tied to the thread that
// made it; Windows documents printing from a worker thread. `PrinterDc` is not
// `Clone` or `Sync`, so moving it to another thread hands over the only copy:
// the DC is never used by two threads, and all its printing calls stay on the
// thread it was moved to. (`Send` = may be moved to another thread.)
unsafe impl Send for PrinterDc {}

impl PrinterDc {
    /// Opens the printer named `name` directly, without the dialog.
    #[cfg(test)]
    fn open(name: &str) -> Option<Self> {
        use windows::Win32::Graphics::Gdi::CreateDCW;
        let name = wide(name);
        // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call;
        // no DEVMODE is passed, so the printer's defaults are used.
        let dc = unsafe {
            CreateDCW(
                windows::core::w!("WINSPOOL"),
                PCWSTR(name.as_ptr()),
                PCWSTR::null(),
                None,
            )
        };
        (!dc.is_invalid()).then_some(Self(dc))
    }

    /// One of the printer's numbers, such as its DPI or printable width.
    fn caps(&self, index: windows::Win32::Graphics::Gdi::GET_DEVICE_CAPS_INDEX) -> i32 {
        // SAFETY: `self.0` is a live DC owned by `self`; GetDeviceCaps only reads it.
        unsafe { GetDeviceCaps(Some(self.0), index) }
    }
}

/// A memory block the print dialog allocated (its DEVMODE and DEVNAMES). `Drop` frees it.
struct GlobalMemory(HGLOBAL);

impl Drop for GlobalMemory {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: the dialog handed this block to us to free; it is not locked
            // and nothing uses it after this. (GlobalFree's result is not useful
            // here: there is nothing to do if freeing fails.)
            unsafe {
                let _ = GlobalFree(Some(self.0));
            }
        }
    }
}

impl GlobalMemory {
    /// The printer name and port in a DEVNAMES block.
    fn dev_names(&self) -> Option<DevNames> {
        if self.0.is_invalid() {
            return None;
        }
        // SAFETY: `self.0` is a live global block owned by `self`. GlobalLock
        // returns its address (or null, checked), valid for GlobalSize bytes
        // until GlobalUnlock; the bytes are copied out before unlocking.
        unsafe {
            let size = GlobalSize(self.0);
            let start = GlobalLock(self.0).cast::<u8>();
            if start.is_null() {
                return None;
            }
            let names = dev_names(std::slice::from_raw_parts(start, size));
            let _ = GlobalUnlock(self.0);
            names
        }
    }
}

/// The printer's name and the port it prints to, e.g. "Microsoft Print to PDF"
/// on "PORTPROMPT:".
#[derive(Debug, Clone, PartialEq, Eq)]
struct DevNames {
    device: String,
    port: String,
}

/// Reads a DEVNAMES block: four 16-bit offsets (driver, device, output port,
/// flags), counted in UTF-16 units from the start, each to a NUL-terminated
/// UTF-16 string.
fn dev_names(devnames: &[u8]) -> Option<DevNames> {
    Some(DevNames {
        device: devnames_string(devnames, 1)?,
        port: devnames_string(devnames, 2)?,
    })
}

/// The string the `field`th offset of a DEVNAMES block points at.
fn devnames_string(devnames: &[u8], field: usize) -> Option<String> {
    let at = field * 2;
    let offset = usize::from(u16::from_le_bytes([
        *devnames.get(at)?,
        *devnames.get(at + 1)?,
    ]));
    let units: Vec<u16> = devnames
        .get(offset * 2..)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .take_while(|&unit| unit != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// `text` as NUL-terminated UTF-16, the string type Windows APIs take.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Whether a printer on `port` writes a file and would ask where, from inside
/// StartDocW: "PORTPROMPT:" (Microsoft Print to PDF, XPS Document Writer) and
/// "FILE:" (print to file). pdf-light asks first, so the print thread never
/// waits on a box the owner may not see.
pub fn needs_output_path(port: &str) -> bool {
    let port = port.trim();
    port.eq_ignore_ascii_case("PORTPROMPT:") || port.eq_ignore_ascii_case("FILE:")
}

/// Where the save box starts: "report-print.pdf" next to "report.pdf" for a
/// PDF printer ("PORTPROMPT:"), "report-print.prn" for "FILE:".
fn suggested_output(pdf_path: &Path, port: &str) -> PathBuf {
    let stem = pdf_path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = if port.trim().eq_ignore_ascii_case("FILE:") {
        "prn"
    } else {
        "pdf"
    };
    pdf_path.with_file_name(format!("{stem}-print.{extension}"))
}

/// Asks where to save printer output, with the Windows save box owned by the
/// main window (it blocks, like the Open dialog). `None` when cancelled.
fn ask_output_path(owner: Owner, suggested: &Path) -> Option<PathBuf> {
    let extension = suggested
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let label = if extension == "pdf" {
        "PDF"
    } else {
        "Printer file"
    };
    let mut dialog = rfd::FileDialog::new()
        .set_parent(&owner)
        .add_filter(label, &[extension.as_str()]);
    if let Some(name) = suggested.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    if let Some(folder) = suggested.parent().filter(|f| f.is_dir()) {
        dialog = dialog.set_directory(folder);
    }
    let mut path = dialog.save_file()?;
    if path.extension().is_none() {
        path.set_extension(&extension);
    }
    Some(path)
}

/// What the print dialog's pages and copies boxes said.
pub struct Settings {
    pub pages: PageChoice,
    /// Copies pdf-light sends itself; 1 when the printer driver makes the copies.
    pub copies: u32,
    pub collate: bool,
}

/// What the owner chose in the print dialog, with the printer ready to use.
pub struct PrintJob {
    dc: PrinterDc,
    pub printer_name: String,
    pub pages: PageChoice,
    /// Copies pdf-light sends itself; 1 when the printer driver makes the copies.
    pub copies: u32,
    pub collate: bool,
    /// The name the print queue shows, e.g. "report.pdf".
    pub document_name: String,
    /// Print into this file instead of to the printer's port. Set for printers
    /// that write a file, so StartDocW never asks on the print thread.
    output: Option<PathBuf>,
}

impl PrintJob {
    /// Gets a job ready for `dc`. When the printer's `port` writes a file,
    /// `ask_output` is asked where (given a suggested path) now, on the UI
    /// thread; `None` from it cancels the print. Shared by the app and the tests.
    fn prepare(
        dc: PrinterDc,
        printer_name: String,
        port: &str,
        settings: Settings,
        pdf_path: &Path,
        ask_output: impl FnOnce(&Path) -> Option<PathBuf>,
    ) -> Option<Self> {
        let output = if needs_output_path(port) {
            Some(ask_output(&suggested_output(pdf_path, port))?)
        } else {
            None
        };
        Some(Self {
            dc,
            printer_name,
            pages: settings.pages,
            copies: settings.copies,
            collate: settings.collate,
            document_name: pdf_path
                .file_name()
                .unwrap_or(pdf_path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            output,
        })
    }
}

/// Shows the Windows print dialog (blocking, like any modal dialog), then for
/// a printer that writes a file (a PDF printer, say) the save box. Returns
/// `None` when the owner cancels either. "Selection" is offered when `has_selection`.
pub fn choose_printer(
    owner: Owner,
    page_count: usize,
    has_selection: bool,
    pdf_path: &Path,
) -> Result<Option<PrintJob>, String> {
    let mut ranges = vec![PRINTPAGERANGE::default(); MAX_RANGES];
    // Let the driver make copies when it can (PD_USEDEVMODECOPIESANDCOLLATE);
    // the dialog then reports 1 copy. Start on "Selection" when there is one.
    let mut flags = PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE | PD_NOCURRENTPAGE;
    flags |= PD_HIDEPRINTTOFILE;
    flags |= if has_selection {
        PD_SELECTION
    } else {
        PD_NOSELECTION
    };
    let mut dialog = PRINTDLGEXW {
        lStructSize: size_of::<PRINTDLGEXW>() as u32,
        hwndOwner: owner.hwnd(),
        Flags: flags,
        nMaxPageRanges: MAX_RANGES as u32,
        lpPageRanges: ranges.as_mut_ptr(),
        nMinPage: 1,
        nMaxPage: u32::try_from(page_count).unwrap_or(u32::MAX),
        nCopies: 1,
        nStartPage: START_PAGE_GENERAL,
        ..Default::default()
    };
    // SAFETY: `dialog` is a fully initialised PRINTDLGEXW with the right size,
    // a live owner window, and `lpPageRanges` pointing at `ranges`, which holds
    // `nMaxPageRanges` entries and outlives the call. The call runs on the UI
    // thread, which owns the window.
    let result = unsafe { PrintDlgExW(&mut dialog) };
    // Own whatever the dialog allocated, even after Cancel or an error, so it is freed.
    let _dev_mode = GlobalMemory(dialog.hDevMode);
    let dev_names = GlobalMemory(dialog.hDevNames);
    let dc = PrinterDc(dialog.hDC);
    result.map_err(|error| reason("the print dialog failed", &error))?;
    if dialog.dwResultAction != PD_RESULT_PRINT {
        return Ok(None);
    }
    if dc.0.is_invalid() {
        return Err("the printer couldn't be opened".to_owned());
    }
    let pages = if dialog.Flags.contains(PD_SELECTION) {
        PageChoice::Selection
    } else if dialog.Flags.contains(PD_PAGENUMS) {
        let count = (dialog.nPageRanges as usize).min(MAX_RANGES);
        PageChoice::Ranges(
            ranges
                .iter()
                .take(count)
                .map(|r| (r.nFromPage, r.nToPage))
                .collect(),
        )
    } else {
        PageChoice::All
    };
    let names = dev_names.dev_names().unwrap_or(DevNames {
        device: String::new(),
        port: String::new(),
    });
    let settings = Settings {
        pages,
        copies: dialog.nCopies.max(1),
        collate: dialog.Flags.contains(PD_COLLATE),
    };
    Ok(PrintJob::prepare(
        dc,
        names.device,
        &names.port,
        settings,
        pdf_path,
        |suggested| ask_output_path(owner, suggested),
    ))
}

/// "context: Windows' own message", for the error dialog.
fn reason(context: &str, error: &windows::core::Error) -> String {
    let message = error.message();
    let message = message.trim().trim_end_matches('.');
    if message.is_empty() {
        context.to_owned()
    } else {
        format!("{context}: {message}")
    }
}

/// The last Windows error after a failed GDI call: `Cancelled` when the owner
/// cancelled (for example the "Save print output as" box of a PDF printer).
fn last_error(context: &str) -> PrintError {
    let error = windows::core::Error::from_thread();
    if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
        PrintError::Cancelled
    } else {
        PrintError::Failed(reason(context, &error))
    }
}

/// A started print job. Until `finish` is called, `Drop` aborts it, so an
/// error or Cancel anywhere throws away what was sent so far.
struct SpoolDocument<'a> {
    dc: &'a PrinterDc,
    open: bool,
}

impl<'a> SpoolDocument<'a> {
    fn start(dc: &'a PrinterDc, name: &str, output: Option<&Path>) -> Result<Self, PrintError> {
        let name = wide(name);
        let output = output.map(|path| wide(&path.to_string_lossy()));
        let info = DOCINFOW {
            cbSize: size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(name.as_ptr()),
            lpszOutput: output
                .as_ref()
                .map_or(PCWSTR::null(), |path| PCWSTR(path.as_ptr())),
            ..Default::default()
        };
        // SAFETY: `dc.0` is a live printer DC; `info` is initialised with its
        // size, and its strings are NUL-terminated and live until after the call.
        let job = unsafe { StartDocW(dc.0, &info) };
        if job <= 0 {
            return Err(last_error("the printer didn't accept the job"));
        }
        Ok(Self { dc, open: true })
    }

    /// One sheet: `draw` draws on the DC between StartPage and EndPage.
    fn page(&mut self, draw: impl FnOnce(HDC) -> Result<(), PrintError>) -> Result<(), PrintError> {
        let dc = self.dc.0;
        // SAFETY: `dc` is a live printer DC inside a started document.
        if unsafe { StartPage(dc) } <= 0 {
            return Err(last_error("the printer didn't accept a page"));
        }
        // Smooth scaling, for when the bitmap is shrunk; HALFTONE needs the brush origin reset.
        // SAFETY: as above; these only set drawing attributes of `dc`.
        unsafe {
            SetStretchBltMode(dc, HALFTONE);
            let _ = SetBrushOrgEx(dc, 0, 0, None);
        }
        draw(dc)?;
        // SAFETY: as above, closing the page opened by StartPage.
        if unsafe { EndPage(dc) } <= 0 {
            return Err(last_error("the printer didn't accept a page"));
        }
        Ok(())
    }

    /// Ends the document: the printer gets it.
    fn finish(mut self) -> Result<(), PrintError> {
        // SAFETY: `self.dc.0` is a live printer DC with a started document.
        if unsafe { EndDoc(self.dc.0) } <= 0 {
            // `open` stays true, so `Drop` aborts what's left of the job.
            return Err(last_error("the printer didn't finish the job"));
        }
        self.open = false;
        Ok(())
    }
}

impl Drop for SpoolDocument<'_> {
    fn drop(&mut self) {
        if self.open {
            // SAFETY: `self.dc.0` is a live printer DC with a started, unfinished document.
            unsafe {
                AbortDoc(self.dc.0);
            }
        }
    }
}

/// Prints `positions` (0-based, current numbering; `kept[position]` is the
/// page in the file, with the owner's turn) of the PDF in `bytes` on `job`'s printer. Runs on a
/// background thread. `progress(done, total)` runs before each sheet; when it
/// returns `false` the job is aborted with `Err(Cancelled)`. Returns how many
/// sheets were sent (pages × the copies pdf-light made itself).
pub fn print(
    bytes: &Arc<Vec<u8>>,
    kept: &[KeptPage],
    positions: &[usize],
    job: PrintJob,
    progress: &dyn Fn(usize, usize) -> bool,
) -> Result<usize, PrintError> {
    let failed = |error: pdf_light::error::PdfError| PrintError::Failed(error.to_string());
    let pages: Vec<KeptPage> = positions
        .iter()
        .filter_map(|&position| kept.get(position).copied())
        .collect();
    if pages.is_empty() {
        return Err(PrintError::Failed("there are no pages to print".to_owned()));
    }
    let pdf = document::open_pdf(bytes).map_err(failed)?;
    let dpi = (job.dc.caps(LOGPIXELSX), job.dc.caps(LOGPIXELSY));
    let area = (job.dc.caps(HORZRES), job.dc.caps(VERTRES));
    if dpi.0 <= 0 || dpi.1 <= 0 || area.0 <= 0 || area.1 <= 0 {
        return Err(PrintError::Failed(
            "the printer didn't report its paper size".to_owned(),
        ));
    }

    let order = print_order(pages.len(), job.copies, job.collate);
    let total = order.len();
    let mut document = SpoolDocument::start(&job.dc, &job.document_name, job.output.as_deref())?;
    // Uncollated copies repeat a page, so the last page's bitmap is kept for the next sheet.
    let mut last: Option<(usize, Dib, Placement)> = None;
    for (done, &which) in order.iter().enumerate() {
        if !progress(done, total) {
            return Err(PrintError::Cancelled);
        }
        if last.as_ref().is_none_or(|(page, _, _)| *page != which) {
            let invalid = || failed(pdf_light::error::PdfError::Invalid);
            let kept = pages.get(which).copied().ok_or_else(invalid)?;
            let page = pdf.pages().get(kept.index).ok_or_else(invalid)?;
            // The page as the owner sees it, so a turned page is placed turned.
            let size = kept.rotation.size(page.render_dimensions());
            let placement = print_layout::place(size, area, dpi);
            let scale = print_layout::render_scale(size, &placement, dpi);
            // Free the previous bitmap before making the next one.
            drop(last.take());
            let rendered =
                render::render_turned(&pdf, kept.index, scale, kept.rotation).map_err(failed)?;
            let dib = bgr_dib(
                &rendered.rgba,
                rendered.width,
                rendered.height,
                placement.rotate,
            )
            .ok_or_else(|| PrintError::Failed("the page image came out wrong".to_owned()))?;
            last = Some((which, dib, placement));
        }
        if let Some((_, dib, placement)) = &last {
            document.page(|dc| draw(dc, dib, placement))?;
        }
    }
    document.finish()?;
    // `job` (and its DC) is dropped here, on this thread, after the document is done.
    Ok(total)
}

/// Draws `dib` into `placement` on the page, in strips (see `print_layout::bands`).
fn draw(dc: HDC, dib: &Dib, placement: &Placement) -> Result<(), PrintError> {
    let too_big = || PrintError::Failed("the page image is too large".to_owned());
    let width = i32::try_from(dib.width).map_err(|_| too_big())?;
    let max_rows = u32::try_from(BAND_BYTES / dib.stride.max(1))
        .unwrap_or(u32::MAX)
        .max(1);
    for band in bands(dib.height, placement.y, placement.height, max_rows) {
        if band.dest_height <= 0 {
            continue; // Shrunk to nothing; the neighbouring strips cover it.
        }
        let start = band.row as usize * dib.stride;
        let end = start + band.rows as usize * dib.stride;
        let bits = dib.bgr.get(start..end).ok_or_else(too_big)?;
        let rows = i32::try_from(band.rows).map_err(|_| too_big())?;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative height: the rows are stored top-down.
                biHeight: -rows,
                biPlanes: 1,
                biBitCount: 24,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: `dc` is a live printer DC inside a started page. `info`
        // describes a 24-bit top-down bitmap of `width` × `rows`, and `bits`
        // holds exactly `rows` rows of `dib.stride` bytes (each row padded to 4
        // bytes as GDI expects); both live until the call returns. GDI only reads them.
        let lines = unsafe {
            StretchDIBits(
                dc,
                placement.x,
                band.dest_y,
                placement.width,
                band.dest_height,
                0,
                0,
                width,
                rows,
                Some(bits.as_ptr().cast()),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            )
        };
        if lines <= 0 {
            return Err(last_error("the printer didn't accept the page image"));
        }
    }
    Ok(())
}

/// What a finished print reports to the app.
pub enum Outcome {
    /// The status-bar message.
    Done(String),
    Cancelled,
    Failed(String),
}

/// A print running on its own thread.
pub struct PrintRun {
    cancel: Arc<AtomicBool>,
    /// Sheets sent so far; the thread writes it, the status bar reads it.
    done: Arc<AtomicUsize>,
    total: usize,
    printer_name: String,
    result: mpsc::Receiver<Result<usize, PrintError>>,
}

// Dropping a `PrintRun` (the app going away) asks the thread to abort the job.
impl Drop for PrintRun {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl PrintRun {
    /// Starts printing `positions` on a background thread (the UI thread never renders).
    pub fn start(
        ctx: &egui::Context,
        bytes: Arc<Vec<u8>>,
        kept: Vec<KeptPage>,
        positions: Vec<usize>,
        job: PrintJob,
    ) -> Self {
        let total = print_order(positions.len(), job.copies, job.collate).len();
        let printer_name = job.printer_name.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicUsize::new(0));
        let (sender, result) = mpsc::channel();
        let (thread_cancel, thread_done, thread_ctx) =
            (Arc::clone(&cancel), Arc::clone(&done), ctx.clone());
        // `move` hands `job` (with its printer DC) to the thread; from here on
        // only that thread touches the DC.
        std::thread::spawn(move || {
            let progress = |sent: usize, _total: usize| {
                thread_done.store(sent, Ordering::Relaxed);
                thread_ctx.request_repaint();
                !thread_cancel.load(Ordering::Relaxed)
            };
            let outcome = print(&bytes, &kept, &positions, job, &progress);
            // The app may be gone already; then nobody needs the result.
            let _ = sender.send(outcome);
            thread_ctx.request_repaint();
        });
        Self {
            cancel,
            done,
            total,
            printer_name,
            result,
        }
    }

    /// "Printing page 3 of 12".
    pub fn progress_text(&self) -> String {
        progress_text(self.done.load(Ordering::Relaxed), self.total)
    }

    /// Asks the thread to abort the job; it stops before the next sheet.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Cancels and waits for the thread to abort the job and let go of the
    /// printer, so the window never closes in the middle of spooling.
    pub fn cancel_and_wait(&self) {
        self.cancel();
        let _ = self.result.recv_timeout(CLOSE_WAIT);
    }

    /// Whether the thread has ended, and how.
    pub fn poll(&self) -> Option<Outcome> {
        match self.result.try_recv() {
            Ok(Ok(sent)) => Some(Outcome::Done(sent_message(sent, &self.printer_name))),
            Ok(Err(PrintError::Cancelled)) => Some(Outcome::Cancelled),
            Ok(Err(PrintError::Failed(reason))) => Some(Outcome::Failed(reason)),
            Err(mpsc::TryRecvError::Empty) => None,
            // The thread ended without sending: it must have panicked.
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Outcome::Failed("printing stopped unexpectedly".to_owned()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chosen_positions_should_list_every_page_for_all() {
        assert_eq!(
            chosen_positions(&PageChoice::All, 3, &[]),
            Ok(vec![0, 1, 2])
        );
    }

    #[test]
    fn chosen_positions_should_use_the_selection_in_page_order() {
        assert_eq!(
            chosen_positions(&PageChoice::Selection, 5, &[3, 1, 3]),
            Ok(vec![1, 3])
        );
    }

    #[test]
    fn chosen_positions_should_turn_ranges_into_positions() {
        let choice = PageChoice::Ranges(vec![(1, 3), (7, 7)]);
        assert_eq!(chosen_positions(&choice, 10, &[]), Ok(vec![0, 1, 2, 6]));
    }

    #[test]
    fn chosen_positions_should_refuse_pages_past_the_end() {
        let choice = PageChoice::Ranges(vec![(2, 9)]);
        assert_eq!(
            chosen_positions(&choice, 5, &[]),
            Err("Pages must be between 1 and 5.".to_owned())
        );
    }

    #[test]
    fn chosen_positions_should_refuse_an_empty_selection() {
        assert!(chosen_positions(&PageChoice::Selection, 5, &[]).is_err());
    }

    #[test]
    fn range_text_should_join_ranges_like_the_export_dialog() {
        assert_eq!(range_text(&[(1, 3), (7, 7), (9, 12)]), "1-3, 7, 9-12");
    }

    #[test]
    fn print_order_should_repeat_the_set_when_collated() {
        assert_eq!(print_order(3, 2, true), vec![0, 1, 2, 0, 1, 2]);
    }

    #[test]
    fn print_order_should_repeat_each_page_when_not_collated() {
        assert_eq!(print_order(3, 2, false), vec![0, 0, 1, 1, 2, 2]);
    }

    #[test]
    fn print_order_should_print_once_for_zero_copies() {
        assert_eq!(print_order(2, 0, false), vec![0, 1]);
    }

    #[test]
    fn dev_names_should_read_the_printer_name_and_port() {
        // Offsets (in UTF-16 units): driver 4, device 4, output 7, flags 0;
        // then "Pr" NUL "X:" NUL.
        let units: [u16; 10] = [
            4, 4, 7, 0, 'P' as u16, 'r' as u16, 0, 'X' as u16, ':' as u16, 0,
        ];
        let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(
            dev_names(&bytes),
            Some(DevNames {
                device: "Pr".to_owned(),
                port: "X:".to_owned()
            })
        );
    }

    #[test]
    fn dev_names_should_refuse_a_truncated_block() {
        assert_eq!(dev_names(&[4, 0, 4, 0]), None);
    }

    #[test]
    fn needs_output_path_should_be_true_for_ports_that_write_a_file() {
        assert_eq!(
            [
                needs_output_path("PORTPROMPT:"),
                needs_output_path("portprompt:"),
                needs_output_path(" FILE: "),
                needs_output_path("nul:"),
                needs_output_path("BRW749779EA03BA"),
                needs_output_path("WSD-6515df7b"),
                needs_output_path(""),
            ],
            [true, true, true, false, false, false, false]
        );
    }

    #[test]
    fn suggested_output_should_sit_next_to_the_pdf() {
        let pdf = Path::new(r"C:\docs\report.pdf");
        assert_eq!(
            [
                suggested_output(pdf, "PORTPROMPT:"),
                suggested_output(pdf, "FILE:")
            ],
            [
                PathBuf::from(r"C:\docs\report-print.pdf"),
                PathBuf::from(r"C:\docs\report-print.prn")
            ]
        );
    }

    fn all_once() -> Settings {
        Settings {
            pages: PageChoice::All,
            copies: 1,
            collate: false,
        }
    }

    #[test]
    fn prepare_should_not_ask_for_a_file_for_a_real_printer() {
        let job = PrintJob::prepare(
            PrinterDc(HDC::default()),
            "Office".to_owned(),
            "BRW749779EA03BA",
            all_once(),
            Path::new("report.pdf"),
            |_| panic!("must not ask"),
        );
        assert_eq!(job.map(|j| j.output), Some(None));
    }

    #[test]
    fn prepare_should_cancel_when_the_save_box_is_cancelled() {
        let job = PrintJob::prepare(
            PrinterDc(HDC::default()),
            "Microsoft Print to PDF".to_owned(),
            "PORTPROMPT:",
            all_once(),
            Path::new("report.pdf"),
            |_| None,
        );
        assert!(job.is_none());
    }

    #[test]
    fn progress_text_should_name_the_page_being_printed() {
        assert_eq!(
            [progress_text(2, 12), progress_text(12, 12)],
            ["Printing page 3 of 12", "Printing page 12 of 12"]
        );
    }

    #[test]
    fn sent_message_should_say_page_or_pages_and_the_printer() {
        assert_eq!(
            [sent_message(1, "Office"), sent_message(12, "Office")],
            ["Sent 1 page to Office", "Sent 12 pages to Office"]
        );
    }

    #[test]
    fn dropping_a_print_run_should_cancel_it() {
        let (_sender, result) = mpsc::channel();
        let run = PrintRun {
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicUsize::new(0)),
            total: 1,
            printer_name: String::new(),
            result,
        };
        let cancel = Arc::clone(&run.cancel);
        drop(run);
        assert!(cancel.load(Ordering::Relaxed));
    }

    /// The real thing, without the dialog: prints a generated 3-page PDF (the
    /// middle page landscape) to "Microsoft Print to PDF" through the same
    /// `print` the app uses, then checks the PDF that comes out.
    /// Run with `cargo test -- --ignored print_to_pdf`.
    #[test]
    #[ignore = "uses the Windows print spooler and the Microsoft Print to PDF printer"]
    fn print_to_pdf_should_place_and_turn_pages() {
        let output = OutputFile(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join(format!("print-test-{}.pdf", std::process::id())),
        );
        let _ = std::fs::remove_file(&output.0);
        let job = pdf_printer_job(&output.0);
        let sizes = [(400.0, 500.0), (500.0, 400.0), (612.0, 792.0)];
        let bytes = Arc::new(test_pdf(&sizes));
        let positions = chosen_positions(&job.pages, sizes.len(), &[]).expect("all pages");
        let progress = |_: usize, _: usize| true;
        let kept: Vec<KeptPage> = (0..sizes.len()).map(KeptPage::upright).collect();
        let sent = print(&bytes, &kept, &positions, job, &progress).expect("prints");
        assert_eq!(sent, 3);

        // The spooler writes the file after EndDoc; wait until it opens as a PDF.
        let printed = (0..120)
            .find_map(|_| {
                let opened = document::open(&output.0).ok();
                if opened.is_none() {
                    std::thread::sleep(Duration::from_millis(500));
                }
                opened
            })
            .expect("the printed PDF appears");
        assert_eq!(printed.page_sizes.len(), 3);
        let pdf = document::open_pdf(&printed.bytes).expect("opens");
        for (index, &(paper_w, paper_h)) in printed.page_sizes.iter().enumerate() {
            println!("page {}: paper {paper_w} × {paper_h} pt", index + 1);
            assert!(
                paper_w < paper_h,
                "page {} should be portrait paper",
                index + 1
            );
            let page = render::render_page(&pdf, index, 0.5).expect("renders");
            let ink = ink_box(&page).expect("something was printed");
            let (left, top, right, bottom) = (
                ink.0,
                ink.1,
                page.width - 1 - ink.2,
                page.height - 1 - ink.3,
            );
            println!("  margins left {left} right {right} top {top} bottom {bottom} px");
            assert!(
                left.abs_diff(right) <= 3,
                "page {} centred across",
                index + 1
            );
            assert!(top.abs_diff(bottom) <= 3, "page {} centred down", index + 1);
            if index < 2 {
                // 400 × 500 pt fits on A4 and Letter, so it prints at 100 %:
                // 200 px wide at this half-scale render (turned or not).
                assert!(
                    (ink.2 - ink.0 + 1).abs_diff(200) <= 2,
                    "page {} not enlarged",
                    index + 1
                );
            }
            // The black marker sits in the page's top-left corner. Turned
            // anticlockwise, the landscape page has it bottom-left on the paper.
            let marker_bottom = index == 1;
            let probe_y = if marker_bottom { ink.3 - 8 } else { ink.1 + 8 };
            assert!(
                is_dark(&page, ink.0 + 8, probe_y),
                "page {}: marker in the expected corner",
                index + 1
            );
        }
    }

    /// Cancel after the first sheet: the job is aborted (AbortDoc) and nothing
    /// is left in the queue. Run with `cargo test -- --ignored print_to_pdf`.
    #[test]
    #[ignore = "uses the Windows print spooler and the Microsoft Print to PDF printer"]
    fn print_to_pdf_should_abort_the_job_when_cancelled() {
        let output = OutputFile(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join(format!("print-cancel-test-{}.pdf", std::process::id())),
        );
        let job = pdf_printer_job(&output.0);
        let bytes = Arc::new(test_pdf(&[(400.0, 500.0); 3]));
        let progress = |done: usize, _: usize| done < 1;
        let kept: Vec<KeptPage> = (0..3).map(KeptPage::upright).collect();
        let outcome = print(&bytes, &kept, &[0, 1, 2], job, &progress);
        assert!(matches!(outcome, Err(PrintError::Cancelled)), "{outcome:?}");
        std::thread::sleep(Duration::from_secs(3));
        let printed = document::open(&output.0).map(|d| d.page_sizes.len());
        assert!(printed.is_err(), "no PDF should come out, got {printed:?}");
    }

    /// A job for "Microsoft Print to PDF", made the way the app makes one after
    /// the dialog: its port ("PORTPROMPT:", as `Get-Printer` lists it) needs a
    /// file, so `prepare` asks for one, and the stand-in for the save box answers
    /// `output`. StartDocW then writes there without a box of its own.
    fn pdf_printer_job(output: &Path) -> PrintJob {
        let printer = "Microsoft Print to PDF";
        let dc = PrinterDc::open(printer).expect("Microsoft Print to PDF is installed");
        let mut asked = None;
        let job = PrintJob::prepare(
            dc,
            printer.to_owned(),
            "PORTPROMPT:",
            all_once(),
            Path::new(r"C:\docs\sample.pdf"),
            |suggested| {
                asked = Some(suggested.to_path_buf());
                Some(output.to_path_buf())
            },
        )
        .expect("the save box answered");
        assert_eq!(asked, Some(PathBuf::from(r"C:\docs\sample-print.pdf")));
        assert_eq!(job.output.as_deref(), Some(output));
        job
    }

    /// Deletes the printed file when the test ends, pass or fail.
    struct OutputFile(PathBuf);

    impl Drop for OutputFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// The smallest box (left, top, right, bottom) holding every dark pixel.
    fn ink_box(page: &render::RenderedPage) -> Option<(u32, u32, u32, u32)> {
        let mut found: Option<(u32, u32, u32, u32)> = None;
        for y in 0..page.height {
            for x in 0..page.width {
                if is_dark(page, x, y) {
                    let b = found.get_or_insert((x, y, x, y));
                    *b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
                }
            }
        }
        found
    }

    fn is_dark(page: &render::RenderedPage, x: u32, y: u32) -> bool {
        let at = ((y * page.width + x) * 4) as usize;
        page.rgba
            .get(at..at + 3)
            .is_some_and(|rgb| rgb.iter().all(|&c| c < 100))
    }

    /// A PDF with one page per size: a black frame around the page's edge and a
    /// filled black square in its top-left corner.
    fn test_pdf(sizes: &[(f32, f32)]) -> Vec<u8> {
        use lopdf::{Document, Object, Stream, dictionary};
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids: Vec<Object> = Vec::new();
        for &(w, h) in sizes {
            let content = format!(
                "0 g 0 G 6 w 3 3 {} {} re S 0 {} 60 60 re f",
                w - 6.0,
                h - 6.0,
                h - 60.0
            );
            let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), w.into(), h.into()],
                "Contents" => content_id,
            });
            kids.push(page_id.into());
        }
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("PDF serializes");
        bytes
    }
}

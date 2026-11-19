# pdf-light — design

Date: 2026-11-18 · Status: draft for approval

## 1. Intent

A small, fast PDF app for Windows, written in **pure Rust**, that the owner uses mostly for
**reading**, plus three jobs: **search**, **delete pages**, and **export pages to PNG / TIFF**.

It is also how the owner learns Rust (first Rust project), so the code must be readable by a
Rust newcomer and the project should teach as it goes. **Claude writes the code**; the owner
follows along, reads it, and may ask to write specific parts themself. Plan steps should still
be small and leave something runnable after each one, so the owner can see progress and try it.

It is a **personal project** (owner's GitHub, `henke350/pdf-light`), not COWI work.

## 2. Goals, non-goals, anti-goals

**Goals (v1)**
- Open one PDF at a time and read it: continuous vertical scroll, zoom, page thumbnails.
- Search the open PDF (Ctrl+F): highlight every match, step through them.
- Delete pages: select one or more thumbnails, press Delete; undo; save.
- Export pages to PNG or TIFF at a chosen resolution.
- Feel fast: the window never freezes while pages render.

**Non-goals (v1)** — may come later, not now
- Merging/combining PDFs or moving pages between documents (explicitly deferred by the owner).
- Annotations, highlights-as-markup, forms, signatures, editing text on the page.
- OCR: scanned PDFs without a text layer are viewable but not searchable.
- Password-protected / encrypted PDFs (the renderer doesn't support them; show a clear message).
- Several documents/tabs at once, macOS/Linux builds (egui would allow them later).
  (Printing was a non-goal; it was added at the owner's request on 2026-11-18. See the toolbar
  addendum, section "Print".)

**Anti-goals** — would make it a failure even if it "works"
- Silently changing or overwriting the original file.
- The UI freezing while a page renders, or slow start-up.
- Growing into a heavy app (installer, big runtime, background services).
- Crashing on a broken PDF instead of showing what it can.

## 3. Constraints

- **Pure Rust**: no C libraries (PDFium, MuPDF). Owner's choice.
- **Published crates from crates.io only**; no git dependencies (owner chose this over using
  PdfCraft's unpublished engine crates).
- Windows 11 is the target. Rust is **not installed** on the machine yet, and neither are the
  MSVC C++ build tools Rust needs for linking on Windows.
- The renderer, `hayro`, is CPU-only, has had no performance tuning yet (its README says so),
  and doesn't support encrypted PDFs. The design must hide its speed: render only what is
  visible, at screen resolution, off the UI thread, and cache results.
- Owner is new to Rust: prefer plain, explicit code over clever abstractions; no async runtime.

## 4. Approaches considered

| Approach | Trade-off | Outcome |
|---|---|---|
| PDFium (Chrome's engine) via `pdfium-render` | Fastest path to a robust app; needs a C DLL | Rejected: not pure Rust |
| MuPDF via `mupdf` | Fastest renderer; AGPL licence, needs a C toolchain | Rejected: not pure Rust |
| Fork / strip down PdfCraft (storytold/pdfcraft) | Already does everything, but ~574k lines | Rejected: not "light", unreadable for a beginner |
| Own app on PdfCraft's engine crates (git deps) | Battle-tested text + page ops; pinned to an unpublished, fast-moving repo | Rejected by owner: prefers published crates |
| **Own app on published crates: `hayro` + `lopdf` + `egui`** | Small, all pure Rust, all on crates.io; we write text-position extraction ourselves | **Chosen** |

PdfCraft remains useful as **reference reading** (MIT/Apache) — e.g. how it extracts text in
reading order on top of hayro — but no code dependency.

## 5. The design

### 5.1 Building blocks (crates)

| Job | Crate (version at time of writing) |
|---|---|
| Window and UI | `eframe` / `egui` 0.37 |
| Draw PDF pages to pixels | `hayro` 0.9 |
| Get glyphs + positions for search | `hayro-interpret` 0.9 (`Device` trait, `draw_glyph_run`, `as_unicode()`) |
| Delete pages and save | `lopdf` 0.47 (`delete_pages`, `prune_objects`, `save`) |
| Write PNG / TIFF | `image` 0.25 (PNG), `tiff` 0.11 (TIFF, incl. multi-page) |
| Native Open / Save dialogs | `rfd` |
| Errors | `thiserror` in the library part, `anyhow` in the app part |

Builder may swap a helper crate if a better published one exists. Adding another published,
pure-Rust crate is fine when one of the above falls short (e.g. for text extraction).

**Feasibility check first.** Before building the UI, a small throwaway test renders pages and
extracts text with positions from a few of the owner's real PDFs, and times both. Results go to
the owner before continuing. The owner points to the PDFs. They stay outside the repo and are
never committed. If any of them are COWI work documents, Claude confirms the COWI AI policy
with the owner first.

### 5.2 Shape of the code

One Cargo package with a **library part** (all PDF logic, no UI, unit-testable) and a thin
**app part** (egui). Suggested modules:

- `document` — loads the file bytes once, opens them with hayro, knows page count and page
  sizes, and holds the **kept-pages list** (see 5.4).
- `render` — background worker(s) that turn (page, scale) into an RGBA image; a size-bounded
  cache; stale requests are dropped when the user scrolls away.
- `text` — a `TextCollector` that implements hayro-interpret's `Device` trait, records each
  glyph's Unicode text and on-page rectangle, and builds per-page searchable text with a
  char → rectangle map. Search runs over that.
- `edit` — writes the edited PDF with lopdf.
- `export` — renders pages at a given DPI and writes PNG/TIFF.
- `ui` — eframe app: toolbar, thumbnails sidebar, page view, search bar, export dialog,
  status bar.

UI thread ↔ workers talk over `std::sync::mpsc` channels; plain `std::thread`, no tokio.

### 5.3 Reading

- Open via toolbar/Ctrl+O, by dragging a file onto the window, or as a command-line argument
  (so Windows "Open with" works).
- Continuous vertical scroll; default zoom **fit width**; zoom 25–800 % with Ctrl+wheel,
  Ctrl+plus/minus, and a zoom box. While re-rendering at a new zoom, show the old image scaled
  so the view never goes blank.
- Pages render at (zoom × screen scale factor) resolution; only visible pages plus one ahead /
  behind are requested.
- Thumbnails (~150 px wide) render lazily in the background, lower priority than visible pages.
- Keyboard: PgUp/PgDn, Home/End, arrow keys, Ctrl+F, Delete, Ctrl+Z, Ctrl+S, Ctrl+Shift+S, Ctrl+E (export).
- A page that fails to render shows a grey placeholder saying so; the rest keep working.
- Window title shows file name and a `•` when there are unsaved changes.

```
┌──────────────────────────────────────────────────────────┐
│ Open  Save  Save as  Delete  Export   Find [______]  120% │
├────────┬─────────────────────────────────────────────────┤
│ [ 1 ]  │                                                 │
│▶[ 2 ]  │            page 2, matches highlighted          │
│ [ 3 ]  │                                                 │
│  ...   │                                                 │
├────────┴─────────────────────────────────────────────────┤
│ report.pdf • · page 2 of 14 · 3 of 27 matches             │
└──────────────────────────────────────────────────────────┘
```

### 5.4 Deleting pages

- Select thumbnails with click, Ctrl+click, Shift+click; press Delete (or toolbar button).
- Deletion is **in memory only**: the app keeps a list of which original pages are still in the
  document. Deleting removes entries; **undo** (Ctrl+Z, multi-step) puts them back. The original
  file is untouched until the user saves.
- At least one page must remain (Delete is disabled when all pages are selected).
- **Save as** (Ctrl+Shift+S, the default after edits) and **Save** (Ctrl+S, overwrites the open
  file, asks for confirmation the first time per document). Both: lopdf loads the original bytes,
  deletes the removed pages, prunes orphaned objects, writes to a temporary file in the target
  folder, then renames it into place — so a failed save never leaves a half-written file.
- Closing or opening another file with unsaved changes asks: Save / Discard / Cancel.
- If lopdf can't load a file that hayro could display, saving fails with a clear message; viewing
  still works and nothing is written.

### 5.5 Search

- Ctrl+F focuses the search box. Case-insensitive, plain substring (no regex in v1).
- Whitespace-tolerant: runs of spaces/line breaks in the query and in the page text compare equal.
  The text builder inserts a space where glyphs are separated by a visible gap or a new line.
- Matches do not cross page boundaries.
- The text of each page is extracted in the background the first time a search runs and then
  kept; results stream in page by page, so the first hits show before the whole document is done.
- All matches are highlighted on the page (translucent rectangles); the current one is stronger.
  Enter / F3 = next, Shift+Enter / Shift+F3 = previous; the view scrolls to it. Status shows
  "3 of 27". Searching only covers pages that haven't been deleted.
- Glyphs that map to several characters (ligatures like "fi") share one rectangle.

### 5.6 Export

Dialog (Ctrl+E):
- **Pages**: selected thumbnails / all / a range like `1-3, 7`. Page numbers refer to the
  document as it currently looks (after deletions).
- **Format**: PNG or TIFF. TIFF has a checkbox **"one multi-page TIFF"**; default off = one file
  per page. TIFF uses LZW compression, 8-bit RGB, white background.
- **Resolution**: 72 / 150 / 300 / 600 DPI or a custom number; default 300.
- **Folder** (default: next to the PDF). Files are named `<pdf name>_p001.png` etc.; ask before
  overwriting existing files.
- Runs in the background with a progress bar and Cancel.
- Guard against enormous images: estimate pixel size first; warn above ~100 megapixels per page
  and refuse above a hard limit the builder sets from available memory (e.g. A0 at 600 DPI
  ≈ 600 MP).

### 5.7 Speed and size targets

- From launch to first page visible: **under 1 second** for a typical 50-page text PDF on this PC.
- Scrolling and zooming stay smooth; the UI thread never waits for rendering or text extraction.
- Memory stays bounded by the page-image cache (builder picks the budget, ~256 MB is a fine start).
- Release `.exe` under ~15 MB, single file, no installer. Release profile: LTO, `codegen-units = 1`,
  `strip = true`.
- If some heavy PDFs are slow because of hayro itself, that is a known limitation, not a design bug.
  **Pure Rust wins over speed** (owner's decision): if the feasibility check shows hayro is slow on
  the owner's PDFs, keep hayro and compensate (caching, lower-resolution preview first, then sharpen,
  more render threads). Do not switch to PDFium or MuPDF.

### 5.8 Quality and learning

- Library part has unit tests: search over synthetic glyph lists; delete-and-save round trip
  (build a test PDF with lopdf in code, each page carrying its number as text, delete pages,
  reopen with hayro, check count and text); export (render a test page, decode the PNG/TIFF,
  check pixel size matches the DPI). Test PDFs are generated in code, not committed.
- UI is checked by hand against a short checklist in the README.
- `cargo fmt` and `cargo clippy -- -D warnings` stay clean.
- Code follows the Rust skills vendored in `.claude/skills/`: Apollo's `rust-best-practices`
  (borrow over clone, no `unwrap`/`expect` outside tests, `thiserror` in the library / `anyhow`
  in the app, descriptive test names, `#[expect(...)]` over `#[allow(...)]`, clippy with
  `-D warnings`) and actionbook's rust-skills (coding guidelines, error handling, concurrency,
  performance, anti-patterns). Where they disagree, Apollo's handbook wins.
- Comments explain Rust ideas where they first appear (ownership and borrowing, `Result` and `?`,
  traits — e.g. implementing `Device`, threads and channels) for a newcomer. README covers how
  to build, run and test.

### 5.9 Setup (first step of the plan)

- Install Rust with `rustup` (stable, MSVC host) and the **Visual Studio 2022 Build Tools** with the
  "Desktop development with C++" workload (the linker Rust uses on Windows). Both install via
  `winget`; this changes the owner's machine, so the plan asks before running it. The Build Tools
  are free for personal/open-source use. If they can't be installed (no admin rights), fall back
  to the GNU toolchain (`x86_64-pc-windows-gnu`), which rustup installs per user with no admin.
- Edition 2024, latest stable Rust.

## 6. Left to the builder

Module internals and names, number of render threads, cache sizes, exact spacing/colours/icons,
the text-gap threshold for inserting spaces, the hard pixel limit for export, and whether a
helper crate is swapped for an equivalent published one.

## 7. Calls made on the owner's behalf (to check)

- Windows-only v1, single `.exe`, no installer.
- Deletion is in-memory with undo; "Save" overwrites only after confirmation; saves are atomic.
- Search is case-insensitive substring, no regex, no OCR.
- Export defaults: 300 DPI, one file per page, files next to the PDF; multi-page TIFF optional.
- MSVC toolchain rather than the GNU one.
- Rust skills used in skills-only mode, without the plugin's hook and MCP server.
- When the two Rust skill sets disagree, Apollo's `rust-best-practices` wins.

# pdf-light

A small, fast PDF reader for Windows, written in pure Rust. It is mainly for reading, plus a few jobs: search, delete pages, export pages to images, and print.

No C libraries are involved (no PDFium, no MuPDF). Everything comes from crates.io.

## What it does

- Opens one PDF at a time (Open button, Ctrl+O, drag a file onto the window, or pass the file on the command line).
- Reading: continuous vertical scroll, zoom (25 % up to 800 %), "Fit width" (the default).
- Pages panel with thumbnails. Select pages, delete them, undo, then Save or Save as.
- Find (Ctrl+F): every match is highlighted, step through them with the arrows or F3.
- Export pages to PNG or TIFF (including one multi-page TIFF) at a DPI you choose.
- Print through the normal Windows print dialog.
- Light and dark look, following the Windows setting.
- The window never freezes while pages render; rendering happens on background threads.

Your original file is never changed unless you choose Save and confirm "Overwrite". Save as writes a new file (suggested name: `<name>-edited.pdf`).

## What it doesn't do

- Merge PDFs or move pages between documents.
- Annotations, forms, signatures, or editing text on the page.
- Open password-protected PDFs. You get a clear message instead.
- Search scanned pages that have no text layer (there is no OCR). They can be viewed but not searched.
- Several documents or tabs at once.
- Run on macOS or Linux (only Windows is built and tested).

Known limits:

- At very high zoom (around 600–800 %) pages look a little soft. Pages are drawn as one image and not in tiles.
- It needs a graphics driver with OpenGL (it uses the `glow` backend of eframe). On a very old driver or a bare remote session it may not start.
- Microsoft XPS Document Writer gets a `.pdf` file name suggestion in the "save print output" box. Rename it to `.oxps` if you use that printer.

## Build and run

### 1. Install Rust

1. Install the Visual Studio 2022 Build Tools (free) and tick the workload "Desktop development with C++". Rust on Windows needs this to link programs.
2. Install Rust from <https://rustup.rs> (`rustup-init.exe`, default options). This gives you `cargo`, the Rust build tool.
3. Open a new terminal and check: `cargo --version`.

If you don't want the Visual Studio tools, rustup can use the GNU toolchain instead (`rustup default stable-x86_64-pc-windows-gnu`, which needs MinGW-w64 installed). The MSVC route above is the one this project is built with.

### 2. Run it

```
cargo run --release -- path\to\file.pdf
```

`cargo run` builds the program and starts it. `--release` turns on optimisations (the first build takes a few minutes, later ones are quick, and the program is much faster than a debug build). Everything after `--` goes to pdf-light, here the file to open. Leave the file out to start with an empty window.

### 3. Build the exe

```
cargo build --release
```

The program ends up at `target\release\pdf-light.exe`. It is a single file. You can copy it anywhere.

### Make it your default PDF app (optional)

1. Copy `pdf-light.exe` to a permanent folder, for example `C:\Tools\pdf-light\`.
2. Right-click a PDF, choose Open with, Choose another app, pick `pdf-light.exe`, and tick "Always use this app". Or go to Windows Settings, Apps, Default apps, search for `.pdf`, and choose it there.

### Tests and checks

```
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

`cargo test` runs the automatic tests. Two of them are marked `#[ignore]` because they use the Windows print spooler and the "Microsoft Print to PDF" printer (they print two small pages to a file under `target\`). `cargo test` skips them. To run them:

```
cargo test --bin pdf-light print_to_pdf -- --ignored --nocapture
```

They take about 20 seconds and need the "Microsoft Print to PDF" printer to be installed (it is by default on Windows 11).

### The probe example

`examples\probe.rs` is a small program that times opening a PDF, rendering its first pages, and extracting text:

```
cargo run --release --example probe -- file.pdf other.pdf
```

It prints a few lines per file (page count, open time, render time per page, text extraction). Use it to see how fast a given PDF is.

## Keyboard shortcuts

| Keys | What it does |
|---|---|
| Ctrl+O | Open a PDF |
| Ctrl+S | Save (asks "Overwrite" the first time) |
| Ctrl+Shift+S | Save as (a copy) |
| Ctrl+E | Export pages as PNG or TIFF |
| Ctrl+P | Print pages |
| Ctrl+B | Show or hide the pages panel |
| Ctrl+F | Go to the find field |
| Enter / Shift+Enter in the find field | Next / previous match |
| F3 / Shift+F3 | Next / previous match |
| Esc in the find field | Clear the search |
| Ctrl++ , Ctrl+= | Zoom in |
| Ctrl+− | Zoom out |
| Ctrl+mouse wheel (or touchpad pinch) | Zoom around the pointer |
| PgUp / PgDn | Scroll one screen up / down |
| Arrow keys | Scroll a little |
| Home / End | Go to the top / bottom |
| Delete | Delete the selected pages |
| Ctrl+Z | Undo the last delete |
| Ctrl+A (pages panel) | Select all pages |
| Esc (pages panel) | Clear the selection |
| Click, Ctrl+click, Shift+click on a thumbnail | Select one page / toggle a page / select a range |
| Right-click a thumbnail | Menu: Delete, Export, Print for the selected pages |

Shortcuts for things that aren't possible right now (for example Save with nothing changed) do nothing. Hover over a greyed button to see why it is disabled.

## Manual UI checklist

The automatic tests can't look at the screen, so this list is how to check the real app by eye.

- Use a **copy** of a PDF for everything that edits or saves. Keep the original safe.
- For printing, use only **Microsoft Print to PDF**. Never a real printer.
- You need: a PDF with 3 or more pages and real text, a long one (20+ pages) if you have it, a scanned PDF with no text if you have one, a password-protected PDF if you have one, and a text file renamed to `.pdf`.

### Opening and reading

1. Double-click or "Open with" a PDF, or run `cargo run --release -- file.pdf`. You should see the first page within about a second of the window appearing, at fit width. The window title is `name.pdf — pdf-light` and the status bar shows `Page 1 of N`.
2. Drag a PDF from Explorer onto the window. It opens. Do the same while another file is shown: the new file replaces it straight away.
3. Scroll with the wheel, PgUp/PgDn, arrow keys, Home and End. Scrolling never freezes. Pages that are still rendering are white for a moment, never black or flickering. Home goes to page 1 and End to the last page.
4. The status bar `Page N of M` follows your scrolling.
5. Hold Ctrl and turn the wheel over a word. The word stays under the pointer. The page is briefly blurry, then sharp about 0.2 seconds after you stop.
6. Press Ctrl++ and Ctrl+−, use the − and + buttons, type `150` in the zoom box and press Enter, then click "Fit width". The zoom changes each time and 25 % and 800 % are the limits. With "Fit width" on, resizing the window makes the page follow.
7. Zoom to 800 %. It must not crash. Text may look a little soft at the very top (known limit).
8. Open the text file renamed to `.pdf`. A message says the file isn't a PDF pdf-light can read, and nothing crashes.
9. Open the password-protected PDF, if you have one. A message says it is password-protected.

### Pages panel, deleting and saving

10. The pages panel on the left shows thumbnails that fill in as you scroll. Drag its edge to resize it. Ctrl+B hides and shows it. The current page has an outline, and the panel follows when you scroll the main view.
11. Click a thumbnail: the main view jumps to that page. Ctrl+click adds or removes a page from the selection. Shift+click selects a range. Ctrl+A selects all and Esc clears the selection.
12. On a **copy**: select two pages and press Delete (or click the Delete button, which says "Delete 2 pages"). They disappear from the panel and the page count drops by two. The title gets a dot (`name.pdf •`) and the status bar shows `name.pdf (edited)`.
13. Press Ctrl+Z. The last delete comes back (one step at a time).
14. Right-click a page that isn't selected. It becomes the selection and the menu shows "Delete page", "Export page…" and "Print page…". With several pages selected the labels say "3 pages".
15. Click "Save as". A save box suggests `<name>-edited.pdf` in the same folder. Save. The status says "Saved <file>". Open that file in pdf-light and in another viewer: the page count and content are right, and the original file is unchanged (check its date and page count).
16. Delete a page and press Ctrl+S. It asks "Overwrite <name>?". Choose Cancel first: nothing is written. Try again and choose Overwrite: the file is saved, and the title loses the dot.
17. Delete a page and close the window, or open another file. A dialog offers Save, Discard and Cancel. Cancel keeps you where you were. Discard closes without saving. Save opens the Save as box.
18. Select every page (Ctrl+A). The Delete button is greyed, and its tooltip says "A PDF needs at least one page".

### Find

19. Press Ctrl+F and type a word that appears in a multi-page PDF. The find field shows the match count (for example `3 of 12`) and every match is highlighted directly on the word, and the current match stands out from the others.
20. Press Enter, F3 or the › button to step to the next match. Shift+Enter, Shift+F3 or ‹ steps back. The view scrolls to the match and wraps from the last to the first.
21. Press Esc in the find field. The highlights go away.
22. Search for a word that isn't in the document. The find box shows "No matches" and F3 does nothing.
23. On a scanned page with no text layer, nothing is found (expected, there is no OCR).
24. Delete a page that has a match, then search again: matches on deleted pages are not listed.

### Export

25. Select 3 pages, press Ctrl+E (or Export…). The dialog opens with Pages set to "Selected". Choose PNG and 300 DPI, pick a folder, and click Export. The dialog shows `Exporting page 1 of 3` with a progress bar. The status then says "Exported 3 pages to <folder>".
26. Open the PNG files in the Windows Photos app. They show the right pages, sharp, with names like `<name>_p001.png`.
27. Export again as TIFF with "One multi-page TIFF" ticked. One `.tif` file appears, and Photos shows all 3 pages.
28. Export to a folder that already has files with the same names. The dialog asks "Overwrite N existing files?" with Overwrite and Back (the exact wording of the buttons may differ).
29. Type a bad page range (for example `0` or `99`), a DPI of `5000`, or an empty file name. A red message appears under the field and Export is disabled.
30. Start an export at 600 DPI of many pages and click Cancel part way. The status says "Export cancelled" and no files are left behind in the folder.

### Print

Use only Microsoft Print to PDF.

31. Open a PDF with 3 or more pages and press Ctrl+P. The Windows print dialog opens over the window and "Selection" is greyed out. Choose Microsoft Print to PDF and All, then Print. A save box opens over the window, suggesting `<name>-print.pdf`. Save. The status bar shows `Printing page N of M`, then `Sent M pages to Microsoft Print to PDF`. The output has every page, centred, with landscape pages turned.
32. Ctrl+P again, choose Pages and type `1,3`. The output has exactly pages 1 and 3 of the current numbering. Delete a page first (without saving) to check that deleted pages don't print.
33. In the pages panel select pages 2 and 3, right-click one and choose "Print 2 pages…". The dialog opens with Selection chosen. The output has pages 2 and 3.
34. Ctrl+P with All and Copies 2, Collate on: the output has the set twice (1 2 3 1 2 3). The driver may make the copies itself, and if it ignores the setting, note it. With Collate off the order is 1 1 2 2 3 3.
35. Ctrl+P, click Print, then Cancel in the save box. No job runs and the status bar doesn't change.
36. Print a long PDF and click Cancel in the status bar. The status says "Printing cancelled" and no file is left. Print again and close the window during the print: it closes within about one page render and no job stays in the print queue.

### Look and feel

37. The toolbar groups are Open, pages panel, Save, Save as; Delete, Undo; Find; Export, Print; and zoom. Hover each button for a tooltip with its shortcut. With nothing to do, a button is greyed and its tooltip says why (for example "No unsaved changes" or "Select pages in the sidebar first").
38. With no file open, the window shows "Open a PDF to start reading" with an Open button and "or drop a file here".
39. Switch Windows between light and dark (Settings, Personalisation, Colours, Choose your mode). pdf-light follows. Text, icons and the pages panel are readable in both, and the Delete button turns red when hovered.
40. Make the window narrower. First the find field shrinks, then the button labels disappear and only icons stay, then the zoom controls wrap onto a second row. Nothing overlaps or is cut off.
41. Press Tab to move focus through the toolbar. The focused button has a visible outline.

## Credits and licences

pdf-light's own code is the owner's. It builds on these crates (licences as given in each crate's metadata):

| Crate | Used for | Licence |
|---|---|---|
| [hayro](https://crates.io/crates/hayro) and hayro-interpret 0.9 | Drawing PDF pages, reading text positions | Apache-2.0 OR MIT |
| [lopdf](https://crates.io/crates/lopdf) 0.47 | Deleting pages and saving | MIT |
| [eframe](https://crates.io/crates/eframe) and egui 0.37 | Window and user interface | MIT OR Apache-2.0 |
| [image](https://crates.io/crates/image) 0.25 | Writing PNG files | MIT OR Apache-2.0 |
| [tiff](https://crates.io/crates/tiff) 0.11 | Writing TIFF files | MIT |
| [rfd](https://crates.io/crates/rfd) 0.18 | Open and Save dialogs | MIT |
| [windows](https://crates.io/crates/windows) 0.62 | Windows print dialog and printing | MIT OR Apache-2.0 |
| [raw-window-handle](https://crates.io/crates/raw-window-handle) 0.6 | Window handle for the print dialog | MIT OR Apache-2.0 OR Zlib |
| [thiserror](https://crates.io/crates/thiserror) and [anyhow](https://crates.io/crates/anyhow) | Error handling | MIT OR Apache-2.0 |

Their own dependencies (and fonts bundled with egui and hayro) have their own licences. `cargo metadata` lists them.

The folder `.claude/skills` holds vendored Rust guidance for Claude (from actionbook/rust-skills and apollographql/skills, both MIT). See `.claude/skills/SOURCE.md`. It is not part of the program.

# pdf-light — toolbar and look (addendum to the design)

Date: 2026-11-18 · Owner asked: "add buttons in the UI for the features of the app".

## Job

Every feature is one click away, grouped by what the owner is doing (file, pages, find, export,
zoom), without taking space from the page. Shortcuts stay; buttons make the features discoverable.

## Palette

| Token | Light | Dark | Used for |
|---|---|---|---|
| Desk | `#5A6570` | `#2B3238` | background around pages (slate: pages stand out, low glare) |
| Panel | `#EEF1F3` | `#1E2328` | toolbar, sidebar, status bar, dialogs |
| Ink | `#1D252C` | `#E6EAED` | text, focus ring |
| Quiet | `#66727C` | `#9AA5AE` | secondary text, separators, disabled (at 50 % opacity) |
| Marker | `#FFD43B` | `#F5C518` | highlighter: search matches (≈45 % alpha over the page), current match (full), selected-thumbnail stripe, active toggle |
| Danger | `#B42318` | `#F0857A` | "Delete" text on hover, destructive confirm buttons |

Light or dark follows the Windows setting. Marker yellow is the one bold element; everything else
stays quiet. No gradients, no shadows under buttons, no cards.

Considered and dropped: an ink-blue background around the pages with a yellow accent. Too close
to the generic "dark background, one bright accent" look. Slate keeps the reader's convention of
grey around white pages. The yellow is kept because it comes from the subject: highlighting text.

## Type

Segoe UI Variable Text (Windows 11's own UI font), loaded at start-up from `C:\Windows\Fonts\SegUIVar.ttf`,
then `segoeui.ttf`, then egui's built-in font. Nothing is embedded, so the `.exe` doesn't grow.
Sizes: toolbar and dialogs 14 px; status bar 12 px; empty-state line 18 px. Sentence case everywhere;
no all-caps labels.

## Layout

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ Open  Save  Save as │ Delete page  Undo │ [Find in document    ] ‹ ›  3 of 27 │ Export…   − [100 %] +  Fit width │
├──────────┬───────────────────────────────────────────────────────────────────────────────┤
│▌ 1       │                                                                               │
│  2       │                  slate desk · white pages · yellow search marks               │
│  3       │                                                                               │
├──────────┴───────────────────────────────────────────────────────────────────────────────┤
│ report.pdf (edited)     Page 2 of 14                         Saved report-edited.pdf    │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

- Groups left-aligned in job order: **Pages** panel toggle (see "Pages panel") · **File** (Open, Save, Save as) · **Pages** (Delete, Undo) ·
  **Find** (search field, previous/next, count) · **Export… Print…**. Zoom is right-aligned. A thin Quiet
  separator between groups means "different job". When the window is narrow, the find field
  shrinks to a minimum of 120 px, and then the zoom group wraps to a second row.
- Buttons are a 16 px icon plus a text label, frameless until hover (Panel shifts toward Ink by about
  8 %), with a visible focus ring. "Export…" uses an ellipsis because it opens a dialog. Zoom −/+
  and find ‹ › are icon-only. When the window is too narrow for every label, labels are dropped
  (icons only, tooltips unchanged) before anything wraps. On hover, Delete turns its icon and
  label Danger.
- Each button has a tooltip saying what it does plus its shortcut: "Open a PDF (Ctrl+O)",
  "Save over report.pdf (Ctrl+S)", "Save a copy (Ctrl+Shift+S)", "Delete the selected pages (Delete)",
  "Undo the last delete (Ctrl+Z)", "Find in document (Ctrl+F)", "Export pages as PNG or TIFF (Ctrl+E)",
  "Zoom out (Ctrl+−)", "Zoom in (Ctrl++)", "Fit the page to the window width".
- Disabled buttons are greyed, and their tooltip says why: "Open a PDF first", "No unsaved changes",
  "Nothing to undo", "Select pages in the sidebar first", "A PDF needs at least one page".
- The Delete label follows the selection: "Delete page" for one, "Delete 3 pages" for several.
- Find: the field shows the placeholder "Find in document". With results it shows "3 of 27" and the
  ‹ › buttons; with none, a quiet "No matches"; while still searching, "Searching…".
- Zoom: −, an editable percentage, +, and "Fit width". "Fit width" shows a 2 px Marker underline
  while it's active.
- Selected thumbnails get a 4 px Marker stripe on their left edge, like a highlighter swipe.
- Status bar: the file name plus "(edited)" while there are unsaved changes; "Page N of M"; and on
  the right, short-lived messages in the same words as the action ("Saved report-edited.pdf",
  "Exported 3 pages to C:\…").
- Empty state (no file): the desk shows "Open a PDF to start reading", an Open button, and the
  quiet line "or drop a file here".

## Print (owner request: "add a print feature")

This lifts printing out of the original design's non-goals.

- **How:** Windows' own printing. The standard Windows print dialog (`PrintDlgExW`) picks the printer,
  copies, collation, and All / Selection / Pages. pdf-light then renders each page itself with hayro at
  the printer's resolution (capped at 600 DPI) and sends it to the printer as a bitmap through GDI
  (`StartDocW` / `StartPage` / `StretchDIBits` / `EndPage` / `EndDoc`). It goes through the `windows`
  crate (Rust bindings to Windows APIs), so the app stays pure Rust and nothing else gets installed.
- **What prints:** the document as it currently looks. Deleted pages don't print, even before saving.
  "Selection" in the dialog means the pages selected in the pages panel (offered only when some are
  selected). "Pages" is a page range in the current numbering.
- **Placement:** each page is scaled to fit the printable area, keeping its proportions, and centred. When
  the page's orientation differs from the paper's (a landscape page on portrait paper), it is rotated 90°
  to use the paper well. Pages are never enlarged past 100 % ("shrink to fit", not "stretch to fit").
- **While printing:** rendering and spooling run on a background thread. The status bar shows
  "Printing page 3 of 12" with a Cancel link; Cancel calls `AbortDoc`. Finished: "Sent 12 pages to
  <printer name>". Errors use the error dialog: "Couldn't print: <reason>".
- **Toolbar:** "Print…" (icon Print), next to "Export…" in an output group. Shortcut Ctrl+P. Tooltip
  "Print pages (Ctrl+P)". Disabled with "Open a PDF first" when no file is open. Right-click on thumbnails
  adds "Print page…" / "Print 3 pages…".

## Pages panel (owner request: "add a pages side panel")

```
│ ▣ Pages │ Open  Save …
├─────────────┬──────────────────────
│ ┌───────┐   │
│ │       │   │
│ │   1   │   │      (current page: Ink 1 px outline)
│ └───────┘   │
│     1       │
│▌┌───────┐   │      (selected: 4 px Marker stripe on the left,
│▌│       │   │       Marker at 18 % behind the thumbnail)
│▌└───────┘   │
│     2       │
│    …        │
│  14 pages   │
└─────────────┴──────────────────────
```

- A left side panel, open by default and 176 px wide. Drag its edge to resize it between 120 and 320 px;
  the thumbnails scale with the width. The panel is the full height between the toolbar and the status bar,
  scrolls on its own, and keeps the current page's thumbnail in view while you read.
- The first toolbar button, **Pages** (icon "Pages panel"), shows and hides the panel. It shows a
  2 px Marker underline while the panel is open. Shortcut Ctrl+B. Tooltip: "Show or hide the pages panel (Ctrl+B)".
- Each thumbnail is the page image with a 1 px Quiet border, and its page number centred below
  (12 px, Quiet). Thumbnails draw lazily in the background; until one is ready, the card is a plain Panel-coloured rectangle at the
  page's shape, so the list never jumps around.
- The current page (the one in the middle of the view) gets an Ink 1 px outline. Selected pages get the
  4 px Marker stripe on the left and a Marker tint at 18 % behind the card.
- Click selects one page and scrolls the view to it. Ctrl+click adds or removes a page. Shift+click
  selects a range. Ctrl+A in the panel selects all pages. Esc clears the selection.
- Right-click a thumbnail: a small menu with "Delete page" (or "Delete 3 pages" when it's part of a
  selection) and "Export page…" (or "Export 3 pages…"). These are the same actions as the toolbar buttons.
- The bottom of the panel shows a quiet count: "14 pages".
- With no file open, the panel stays hidden. Whether it's open or closed is remembered for the session.

## Icons (owner request: "make icons for the buttons")

pdf-light draws its own line icons in code, so there is no icon font, no SVG library and no growth
in the `.exe`. Every icon is defined on a 16 × 16 grid (origin top-left, y down) and drawn with a
1.5 px stroke (at 1× UI scale) with round caps and joins, in the button's text colour (Ink, Quiet when disabled,
Danger for Delete on hover). There are no fills, and nothing sits closer than 1 unit to the grid edge.
The coordinates below are the design. Polylines are open unless marked closed.

| Icon | Shapes (16-unit grid) | Reads as |
|---|---|---|
| Pages panel | closed: (1.5,2.5) (14.5,2.5) (14.5,13.5) (1.5,13.5); divider (5.5,2.5)→(5.5,13.5) | window with a side panel |
| Open | closed: (1.5,4) (6,4) (7.5,5.5) (14.5,5.5) (14.5,13) (1.5,13) | folder |
| Save | arrow (8,2)→(8,10); head (5,7) (8,10) (11,7); tray (2,10) (2,14) (14,14) (14,10) | down into the file |
| Save as | the Save tray and arrow shifted right by 1.5, plus (1.5,3.5)→(5.5,3.5) and (3.5,1.5)→(3.5,5.5) | save a new copy |
| Delete page | closed page (3,1.5) (10,1.5) (13,4.5) (13,14.5) (3,14.5); fold (10,1.5) (10,4.5) (13,4.5); minus (5.5,9.5)→(10.5,9.5) | remove a page |
| Undo | (13,12) (13,9.5) (11.5,7) (8.5,6) (4,6); head (6.5,3.5) (4,6) (6.5,8.5) | step back |
| Find | circle centre (7,7) radius 4.5; handle (10.3,10.3)→(14,14) | magnifier |
| Previous match | (10,3) (5,8) (10,13) | chevron left |
| Next match | (6,3) (11,8) (6,13) | chevron right |
| Export | bracket (9,3) (2,3) (2,13) (9,13); arrow (6,8)→(14.5,8); head (11.5,5) (14.5,8) (11.5,11) | send pages out |
| Print | body closed (2,6.5) (14,6.5) (14,11.5) (2,11.5); paper in (4.5,6.5) (4.5,2) (11.5,2) (11.5,6.5); paper out closed (4.5,9.5) (11.5,9.5) (11.5,14.5) (4.5,14.5) | printer |
| Zoom out | (3,8)→(13,8) | minus |
| Zoom in | (3,8)→(13,8) and (8,3)→(8,13) | plus |
| Fit width | bars (1.5,3)→(1.5,13) and (14.5,3)→(14.5,13); arrow (4,8)→(12,8); heads (6,6) (4,8) (6,10) and (10,6) (12,8) (10,10) | stretch to the edges |

The builder may nudge a coordinate by up to 0.5 units after looking at a screenshot, if a shape reads
badly at 16 px. Any change larger than that goes back to the owner.

## Code shape

- `src/app/icons.rs`: `Icon` (one variant per row above), the shapes as plain data, and `paint(painter, rect, icon, colour)`.
  The data is pure and unit-tested; only `paint` touches egui.

- `src/app/theme.rs`: the palette tokens for light and dark, the font loading, and `apply(ctx)`.
- `src/app/toolbar.rs`: `ToolbarState` (what is possible right now) goes in, a list of
  `ToolbarAction`s comes out. The toolbar draws and reports clicks; `app/mod.rs` acts on the actions.
  Keyboard shortcuts produce the same actions, so a button and its shortcut can never behave differently.

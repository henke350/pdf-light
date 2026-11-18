//! Feasibility probe: times opening, rendering and text extraction on real PDFs.
//!
//! Usage: `cargo run --release --example probe -- <file.pdf>...`

use std::path::Path;
use std::time::Instant;

use pdf_light::{document, render, search, text};

/// Pages measured per file (all pages, or this many, whichever is fewer).
const MAX_PAGES: usize = 20;
const SCALE: f32 = 2.0;

/// Milliseconds with one decimal; `Instant::elapsed()` gives a `Duration`.
fn ms(start: Instant) -> String {
    format!("{:.1} ms", start.elapsed().as_secs_f64() * 1000.0)
}

/// Probes one file. Errors from opening bubble up to `main`, which prints them.
fn probe(path: &Path) -> Result<(), pdf_light::error::PdfError> {
    println!("file: {}", path.display());

    let t = Instant::now();
    let doc = document::open(path)?;
    let pdf = document::open_pdf(&doc.bytes)?;
    let open_ms = ms(t);
    let pages = doc.page_sizes.len();
    println!("pages: {pages}");
    println!("open time: {open_ms}");

    let measured = pages.min(MAX_PAGES);

    let mut render_total = 0.0_f64;
    let mut first_render = String::from("n/a");
    for i in 0..measured {
        let t = Instant::now();
        let result = render::render_page(&pdf, i, SCALE);
        render_total += t.elapsed().as_secs_f64() * 1000.0;
        if i == 0 {
            first_render = ms(t);
        }
        // A render error is reported but does not stop the probe.
        if let Err(e) = result {
            println!("page {}: {e}", i + 1);
        }
    }
    println!("first-page render (scale {SCALE}): {first_render}");
    if measured > 0 {
        println!(
            "avg render per page over {measured} page(s): {:.1} ms",
            render_total / measured as f64
        );
    }

    let t = Instant::now();
    let mut empty_pages = 0;
    let mut first_text = String::new();
    for i in 0..measured {
        let glyphs = text::extract_glyphs(&pdf, i);
        let page_text = search::build_page_text(&glyphs);
        if page_text.chars.is_empty() {
            empty_pages += 1;
        }
        if i == 0 {
            first_text = page_text
                .chars
                .iter()
                .take(200)
                .map(|c| if c.is_control() { ' ' } else { *c })
                .collect();
        }
    }
    println!("text extraction total ({measured} page(s)): {}", ms(t));
    println!("page 1 text (first 200 chars): {first_text}");
    println!("pages without text (of {measured}): {empty_pages}");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: cargo run --release --example probe -- <file.pdf>...");
        // `anyhow::bail!` returns an `Err`, so `main` exits with a non-zero code.
        anyhow::bail!("no PDF files given");
    }
    for file in &files {
        if let Err(e) = probe(Path::new(file)) {
            println!("failed to open: {e}");
        }
        println!();
    }
    Ok(())
}

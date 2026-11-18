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
    // `?` returns early from `probe` with the error if `open` fails.
    let doc = document::open(path)?;
    let pdf = document::open_pdf(&doc.bytes)?;
    let open_ms = ms(t);
    let pages = doc.page_sizes.len();
    println!("pages: {pages}");
    println!("open time: {open_ms}");

    let measured = pages.min(MAX_PAGES);

    let mut render_total = 0.0_f64;
    let mut render_ok = 0_usize;
    let mut render_errors = 0_usize;
    let mut first_render = String::from("n/a");
    for i in 0..measured {
        let t = Instant::now();
        let result = render::render_page(&pdf, i, SCALE);
        let elapsed = t.elapsed().as_secs_f64() * 1000.0;
        if i == 0 {
            first_render = ms(t);
        }
        // `match` looks inside the `Result`: a render error is
        // counted and printed, but does not stop the probe.
        match result {
            Ok(_) => {
                render_total += elapsed;
                render_ok += 1;
            }
            Err(e) => {
                render_errors += 1;
                println!("page {}: {e}", i + 1);
            }
        }
    }
    println!("first-page render (scale {SCALE}): {first_render}");
    if render_ok > 0 {
        println!(
            "avg render per page over {render_ok} successful page(s): {:.1} ms",
            render_total / render_ok as f64
        );
    } else {
        println!("avg render per page: n/a");
    }
    println!("render errors: {render_errors}");

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
            // The `String` annotation tells `collect` what to build from the chars.
            first_text = page_text
                .chars
                .iter()
                .take(200)
                .map(|c| if c.is_control() { ' ' } else { *c })
                .collect();
        }
    }
    println!("text extraction total ({measured} page(s)): {}", ms(t));
    println!("page 1 text (lowercased, first 200 chars): {first_text}");
    println!("pages without text (of {measured}): {empty_pages}");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: cargo run --release --example probe -- <file.pdf>...");
        // Exit code 2 = bad usage; no second error message is printed.
        std::process::exit(2);
    }
    for file in &files {
        // `if let Err(e) = ...` runs the block only when the result is an error.
        if let Err(e) = probe(Path::new(file)) {
            println!("failed to open: {e}");
        }
        println!();
    }
    Ok(())
}

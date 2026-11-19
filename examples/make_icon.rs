//! Writes `assets/pdf-light.ico`, the exe's icon, from the drawing in
//! `src/app_icon.rs`. Run it after changing the drawing:
//!
//! ```text
//! cargo run --example make_icon
//! ```
//!
//! `build.rs` then builds the new icon into the exe.

use pdf_light::app_icon;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/pdf-light.ico");
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(&path, app_icon::ico(&app_icon::ICO_SIZES)?)?;
    println!("Wrote {}", path.display());
    Ok(())
}

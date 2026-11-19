//! Build script: gives the Windows exe its icon, so Explorer, "Open with" and
//! the taskbar show pdf-light's red PDF page. The icon file is made by
//! `cargo run --example make_icon` from the drawing in `src/app_icon.rs`.

fn main() {
    println!("cargo:rerun-if-changed=assets/pdf-light.ico");
    // `CARGO_CFG_TARGET_OS` is the system being built for (not the one building).
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/pdf-light.ico");
        if let Err(error) = resource.compile() {
            panic!("couldn't build the icon into the exe: {error}");
        }
    }
}

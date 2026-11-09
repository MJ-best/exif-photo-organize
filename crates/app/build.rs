//! Windows: embed the app icon and version info into the .exe (macOS gets its icon from the
//! .app bundle, Linux from the window icon set at runtime).

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "LOUPE");
        res.set("FileDescription", "LOUPE");
        if let Err(e) = res.compile() {
            // A missing resource compiler should not break the build; the app still runs.
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}

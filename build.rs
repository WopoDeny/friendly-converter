// Embeds the application icon and version info into the Windows executable.
// If the resource compiler is not available the build still succeeds,
// the exe simply has the default icon.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icon.ico");
            res.set("ProductName", "Friendly Converter");
            res.set("FileDescription", "Friendly Converter");
            res.set("CompanyName", "Friendly Converter");
            if let Err(err) = res.compile() {
                println!("cargo:warning=Could not embed the exe icon: {err}");
            }
        }
    }
}

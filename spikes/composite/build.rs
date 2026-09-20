//! Embed the app icon in the exe so the taskbar, Alt-Tab and Explorer
//! show nus and not the default exe glyph before a window sets its own.
fn main() {
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
    let revision = std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"])
        .output().ok().filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=NUS_BUILD_REVISION={}", revision.trim());
    println!("cargo:rerun-if-changed=../../assets/icon/nus.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/nus.ico");
        res.set("ProductName", "nus");
        res.set("FileDescription", "nus");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon resource not embedded: {e}");
        }
    }
}

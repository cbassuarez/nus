//! Embed the app icon in the exe so the taskbar, Alt-Tab and Explorer
//! show nus and not the default exe glyph before a window sets its own.
fn main() {
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

//! Embed the app icon in the exe so the taskbar, Alt-Tab and Explorer
//! show nus and not the default exe glyph before a window sets its own.
fn main() {
    println!("cargo:rerun-if-env-changed=NUS_RELEASE_VERSION");
    let release = std::env::var("NUS_RELEASE_VERSION").ok().or_else(|| {
        std::process::Command::new("git")
            .args(["describe", "--tags", "--exact-match"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
    });
    let version = release
        .as_deref()
        .map(str::trim)
        .map(|s| s.trim_start_matches('v'))
        .filter(|s| {
            (*s == std::env::var("CARGO_PKG_VERSION").unwrap()
                || s.starts_with(&format!("{}-", std::env::var("CARGO_PKG_VERSION").unwrap())))
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
        })
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}-dev", std::env::var("CARGO_PKG_VERSION").unwrap()));
    println!("cargo:rustc-env=NUS_BUILD_VERSION={version}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=NUS_BUILD_REVISION={}", revision.trim());
    // When this commit was made: a clock reading earlier is certainly wrong
    // (interstitial.rs, the clock page).
    let epoch = std::process::Command::new("git")
        .args(["log", "-1", "--format=%ct"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    println!("cargo:rustc-env=NUS_BUILD_EPOCH={}", epoch.trim());
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

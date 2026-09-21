//! Installed Windows/Linux packages keep mutable data out of their install
//! directory. Development checkouts retain their existing local profile.
#[cfg(not(target_os = "macos"))]
pub fn settle() -> std::io::Result<()> {
    use std::path::PathBuf;
    if crate::private::enabled() { return Ok(()); }
    let exe = std::env::current_exe()?;
    let dir = exe.parent().unwrap();
    let fixture = std::env::var_os("NUS_SHOT_DIR").filter(|_| std::env::var_os("NUS_SHOT").is_some());
    if fixture.is_none() && !dir.join("nus-package.json").is_file() { return Ok(()); }
    let root = if let Some(path) = fixture { PathBuf::from(path) } else {
        #[cfg(windows)]
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        #[cfg(target_os = "linux")]
        let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")));
        let base = base.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "user data directory unavailable"))?.join("nus");
        crate::install::package_root(&base, &exe)?
    };
    std::fs::create_dir_all(&root)?;
    std::env::set_current_dir(root)?;
    let mut paths = vec![dir.join("bin")];
    paths.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
    if let Ok(path) = std::env::join_paths(paths) { std::env::set_var("PATH", path); }
    crate::prefs::apply_start_switches();
    Ok(())
}

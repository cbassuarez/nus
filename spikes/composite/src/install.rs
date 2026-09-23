//! Each installed app gets its own profile. Redownloads may explicitly import
//! preferences, but never silently inherit another build's settings or tour state.
use std::path::{Path, PathBuf};

/// Stable across launches and moving an app on the same volume; a replacement
/// bundle has a new filesystem identity, even when its version is unchanged.
#[cfg(target_os = "macos")]
pub fn bundle_root(base: &Path, bundle: &Path) -> std::io::Result<PathBuf> {
    let identity=bundle_identity(bundle)?;
    let channel = nus_compat::Channel::for_version(env!("NUS_BUILD_VERSION")).directory();
    prepare(base, channel, &identity)
}
#[cfg(target_os="macos")]
fn bundle_identity(bundle:&Path)->std::io::Result<String>{
    use std::os::macos::fs::MetadataExt;
    let m = std::fs::metadata(bundle)?;
    let identity = format!("{:x}-{:x}-{:x}-{:x}", m.st_dev(), m.st_ino(), m.st_birthtime(), m.st_birthtime_nsec());
    Ok(identity)
}

#[cfg(not(target_os = "macos"))]
pub fn package_root(base: &Path, exe: &Path) -> std::io::Result<PathBuf> {
    let identity=package_identity(exe)?;
    let channel = nus_compat::Channel::for_version(env!("NUS_BUILD_VERSION")).directory();
    prepare(base, channel, &identity)
}
#[cfg(not(target_os="macos"))]
fn package_identity(exe:&Path)->std::io::Result<String>{
    let meta = std::fs::metadata(exe)?;
    let directory = std::fs::metadata(exe.parent().unwrap())?;
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    directory.created().ok().hash(&mut hash);
    meta.created().ok().hash(&mut hash);
    meta.modified().ok().hash(&mut hash);
    meta.len().hash(&mut hash);
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        meta.dev().hash(&mut hash);
        meta.ino().hash(&mut hash);
    }
    Ok(format!("{:x}",hash.finish()))
}

/// Only the verified in-app updater registers this continuation. Redownloads
/// retain their separate onboarding/import behavior.
pub fn continue_after_update(current:&Path,candidate:&Path)->std::io::Result<()> {
    let channel=current.parent().ok_or_else(||std::io::Error::other("No installation channel"))?;
    if !matches!(channel.file_name().and_then(|n|n.to_str()),Some("release"|"preview"|"development")) || !current.join("profile").is_dir(){return Err(std::io::Error::other("Updates require an installed profile"));}
    #[cfg(target_os="macos")] let identity=bundle_identity(candidate)?;
    #[cfg(not(target_os="macos"))] let identity=package_identity(&candidate.join(if cfg!(windows){"nus.exe"}else{"nus-desktop"}))?;
    crate::security::write_secret(&channel.join(format!("{identity}.update")),current.to_string_lossy().as_bytes())
}

fn prepare(base: &Path, channel: &str, identity: &str) -> std::io::Result<PathBuf> {
    let channel_root = base.join("installs").join(channel);
    if let Ok(previous)=std::fs::read_to_string(channel_root.join(format!("{identity}.update"))) {
        let previous=PathBuf::from(previous);
        if let (Ok(root),Ok(allowed))=(previous.canonicalize(),channel_root.canonicalize()) {
            if root.parent()==Some(allowed.as_path()) && root.join("profile").is_dir(){return Ok(root);}
        }
        return Err(std::io::Error::other("Update profile continuation is invalid; original profile preserved"));
    }
    let root = channel_root.join(identity);
    let profile = root.join("profile");
    let exists = profile.exists();
    std::fs::create_dir_all(&profile)?;
    if !exists {
        let previous = std::fs::read_to_string(channel_root.join("last-install")).ok()
            .map(PathBuf::from).filter(|p| p.join("settings.json").is_file())
            .or_else(|| base.join("profile/settings.json").is_file().then(|| base.join("profile")));
        if let Some(previous) = previous {
            std::fs::write(profile.join("previous-install"), previous.to_string_lossy().as_bytes())?;
        }
        std::fs::write(profile.join("onboarding-pending"), b"1")?;
    }
    std::fs::write(channel_root.join("last-install"), profile.to_string_lossy().as_bytes())?;
    Ok(root)
}

pub fn previous() -> Option<PathBuf> {
    let p = std::fs::read_to_string("profile/previous-install").ok()?;
    let p = PathBuf::from(p);
    p.join("settings.json").is_file().then_some(p)
}

pub fn complete() {
    if crate::private::enabled() { return; }
    let _ = std::fs::remove_file("profile/onboarding-pending");
    let _ = std::fs::remove_file("profile/previous-install");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_registered_updates_continue_an_existing_profile() {
        let temp=tempfile::tempdir().unwrap();
        let current=prepare(temp.path(),"release","old").unwrap();
        let marker=current.parent().unwrap().join("verified.update");
        std::fs::write(&marker,current.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(prepare(temp.path(),"release","verified").unwrap(),current.canonicalize().unwrap());
        assert_ne!(prepare(temp.path(),"release","redownload").unwrap(),current);
        std::fs::write(marker,temp.path().to_string_lossy().as_bytes()).unwrap();
        assert!(prepare(temp.path(),"release","verified").is_err());
    }
    #[test]
    fn fresh_regular_redownload_and_other_builds() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        let first = prepare(base, "release", "one").unwrap();
        assert!(first.join("profile/onboarding-pending").exists());
        assert!(!first.join("profile/previous-install").exists());
        std::fs::write(first.join("profile/settings.json"), r#"{"preset_name":"custom"}"#).unwrap();
        std::fs::remove_file(first.join("profile/onboarding-pending")).unwrap();
        assert_eq!(prepare(base, "release", "one").unwrap(), first);
        assert!(!first.join("profile/onboarding-pending").exists());
        let second = prepare(base, "release", "two").unwrap();
        assert!(second.join("profile/onboarding-pending").exists());
        assert!(!second.join("profile/settings.json").exists());
        assert_eq!(std::fs::read_to_string(second.join("profile/previous-install")).unwrap(), first.join("profile").to_string_lossy());
        let dev = prepare(base, "development", "two").unwrap();
        assert!(!dev.join("profile/previous-install").exists());
        assert_ne!(dev, second);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn moving_a_bundle_keeps_identity_but_redownloading_changes_it() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("data");
        let bundle = temp.path().join("nus.app");
        std::fs::create_dir(&bundle).unwrap();
        let first = bundle_root(&base, &bundle).unwrap();
        let moved = temp.path().join("moved.app");
        std::fs::rename(&bundle, &moved).unwrap();
        assert_eq!(first, bundle_root(&base, &moved).unwrap());
        std::fs::create_dir(&bundle).unwrap();
        assert_ne!(first, bundle_root(&base, &bundle).unwrap());
    }

    #[test]
    fn legacy_settings_are_offered_without_being_applied() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("profile")).unwrap();
        std::fs::write(temp.path().join("profile/settings.json"), "{}").unwrap();
        let root = prepare(temp.path(), "release", "new").unwrap();
        assert!(root.join("profile/previous-install").exists());
        assert!(!root.join("profile/settings.json").exists());
    }
}

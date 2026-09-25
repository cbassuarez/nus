//! Where a copy of nus keeps its profile. Every copy of a channel shares one
//! (`installs/<channel>/shared`): rebuilds, redownloads and moved bundles
//! come back to the same settings, sessions and sign-ins. The profile lock
//! lets one copy in at a time; the compatibility contract snapshots it
//! before a newer version upgrades it and refuses an older one.
//!
//! A copy can keep a profile of its own instead (Settings · Updates ·
//! PROFILE): its identity is listed in `installs/<channel>/separate`, and
//! it gets `installs/<channel>/<identity>` as before — fresh, with an offer
//! to import. Channels never share: Current, Preview and Development keep
//! separate profiles by contract.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The shared profile's folder name inside a channel.
const SHARED: &str = "shared";

/// Where this copy's profile was placed at launch.
#[derive(Clone, Debug)]
pub struct Placement {
    pub channel_root: PathBuf,
    pub identity: String,
    pub root: PathBuf,
    pub separate: bool,
}

static PLACED: OnceLock<Placement> = OnceLock::new();

/// This copy's placement, when it was placed (a bundle or package launch).
pub fn placement() -> Option<&'static Placement> {
    PLACED.get()
}

/// Stable across launches and moving an app on the same volume; a replacement
/// bundle has a new filesystem identity, even when its version is unchanged.
#[cfg(target_os = "macos")]
pub fn bundle_root(base: &Path, bundle: &Path) -> std::io::Result<PathBuf> {
    let identity=bundle_identity(bundle)?;
    let channel = nus_compat::Channel::for_version(env!("NUS_BUILD_VERSION")).directory();
    place(base, channel, &identity)
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
    let channel = nus_compat::Channel::for_version(env!("NUS_BUILD_VERSION")).directory();
    let identity = if installed_here(&base.join("installs").join(channel), exe) { INSTALLED.into() } else { package_identity(exe)? };
    place(base, channel, &identity)
}

/// The Windows installer always installs a channel to one fixed folder and
/// records it here. That copy is the same installation across installer
/// upgrades and in-app updates, so it keeps one profile; a portable copy is
/// still identified by its files.
const INSTALLED: &str = "installed";
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn installed_here(channel_root: &Path, exe: &Path) -> bool {
    let Ok(recorded) = std::fs::read_to_string(channel_root.join("installed-location")) else { return false };
    match (Path::new(recorded.trim()).canonicalize(), exe.parent().map(Path::canonicalize)) {
        (Ok(recorded), Some(Ok(folder))) => recorded == folder,
        _ => false,
    }
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
    // The swap keeps the installer's folder, and with it this identity.
    if current.file_name().is_some_and(|n| n == INSTALLED) { return Ok(()); }
    // The shared profile needs no continuation: the new copy shares it too.
    if current.file_name().is_some_and(|n| n == SHARED) { return Ok(()); }
    #[cfg(target_os="macos")] let identity=bundle_identity(candidate)?;
    #[cfg(not(target_os="macos"))] let identity=package_identity(&candidate.join(if cfg!(windows){"nus.exe"}else{"nus-desktop"}))?;
    crate::security::write_secret(&channel.join(format!("{identity}.update")),current.to_string_lossy().as_bytes())
}

/// Place this copy and remember where (see `placement`).
fn place(base: &Path, channel: &str, identity: &str) -> std::io::Result<PathBuf> {
    let (root, separate) = resolve(base, channel, identity)?;
    let _ = PLACED.set(Placement { channel_root: base.join("installs").join(channel), identity: identity.into(), root: root.clone(), separate });
    Ok(root)
}

/// The profile root for a copy: its own when it asked for one (or is an
/// in-app update of such a copy), else the channel's shared one.
fn resolve(base: &Path, channel: &str, identity: &str) -> std::io::Result<(PathBuf, bool)> {
    let channel_root = base.join("installs").join(channel);
    std::fs::create_dir_all(&channel_root)?;
    let continues = channel_root.join(format!("{identity}.update")).exists();
    if continues || is_separate(&channel_root, identity) {
        return Ok((prepare(base, channel, identity)?, true));
    }
    Ok((shared(base, channel)?, false))
}

fn is_separate(channel_root: &Path, identity: &str) -> bool {
    std::fs::read_to_string(channel_root.join("separate")).is_ok_and(|s| s.lines().any(|l| l.trim() == identity))
}

/// The channel's shared profile. The first time, the profile last used in
/// this channel moves in whole — when no nus has it open; otherwise the
/// shared one starts fresh and offers to import it.
fn shared(base: &Path, channel: &str) -> std::io::Result<PathBuf> {
    let channel_root = base.join("installs").join(channel);
    let root = channel_root.join(SHARED);
    if !root.join("profile").exists() {
        if let Some(last) = last_root(&channel_root).filter(|r| r.file_name().is_some_and(|n| n != SHARED)) {
            if is_idle(&last) {
                let _ = std::fs::rename(&last, &root);
            }
        }
    }
    prepare(base, channel, SHARED)
}

/// The root of the profile this channel used last, when it is one of its own.
fn last_root(channel_root: &Path) -> Option<PathBuf> {
    let profile = PathBuf::from(std::fs::read_to_string(channel_root.join("last-install")).ok()?.trim());
    let root = profile.parent()?.to_path_buf();
    let inside = root.parent()?.canonicalize().ok()? == channel_root.canonicalize().ok()?;
    (inside && profile.join("settings.json").is_file()).then_some(root)
}

/// No nus holds this profile's lock (compat's Guard takes the same one).
fn is_idle(root: &Path) -> bool {
    let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join(".nus-profile.lock")) else { return false };
    file.try_lock().is_ok()
}

/// Settings · PROFILE: keep this copy's own profile (`true`) or share the
/// channel's. Takes effect when the copy starts again.
pub fn set_separate(separate: bool) -> std::io::Result<()> {
    let Some(p) = placement() else { return Err(std::io::Error::other("This copy has no installation profile")) };
    let path = p.channel_root.join("separate");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut ids: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && *l != p.identity).collect();
    if separate {
        ids.push(&p.identity);
    }
    let mut out = ids.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(path, out)
}

/// Whether this copy will keep its own profile from its next launch.
pub fn wants_separate() -> bool {
    placement().is_some_and(|p| is_separate(&p.channel_root, &p.identity))
}

/// The other profiles in this channel: copies that kept their own, and
/// ones from before sharing. (root, bytes on disk)
pub fn other_profiles() -> Vec<PathBuf> {
    let Some(p) = placement() else { return Vec::new() };
    let Ok(dir) = std::fs::read_dir(&p.channel_root) else { return Vec::new() };
    let here = p.root.canonicalize().ok();
    dir.flatten().map(|e| e.path()).filter(|r| r.join("profile").is_dir() && r.canonicalize().ok() != here).collect()
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

/// The channel folder this profile belongs to, when it is a real install
/// (`installs/<channel>/<identity>`) — not a test's or a portable one.
fn channel_root() -> Option<PathBuf> {
    let here = std::env::current_dir().ok()?;
    let channel = here.parent()?;
    let named = matches!(channel.file_name().and_then(|n| n.to_str()), Some("release" | "preview" | "development"));
    (named && channel.parent()?.file_name().is_some_and(|n| n == "installs")).then(|| channel.to_path_buf())
}

fn welcomed_in(channel: &Path, version: &str) -> bool {
    std::fs::read_to_string(channel.join("welcomed")).is_ok_and(|s| s.lines().any(|l| l.trim() == version))
}

fn mark_welcomed_in(channel: &Path, version: &str) {
    if welcomed_in(channel, version) {
        return;
    }
    let mut text = std::fs::read_to_string(channel.join("welcomed")).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(version);
    text.push('\n');
    let _ = std::fs::write(channel.join("welcomed"), text);
}

/// Welcome is for a version's first opening. A rebuilt or redownloaded
/// copy of a version already welcomed is a new profile (see `prepare`),
/// but not a new first time: it starts without the tour.
pub fn version_welcomed() -> bool {
    channel_root().is_some_and(|c| welcomed_in(&c, env!("NUS_BUILD_VERSION")))
}

/// Remember that this version has been welcomed, for the copies after it.
pub fn mark_version_welcomed() {
    if crate::private::enabled() { return; }
    if let Some(c) = channel_root() {
        mark_welcomed_in(&c, env!("NUS_BUILD_VERSION"));
    }
}

pub fn previous() -> Option<PathBuf> {
    let p = std::fs::read_to_string("profile/previous-install").ok()?;
    let p = PathBuf::from(p);
    p.join("settings.json").is_file().then_some(p)
}

pub fn complete() {
    if crate::private::enabled() { return; }
    mark_version_welcomed();
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
    fn only_the_recorded_installer_folder_is_the_installed_copy() {
        let temp=tempfile::tempdir().unwrap();
        let channel=temp.path().join("installs/preview");
        let installed=temp.path().join("Programs/nus/preview");
        let portable=temp.path().join("Downloads/nus");
        for dir in [&channel,&installed,&portable] { std::fs::create_dir_all(dir).unwrap(); }
        assert!(!installed_here(&channel,&installed.join("nus.exe")));
        std::fs::write(channel.join("installed-location"),format!("{}\r\n",installed.display())).unwrap();
        assert!(installed_here(&channel,&installed.join("nus.exe")));
        assert!(!installed_here(&channel,&portable.join("nus.exe")));
        // An installed copy continues its own profile without an update marker.
        let root=prepare(temp.path(),"preview",INSTALLED).unwrap();
        assert!(continue_after_update(&root,&portable).is_ok());
        assert!(!channel.join(format!("{INSTALLED}.update")).exists());
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
    fn moving_a_bundle_keeps_identity_redownloading_changes_it_both_share() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("data");
        let bundle = temp.path().join("nus.app");
        std::fs::create_dir(&bundle).unwrap();
        let id = bundle_identity(&bundle).unwrap();
        let first = bundle_root(&base, &bundle).unwrap();
        let moved = temp.path().join("moved.app");
        std::fs::rename(&bundle, &moved).unwrap();
        assert_eq!(id, bundle_identity(&moved).unwrap());
        std::fs::create_dir(&bundle).unwrap();
        assert_ne!(id, bundle_identity(&bundle).unwrap());
        // A redownload is a new copy, and still comes home to the same profile.
        assert_eq!(first, bundle_root(&base, &bundle).unwrap());
    }

    #[test]
    fn copies_share_one_profile_unless_they_keep_their_own() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        let (a, sa) = resolve(base, "release", "one").unwrap();
        let (b, sb) = resolve(base, "release", "two").unwrap();
        assert_eq!(a, b);
        assert!(!sa && !sb);
        assert!(a.ends_with("installs/release/shared"));
        // Channels never share.
        let (dev, _) = resolve(base, "development", "one").unwrap();
        assert_ne!(dev, a);
        // A copy that keeps its own gets its own, offered the shared one to import.
        std::fs::write(a.join("profile/settings.json"), "{}").unwrap();
        std::fs::write(base.join("installs/release/separate"), "two\n").unwrap();
        let (own, separate) = resolve(base, "release", "two").unwrap();
        assert!(separate);
        assert_ne!(own, a);
        assert_eq!(std::fs::read_to_string(own.join("profile/previous-install")).unwrap(), a.join("profile").to_string_lossy());
        assert_eq!(resolve(base, "release", "one").unwrap().0, a);
    }

    #[test]
    fn the_last_used_profile_moves_in_when_idle() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        let old = prepare(base, "preview", "old").unwrap();
        std::fs::write(old.join("profile/settings.json"), r#"{"preset_name":"mine"}"#).unwrap();
        std::fs::write(old.join("profile/.vault-id"), "0123456789abcdef0123456789abcdef").unwrap();
        let (shared, _) = resolve(base, "preview", "new").unwrap();
        assert!(!old.exists(), "the old profile should have moved");
        assert_eq!(std::fs::read_to_string(shared.join("profile/settings.json")).unwrap(), r#"{"preset_name":"mine"}"#);
        assert!(shared.join("profile/.vault-id").is_file(), "the vault moves with it");
    }

    #[test]
    fn a_profile_in_use_is_imported_not_moved() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        let old = prepare(base, "preview", "old").unwrap();
        std::fs::write(old.join("profile/settings.json"), "{}").unwrap();
        let held = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(old.join(".nus-profile.lock")).unwrap();
        held.lock().unwrap();
        let (shared, _) = resolve(base, "preview", "new").unwrap();
        assert!(old.join("profile/settings.json").is_file());
        assert!(!shared.join("profile/settings.json").exists());
        assert!(shared.join("profile/previous-install").is_file());
    }

    #[test]
    fn a_version_is_welcomed_once_per_channel() {
        let base = std::env::temp_dir().join(format!("nus-welcomed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert!(!welcomed_in(&base, "0.9.0"));
        mark_welcomed_in(&base, "0.9.0");
        mark_welcomed_in(&base, "0.9.0");
        assert!(welcomed_in(&base, "0.9.0"));
        assert!(!welcomed_in(&base, "0.10.0"));
        mark_welcomed_in(&base, "0.10.0");
        assert_eq!(std::fs::read_to_string(base.join("welcomed")).unwrap(), "0.9.0\n0.10.0\n");
        let _ = std::fs::remove_dir_all(&base);
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

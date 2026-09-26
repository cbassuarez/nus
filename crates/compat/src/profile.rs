//! Quiescent profile generations, outside the live profile. A lifetime OS lock
//! serializes participating app versions. Copies never follow links or touch
//! project files. A completed generation is immutable; recovery preserves the
//! profile being replaced, including work saved after an upgrade.
use crate::{Channel, PROFILE_FORMAT};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

const CONTRACT: &str = "compatibility.json";
const LIMIT: u64 = 20 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub schema: u32,
    pub format: u32,
    pub version: String,
    pub channel: Channel,
    pub chromium_major: u32,
    pub settings: u32,
    pub formats: std::collections::BTreeMap<String, u32>,
    pub healthy: bool,
}
impl Contract {
    pub fn new(version: &str, chromium_major: u32, settings: u32) -> Self {
        Self {
            schema: 1,
            format: PROFILE_FORMAT,
            version: version.into(),
            channel: Channel::for_version(version),
            chromium_major,
            settings,
            formats: [
                ("reading_list", 1),
                ("saved_commands", settings),
                ("sessions", 1),
                ("vault", 1),
                ("process_snapshots", 1),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect(),
            healthy: false,
        }
    }
    pub fn accepts(&self, previous: &Self) -> io::Result<()> {
        if previous.schema != 1
            || previous.format != self.format
            || previous.settings > self.settings
            || previous.formats.iter().any(|(key, value)| {
                self.formats
                    .get(key)
                    .is_none_or(|supported| value > supported)
            })
        {
            return Err(fail("PROFILE_FORMAT_NEWER: this build cannot write this profile; use its newer nus version or recover a compatible generation"));
        }
        if previous.channel != self.channel {
            return Err(fail(
                "PROFILE_CHANNEL: Current, Preview and Development require separate profiles",
            ));
        }
        if previous.chromium_major > self.chromium_major {
            return Err(fail(
                "BROWSER_FORMAT_NEWER: Chromium downgrade requires profile recovery",
            ));
        }
        let before = semver::Version::parse(&previous.version)
            .map_err(|_| fail("PROFILE_VERSION_INVALID"))?;
        let after =
            semver::Version::parse(&self.version).map_err(|_| fail("BUILD_VERSION_INVALID"))?;
        if after < before {
            return Err(fail(
                "PROFILE_DOWNGRADE: recover a saved generation before opening an older nus",
            ));
        }
        Ok(())
    }
}
fn fail(s: &str) -> io::Error {
    io::Error::other(s)
}
/// A symbolic link, or on Windows any reparse point (junctions included).
fn is_link(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    m.file_type().is_symlink()
}
fn ordinary(path: &Path) -> io::Result<()> {
    if is_link(&fs::symlink_metadata(path)?) {
        return Err(fail(&format!(
            "PROFILE_LINK: profile paths must not be symbolic links ({})",
            path.display()
        )));
    }
    Ok(())
}
fn private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    ordinary(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or_else(|| fail("No parent"))?)?;
    temp.write_all(&serde_json::to_vec_pretty(value)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
pub fn read(profile: &Path) -> io::Result<Option<Contract>> {
    let path = profile.join(CONTRACT);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
        Ok(_) => {
            ordinary(&path)?;
            Ok(Some(serde_json::from_reader(File::open(path)?)?))
        }
    }
}

pub struct Guard {
    root: PathBuf,
    _lock: File,
}
impl Guard {
    /// Obtain before settings reads (which may salvage and write), CEF, vault
    /// migration, holder discovery, or any other profile mutation.
    pub fn acquire(root: &Path) -> io::Result<Self> {
        ordinary(root)?;
        let path = root.join(".nus-profile.lock");
        if path.symlink_metadata().is_ok() {
            ordinary(&path)?;
        }
        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let lock = opts.open(path)?;
        lock.try_lock()
            .map_err(|_| fail("PROFILE_BUSY: another nus is using this profile"))?;
        Ok(Self {
            root: root.to_path_buf(),
            _lock: lock,
        })
    }
    pub fn open(&self, next: &Contract) -> io::Result<Option<String>> {
        let profile = self.root.join("profile");
        if profile.exists() {
            ordinary(&profile)?;
        }
        // A crash between the two recovery renames leaves an explicit journal.
        if self.root.join("recovery-pending.json").exists() {
            return Err(fail(
                "RECOVERY_PENDING: finish the recorded recovery before opening nus",
            ));
        }
        let previous = read(&profile)?;
        if let Some(p) = &previous {
            next.accepts(p)?;
        }
        // Legacy preferences have their own schema even before this contract.
        let settings = profile.join("settings.json");
        if settings.exists() {
            ordinary(&settings)?;
            let data: serde_json::Value = serde_json::from_reader(File::open(settings)?)?;
            if data
                .get("schema")
                .is_some_and(|v| v.as_u64().is_none_or(|n| n > next.settings as u64))
            {
                return Err(fail(
                    "SETTINGS_FORMAT_NEWER: settings were preserved without modification",
                ));
            }
        }
        private_dir(&profile)?;
        let changed = previous.as_ref().is_none_or(|p| {
            p.version != next.version
                || p.chromium_major != next.chromium_major
                || p.settings != next.settings
                || p.formats != next.formats
        });
        let snapshot = if changed && previous.is_some() && fs::read_dir(&profile)?.next().is_some()
        {
            Some(self.checkpoint(previous.as_ref())?)
        } else {
            None
        };
        if let Some(id) = &snapshot {
            json(&self.root.join("last-generation.json"), id)?;
        }
        json(&profile.join(CONTRACT), next)?;
        Ok(snapshot)
    }
    pub fn healthy(&self) -> io::Result<()> {
        let profile = self.root.join("profile");
        let mut c = read(&profile)?.ok_or_else(|| fail("Missing profile contract"))?;
        if !c.healthy {
            c.healthy = true;
            json(&profile.join(CONTRACT), &c)?;
        }
        Ok(())
    }
    fn checkpoint(&self, contract: Option<&Contract>) -> io::Result<String> {
        let base = self.root.join("generations");
        private_dir(&base)?;
        let temp = tempfile::Builder::new()
            .prefix("generation-")
            .tempdir_in(&base)?;
        let mut budget = LIMIT;
        copy_tree(
            &self.root.join("profile"),
            &temp.path().join("profile"),
            &mut budget,
            0,
        )?;
        json(&temp.path().join("complete.json"), &contract)?;
        let path = temp.keep();
        Ok(path.file_name().unwrap().to_string_lossy().into_owned())
    }
    /// Offline recovery only. The supplied binary must match the saved version.
    /// Retrying after a crash completes the same two-rename transaction.
    pub fn recover(&self, id: &str, expected: &Contract) -> io::Result<PathBuf> {
        if !id.starts_with("generation-")
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(fail("Invalid generation"));
        }
        let generation = self.root.join("generations").join(id);
        ordinary(&self.root.join("generations"))?;
        ordinary(&generation)?;
        ordinary(&generation.join("complete.json"))?;
        let contract: Option<Contract> =
            serde_json::from_reader(File::open(generation.join("complete.json"))?)?;
        let contract = contract.ok_or_else(|| {
            fail("LEGACY_RECOVERY: use a manual isolated restore for a pre-contract profile")
        })?;
        expected.accepts(&contract)?;
        if contract.version != expected.version {
            return Err(fail(
                "RECOVERY_VERSION: launch the version that created this generation",
            ));
        }
        let pending = self.root.join("recovery-pending.json");
        let staged = self.root.join("recovery-profile");
        let saved = self.root.join(format!("preserved-{id}"));
        let live = self.root.join("profile");
        if pending.exists() {
            ordinary(&pending)?;
            let selected: String = serde_json::from_reader(File::open(&pending)?)?;
            if selected != id {
                return Err(fail("Finish the previously selected recovery first"));
            }
        } else {
            if saved.exists() || staged.exists() {
                return Err(fail(
                    "Recovery paths already exist; nothing was overwritten",
                ));
            }
            let mut budget = LIMIT;
            // Incomplete copies are never installed or marked ready.
            if let Err(e) = copy_tree(&generation.join("profile"), &staged, &mut budget, 0) {
                let _ = fs::remove_dir_all(&staged);
                return Err(e);
            }
            json(&pending, &id)?;
        }
        if !saved.exists() {
            ordinary(&live)?;
            fs::rename(&live, &saved)?;
        }
        if !live.exists() {
            ordinary(&staged)?;
            fs::rename(&staged, &live)?;
        }
        // If live exists and staged is gone, the final rename already succeeded.
        if staged.exists() {
            return Err(fail("Ambiguous recovery state; both profiles preserved"));
        }
        #[cfg(unix)]
        File::open(&self.root)?.sync_all()?;
        fs::remove_file(pending)?;
        #[cfg(unix)]
        File::open(&self.root)?.sync_all()?;
        Ok(saved)
    }
}
fn copy_tree(src: &Path, dst: &Path, budget: &mut u64, depth: u32) -> io::Result<()> {
    if depth > 64 {
        return Err(fail("Profile directory nesting limit exceeded"));
    }
    ordinary(src)?;
    private_dir(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        // Runtime credentials and browser locks do not describe resumable state.
        if depth == 0
            && matches!(
                name.to_str(),
                Some(
                    "instance"
                        | "hold"
                        | ".vault-lock"
                        | "SingletonLock"
                        | "SingletonCookie"
                        | "SingletonSocket"
                )
            )
        {
            continue;
        }
        // Links inside the profile are never followed and are not state:
        // Chromium keeps its own (SingletonLock, RunningChromeVersion...) in
        // the root it is given, which is this profile. Sockets and pipes
        // are runtime-only too.
        let meta = fs::symlink_metadata(entry.path())?;
        if is_link(&meta) || !(meta.is_dir() || meta.is_file()) {
            continue;
        }
        let to = dst.join(name);
        if meta.is_dir() {
            copy_tree(&entry.path(), &to, budget, depth + 1)?;
        } else if meta.is_file() {
            *budget = budget
                .checked_sub(meta.len())
                .ok_or_else(|| fail("Profile exceeds the 20 GiB automatic recovery budget"))?;
            let mut source = File::open(entry.path())?;
            let mut dest = OpenOptions::new().write(true).create_new(true).open(&to)?;
            use std::io::Read;
            let copied = io::copy(
                &mut std::io::Read::by_ref(&mut source).take(meta.len() + 1),
                &mut dest,
            )?;
            if copied != meta.len() {
                return Err(fail("Profile changed while making its recovery generation"));
            }
            dest.sync_all()?;
        }
    }
    #[cfg(unix)]
    File::open(dst)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn contract(v: &str) -> Contract {
        Contract::new(v, 140, 2)
    }
    #[test]
    fn ahead_formats_and_channels_do_not_modify_user_data() {
        let d = tempfile::tempdir().unwrap();
        let g = Guard::acquire(d.path()).unwrap();
        let newer = contract("0.9.0");
        g.open(&newer).unwrap();
        fs::write(
            d.path().join("profile/settings.json"),
            br#"{"schema":2,"name":"keep"}"#,
        )
        .unwrap();
        let before = fs::read(d.path().join("profile/compatibility.json")).unwrap();
        assert!(g.open(&contract("0.8.0")).is_err());
        assert!(g.open(&contract("0.10.0-preview.1")).is_err());
        let mut browser = contract("0.10.0");
        browser.chromium_major = 139;
        assert!(g.open(&browser).is_err());
        assert_eq!(
            fs::read(d.path().join("profile/compatibility.json")).unwrap(),
            before
        );
        fs::write(d.path().join("profile/settings.json"), br#"{"schema":999}"#).unwrap();
        assert!(g.open(&contract("0.10.0")).is_err());
        assert_eq!(
            fs::read_to_string(d.path().join("profile/settings.json")).unwrap(),
            r#"{"schema":999}"#
        );
    }
    #[cfg(unix)]
    #[test]
    fn links_inside_the_profile_do_not_block_an_upgrade() {
        use std::os::unix::fs::symlink;
        let d = tempfile::tempdir().unwrap();
        let g = Guard::acquire(d.path()).unwrap();
        g.open(&contract("0.8.0")).unwrap();
        let profile = d.path().join("profile");
        fs::write(profile.join("notes"), b"kept").unwrap();
        // What Chromium leaves in its user-data root on macOS and Linux.
        symlink("140.0.7339.0", profile.join("RunningChromeVersion")).unwrap();
        fs::create_dir(profile.join("Default")).unwrap();
        symlink("/nonexistent", profile.join("Default/link")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), b"not ours").unwrap();
        symlink(outside.path(), profile.join("elsewhere")).unwrap();
        let id = g.open(&contract("0.9.0")).unwrap().unwrap();
        let saved = d.path().join("generations").join(id).join("profile");
        assert_eq!(fs::read(saved.join("notes")).unwrap(), b"kept");
        for link in ["RunningChromeVersion", "Default/link", "elsewhere"] {
            assert!(
                fs::symlink_metadata(saved.join(link)).is_err(),
                "{link} was copied"
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn a_linked_profile_folder_is_still_refused_by_name() {
        use std::os::unix::fs::symlink;
        let d = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        symlink(real.path(), d.path().join("profile")).unwrap();
        let g = Guard::acquire(d.path()).unwrap();
        let e = g.open(&contract("0.8.0")).unwrap_err().to_string();
        assert!(
            e.starts_with("PROFILE_LINK") && e.contains("profile"),
            "{e}"
        );
    }
    #[test]
    fn changing_a_store_format_checkpoints_even_without_an_app_version_change() {
        let d = tempfile::tempdir().unwrap();
        let g = Guard::acquire(d.path()).unwrap();
        let old = contract("0.8.0");
        assert!(g.open(&old).unwrap().is_none());
        fs::write(d.path().join("profile/notes"), b"original").unwrap();
        let mut next = old.clone();
        next.formats.insert("reading_list".into(), 2);
        let id = g.open(&next).unwrap().unwrap();
        assert_eq!(
            fs::read(d.path().join("generations").join(id).join("profile/notes")).unwrap(),
            b"original"
        );
        assert!(g.open(&old).is_err());
    }

    #[test]
    fn recovery_preserves_newer_work_and_original_generation() {
        let d = tempfile::tempdir().unwrap();
        let g = Guard::acquire(d.path()).unwrap();
        g.open(&contract("0.8.0")).unwrap();
        g.healthy().unwrap();
        fs::write(d.path().join("profile/notes"), b"before").unwrap();
        let id = g.open(&contract("0.9.0")).unwrap().unwrap();
        fs::write(d.path().join("profile/notes"), b"after").unwrap();
        assert!(g.recover(&id, &contract("0.9.0")).is_err());
        let saved = g.recover(&id, &contract("0.8.0")).unwrap();
        assert_eq!(fs::read(saved.join("notes")).unwrap(), b"after");
        assert_eq!(fs::read(d.path().join("profile/notes")).unwrap(), b"before");
        assert!(d
            .path()
            .join("generations")
            .join(id)
            .join("complete.json")
            .exists());
    }
    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let d = tempfile::tempdir().unwrap();
        let g = Guard::acquire(d.path()).unwrap();
        assert!(Guard::acquire(d.path()).is_err());
        drop(g);
        assert!(Guard::acquire(d.path()).is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn external_links_are_never_copied_or_followed() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let g = Guard::acquire(d.path()).unwrap();
        g.open(&contract("0.8.0")).unwrap();
        fs::write(outside.path().join("file"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), d.path().join("profile/project")).unwrap();
        let id = g.open(&contract("0.9.0")).unwrap().unwrap();
        let saved = d.path().join("generations").join(id).join("profile");
        assert!(fs::symlink_metadata(saved.join("project")).is_err());
        assert_eq!(fs::read(outside.path().join("file")).unwrap(), b"outside");
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
    }
    #[test]
    fn interrupted_restore_finishes_without_discarding_either_profile() {
        let d = tempfile::tempdir().unwrap();
        let g = Guard::acquire(d.path()).unwrap();
        g.open(&contract("0.8.0")).unwrap();
        let id = g.open(&contract("0.9.0")).unwrap().unwrap();
        let mut budget = LIMIT;
        copy_tree(
            &d.path().join("generations").join(&id).join("profile"),
            &d.path().join("recovery-profile"),
            &mut budget,
            0,
        )
        .unwrap();
        json(&d.path().join("recovery-pending.json"), &id).unwrap();
        fs::rename(
            d.path().join("profile"),
            d.path().join(format!("preserved-{id}")),
        )
        .unwrap();
        assert!(g.open(&contract("0.9.0")).is_err());
        g.recover(&id, &contract("0.8.0")).unwrap();
        g.open(&contract("0.8.0")).unwrap();
    }
}

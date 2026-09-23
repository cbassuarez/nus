//! Anonymous release discovery; download and installation only after a click.
use semver::Version;
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        LazyLock, Mutex,
    },
    time::Instant,
};
pub const INTERRUPTION_WARNING:&str="nus will download and verify the update, then close all its windows and restart. Save unsaved work first. Shells, agents and other processes may be interrupted. Automatic process resumption has not been extensively tested; do not rely on it for running work. The previous installation is retained for recovery.";
pub const REPO: &str = "https://github.com/cbassuarez/nus";
pub const CURRENT: &str = env!("NUS_BUILD_VERSION");
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub signing: String,
}
#[derive(Clone, Default)]
pub struct Status {
    pub message: String,
    pub failure_code: &'static str,
    pub available: bool,
    pub busy: bool,
    pub confirming: bool,
}
struct State {
    status: Status,
    release: Option<Release>,
    checked: Option<Instant>,
}
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State{status:Status{message:std::fs::read_to_string("profile/update-result.txt").ok().filter(|s|s.len()<512).unwrap_or_else(||format!("Installed: {CURRENT}. Updates are checked automatically for release builds.")),..Default::default()},release:None,checked:None})
});
static REVISION: AtomicU64 = AtomicU64::new(1);
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}
fn changed() {
    REVISION.fetch_add(1, Ordering::Relaxed);
}
pub fn status() -> Status {
    STATE.lock().unwrap().status.clone()
}
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("macos-arm64"),
        ("windows", "x86_64") => Some("windows-x86_64"),
        ("linux", "x86_64") => Some("linux-x86_64"),
        _ => None,
    }
}
pub fn select(catalog: &Value, current: &str, target: &str) -> Option<Release> {
    let current = Version::parse(current).ok()?;
    let previews = !current.pre.is_empty();
    catalog
        .as_array()?
        .iter()
        .filter_map(|item| {
            if item["draft"].as_bool() != Some(false) {
                return None;
            }
            let tag = item["tag_name"].as_str()?;
            let version = Version::parse(tag.strip_prefix('v')?).ok()?;
            if !tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
                || version <= current
                || previews != !version.pre.is_empty()
            {
                return None;
            }
            let body = item["body"].as_str()?;
            let manifest = body
                .split("<!-- nus-release:")
                .nth(1)?
                .split(" -->")
                .next()?;
            let manifest: Value = serde_json::from_str(manifest).ok()?;
            if manifest.get("state").is_some_and(|s| s != "active") { return None; }
            if let Some(min) = manifest.get("minimum_updater") {
                if current < Version::parse(min.as_str()?).ok()? { return None; }
            }
            if manifest["schema"] != 1
                || manifest["version"] != tag
                || manifest["channel"]
                    != if version.pre.is_empty() {
                        "stable"
                    } else {
                        "preview"
                    }
            {
                return None;
            }
            let record = manifest["assets"]
                .as_array()?
                .iter()
                .find(|a| a["target"] == target)?;
            let name = record["name"].as_str()?;
            let extension = if target.starts_with("linux") {
                "tar.gz"
            } else {
                "zip"
            };
            if name != format!("nus-{version}-{target}.{extension}") || record["version"] != tag {
                return None;
            }
            let asset = item["assets"]
                .as_array()?
                .iter()
                .find(|a| a["name"] == name)?;
            let size = record["size"].as_u64()?;
            let sha256 = record["sha256"].as_str()?;
            if size == 0
                || size > 1024 * 1024 * 1024
                || asset["size"] != size
                || sha256.len() != 64
                || !sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return None;
            }
            if asset["digest"].as_str() != Some(format!("sha256:{sha256}").as_str()) {
                return None;
            }
            if asset["browser_download_url"] != format!("{REPO}/releases/download/{tag}/{name}") {
                return None;
            }
            let signing = record["signing"].as_str()?;
            if target=="windows-x86_64" && signing!="authenticode"{return None;}
            if version.pre.is_empty()
                && signing
                    != match target {
                        "macos-arm64" => "notarized",
                        "windows-x86_64" => "authenticode",
                        _ => "checksum",
                    }
            {
                return None;
            }
            Some((
                version,
                Release {
                    version: tag.into(),
                    name: name.into(),
                    size,
                    sha256: sha256.into(),
                    signing: signing.into(),
                },
            ))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, r)| r)
}
pub fn curl() -> std::process::Command {
    let mut cmd = std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    cmd.args([
        "--disable",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "15",
        "--user-agent",
        "nus-updater",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}
/// Re-fetch the selected tag immediately before installation. A withdrawn or
/// changed release must not install from a six-hour-old discovery result.
pub fn revalidate(release: &Release) -> Result<(), String> {
    let out = curl().args(["--max-time", "30", "--max-filesize", "4194304", "--header", "Accept: application/vnd.github+json"])
        .arg(format!("https://api.github.com/repos/cbassuarez/nus/releases/tags/{}", release.version))
        .output().map_err(|_| "Could not recheck this release")?;
    if !out.status.success() { return Err("Release readiness could not be rechecked; installation was stopped".into()); }
    let item: Value = serde_json::from_slice(&out.stdout).map_err(|_| "Invalid release readiness metadata")?;
    let selected = select(&serde_json::json!([item]), CURRENT, target().ok_or("Unsupported platform")?);
    if selected.as_ref() != Some(release) { return Err("This update was withdrawn or changed; check for updates again".into()); }
    Ok(())
}
pub fn check() {
    {
        let mut s = STATE.lock().unwrap();
        if s.status.busy {
            return;
        }
        s.status = Status {
            message: "Checking GitHub Releases…".into(),
            busy: true,
            ..Default::default()
        };
        s.checked = Some(Instant::now());
        changed();
    }
    std::thread::spawn(|| {
        let result = (|| -> Result<Option<Release>, String> {
            let target = target().ok_or("No update package for this platform")?;
            let out = curl()
                .args([
                    "--max-time",
                    "30",
                    "--max-filesize",
                    "4194304",
                    "--header",
                    "Accept: application/vnd.github+json",
                    "https://api.github.com/repos/cbassuarez/nus/releases?per_page=100",
                ])
                .output()
                .map_err(|_| "Could not start the HTTPS download client")?;
            if !out.status.success() {
                return Err(
                    "GitHub could not be reached or rate-limited the check. Try again later."
                        .into(),
                );
            }
            let catalog: Value = serde_json::from_slice(&out.stdout)
                .map_err(|_| "GitHub returned invalid release metadata")?;
            Ok(select(&catalog, CURRENT, target))
        })();
        let mut s = STATE.lock().unwrap();
        s.status.busy = false;
        match result {
            Ok(release) => {
                s.status.available = release.is_some();
                s.status.message = release
                    .as_ref()
                    .map(|r| {
                        format!(
                            "{} is ready · {:.0} MB · {}",
                            r.version,
                            r.size as f64 / 1_000_000.0,
                            r.signing
                        )
                    })
                    .unwrap_or_else(|| {
                        format!(
                            "{CURRENT}: no newer verified package is available for this channel."
                        )
                    });
                s.release = release;
            }
            Err(e) => { s.status.failure_code = "UPDATE_CHECK_FAILED"; s.status.message = e; },
        };
        changed();
    });
}
pub fn confirm(on: bool) {
    let mut s = STATE.lock().unwrap();
    s.status.confirming = on && s.status.available && !s.status.busy;
    changed();
}
impl crate::app::App {
    pub(crate) fn tend_updates(&mut self) {
        if let Some(message) = crate::protected_state::take_notice() {
            self.notice(&message);
        }
        let revision = revision();
        if self.update_revision != revision {
            self.update_revision = revision;
            self.dirty = true;
        }
        if !self.behavior.update_checks
            || crate::private::enabled()
            || std::env::var_os("NUS_SHOT").is_some()
            || CURRENT.contains("dev")
        {
            return;
        }
        let due = {
            let s = STATE.lock().unwrap();
            !s.status.busy && s.checked.is_none_or(|at| at.elapsed().as_secs() > 21600)
        };
        if due {
            check();
        }
    }
    pub(crate) fn recover_previous_version(&mut self) {
        if status().busy { return; }
        let Some(recovery) = crate::update_install::recovery() else { return };
        let approved = rfd::MessageDialog::new().set_title("Return to previous nus version?")
            .set_description(format!("Return to {} and restore the profile saved before updating. This version's profile and application will be preserved separately. Project files will not be changed.\n\nSave unsaved work first. Running shells and agents may be interrupted. Automatic process resumption has not been extensively tested.", recovery.previous_version))
            .set_level(rfd::MessageLevel::Warning).set_buttons(rfd::MessageButtons::OkCancel).show();
        if approved != rfd::MessageDialogResult::Ok { return; }
        {
            let mut s = STATE.lock().unwrap();
            if s.status.busy { return; }
            s.status.busy = true; s.status.message = "Preparing recovery…".into(); changed();
        }
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let result = crate::update_install::return_to_previous();
            let mut s = STATE.lock().unwrap(); s.status.busy = false;
            match result {
                Ok(()) => { s.status.message = "Restarting into the previous version…".into(); let _ = proxy.send_event(crate::UserEvent::HatchQuit); }
                Err(e) => { s.status.failure_code = "RECOVERY_PREPARE_FAILED"; s.status.message = format!("Recovery did not start: {e}"); }
            }
            changed();
        });
    }
    pub(crate) fn install_update(&mut self) {
        let release = {
            let mut s = STATE.lock().unwrap();
            if !s.status.confirming || s.status.busy {
                return;
            }
            s.status.confirming = false;
            s.status.busy = true;
            s.status.message = "Downloading and verifying the update…".into();
            changed();
            s.release.clone()
        };
        let Some(release) = release else {
            return;
        };
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let result = crate::update_install::stage_and_launch(&release);
            let mut s = STATE.lock().unwrap();
            s.status.busy = false;
            match result {
                Ok(()) => {
                    s.status.message = "Restarting to finish installation…".into();
                    let _ = proxy.send_event(crate::UserEvent::HatchQuit);
                }
                Err(e) => { s.status.failure_code = "UPDATE_INSTALL_FAILED"; s.status.message = format!("Update was not installed: {e}"); }
            };
            changed();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(version: &str) -> Value {
        let name = format!("nus-{version}-linux-x86_64.tar.gz");
        let tag = format!("v{version}");
        let sha = "a".repeat(64);
        let record = serde_json::json!({"name":name,"target":"linux-x86_64","version":tag,"size":40,"sha256":sha,"signing":"checksum"});
        let manifest = serde_json::json!({"schema":1,"version":tag,"channel":if version.contains('-'){"preview"}else{"stable"},"assets":[record]});
        serde_json::json!({"tag_name":tag,"draft":false,"body":format!("<!-- nus-release:{manifest} -->"),"assets":[{"name":name,"size":40,"digest":format!("sha256:{sha}"),"browser_download_url":format!("{REPO}/releases/download/{tag}/{name}")}]})
    }
    #[test]
    fn withdrawn_bridge_and_other_channel_releases_are_not_offered() {
        let change = |field: &str, value: Value| {
            let mut item = release("1.1.0");
            let text = item["body"].as_str().unwrap();
            let mut manifest: Value = serde_json::from_str(text.strip_prefix("<!-- nus-release:").unwrap().strip_suffix(" -->").unwrap()).unwrap();
            manifest[field] = value;
            item["body"] = Value::String(format!("<!-- nus-release:{manifest} -->"));
            serde_json::json!([item])
        };
        assert!(select(&change("state", serde_json::json!("withdrawn")), "1.0.0", "linux-x86_64").is_none());
        assert!(select(&change("minimum_updater", serde_json::json!("1.0.5")), "1.0.0", "linux-x86_64").is_none());
        assert!(select(&change("state", serde_json::json!("active")), "1.0.0", "linux-x86_64").is_some());
        assert!(select(&serde_json::json!([release("1.1.0")]), "1.0.0-preview.1", "linux-x86_64").is_none());
    }
    #[test]
    fn semantic_versions_channels_and_assets_are_verified() {
        let catalog = serde_json::json!([
            release("1.0.9"),
            release("1.0.10"),
            release("1.1.0-preview.1")
        ]);
        assert_eq!(
            select(&catalog, "1.0.8", "linux-x86_64").unwrap().version,
            "v1.0.10"
        );
        assert_eq!(
            select(&catalog, "1.0.8-preview.1", "linux-x86_64")
                .unwrap()
                .version,
            "v1.1.0-preview.1"
        );
        assert!(select(&catalog, "2.0.0", "linux-x86_64").is_none());
        let windows=release("1.1.0-preview.1").to_string().replace("linux-x86_64","windows-x86_64").replace(".tar.gz",".zip");
        let unsigned:Value=serde_json::from_str(&windows).unwrap();
        assert!(select(&serde_json::json!([unsigned]),"1.0.0-preview.1","windows-x86_64").is_none());
        let signed:Value=serde_json::from_str(&windows.replace("checksum","authenticode")).unwrap();
        assert!(select(&serde_json::json!([signed]),"1.0.0-preview.1","windows-x86_64").is_some());
        let mut bad = release("1.0.10");
        bad["assets"][0]["digest"] = serde_json::json!("sha256:bad");
        assert!(select(&serde_json::json!([bad]), "1.0.0", "linux-x86_64").is_none());
    }
}

/// The existing isolated native-shot harness can inspect the warning without
/// downloading, installing or quitting. No release is armed by this fixture.
pub fn preview_warning() {
    if std::env::var_os("NUS_SHOT").is_none() || std::env::var_os("NUS_SHOT_DIR").is_none() {
        return;
    }
    let mut s = STATE.lock().unwrap();
    s.release = None;
    s.status = Status {
        message: "Native review fixture · no download will run".into(),
        available: true,
        confirming: true,
        busy: false,
        failure_code: "",
    };
    changed();
}

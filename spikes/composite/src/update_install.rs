//! Staged, hash-verified installs. No elevated permissions, no in-place file
//! overwrite, and no process killing. A failed swap restores the old package.
use crate::updates::Release;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Recovery {
    pub schema: u32,
    pub previous_version: String,
    pub next_version: String,
    pub installation: PathBuf,
    pub package: PathBuf,
    pub generation: Option<String>,
}
pub fn recovery() -> Option<Recovery> {
    let r: Recovery = serde_json::from_slice(&std::fs::read("update-recovery.json").ok()?).ok()?;
    let id = r.generation.as_ref()?;
    let installed = installation().ok()?;
    if r.schema != 1 || r.next_version != crate::updates::CURRENT || r.installation != installed
        || r.package.parent() != installed.parent() || !r.package.is_dir()
        || !r.package.file_name()?.to_str()?.starts_with(".nus-previous-")
        || !id.starts_with("generation-") || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { return None; }
    let c = nus_compat::profile::read(&Path::new("generations").join(id).join("profile")).ok()??;
    (c.version == r.previous_version && c.channel == crate::compatibility::contract().channel
        && Path::new("generations").join(id).join("complete.json").is_file()).then_some(r)
}
pub fn record_generation(id: &str) -> std::io::Result<()> {
    if !id.starts_with("generation-") || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { return Err(std::io::Error::other("Invalid recovery generation")); }
    let Ok(bytes) = std::fs::read("update-recovery.json") else { return Ok(()) };
    let mut r: Recovery = serde_json::from_slice(&bytes)?;
    let old = nus_compat::profile::read(&Path::new("generations").join(id).join("profile"))?;
    if r.next_version == crate::updates::CURRENT && r.generation.as_deref() != Some(id) && old.is_some_and(|c| c.version == r.previous_version) {
        r.generation = Some(id.into());
        crate::store::write_json(Path::new("update-recovery.json"), &r)?;
    }
    Ok(())
}
/// Same quiescent package swap as update. The old app receives the exact saved
/// generation, and its startup gate preserves the newer profile before restore.
pub fn return_to_previous() -> Result<(), String> {
    let r = recovery().ok_or("No compatible previous installation and profile generation are available")?;
    if crate::private::enabled() { return Err("Recovery is unavailable in incognito".into()); }
    let exe = if cfg!(target_os = "macos") { r.package.join("Contents/MacOS/nus") }
        else { r.package.join(if cfg!(windows) { "nus.exe" } else { "nus" }) };
    let report = std::process::Command::new(exe).arg("--compatibility").output().map_err(error)?;
    let old: nus_compat::profile::Contract = serde_json::from_slice(&report.stdout).map_err(|_| "The retained app has no supported recovery contract")?;
    let saved = nus_compat::profile::read(&Path::new("generations").join(r.generation.as_ref().unwrap()).join("profile"))
        .map_err(error)?.ok_or("The recovery profile has no compatibility contract")?;
    if !report.status.success() || old.version != r.previous_version { return Err("The retained app does not match the recovery profile".into()); }
    old.accepts(&saved).map_err(error)?;
    let parent = r.installation.parent().ok_or("No installation parent")?;
    let staging = tempfile::Builder::new().prefix(".nus-recovery-").tempdir_in(parent).map_err(error)?;
    let backup = staging.path().join("newer-installation");
    let ready = staging.path().join("ready");
    let script = staging.path().join("recover.sh");
    crate::security::write_secret(&script, UNIX.as_bytes()).map_err(error)?;
    // Re-register the old bundle/package identity before it moves, preserving
    // the installation's root rather than opening a new Welcome profile.
    crate::install::continue_after_update(&std::env::current_dir().map_err(error)?, &r.package).map_err(error)?;
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-Command", WINDOWS]); c
    } else { let mut c = std::process::Command::new("/bin/sh"); c.arg(&script); c };
    cmd.env("NUS_PARENT_PID", std::process::id().to_string())
        .env("NUS_TARGET", &r.installation).env("NUS_STAGED", &r.package)
        .env("NUS_BACKUP", &backup).env("NUS_READY", &ready)
        .env("NUS_RESULT", std::env::current_dir().map_err(error)?.join("profile/update-result.txt"))
        .env("NUS_PLATFORM", std::env::consts::OS).env("NUS_RECOVER", r.generation.as_ref().unwrap())
        .env("NUS_VERSION", &r.previous_version)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)] { use std::os::windows::process::CommandExt; cmd.creation_flags(0x08000200); }
    let mut child = cmd.spawn().map_err(error)?;
    for _ in 0..40 {
        if ready.is_file() { let _ = staging.keep(); return Ok(()); }
        if child.try_wait().map_err(error)?.is_some() { return Err("Recovery helper could not start; nus remains open".into()); }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    Err("Recovery helper did not acknowledge readiness; nus remains open".into())
}

fn installation() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(error)?;
    #[cfg(target_os = "macos")]
    {
        let bundle = exe.ancestors().nth(3).ok_or("Not an app bundle")?;
        if bundle.extension().is_none_or(|s| s != "app") {
            return Err("Use an installed app bundle to update".into());
        }
        Ok(bundle.to_path_buf())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let folder = exe.parent().ok_or("No package directory")?;
        if !folder.join("nus-package.json").is_file() {
            return Err("Use an installed nus package to update".into());
        }
        Ok(folder.to_path_buf())
    }
}
fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}
fn safe_link(path: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    let mut depth = path.parent().map_or(0, |p| p.components().count());
    for c in target.components() {
        match c {
            Component::ParentDir => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            _ => return false,
        }
    }
    true
}
fn extract(archive: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(error)?;
    if archive.extension().is_some_and(|s| s == "zip") {
        let mut zip =
            zip::ZipArchive::new(std::fs::File::open(archive).map_err(error)?).map_err(error)?;
        let mut links = Vec::new();
        let mut total = 0u64;
        for index in 0..zip.len() {
            let mut item = zip.by_index(index).map_err(error)?;
            let relative = item
                .enclosed_name()
                .ok_or("Archive path escapes the package")?
                .to_path_buf();
            if !safe_relative(&relative) {
                return Err("Invalid archive path".into());
            }
            if relative.starts_with("__MACOSX") {
                continue;
            }
            total = total
                .checked_add(item.size())
                .ok_or("Archive size overflow")?;
            if total > 4 * 1024 * 1024 * 1024 {
                return Err("Expanded package is too large".into());
            }
            let path = dest.join(&relative);
            let mode = item.unix_mode().unwrap_or(0o644);
            if mode & 0o170000 == 0o120000 {
                if item.size() > 4096 {
                    return Err("Invalid package link".into());
                }
                let mut target = String::new();
                item.read_to_string(&mut target).map_err(error)?;
                if !safe_link(&relative, Path::new(&target)) {
                    return Err("Package link escapes the archive".into());
                }
                links.push((path, target));
                continue;
            }
            if item.is_dir() {
                std::fs::create_dir_all(&path).map_err(error)?;
                continue;
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(error)?;
            }
            let mut out = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .map_err(error)?;
            std::io::copy(&mut item, &mut out).map_err(error)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777))
                    .map_err(error)?;
            }
        }
        for (path, target) in links {
            let mut parent = path.parent();
            while let Some(p) = parent {
                if p == dest {
                    break;
                }
                if p.symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_symlink())
                {
                    return Err("Archive link has a symbolic-link parent".into());
                }
                parent = p.parent();
            }
            #[cfg(unix)]
            {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(error)?;
                }
                std::os::unix::fs::symlink(target, path).map_err(error)?;
            }
            #[cfg(not(unix))]
            {
                let _ = (path, target);
                return Err("Unexpected symbolic link in a Windows update".into());
            }
        }
    } else {
        let file = std::fs::File::open(archive).map_err(error)?;
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let mut total = 0u64;
        for entry in archive.entries().map_err(error)? {
            let mut entry = entry.map_err(error)?;
            let path = entry.path().map_err(error)?.into_owned();
            let kind = entry.header().entry_type();
            if !(kind.is_file() || kind.is_dir() || kind.is_symlink() || kind.is_hard_link()) {
                return Err("Unsupported package entry".into());
            }
            if !safe_relative(&path) {
                return Err("Archive path escapes the package".into());
            }
            total = total
                .checked_add(entry.size())
                .ok_or("Archive size overflow")?;
            if total > 4 * 1024 * 1024 * 1024 {
                return Err("Expanded package is too large".into());
            }
            if let Some(target) = entry.link_name().map_err(error)? {
                if !safe_link(&path, &target) {
                    return Err("Package link escapes the archive".into());
                }
            }
            if !entry.unpack_in(dest).map_err(error)? {
                return Err("Unsafe archive member".into());
            }
        }
    }
    let root = dest.canonicalize().map_err(error)?;
    let mut dirs = vec![dest.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).map_err(error)? {
            let entry = entry.map_err(error)?;
            let kind = entry.file_type().map_err(error)?;
            if kind.is_symlink() {
                if !entry
                    .path()
                    .canonicalize()
                    .map_err(|_| "Dangling or cyclic package link")?
                    .starts_with(&root)
                {
                    return Err("Package link resolves outside the archive".into());
                }
            } else if kind.is_dir() {
                dirs.push(entry.path());
            } else if !kind.is_file() {
                return Err("Unsupported package entry".into());
            }
        }
    }
    Ok(())
}
fn verify_signature(candidate: &Path, current: &Path, release: &Release) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let verify = std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(candidate)
            .output()
            .map_err(error)?;
        if !verify.status.success() {
            return Err("The new app's signature is invalid".into());
        }
        let team = |path: &Path| -> Result<Option<String>, String> {
            let out = std::process::Command::new("/usr/bin/codesign")
                .args(["-dv", "--verbose=4"])
                .arg(path)
                .output()
                .map_err(error)?;
            Ok(String::from_utf8_lossy(&out.stderr).lines().find_map(|l| {
                l.strip_prefix("TeamIdentifier=")
                    .filter(|s| *s != "not set")
                    .map(str::to_owned)
            }))
        };
        let before = team(current)?;
        let after = team(candidate)?;
        if before.is_some() && before != after {
            return Err("The update's signing team differs from this installation".into());
        }
        if release.signing == "notarized" {
            if after.is_none()
                || !std::process::Command::new("/usr/sbin/spctl")
                    .args(["--assess", "--type", "execute"])
                    .arg(candidate)
                    .status()
                    .map_err(error)?
                    .success()
            {
                return Err("The notarized update failed macOS verification".into());
            }
        } else if before.is_some() || !release.version.contains("-preview.") {
            return Err("An unsigned update cannot replace this installation".into());
        }
    }
    #[cfg(windows)]
    {
        if release.signing != "authenticode" {
            return Err("In-app Windows updates require Authenticode signing".into());
        }
        let script="$a=Get-AuthenticodeSignature -LiteralPath $env:NUS_CANDIDATE; $b=Get-AuthenticodeSignature -LiteralPath $env:NUS_CURRENT; if($a.Status -ne 'Valid' -or $b.Status -ne 'Valid' -or $a.SignerCertificate.Subject -ne $b.SignerCertificate.Subject){exit 1}";
        if !std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("NUS_CANDIDATE", candidate.join("nus.exe"))
            .env("NUS_CURRENT", current.join("nus.exe"))
            .status()
            .map_err(error)?
            .success()
        {
            return Err("The update's Windows publisher signature could not be verified".into());
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = (candidate, current, release);
    }
    Ok(())
}
pub fn stage_and_launch(release: &Release) -> Result<(), String> {
    if crate::private::enabled() {
        return Err("Updates are unavailable in incognito".into());
    }
    crate::updates::revalidate(release)?;
    let installed = installation()?;
    let parent = installed.parent().ok_or("No installation parent")?;
    let staging=tempfile::Builder::new().prefix(".nus-update-").tempdir_in(parent).map_err(|_|"The installation directory is not writable. Move nus to a user-writable location or install manually.")?;
    let archive = staging.path().join(&release.name);
    let url = format!(
        "{}/releases/download/{}/{}",
        crate::updates::REPO,
        release.version,
        release.name
    );
    let output = crate::updates::curl()
        .args([
            "--max-time",
            "3600",
            "--max-filesize",
            &release.size.to_string(),
            "--output",
        ])
        .arg(&archive)
        .arg(&url)
        .output()
        .map_err(error)?;
    if !output.status.success() {
        return Err("Download failed; the current installation is unchanged".into());
    }
    let mut file = std::fs::File::open(&archive).map_err(error)?;
    if file.metadata().map_err(error)?.len() != release.size {
        return Err("The downloaded size differs from the verified release".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(error)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if format!("{:x}", hash.finalize()) != release.sha256.to_lowercase() {
        return Err("The downloaded checksum is incorrect".into());
    }
    drop(file);
    let unpack = staging.path().join("unpacked");
    extract(&archive, &unpack)?;
    std::fs::remove_file(&archive).map_err(error)?;
    let candidate = if cfg!(target_os = "macos") {
        unpack.join("nus.app")
    } else {
        unpack.join(
            release
                .name
                .trim_end_matches(".tar.gz")
                .trim_end_matches(".zip"),
        )
    };
    verify_signature(&candidate, &installed, release)?;
    let executable = if cfg!(target_os = "macos") {
        candidate.join("Contents/MacOS/nus")
    } else {
        candidate.join(if cfg!(windows) { "nus.exe" } else { "nus" })
    };
    let version = std::process::Command::new(&executable)
        .arg("--version")
        .output()
        .map_err(error)?;
    if !version.status.success()
        || !String::from_utf8_lossy(&version.stdout).starts_with(&format!(
            "nus {} (",
            release.version.trim_start_matches('v')
        ))
    {
        return Err("The package reports a different version than the release".into());
    }
    let report = std::process::Command::new(&executable).arg("--compatibility").output().map_err(error)?;
    let candidate_contract: nus_compat::profile::Contract = serde_json::from_slice(&report.stdout)
        .map_err(|_| "This package has no supported profile compatibility contract; install it separately")?;
    if !report.status.success() || candidate_contract.version != release.version.trim_start_matches('v') {
        return Err("The package compatibility contract does not match the release".into());
    }
    candidate_contract.accepts(&crate::compatibility::contract()).map_err(error)?;
    crate::updates::revalidate(release)?;
    let current = std::env::current_dir().map_err(error)?;
    crate::install::continue_after_update(&current, &candidate).map_err(error)?;
    let backup = parent.join(format!(".nus-previous-{}", crate::journal::now()));
    if backup.exists() {
        return Err("A recovery backup already exists; retry in a moment".into());
    }
    crate::store::write_json(&current.join("update-recovery.json"), &Recovery {
        schema: 1, previous_version: crate::updates::CURRENT.into(), next_version: candidate_contract.version,
        installation: installed.clone(), package: backup.clone(), generation: None,
    }).map_err(error)?;
    let ready = staging.path().join("ready");
    let result = current.join("profile/update-result.txt");
    let script = staging.path().join(if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    });
    crate::security::write_secret(
        &script,
        if cfg!(windows) {
            WINDOWS.as_bytes()
        } else {
            UNIX.as_bytes()
        },
    )
    .map_err(error)?;
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-Command", WINDOWS]);
        c
    } else {
        let mut c = std::process::Command::new("/bin/sh");
        c.arg(&script);
        c
    };
    let pid = std::process::id().to_string();
    cmd.env("NUS_PARENT_PID", pid)
        .env_remove("NUS_RECOVER")
        .env("NUS_TARGET", &installed)
        .env("NUS_STAGED", &candidate)
        .env("NUS_BACKUP", &backup)
        .env("NUS_READY", &ready)
        .env("NUS_RESULT", &result)
        .env("NUS_VERSION", release.version.trim_start_matches('v'))
        .env("NUS_PLATFORM", std::env::consts::OS);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000200);
    }
    let mut child = cmd.spawn().map_err(error)?;
    for _ in 0..40 {
        if ready.is_file() {
            let _ = staging.keep();
            return Ok(());
        }
        if child.try_wait().map_err(error)?.is_some() {
            return Err("The installer could not start; the app remains open".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    Err("Installer did not acknowledge readiness; the app remains open".into())
}
const UNIX: &str = r#"set -eu
printf ready > "$NUS_READY"
i=0
while kill -0 "$NUS_PARENT_PID" 2>/dev/null; do
  i=$((i+1)); if [ "$i" -gt 960 ]; then printf 'Update cancelled: nus did not exit.\n' > "$NUS_RESULT"; exit 1; fi
  sleep 0.25
done
if ! mv "$NUS_TARGET" "$NUS_BACKUP"; then printf 'Update failed: installation could not be moved.\n' > "$NUS_RESULT"; exit 1; fi
if ! mv "$NUS_STAGED" "$NUS_TARGET"; then mv "$NUS_BACKUP" "$NUS_TARGET"; printf 'Update failed; previous installation restored.\n' > "$NUS_RESULT"; exit 1; fi
printf 'Update installed. Previous installation retained for recovery.\n' > "$NUS_RESULT"
if [ "$NUS_PLATFORM" = macos ]; then
  if [ -n "${NUS_RECOVER:-}" ]; then /usr/bin/open "$NUS_TARGET" --args "--recover-profile=$NUS_RECOVER"; else /usr/bin/open "$NUS_TARGET"; fi
else
  if [ -n "${NUS_RECOVER:-}" ]; then "$NUS_TARGET/nus" "--recover-profile=$NUS_RECOVER" >/dev/null 2>&1 & else "$NUS_TARGET/nus" >/dev/null 2>&1 & fi
fi
"#;
const WINDOWS: &str = r#"$ErrorActionPreference='Stop'
Set-Content -LiteralPath $env:NUS_READY -Value 'ready'
$until=(Get-Date).AddMinutes(4)
while(Get-Process -Id $env:NUS_PARENT_PID -ErrorAction SilentlyContinue){if((Get-Date) -gt $until){Set-Content -LiteralPath $env:NUS_RESULT -Value 'Update cancelled: nus did not exit.';exit 1};Start-Sleep -Milliseconds 250}
try{Move-Item -LiteralPath $env:NUS_TARGET -Destination $env:NUS_BACKUP;try{Move-Item -LiteralPath $env:NUS_STAGED -Destination $env:NUS_TARGET}catch{Move-Item -LiteralPath $env:NUS_BACKUP -Destination $env:NUS_TARGET;throw};Set-Content -LiteralPath $env:NUS_RESULT -Value 'Update installed. Previous installation retained for recovery.';try{Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue|ForEach-Object{$k=Get-ItemProperty -LiteralPath $_.PSPath;if($k.InstallLocation -and $k.InstallLocation.TrimEnd('\') -ieq $env:NUS_TARGET.TrimEnd('\')){Set-ItemProperty -LiteralPath $_.PSPath -Name DisplayVersion -Value $env:NUS_VERSION}}}catch{};if($env:NUS_RECOVER){Start-Process -FilePath (Join-Path $env:NUS_TARGET 'nus.exe') -ArgumentList ('--recover-profile='+$env:NUS_RECOVER)}else{Start-Process -FilePath (Join-Path $env:NUS_TARGET 'nus.exe')}}catch{Set-Content -LiteralPath $env:NUS_RESULT -Value 'Update failed; inspect the retained previous installation.';exit 1}
"#;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_paths_and_links_cannot_escape() {
        assert!(!safe_relative(Path::new("../outside")));
        assert!(!safe_relative(Path::new("/outside")));
        assert!(safe_relative(Path::new("nus.app/Contents/MacOS/nus")));
        assert!(safe_link(
            Path::new("nus.app/Contents/Frameworks/F.framework/F"),
            Path::new("Versions/Current/F")
        ));
        assert!(!safe_link(
            Path::new("nus/link"),
            Path::new("../../outside")
        ));
    }
}

#[cfg(all(test, unix))]
mod install_tests {
    use super::*;
    #[test]
    fn helper_swaps_complete_packages_and_restores_failed_swap() {
        for success in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let target = temp.path().join("nus package");
            let staged = temp.path().join("candidate");
            let backup = temp.path().join("previous");
            std::fs::create_dir(&target).unwrap();
            std::fs::write(target.join("version"), "old").unwrap();
            if success {
                std::fs::create_dir(&staged).unwrap();
                std::fs::write(staged.join("version"), "new").unwrap();
            }
            let status = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(UNIX)
                .env("NUS_PARENT_PID", "99999999")
                .env("NUS_READY", temp.path().join("ready"))
                .env("NUS_TARGET", &target)
                .env("NUS_STAGED", &staged)
                .env("NUS_BACKUP", &backup)
                .env("NUS_RESULT", temp.path().join("result"))
                .env("NUS_PLATFORM", "fixture")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert_eq!(status.success(), success);
            assert_eq!(
                std::fs::read_to_string(target.join("version")).unwrap(),
                if success { "new" } else { "old" }
            );
            if success {
                assert_eq!(
                    std::fs::read_to_string(backup.join("version")).unwrap(),
                    "old"
                );
            }
        }
    }
    #[test]
    fn zip_extraction_rejects_escaping_links_and_preserves_valid_framework_links() {
        for escaping in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let archive = temp.path().join("package.zip");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            zip.start_file(
                "nus.app/Contents/real",
                zip::write::SimpleFileOptions::default().unix_permissions(0o755),
            )
            .unwrap();
            zip.write_all(b"synthetic executable").unwrap();
            zip.add_symlink(
                "nus.app/Contents/link",
                if escaping { "../../../outside" } else { "real" },
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.finish().unwrap();
            assert_eq!(
                extract(&archive, &temp.path().join("out")).is_err(),
                escaping
            );
        }
    }
}

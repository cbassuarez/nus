//! GNOME: nus's launch on its own dock icon, through the bundled Shell
//! extension (packaging/gnome-shell/dock-motion@nus.dev). GNOME gives an app
//! no way to change its dock icon while it runs, so nus carries a tiny
//! extension that can, for nus alone.
//!
//! nus writes the extension into the user's extensions folder whenever it
//! is missing or older than this nus, and enables it once — never again if
//! it is turned off afterwards. GNOME on Wayland looks for new extensions
//! only at login, so the motion starts from the next one. At launch nus
//! renders the six faces in the current signal colour and calls the
//! extension on Shell's bus; when the window is up it says so, and the
//! extension finishes the pass. With no extension, nothing happens.

use std::path::{Path, PathBuf};

use nus_render::dock_icon::{self, Face, STEP_SECONDS};

const UUID: &str = "dock-motion@nus.dev";
const METADATA: &str = include_str!("../../../packaging/gnome-shell/dock-motion@nus.dev/metadata.json");
const EXTENSION: &str = include_str!("../../../packaging/gnome-shell/dock-motion@nus.dev/extension.js");

pub(crate) fn gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("GNOME")))
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

/// The extension's files, written when missing or different.
fn install(data: &Path) -> std::io::Result<bool> {
    let dir = data.join("gnome-shell/extensions").join(UUID);
    let same = |name: &str, text: &str| std::fs::read_to_string(dir.join(name)).is_ok_and(|t| t == text);
    if same("metadata.json", METADATA) && same("extension.js", EXTENSION) {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir)?;
    for (name, text) in [("metadata.json", METADATA), ("extension.js", EXTENSION)] {
        let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, dir.join(name))?;
    }
    Ok(true)
}

/// Turn it on in GNOME's list once. A marker remembers that nus did, so a
/// person who turns it off keeps it off.
fn enable_once(data: &Path) {
    let marker = data.join("nus/dock-motion-enabled");
    if marker.exists() {
        return;
    }
    let get = |key: &str| {
        nus_compat::command("gsettings").args(["get", "org.gnome.shell", key]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let Some(list) = get("enabled-extensions") else { return };
    if !list.contains(UUID) {
        // A GVariant string array, as gsettings prints it: '@as []' or ['a', 'b'].
        let inner = list.trim_start_matches("@as").trim().trim_start_matches('[').trim_end_matches(']').trim();
        let next = if inner.is_empty() { format!("['{UUID}']") } else { format!("[{inner}, '{UUID}']") };
        let ok = nus_compat::command("gsettings").args(["set", "org.gnome.shell", "enabled-extensions", &next]).status().is_ok_and(|s| s.success());
        if !ok {
            return;
        }
    }
    if let Some(dir) = marker.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(marker, "enabled by nus; turn it off in Extensions and it stays off\n");
}

/// The six faces in this signal colour, as files the extension may read.
fn frames(data: &Path, signal: nus_render::Color) -> Vec<String> {
    let icons = data.join("nus/icons");
    let _ = std::fs::create_dir_all(&icons);
    let rgb = signal.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    Face::ALL
        .iter()
        .filter_map(|&face| {
            let path = icons.join(format!("launch-{}-{face:?}-{:02x}{:02x}{:02x}.png", dock_icon::ART_VERSION, rgb[0], rgb[1], rgb[2]));
            if !path.exists() {
                let rgba = dock_icon::render(256, signal, face);
                std::fs::write(&path, nus_render::icon::png(&rgba, 256, 256)).ok()?;
            }
            Some(path.to_string_lossy().into_owned())
        })
        .collect()
}

fn call(method: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) {
    let Ok(conn) = zbus::blocking::Connection::session() else { return };
    let r = conn.call_method(Some("org.gnome.Shell"), "/dev/nus/DockMotion", Some("dev.nus.DockMotion"), method, body);
    if let Err(e) = r {
        tracing::debug!("dock motion: {method}: {e}");
    }
}

/// Starting: install and enable as needed, then ask for the motion. Off the
/// main thread; nothing waits for it.
pub fn launch(signal: nus_render::Color, reduced: bool) {
    if !gnome() || crate::private::enabled() {
        return;
    }
    let _ = std::thread::Builder::new().name("dock motion".into()).spawn(move || {
        let Some(data) = data_home() else { return };
        match install(&data) {
            Ok(true) => tracing::info!("dock motion: the GNOME extension is in place; it loads at the next login"),
            Ok(false) => {}
            Err(e) => tracing::warn!("dock motion: could not write the GNOME extension: {e}"),
        }
        enable_once(&data);
        if reduced {
            return;
        }
        let frames = frames(&data, signal);
        if frames.len() == Face::ALL.len() {
            call("Launch", &(crate::default_browser::app_id(), frames, (STEP_SECONDS * 1000.0).round() as u32));
        }
    });
}

/// The window is up: the motion finishes its pass.
pub fn ready() {
    if !gnome() || crate::private::enabled() {
        return;
    }
    let _ = std::thread::Builder::new().name("dock motion".into()).spawn(|| call("Ready", &(crate::default_browser::app_id(),)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_is_written_once_and_kept_current() {
        let data = std::env::temp_dir().join(format!("nus-dock-motion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        assert!(install(&data).unwrap());
        assert!(!install(&data).unwrap());
        let js = data.join("gnome-shell/extensions").join(UUID).join("extension.js");
        std::fs::write(&js, "old").unwrap();
        assert!(install(&data).unwrap());
        assert_eq!(std::fs::read_to_string(js).unwrap(), EXTENSION);
        let meta: serde_json::Value = serde_json::from_str(METADATA).unwrap();
        assert_eq!(meta["uuid"], UUID);
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn frames_are_six_pngs_in_nus_icons() {
        let data = std::env::temp_dir().join(format!("nus-dock-frames-{}", std::process::id()));
        let f = frames(&data, [0.9, 0.2, 0.2, 1.0]);
        assert_eq!(f.len(), 6);
        assert!(f.iter().all(|p| p.contains("/nus/icons/launch-") && p.ends_with(".png")));
        let _ = std::fs::remove_dir_all(data);
    }
}

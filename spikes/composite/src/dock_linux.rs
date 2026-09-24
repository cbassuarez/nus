//! Wayland docks resolve the desktop entry's icon, not winit's window icon.
//! Publish a colour-specific asset and update Icon= so launchers invalidate it.
use nus_render::{
    dock_icon,
    Color,
};
use std::{
    io,
    path::{Path, PathBuf},
};
pub const APP_ID: &str = "dev.nus.app";

#[cfg(target_os = "linux")]
pub struct Publisher(std::sync::mpsc::Sender<Color>);
#[cfg(target_os = "linux")]
impl Publisher {
    pub fn new() -> Self {
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("launcher artwork".into())
            .spawn(move || {
                while let Ok(mut signal) = receive.recv() {
                    for newer in receive.try_iter() {
                        signal = newer;
                    }
                    if let Err(error) = refresh(signal) {
                        tracing::warn!("Linux launcher icon: {error}");
                    }
                }
            })
            .expect("launcher renderer thread");
        Self(send)
    }
    pub fn update(&self, signal: Color) {
        let _ = self.0.send(signal);
    }
}

#[cfg(target_os = "linux")]
pub fn refresh(signal: Color) -> io::Result<()> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no user data directory"))?;
    publish(&data, &std::env::current_exe()?, signal)
}

fn publish(data: &Path, exe: &Path, signal: Color) -> io::Result<()> {
    let colour = signal.map(|n| (n.clamp(0.0, 1.0) * 255.0).round() as u8);
    let icons = data.join("nus/icons");
    let apps = data.join("applications");
    std::fs::create_dir_all(&icons)?;
    std::fs::create_dir_all(&apps)?;
    let mercury=crate::app_icon::mercury();
    let path = if mercury{icons.join("mercury-tidal-v1.png")}else{icons.join(format!(
        "orbit-{:?}-{:02x}{:02x}{:02x}{:02x}.png",
        crate::app_icon::face(),
        colour[0], colour[1], colour[2], colour[3]
    ))};
    if !path.exists() {
        let rgba = if mercury{crate::mercury::icon(256)}else{dock_icon::render(256, signal, crate::app_icon::face())};
        atomic_write(&path, &nus_render::icon::png(&rgba, 256, 256))?;
    }
    let entry = apps.join(format!("{APP_ID}.desktop"));
    let existing = std::fs::read_to_string(&entry).ok().or_else(|| {
        // Preserve distributor launch flags/actions when making a user override.
        let dirs = std::env::var_os("XDG_DATA_DIRS")
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        std::env::split_paths(&dirs)
            .filter(|p| p.is_absolute())
            .find_map(|p| {
                std::fs::read_to_string(p.join("applications").join(format!("{APP_ID}.desktop")))
                    .ok()
            })
    });
    let text=match existing {Some(text)=>set_icon(&text,&path),None=>format!("[Desktop Entry]\nType=Application\nName=nus\nComment=A browser and terminal workspace\nExec={} %U\nIcon={}\nTerminal=false\nStartupWMClass={APP_ID}\nCategories=Development;WebBrowser;\n",exec_argument(exe),entry_value(&path.to_string_lossy()))};
    atomic_write(&entry, text.as_bytes())
}
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}
fn entry_value(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
fn exec_argument(path: &Path) -> String {
    let s = path.to_string_lossy().replace('%', "%%");
    let mut quoted = String::from("\"");
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '`' | '$') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    entry_value(&quoted)
}
fn set_icon(text: &str, path: &Path) -> String {
    let value = format!("Icon={}", entry_value(&path.to_string_lossy()));
    let mut out = Vec::new();
    let mut main = false;
    let mut added = false;
    for line in text.lines() {
        if line.starts_with('[') {
            if main && !added {
                out.push(value.clone());
                added = true;
            }
            main = line == "[Desktop Entry]";
        }
        if main && line.starts_with("Icon=") {
            if !added {
                out.push(value.clone());
                added = true;
            }
        } else {
            out.push(line.to_string());
        }
    }
    if main && !added {
        out.push(value);
    }
    out.join("\n") + "\n"
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn theme_changes_update_launcher_and_keep_custom_commands() {
        let root = std::env::temp_dir().join(format!("nus-dock-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("applications")).unwrap();
        let entry = root.join(format!("applications/{APP_ID}.desktop"));
        std::fs::write(&entry,"[Desktop Entry]\nName=nus\nExec=nus --custom %U\nIcon=old\n[Desktop Action New]\nIcon=action-icon\nExec=nus --new\n").unwrap();
        publish(&root, Path::new("/opt/nus"), [0.8, 0.1, 0.2, 1.0]).unwrap();
        let a = std::fs::read_to_string(&entry).unwrap();
        publish(&root, Path::new("/opt/nus"), [0.1, 0.6, 0.9, 1.0]).unwrap();
        let b = std::fs::read_to_string(&entry).unwrap();
        assert_ne!(a, b);
        assert!(b.contains("Exec=nus --custom %U"));
        assert!(b.contains("Icon=action-icon"));
        let png = b.lines().find_map(|s| s.strip_prefix("Icon=")).unwrap();
        assert!(Path::new(png).exists());
        assert_eq!(&std::fs::read(png).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_icon_is_inserted_in_main_section() {
        let text = set_icon(
            "[Desktop Entry]\nName=nus\n[Desktop Action New]\nName=New",
            Path::new("/tmp/icon.png"),
        );
        assert!(text.contains("Name=nus\nIcon=/tmp/icon.png\n[Desktop Action New]"));
        assert_eq!(
            exec_argument(Path::new("/tmp/nus folder/100%")),
            "\"/tmp/nus folder/100%%\""
        );
    }
}

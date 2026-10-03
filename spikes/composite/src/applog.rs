//! nus's log, on disk, for `nus logs`: what goes to stderr also goes to
//! `<data>/logs/<channel>/nus.log`, without colour. It is bounded: at launch
//! a log over 4 MB becomes nus.1.log (replacing the one before), and a run
//! that writes 8 MB stops there with one line saying so. Two runs' worth at
//! most, whatever happens.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const ROTATE_AT: u64 = 4 << 20;
const RUN_CAP: u64 = 8 << 20;

/// The folder nus keeps its data in; the same place `nus logs` looks.
fn data_home() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    Some(if cfg!(target_os = "macos") {
        home()?.join("Library/Application Support/nus")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("nus")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home().map(|h| h.join(".local/share")))?
            .join("nus")
    })
}

pub fn dir() -> Option<PathBuf> {
    let channel = nus_compat::Channel::for_version(env!("NUS_BUILD_VERSION")).directory();
    Some(data_home()?.join("logs").join(channel))
}

struct State {
    file: Option<File>,
    written: u64,
}

/// A writer for tracing; clones share one file and one count.
#[derive(Clone)]
pub struct Log(Arc<Mutex<State>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let Ok(mut s) = self.0.lock() else { return Ok(buf.len()) };
        if s.written >= RUN_CAP {
            return Ok(buf.len());
        }
        s.written += buf.len() as u64;
        let full = s.written >= RUN_CAP;
        if let Some(f) = s.file.as_mut() {
            let _ = f.write_all(buf);
            if full {
                let _ = f.write_all(b"-- the log reached its limit for this run; nothing more is written until nus restarts\n");
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The log file for this run, or a writer that drops everything when the
/// folder cannot be made: logging never stops nus.
pub fn open() -> Log {
    let file = dir().and_then(|d| {
        std::fs::create_dir_all(&d).ok()?;
        let path = d.join("nus.log");
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > ROTATE_AT) {
            let _ = std::fs::rename(&path, d.join("nus.1.log"));
        }
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
    });
    let mut log = Log(Arc::new(Mutex::new(State { file, written: 0 })));
    let _ = writeln!(log, "-- nus {} started", env!("NUS_BUILD_VERSION"));
    log
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_stops_writing_at_its_cap() {
        let dir = std::env::temp_dir().join(format!("nus-applog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nus.log");
        let file = File::create(&path).unwrap();
        let mut log = Log(Arc::new(Mutex::new(State { file: Some(file), written: RUN_CAP - 10 })));
        log.write_all(b"0123456789abcdef\n").unwrap();
        log.write_all(b"never\n").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("0123456789abcdef\n-- the log reached its limit"));
        assert!(!text.contains("never"));
        let _ = std::fs::remove_dir_all(dir);
    }
}

//! NUS_INPUT_LOG=<file>: every byte written to a shell, with the line that
//! wrote it, and whatever the app notes beside it (key events, focus). For
//! chasing input nobody typed; off unless the variable is set.
use std::io::Write;
use std::sync::OnceLock;

fn path() -> Option<&'static std::path::Path> {
    static PATH: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::var_os("NUS_INPUT_LOG")
            .filter(|p| !p.is_empty())
            .map(Into::into)
    })
    .as_deref()
}

pub fn enabled() -> bool {
    path().is_some()
}

fn append(line: &str) {
    let Some(p) = path() else { return };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() % 100_000_000)
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
    {
        let _ = writeln!(f, "{ms:>9} {line}");
    }
}

/// A line of context from the app (a key event, a focus change).
pub fn note(line: impl AsRef<str>) {
    if enabled() {
        append(line.as_ref());
    }
}

/// Bytes on their way to a shell, escaped, and the line that sent them
/// (`Pty::write` tracks its caller, so this needs no debug symbols).
pub fn wrote(bytes: &[u8], at: &std::panic::Location<'_>) {
    if !enabled() {
        return;
    }
    let text: String = bytes.escape_ascii().map(char::from).collect();
    append(&format!("write \"{text}\"  <- {}:{}", at.file(), at.line()));
}

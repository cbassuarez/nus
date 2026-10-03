//! Shared contracts. App versions, disk formats and wire protocols are separate
//! numbers: a patch release need not change either compatibility boundary.
pub mod profile;

/// A program nus runs in the background (git, netstat, reg, a language
/// server…): a plain `Command`, except that on Windows it opens no console
/// window. nus is a windowed program, so without this every console
/// program it starts flashes a window of its own. Shells in a terminal pane
/// go through the PTY instead and keep theirs.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

pub const CLI_PROTOCOL: u32 = 1;
pub const HOLD_PROTOCOL: u32 = 1;
pub const PROFILE_FORMAT: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Current,
    Preview,
    Development,
}

impl Channel {
    pub fn for_version(version: &str) -> Self {
        match semver::Version::parse(version) {
            Ok(v) if v.pre.is_empty() => Self::Current,
            Ok(v) if v.pre.as_str().starts_with("preview.") => Self::Preview,
            _ => Self::Development,
        }
    }
    pub fn directory(self) -> &'static str {
        match self {
            Self::Current => "release",
            Self::Preview => "preview",
            Self::Development => "development",
        }
    }
}

/// Missing is the explicitly supported legacy v1 wire format. Unknown/malformed
/// values must fail before dispatch; do not interpret them as legacy requests.
pub fn protocol_matches(value: Option<&serde_json::Value>, expected: u32) -> bool {
    value.map_or(expected == 1, |v| v.as_u64() == Some(expected as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channels_and_wire_versions_are_explicit() {
        assert_eq!(Channel::for_version("0.8.0-preview.2"), Channel::Preview);
        assert_eq!(Channel::for_version("0.8.0"), Channel::Current);
        assert_eq!(Channel::for_version("0.8.0-dev"), Channel::Development);
        assert!(protocol_matches(None, 1));
        assert!(!protocol_matches(None, 2));
        for v in [
            serde_json::json!(2),
            serde_json::json!("1"),
            serde_json::Value::Null,
        ] {
            assert!(!protocol_matches(Some(&v), 1));
        }
    }
}

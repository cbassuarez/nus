//! Opens an editable GitHub draft with build and OS only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bug,
    Feature,
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub fn version() -> String {
    format!(
        "nus {} ({}); Chromium {}",
        env!("NUS_BUILD_VERSION"),
        env!("NUS_BUILD_REVISION"),
        crate::chromium_version()
    )
}

/// Deliberate allowlist: no instance identity, paths, URLs, logs or process data.
/// The exact text is shown in Settings before the user copies it.
pub fn details() -> String {
    format!("{} · OS {} · architecture {} · channel {:?} · profile format {} · settings schema {} · CLI protocol {} · holder protocol {} · last update failure {}",
        version(), std::env::consts::OS, std::env::consts::ARCH,
        nus_compat::Channel::for_version(env!("NUS_BUILD_VERSION")), nus_compat::PROFILE_FORMAT,
        crate::prefs::SCHEMA, nus_compat::CLI_PROTOCOL, nus_compat::HOLD_PROTOCOL,
        { let code = crate::updates::status().failure_code; if code.is_empty() { "none this session" } else { code } })
}

pub fn issue_url(kind: Kind) -> String {
    let template = match kind {
        Kind::Bug => "bug.yml",
        Kind::Feature => "feature.yml",
    };
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    format!(
        "https://github.com/cbassuarez/nus/issues/new?template={template}&version={}&os={}&body={}",
        encode(&version()),
        encode(os),
        encode(&format!(
            "Version: {}\nOS: {os}\n\n{}\n\n",
            version(),
            match kind {
                Kind::Bug => "Steps to reproduce:\n\nExpected behavior:\n\nActual behavior:",
                Kind::Feature => "What would you like to do?\n\nHow would this help?",
            }
        ))
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_are_fixed_origin_encoded_and_minimal() {
        for kind in [super::Kind::Bug, super::Kind::Feature] {
            let url = super::issue_url(kind);
            assert!(url.starts_with("https://github.com/cbassuarez/nus/issues/new?template="));
            assert!(!url.contains(' ') && !url.contains('\n'));
            assert!(url.contains("&version=") && url.contains("&os="));
            assert!(!url.contains("/Users/") && !url.contains("token="));
        }
        assert_eq!(super::encode("a&b=é"), "a%26b%3D%C3%A9");
    }
}

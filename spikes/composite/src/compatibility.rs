//! The only entry into a mutable desktop profile. Keep this before prefs/CEF.
use nus_compat::profile::{Contract, Guard};
use std::{io, path::Path};

pub fn recovery_launch() -> bool {
    std::env::args().any(|a| a.starts_with("--recover-profile="))
}

pub fn contract() -> Contract {
    Contract::new(
        env!("NUS_BUILD_VERSION"),
        cef::sys::CHROME_VERSION_MAJOR as u32,
        crate::prefs::SCHEMA,
    )
}
pub fn start() -> io::Result<Guard> {
    let root = std::env::current_dir()?;
    let guard = Guard::acquire(&root)?;
    let expected = contract();
    // Recovery is deliberately an offline operation. The running old binary
    // must match the generation; a newer binary must not immediately remigrate it.
    if let Some(id) =
        std::env::args().find_map(|a| a.strip_prefix("--recover-profile=").map(str::to_owned))
    {
        guard.recover(&id, &expected)?;
    }
    if let Some(generation) = guard.open(&expected)? {
        crate::update_install::record_generation(&generation)?;
        tracing::info!("Saved pre-upgrade profile generation {generation}");
    }
    if let Ok(bytes) = std::fs::read("last-generation.json") {
        if let Ok(id) = serde_json::from_slice::<String>(&bytes) {
            crate::update_install::record_generation(&id)?;
        }
    }
    Ok(guard)
}
pub fn summary() -> String {
    match nus_compat::profile::read(Path::new("profile")) {
        Ok(Some(c)) => format!(
            "{:?} · profile {} · settings {} · Chromium {} · launch {}",
            c.channel,
            c.format,
            c.settings,
            c.chromium_major,
            if c.healthy { "acknowledged" } else { "pending" }
        ),
        _ => "Profile compatibility information unavailable".into(),
    }
}

//! Protected content (Widevine), for streaming sites that need DRM.
//!
//! Chromium ships no CDM: its component updater installs one into the
//! profile, and on its own schedule the first check waits minutes after
//! launch, so a stream opened early finds nothing to play with. nus asks
//! for it as soon as the browser is up, and Settings · Browser shows where
//! it stands. Chromium's own settings pages do not exist in nus's
//! off-screen pages, so `chrome://settings` is answered with nus's.
use cef::*;
use std::sync::Mutex;

/// Chromium's component id for the Widevine CDM.
pub const ID: &str = "oimompecagnajdejgnnjijobebaeigek";

/// The last on-demand update's outcome, in words.
static LAST: Mutex<String> = Mutex::new(String::new());

wrap_component_update_callback! {
    struct Done;

    impl ComponentUpdateCallback {
        fn on_complete(&self, _component_id: Option<&CefString>, error: ComponentUpdateError) {
            let words = if error == ComponentUpdateError::NONE {
                ""
            } else if error == ComponentUpdateError::UPDATE_IN_PROGRESS {
                "already downloading"
            } else if error == ComponentUpdateError::RETRY_LATER {
                "the update service asked to retry later"
            } else if error == ComponentUpdateError::CRX_NOT_FOUND {
                "not offered for this system"
            } else {
                "could not be fetched"
            };
            tracing::info!("widevine: update finished ({})", error.get_raw());
            if let Ok(mut last) = LAST.lock() {
                *last = words.into();
            }
        }
    }
}

/// Ask Chromium to install or refresh the CDM now. UI thread, after CEF is up.
pub fn fetch() {
    if crate::private::enabled() {
        return;
    }
    let Some(updater) = component_updater_get() else { return };
    let mut done = Done::new();
    updater.update(Some(&ID.into()), ComponentUpdatePriority::FOREGROUND, Some(&mut done));
}

/// Where the CDM stands, for Settings · Browser.
pub fn status() -> String {
    if !crate::browser_runtime::ready() {
        return "starts with the first page".into();
    }
    let Some(updater) = component_updater_get() else { return "component updater unavailable".into() };
    let Some(cdm) = updater.component_by_id(Some(&ID.into())) else {
        return "not part of this build of Chromium".into();
    };
    let version = CefString::from(&cdm.version()).to_string();
    let state = cdm.state();
    let installed = [ComponentState::UPDATED, ComponentState::UP_TO_DATE, ComponentState::RUN].contains(&state);
    let words = if installed {
        format!("Widevine {version} · installed")
    } else if [ComponentState::DOWNLOADING, ComponentState::DECOMPRESSING, ComponentState::PATCHING, ComponentState::UPDATING].contains(&state) {
        "Widevine · downloading".into()
    } else if state == ComponentState::CHECKING {
        "Widevine · checking".into()
    } else if !version.is_empty() {
        format!("Widevine {version}")
    } else {
        "Widevine · not installed yet".into()
    };
    match LAST.lock().map(|l| l.clone()) {
        Ok(last) if !last.is_empty() && !installed => format!("{words} · {last}"),
        _ => words,
    }
}

/// `chrome://settings…` is Chromium's own UI, which nus's pages cannot show.
pub fn is_chrome_settings(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower == "chrome://settings" || lower.starts_with("chrome://settings/") || lower.starts_with("chrome://settings?")
}

#[cfg(test)]
mod tests {
    #[test]
    fn chrome_settings_pages_are_recognized() {
        assert!(super::is_chrome_settings("chrome://settings/content/protectedContent"));
        assert!(super::is_chrome_settings(" CHROME://SETTINGS "));
        assert!(!super::is_chrome_settings("chrome://version"));
        assert!(!super::is_chrome_settings("https://example.com/chrome://settings"));
    }
}

//! The power budget: what the app may spend on things that are only
//! pretty. An art behind the prompt redraws every frame while the window
//! is looked at and the machine is plugged in; on battery it requests a
//! frame every 1/30 second, and while unfocused it stands still. The
//! same budget is what the plate and the sky read. Nothing here shows in
//! the UI: it is a policy, kept in one place so the next screen or the
//! next chip changes one file.

use std::time::{Duration, Instant};

/// Whether the machine is running on its battery, asked at most every
/// few seconds (the answer is cached; the OS call is cheap but not free).
pub fn on_battery() -> bool {
    use std::sync::Mutex;
    static LAST: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = *g {
        if crate::clock::since(at) < Duration::from_secs(5) {
            return v;
        }
    }
    let v = probe_battery();
    *g = Some((crate::clock::now(), v));
    v
}

#[cfg(windows)]
fn probe_battery() -> bool {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS { ACLineStatus: 255, BatteryFlag: 0, BatteryLifePercent: 0, SystemStatusFlag: 0, BatteryLifeTime: 0, BatteryFullLifeTime: 0 };
    // ACLineStatus: 0 offline (battery), 1 online, 255 unknown.
    unsafe { GetSystemPowerStatus(&mut s) != 0 && s.ACLineStatus == 0 }
}

#[cfg(target_os = "linux")]
fn probe_battery() -> bool {
    // Any power supply of type Mains that is offline, with a battery present.
    let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") else { return false };
    let mut mains_online = None;
    let mut has_battery = false;
    for e in rd.flatten() {
        let p = e.path();
        let kind = std::fs::read_to_string(p.join("type")).unwrap_or_default();
        if kind.trim() == "Mains" {
            let online = std::fs::read_to_string(p.join("online")).unwrap_or_default();
            mains_online = Some(online.trim() == "1");
        } else if kind.trim() == "Battery" {
            has_battery = true;
        }
    }
    has_battery && mains_online == Some(false)
}

#[cfg(target_os = "macos")]
fn probe_battery() -> bool {
    crate::finish_work_native::on_battery()
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn probe_battery() -> bool {
    false
}

/// What an art may do this frame: draw and ask for the next, draw and
/// wait, or hold the last frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Budget {
    /// Every frame.
    Full,
    /// 30 frames per second: on battery.
    Half,
    /// Still: the window is not being looked at.
    Still,
}

impl crate::app::App {
    /// The budget for a pretty thing in this window, now.
    pub(crate) fn art_budget(&self) -> Budget {
        if !self.window_focused && self.shot.is_none() {
            return Budget::Still;
        }
        if on_battery() {
            Budget::Half
        } else {
            Budget::Full
        }
    }

    /// A skipped request is not a slower clock: it can stop animation until
    /// an unrelated event arrives. Schedule the next battery frame explicitly.
    pub(crate) fn request_art_frame(&mut self) {
        match self.art_budget() {
            Budget::Full => self.dirty = true,
            // Count from the start of the frame, so heavier artwork does not
            // add its rendering time to the intended 30 Hz interval.
            Budget::Half => self.art_deadline = Some(self.art_frame_started + Duration::from_nanos(1_000_000_000 / 30)),
            Budget::Still => {},
        }
    }

    pub(crate) fn art_frame_wait(&self) -> Option<Duration> {
        self.art_deadline.map(|at| at.saturating_duration_since(crate::clock::now()))
    }
}

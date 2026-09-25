//! The platform side of Finish Work (finish_work.rs): one small backend per OS,
//! and nothing else of the OS leaks into the UI.
//!
//! - macOS: an IOKit PreventUserIdleSystemSleep assertion. The display may
//!   still sleep, and a MacBook still sleeps when its lid closes; there is
//!   no privileged helper, so nothing here can change that. The kernel
//!   drops the assertion if nus dies.
//! - Windows: a power request (PowerCreateRequest, SystemRequired), closed
//!   with its handle. The user's power plan, lid action included, is never
//!   touched.
//! - Linux: a logind inhibitor over D-Bus. The lease is the file descriptor
//!   logind hands back: open, the inhibitor holds; closed (dropped, or nus
//!   gone), it doesn't. Where the session may, the lid switch is inhibited
//!   too; otherwise only sleep and idle.
//!
//! Every lease is RAII: dropping it releases the OS's hold.

use crate::finish_work::{Capability, Lease, Platform, PowerState};
#[allow(unused_imports)]
use crate::finish_work::Thermal;

/// Is this a laptop (or another machine with its own battery)? Asked once
/// and cached; anything unclear counts as no, and the control stays hidden.
pub fn portable() -> bool {
    static PORTABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PORTABLE.get_or_init(|| {
        if let Some(v) = std::env::var_os("NUS_PORTABLE") {
            return v == "1";
        }
        detect_portable()
    })
}

pub fn native() -> Box<dyn Platform> {
    Box::new(Native::default())
}

// ── macOS ────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void};
    pub type CFTypeRef = *const c_void;
    pub type CFStringRef = *const c_void;
    pub type CFArrayRef = *const c_void;
    pub type CFDictionaryRef = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, encoding: u32) -> CFStringRef;
        pub fn CFRelease(value: CFTypeRef);
        pub fn CFArrayGetCount(array: CFArrayRef) -> isize;
        pub fn CFArrayGetValueAtIndex(array: CFArrayRef, index: isize) -> CFTypeRef;
        pub fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
        pub fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
        pub fn CFNumberGetValue(number: CFTypeRef, kind: isize, out: *mut c_void) -> u8;
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        pub fn IOPMAssertionCreateWithName(kind: CFStringRef, level: u32, name: CFStringRef, id: *mut u32) -> i32;
        pub fn IOPMAssertionRelease(id: u32) -> i32;
        pub fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        pub fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
        pub fn IOPSGetPowerSourceDescription(blob: CFTypeRef, source: CFTypeRef) -> CFDictionaryRef;
        pub fn IOPSGetProvidingPowerSourceType(blob: CFTypeRef) -> CFStringRef;
        pub fn IOPSGetBatteryWarningLevel() -> u32;
        pub fn IOPMGetThermalWarningLevel(level: *mut u32) -> i32;
    }

    const UTF8: u32 = 0x0800_0100;
    const SINT32: isize = 3;
    pub const LEVEL_ON: u32 = 255;
    /// kIOPSLowBatteryWarningFinal: the OS's own "critical".
    pub const WARNING_FINAL: u32 = 3;
    /// kIOPMThermalWarningLevelCrisis.
    pub const THERMAL_CRISIS: u32 = 10;
    /// kIOPMThermalWarningLevelDanger.
    pub const THERMAL_DANGER: u32 = 5;

    /// An owned CFString, released on drop.
    pub struct Cf(pub CFStringRef);
    impl Cf {
        pub fn new(s: &str) -> Cf {
            let c = std::ffi::CString::new(s.replace('\0', "")).unwrap_or_default();
            // SAFETY: a NUL-terminated UTF-8 buffer; the result is owned (Create rule).
            Cf(unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) })
        }
    }
    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: owned by this wrapper since creation.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    pub fn number(dict: CFDictionaryRef, key: &str) -> Option<i32> {
        let k = Cf::new(key);
        // SAFETY: `dict` is a live dictionary borrowed from its blob (Get
        // rule); the value is not retained past this call.
        unsafe {
            let v = CFDictionaryGetValue(dict, k.0);
            if v.is_null() {
                return None;
            }
            let mut out: i32 = 0;
            (CFNumberGetValue(v, SINT32, &mut out as *mut i32 as *mut c_void) != 0).then_some(out)
        }
    }

    pub fn is(dict_value: CFTypeRef, s: &str) -> bool {
        let k = Cf::new(s);
        // SAFETY: both are valid CF objects for the duration of the call.
        !dict_value.is_null() && unsafe { CFEqual(dict_value, k.0) } != 0
    }

    /// The internal battery: (present, percent), from the power-source list.
    /// Also answers whether the Mac is on battery right now.
    pub fn battery() -> (bool, Option<u8>, bool) {
        // SAFETY: the blob and list follow the Copy rule and are released
        // here; descriptions are borrowed from the blob while it lives.
        unsafe {
            let blob = IOPSCopyPowerSourcesInfo();
            if blob.is_null() {
                return (false, None, false);
            }
            let on_battery = is(IOPSGetProvidingPowerSourceType(blob), "Battery Power");
            let list = IOPSCopyPowerSourcesList(blob);
            let mut found = (false, None);
            if !list.is_null() {
                for i in 0..CFArrayGetCount(list) {
                    let desc = IOPSGetPowerSourceDescription(blob, CFArrayGetValueAtIndex(list, i));
                    if desc.is_null() {
                        continue;
                    }
                    let kind = Cf::new("Type");
                    if !is(CFDictionaryGetValue(desc, kind.0), "InternalBattery") {
                        continue;
                    }
                    let pct = match (number(desc, "Current Capacity"), number(desc, "Max Capacity")) {
                        (Some(c), Some(m)) if m > 0 => Some(((c as f32 / m as f32) * 100.0).round().clamp(0.0, 100.0) as u8),
                        _ => None,
                    };
                    found = (true, pct);
                    break;
                }
                CFRelease(list);
            }
            CFRelease(blob);
            (found.0, found.1, on_battery)
        }
    }
}

#[cfg(target_os = "macos")]
struct MacLease(u32);
#[cfg(target_os = "macos")]
impl Lease for MacLease {}
#[cfg(target_os = "macos")]
impl Drop for MacLease {
    fn drop(&mut self) {
        // SAFETY: an id this process created and hasn't released.
        unsafe { mac::IOPMAssertionRelease(self.0) };
    }
}

#[cfg(target_os = "macos")]
fn detect_portable() -> bool {
    mac::battery().0
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct Native;

#[cfg(target_os = "macos")]
impl Platform for Native {
    fn capability(&self) -> Capability {
        Capability::IdleSleep
    }

    fn acquire(&mut self, reason: &str) -> Result<Box<dyn Lease>, String> {
        // Prevents idle system sleep only; the display may sleep.
        let kind = mac::Cf::new("PreventUserIdleSystemSleep");
        let name = mac::Cf::new(reason);
        let mut id: u32 = 0;
        // SAFETY: two live CFStrings and an out pointer to a local.
        let rc = unsafe { mac::IOPMAssertionCreateWithName(kind.0, mac::LEVEL_ON, name.0, &mut id) };
        if rc == 0 { Ok(Box::new(MacLease(id))) } else { Err(format!("IOPMAssertionCreateWithName returned {rc:#x}")) }
    }

    fn power_state(&mut self) -> PowerState {
        let (_, battery, on_battery) = mac::battery();
        // SAFETY: plain value calls.
        let warning = unsafe { mac::IOPSGetBatteryWarningLevel() };
        let mut level: u32 = 0;
        let thermal = if unsafe { mac::IOPMGetThermalWarningLevel(&mut level) } == 0 {
            if level >= mac::THERMAL_CRISIS { Thermal::Critical } else if level >= mac::THERMAL_DANGER { Thermal::Elevated } else { Thermal::Nominal }
        } else {
            Thermal::Nominal
        };
        PowerState { on_battery, battery, critical: warning >= mac::WARNING_FINAL, thermal }
    }
}

// ── Windows ──────────────────────────────────────────────────────────────

#[cfg(windows)]
struct WinLease(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Lease for WinLease {}
#[cfg(windows)]
impl Drop for WinLease {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Power::{PowerClearRequest, PowerRequestSystemRequired};
        // SAFETY: a request handle this lease owns; cleared, then closed once.
        unsafe {
            PowerClearRequest(self.0, PowerRequestSystemRequired);
            CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn detect_portable() -> bool {
    use windows_sys::Win32::System::Power::{GetPwrCapabilities, SYSTEM_POWER_CAPABILITIES};
    // SAFETY: the struct is plain data, zeroed, filled by the call.
    unsafe {
        let mut caps: SYSTEM_POWER_CAPABILITIES = std::mem::zeroed();
        GetPwrCapabilities(&mut caps) && caps.SystemBatteriesPresent && caps.LidPresent && !caps.BatteriesAreShortTerm
    }
}

#[cfg(windows)]
#[derive(Default)]
struct Native;

#[cfg(windows)]
impl Platform for Native {
    fn capability(&self) -> Capability {
        Capability::IdleSleep
    }

    fn acquire(&mut self, reason: &str) -> Result<Box<dyn Lease>, String> {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Power::{PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest};
        use windows_sys::Win32::System::Threading::{POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0};
        let mut wide: Vec<u16> = reason.encode_utf16().chain(std::iter::once(0)).collect();
        let context = REASON_CONTEXT { Version: 0, Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING, Reason: REASON_CONTEXT_0 { SimpleReasonString: wide.as_mut_ptr() } };
        // SAFETY: the context and its string outlive the call (the OS copies
        // the reason); a valid handle is cleared and closed by the lease.
        unsafe {
            let handle = PowerCreateRequest(&context);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Err("PowerCreateRequest failed".into());
            }
            if PowerSetRequest(handle, PowerRequestSystemRequired) == 0 {
                CloseHandle(handle);
                return Err("PowerSetRequest failed".into());
            }
            Ok(Box::new(WinLease(handle)))
        }
    }

    fn power_state(&mut self) -> PowerState {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        // SAFETY: plain data, filled by the call.
        let status = unsafe {
            let mut s: SYSTEM_POWER_STATUS = std::mem::zeroed();
            (GetSystemPowerStatus(&mut s) != 0).then_some(s)
        };
        let Some(s) = status else { return PowerState::default() };
        let no_battery = s.BatteryFlag == 128 || s.BatteryFlag == 255;
        PowerState {
            on_battery: s.ACLineStatus == 0 && !no_battery,
            battery: (!no_battery && s.BatteryLifePercent <= 100).then_some(s.BatteryLifePercent),
            critical: !no_battery && s.BatteryFlag & 4 != 0,
            thermal: Thermal::Nominal,
        }
    }
}

// ── Linux ────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
struct LinuxLease(#[allow(dead_code)] zbus::zvariant::OwnedFd);
#[cfg(target_os = "linux")]
impl Lease for LinuxLease {}

#[cfg(target_os = "linux")]
fn supplies() -> Vec<std::path::PathBuf> {
    std::fs::read_dir("/sys/class/power_supply").map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn read(path: &std::path::Path, file: &str) -> String {
    std::fs::read_to_string(path.join(file)).map(|s| s.trim().to_string()).unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn system_battery(p: &std::path::Path) -> bool {
    read(p, "type") == "Battery" && read(p, "scope") != "Device"
}

#[cfg(target_os = "linux")]
fn detect_portable() -> bool {
    // SMBIOS chassis types: portable, laptop, notebook, sub-notebook,
    // convertible, detachable, tablet. A system battery also counts.
    let chassis = std::fs::read_to_string("/sys/class/dmi/id/chassis_type").ok().and_then(|s| s.trim().parse::<u32>().ok());
    matches!(chassis, Some(8 | 9 | 10 | 14 | 30 | 31 | 32)) || supplies().iter().any(|p| system_battery(p))
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct Native {
    /// Set once logind granted the lid switch too.
    lid: bool,
    /// logind unreachable: say so rather than pretend.
    missing: std::cell::Cell<Option<bool>>,
}

#[cfg(target_os = "linux")]
impl Native {
    fn inhibit(what: &str, reason: &str) -> Result<zbus::zvariant::OwnedFd, String> {
        let conn = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
        let reply = conn
            .call_method(Some("org.freedesktop.login1"), "/org/freedesktop/login1", Some("org.freedesktop.login1.Manager"), "Inhibit", &(what, "nus", reason, "block"))
            .map_err(|e| e.to_string())?;
        reply.body().deserialize::<zbus::zvariant::OwnedFd>().map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "linux")]
impl Platform for Native {
    fn capability(&self) -> Capability {
        if self.missing.get().is_none() {
            let up = zbus::blocking::Connection::system()
                .and_then(|c| c.call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "NameHasOwner", &("org.freedesktop.login1",)))
                .ok()
                .and_then(|m| m.body().deserialize::<bool>().ok())
                .unwrap_or(false);
            self.missing.set(Some(!up));
        }
        if self.missing.get() == Some(true) {
            Capability::None
        } else if self.lid {
            Capability::ClosedLidInhibitor
        } else {
            Capability::IdleSleep
        }
    }

    fn acquire(&mut self, reason: &str) -> Result<Box<dyn Lease>, String> {
        // The lid switch where the session is allowed it; otherwise sleep
        // and idle alone. Never an edit to logind.conf.
        match Self::inhibit("sleep:idle:handle-lid-switch", reason) {
            Ok(fd) => {
                self.lid = true;
                Ok(Box::new(LinuxLease(fd)))
            }
            Err(_) => {
                self.lid = false;
                Self::inhibit("sleep:idle", reason).map(|fd| Box::new(LinuxLease(fd)) as Box<dyn Lease>)
            }
        }
    }

    fn power_state(&mut self) -> PowerState {
        let all = supplies();
        let batteries: Vec<_> = all.iter().filter(|p| system_battery(p)).collect();
        let mains = all.iter().any(|p| matches!(read(p, "type").as_str(), "Mains" | "USB") && read(p, "online") == "1");
        let discharging = batteries.iter().any(|p| read(p, "status") == "Discharging");
        let battery = batteries.iter().filter_map(|p| read(p, "capacity").parse::<u8>().ok()).min();
        PowerState {
            on_battery: !batteries.is_empty() && (discharging || !mains),
            battery,
            critical: batteries.iter().any(|p| read(p, "capacity_level") == "Critical"),
            thermal: Thermal::Nominal,
        }
    }
}

// ── Elsewhere ────────────────────────────────────────────────────────────

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn detect_portable() -> bool {
    false
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
#[derive(Default)]
struct Native;

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
impl Platform for Native {
    fn capability(&self) -> Capability {
        Capability::None
    }
    fn acquire(&mut self, _reason: &str) -> Result<Box<dyn Lease>, String> {
        Err("unsupported".into())
    }
    fn power_state(&mut self) -> PowerState {
        PowerState::default()
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// The assertion the OS lists while a lease lives, and doesn't after
    /// it drops (pmset -g assertions is the diagnostic).
    #[test]
    fn a_mac_lease_is_visible_to_the_os_and_gone_when_dropped() {
        let reason = format!("nus finish-work test {}", std::process::id());
        let listed = || {
            let out = std::process::Command::new("pmset").args(["-g", "assertions"]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
            out.contains(&reason)
        };
        let mut native = Native;
        let lease = native.acquire(&reason).expect("assertion");
        assert!(listed(), "assertion not listed by pmset");
        drop(lease);
        assert!(!listed(), "assertion survived its lease");
    }

    #[test]
    fn power_state_reads_without_panicking() {
        let s = Native.power_state();
        if let Some(b) = s.battery {
            assert!(b <= 100);
        }
    }
}

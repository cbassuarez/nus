//! OS pressure, not virtual address-space size. Reclaim disposable caches;
//! never close tabs, terminate processes, or discard user buffers here.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum Level { Normal, Warning, Critical }

#[cfg(target_os = "macos")]
pub fn system_level() -> Option<Level> {
    let mut value: libc::c_int = 0;
    let mut len = std::mem::size_of_val(&value);
    let ok = unsafe { libc::sysctlbyname(c"kern.memorystatus_vm_pressure_level".as_ptr(), (&mut value as *mut libc::c_int).cast(), &mut len, std::ptr::null_mut(), 0) };
    if ok != 0 { return None; }
    match value { 1 => Some(Level::Normal), 2 => Some(Level::Warning), 4 => Some(Level::Critical), _ => None }
}

#[cfg(windows)]
pub fn system_level() -> Option<Level> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of_val(&status) as u32;
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 { return None; }
    available_level(status.ullAvailPhys, status.ullTotalPhys)
}

#[cfg(target_os = "linux")]
pub fn system_level() -> Option<Level> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let value = |name| text.lines().find_map(|l| l.strip_prefix(name).and_then(|s| s.split_whitespace().next()?.parse::<u64>().ok()));
    available_level(value("MemAvailable:")?, value("MemTotal:")?)
}

#[cfg(any(windows, target_os = "linux", test))]
fn available_level(available: u64, total: u64) -> Option<Level> {
    if total == 0 { return None; }
    let ratio = available as f64 / total as f64;
    Some(if ratio < 0.05 { Level::Critical } else if ratio < 0.10 { Level::Warning } else { Level::Normal })
}

#[derive(Default)]
struct Monitor { sampled: Option<Instant>, reclaimed: Option<Instant>, last: Option<Level> }
impl Monitor {
    fn accept(&mut self, now: Instant, level: Option<Level>) -> Option<Level> {
        let previous = self.last;
        self.last = level;
        let level = level?;
        if level == Level::Normal { return None; }
        // Escalation is immediate; sustained/flapping pressure is rate limited.
        if previous == Some(Level::Warning) && level == Level::Critical
            || self.reclaimed.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(30)) {
            self.reclaimed = Some(now);
            Some(level)
        } else { None }
    }
}
thread_local! { static MONITOR: std::cell::RefCell<Monitor> = Default::default(); }
pub fn poll() -> Option<Level> {
    MONITOR.with(|m| {
        let mut m = m.borrow_mut();
        let now = Instant::now();
        if m.sampled.is_some_and(|t| now.duration_since(t) < Duration::from_secs(5)) { return None; }
        m.sampled = Some(now);
        m.accept(now, system_level())
    })
}

#[cfg(target_os = "macos")]
pub fn footprint_kib(pid: u32) -> Option<u64> {
    let mut usage: libc::rusage_info_v0 = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V0, (&mut usage as *mut libc::rusage_info_v0).cast()) };
    (ok == 0).then_some(usage.ri_phys_footprint / 1024)
}
#[cfg(not(target_os = "macos"))]
pub fn footprint_kib(_pid: u32) -> Option<u64> { None }

impl crate::app::App {
    pub(crate) fn reclaim_memory(&mut self, level: Level) {
        self.fonts.reclaim_caches();
        self.icon_previews.clear();
        self.pic_icons.clear();
        self.gpu.flush_uploads();
        let notification = match level { Level::Normal => return, Level::Warning => "moderate", Level::Critical => "critical" };
        for tab in &self.tabs {
            for pane in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let crate::app::Pane::Web(w) = pane {
                    w.tab.devtools("Memory.simulatePressureNotification", serde_json::json!({"level":notification}));
                }
            }
        }
        if let Some(little) = &self.little {
            little.pane.tab.devtools("Memory.simulatePressureNotification", serde_json::json!({"level":notification}));
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_metrics_and_pressure_flapping_do_not_thrash() {
        assert_eq!(available_level(0, 0), None);
        assert_eq!(available_level(4, 100), Some(Level::Critical));
        assert_eq!(available_level(9, 100), Some(Level::Warning));
        assert_eq!(available_level(10, 100), Some(Level::Normal));
        let mut m = Monitor::default(); let t = Instant::now();
        assert_eq!(m.accept(t, None), None);
        assert_eq!(m.accept(t, Some(Level::Warning)), Some(Level::Warning));
        assert_eq!(m.accept(t, Some(Level::Warning)), None);
        assert_eq!(m.accept(t, Some(Level::Critical)), Some(Level::Critical));
        assert_eq!(m.accept(t, Some(Level::Normal)), None);
        assert_eq!(m.accept(t, Some(Level::Critical)), None);
        assert_eq!(m.accept(t + Duration::from_secs(30), Some(Level::Critical)), Some(Level::Critical));
    }
}

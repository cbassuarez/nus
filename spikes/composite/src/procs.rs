//! What the kernel is doing: every process with its parent, its share of
//! the processor over the last second, its working set and its threads,
//! plus the kernel's own counters — context switches and system calls per
//! second, threads and handles in all. Sampled once a second on a thread
//! of its own (opening two hundred processes takes a few milliseconds);
//! the brain reads the last sample. Windows reads the kernel directly;
//! elsewhere `ps` is asked once a second (no kernel counters there yet).

use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    /// Percent of one processor over the last second (0..100 × cores).
    pub cpu: f32,
    /// Working set, MB.
    pub mem: f32,
    pub threads: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub procs: Vec<Proc>,
    pub ctx_per_s: f64,
    pub syscalls_per_s: f64,
    pub threads: u32,
    pub handles: u32,
    pub taken: Option<std::time::Instant>,
}

pub type Shared = Arc<Mutex<Sample>>;

/// Start sampling; the handle is what the art reads.
pub fn start() -> Shared {
    let shared: Shared = Arc::new(Mutex::new(Sample::default()));
    let out = shared.clone();
    std::thread::Builder::new()
        .name("nus-procs".into())
        .spawn(move || {
            let mut last: Option<(std::collections::HashMap<u32, u64>, u64, u64, std::time::Instant)> = None;
            loop {
                let (procs, cpu_times, ctx, sys, threads, handles) = sample();
                let now = std::time::Instant::now();
                let mut out_procs = procs;
                let (mut ctx_per_s, mut sys_per_s) = (0.0, 0.0);
                if let Some((prev, pctx, psys, pat)) = &last {
                    let secs = now.duration_since(*pat).as_secs_f64().max(0.05);
                    for p in out_procs.iter_mut() {
                        let now_t = cpu_times.get(&p.pid).copied().unwrap_or(0);
                        let then = prev.get(&p.pid).copied().unwrap_or(now_t);
                        // 100 ns units of processor time over wall seconds.
                        p.cpu = ((now_t.saturating_sub(then)) as f64 / 1e7 / secs * 100.0) as f32;
                    }
                    if ctx >= *pctx && sys >= *psys {
                        ctx_per_s = (ctx - pctx) as f64 / secs;
                        sys_per_s = (sys - psys) as f64 / secs;
                    }
                }
                last = Some((cpu_times, ctx, sys, now));
                if let Ok(mut s) = shared.lock() {
                    *s = Sample { procs: out_procs, ctx_per_s, syscalls_per_s: sys_per_s, threads, handles, taken: Some(now) };
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
        })
        .ok();
    out
}

#[cfg(windows)]
fn sample() -> (Vec<Proc>, std::collections::HashMap<u32, u64>, u64, u64, u32, u32) {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{GetProcessHandleCount, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let mut procs = Vec::new();
    let mut times = std::collections::HashMap::new();
    let (mut threads, mut handles) = (0u32, 0u32);
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap != INVALID_HANDLE_VALUE {
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e) != 0;
            while ok {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                let name = name.strip_suffix(".exe").map(str::to_string).unwrap_or(name);
                let mut p = Proc { pid: e.th32ProcessID, ppid: e.th32ParentProcessID, name, cpu: 0.0, mem: 0.0, threads: e.cntThreads };
                threads = threads.saturating_add(e.cntThreads);
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
                if !h.is_null() {
                    let mut c: FILETIME = std::mem::zeroed();
                    let mut x: FILETIME = std::mem::zeroed();
                    let mut k: FILETIME = std::mem::zeroed();
                    let mut u: FILETIME = std::mem::zeroed();
                    if GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u) != 0 {
                        let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
                        times.insert(p.pid, ft(k) + ft(u));
                    }
                    let mut mc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
                    mc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                    if K32GetProcessMemoryInfo(h, &mut mc, mc.cb) != 0 {
                        p.mem = mc.WorkingSetSize as f32 / (1024.0 * 1024.0);
                    }
                    let mut hc = 0u32;
                    if GetProcessHandleCount(h, &mut hc) != 0 {
                        handles = handles.saturating_add(hc);
                    }
                    CloseHandle(h);
                }
                procs.push(p);
                ok = Process32NextW(snap, &mut e) != 0;
            }
            CloseHandle(snap);
        }
    }
    let (ctx, sys) = kernel_counters();
    (procs, times, ctx, sys, threads, handles)
}

/// Context switches and system calls since boot, from the kernel's
/// performance block (SystemPerformanceInformation, the layout Process
/// Hacker documents: ContextSwitches at 296, SystemCalls at 308).
#[cfg(windows)]
fn kernel_counters() -> (u64, u64) {
    use windows_sys::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemPerformanceInformation};
    let mut buf = [0u8; 512];
    let mut len = 0u32;
    let status = unsafe { NtQuerySystemInformation(SystemPerformanceInformation, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut len) };
    if status != 0 || (len as usize) < 312 {
        return (0, 0);
    }
    let u32_at = |o: usize| u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]) as u64;
    (u32_at(296), u32_at(308))
}

/// Elsewhere: `ps`, which every Unix has. Processor time comes back as
/// seconds; it is kept in the same 100 ns units the Windows path uses.
#[cfg(not(windows))]
fn sample() -> (Vec<Proc>, std::collections::HashMap<u32, u64>, u64, u64, u32, u32) {
    let mut procs = Vec::new();
    let mut times = std::collections::HashMap::new();
    let mut threads_all = 0u32;
    let out = std::process::Command::new("ps").args(["-eo", "pid=,ppid=,time=,rss=,nlwp=,comm="]).output();
    let Ok(out) = out else { return (procs, times, 0, 0, 0, 0) };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut it = line.split_whitespace();
        let (Some(pid), Some(ppid), Some(time), Some(rss)) = (it.next(), it.next(), it.next(), it.next()) else { continue };
        // nlwp is Linux's; macOS's ps has no thread count and the column reads as the name.
        let rest: Vec<&str> = it.collect();
        let (threads, name) = match rest.first().and_then(|t| t.parse::<u32>().ok()) {
            Some(n) if rest.len() > 1 => (n, rest[1..].join(" ")),
            _ => (1, rest.join(" ")),
        };
        let (Ok(pid), Ok(ppid), Ok(rss)) = (pid.parse::<u32>(), ppid.parse::<u32>(), rss.parse::<f32>()) else { continue };
        // time is [[dd-]hh:]mm:ss
        let secs: u64 = {
            let (days, clock) = match time.split_once('-') { Some((d, c)) => (d.parse::<u64>().unwrap_or(0), c), None => (0, time) };
            let parts: Vec<u64> = clock.split(':').map(|p| p.parse::<u64>().unwrap_or(0)).collect();
            let hms = match parts.len() { 3 => parts[0] * 3600 + parts[1] * 60 + parts[2], 2 => parts[0] * 60 + parts[1], _ => 0 };
            days * 86400 + hms
        };
        times.insert(pid, secs * 10_000_000);
        threads_all += threads;
        let name = name.rsplit('/').next().unwrap_or(&name).to_string();
        procs.push(Proc { pid, ppid, name, cpu: 0.0, mem: rss / 1024.0, threads });
    }
    (procs, times, 0, 0, threads_all, 0)
}

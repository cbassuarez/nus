//! Opt-in measurements with bounded in-memory samples. No files or recurring
//! log output. These are CPU/event-loop timings, not display scanout timings.
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    sync::OnceLock,
    time::Instant,
};

#[derive(Default)]
struct Samples {
    count: u64,
    over_16ms: u64,
    worst: f64,
    recent: VecDeque<f64>,
}
impl Samples {
    fn add(&mut self, ms: f64) {
        self.count += 1;
        self.over_16ms += u64::from(ms > 1000.0 / 60.0);
        self.worst = self.worst.max(ms);
        if self.recent.len() == 2048 {
            self.recent.pop_front();
        }
        self.recent.push_back(ms);
    }
    fn json(&self) -> serde_json::Value {
        let mut v: Vec<f64> = self.recent.iter().copied().collect();
        v.sort_by(f64::total_cmp);
        let p = |n: usize| {
            v.get((v.len() * n).div_ceil(100).saturating_sub(1))
                .copied()
                .unwrap_or(0.0)
        };
        serde_json::json!({"count":self.count,"p50_ms":p(50),"p95_ms":p(95),"p99_ms":p(99),"max_ms":self.worst,"over_16_67_ms":self.over_16ms})
    }
}
thread_local! {
    static DATA: RefCell<BTreeMap<&'static str, Samples>> = RefCell::new(BTreeMap::new());
    static LAUNCH: RefCell<Option<Instant>> = const { RefCell::new(None) };
}
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NUS_PERF").is_some())
}
pub fn start() {
    if enabled() {
        LAUNCH.with(|t| *t.borrow_mut() = Some(Instant::now()));
    }
}
pub fn first_frame() {
    if enabled() {
        LAUNCH.with(|t| {
            if let Some(at) = t.borrow_mut().take() {
                record("main_to_first_submit", at.elapsed().as_secs_f64() * 1000.0);
            }
        });
    }
}
pub fn record(name: &'static str, ms: f64) {
    if enabled() {
        DATA.with(|d| d.borrow_mut().entry(name).or_default().add(ms));
    }
}
pub struct Scope(&'static str, Option<Instant>);
pub fn scope(name: &'static str) -> Scope {
    Scope(name, enabled().then(Instant::now))
}
impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(at) = self.1 {
            record(self.0, at.elapsed().as_secs_f64() * 1000.0);
        }
    }
}
pub fn snapshot() -> serde_json::Value {
    DATA.with(|d| {
        d.borrow()
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.json()))
            .collect::<serde_json::Map<_, _>>()
            .into()
    })
}
pub fn reset() {
    DATA.with(|d| d.borrow_mut().clear());
}

/// Explicit test-time sample only. RSS includes shared resident pages in each
/// process, so the tree sum is an upper estimate, not physical/private memory.
pub fn memory_snapshot() -> serde_json::Value {
    let Ok(child) = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,rss="])
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        return serde_json::json!({"available":false});
    };
    let sampler = child.id();
    let Ok(out) = child.wait_with_output() else {
        return serde_json::json!({"available":false});
    };
    let rows: Vec<(u32, u32, u64)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut v = l.split_whitespace();
            Some((
                v.next()?.parse().ok()?,
                v.next()?.parse().ok()?,
                v.next()?.parse().ok()?,
            ))
        })
        .collect();
    let pid = std::process::id();
    let mut children = std::collections::HashSet::from([pid]);
    loop {
        let before = children.len();
        for &(child, parent, _) in &rows {
            if child != sampler && children.contains(&parent) {
                children.insert(child);
            }
        }
        if children.len() == before {
            break;
        }
    }
    // The ps sampler itself is a direct child; its RSS is excluded below.
    let main = rows.iter().find(|r| r.0 == pid).map(|r| r.2);
    let tree: u64 = rows
        .iter()
        .filter(|r| children.contains(&r.0))
        .map(|r| r.2)
        .sum();
    serde_json::json!({"available":main.is_some(),"main_rss_kib":main,"tree_rss_kib":tree,"processes":children.len(),"method":"sum RSS, includes shared pages"})
}

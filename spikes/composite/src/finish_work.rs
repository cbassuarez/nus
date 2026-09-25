//! Finish Work: "I explicitly started this work. Let it finish, then give
//! power management back to the operating system."
//!
//! It is not a caffeine toggle. Four layers, each with one job:
//!
//! ```text
//! user intent   the coffee icon in the header (app.rs), clicked
//!     ↓
//! registry      which work is protected: user-started shell commands and
//!               downloads, as ids, joined while engaged, gone when done
//!     ↓
//! policy        FinishWork below: one lease while anything is protected,
//!               none otherwise; battery and thermal safety always win
//!     ↓
//! platform      finish_work_native.rs: an IOKit assertion, a Windows power
//!               request, a logind inhibitor. Dropping the lease releases it.
//! ```
//!
//! # Trust boundaries
//!
//! Keeping the machine awake is a capability, and it only ever starts from
//! the user's own hand:
//!
//! - A shell command counts when the user pressed Enter at that prompt, or
//!   clicked something in nus that typed it (Home, Run again, a saved
//!   command). A rule, an assistant or a restored session re-running
//!   something does not, and neither does a shell without shell
//!   integration: the unit of work is the command generation the shell
//!   reports (OSC 133), never a PID or CPU use.
//! - A download counts when nus saw the user's own input in that page just
//!   before it began, the navigation carried Chromium's user gesture, or
//!   nus itself started it for the user. Timers, service workers, sockets
//!   and background requests never count. Pages get no API for any of this.
//! - Nothing is persisted: a restart never brings a lease back.
//!
//! Nothing here reaches past the OS's own protections. The display may
//! sleep; the OS may still sleep for a closed lid (where its policy says
//! so), a critical battery, heat, or an explicit shutdown.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Work, by what nus already calls it: a shell command's generation, a
/// download's key. Ids are never reused within a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkId {
    Shell(u64),
    Download(u64),
}

/// How the user started it. Work without an origin was not started by the
/// user and can never be protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Enter, pressed at the prompt.
    ShellInput,
    /// Native input into the page, or a navigation Chromium marked as a
    /// user gesture.
    BrowserGesture,
    /// Something the user did in nus itself: Home, Run again, a saved
    /// command, Save image.
    NusAction,
}

/// One piece of live work, as the app sees it this turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Work {
    pub id: WorkId,
    pub origin: Option<Origin>,
}

impl Work {
    /// Whether this work may hold a lease: only what the user started.
    pub fn eligible(&self) -> bool {
        self.origin.is_some()
    }
}

static NEXT_COMMAND: AtomicU64 = AtomicU64::new(1);

/// A fresh command generation, for a shell command that just started.
pub fn next_command() -> u64 {
    NEXT_COMMAND.fetch_add(1, Ordering::Relaxed)
}

/// What the platform can promise, said as it is in the tooltip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// No way to hold the machine awake here.
    None,
    /// Idle sleep is held off; closing the lid follows the system's policy.
    IdleSleep,
    /// Idle sleep and the lid switch are both held (logind, Linux only).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    ClosedLidInhibitor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thermal {
    Nominal,
    Elevated,
    Critical,
}

/// The power situation, as the platform reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerState {
    pub on_battery: bool,
    /// Charge in percent, when there is a battery to ask.
    pub battery: Option<u8>,
    /// The OS says the battery is critical.
    pub critical: bool,
    pub thermal: Thermal,
}

impl Default for PowerState {
    fn default() -> Self {
        PowerState { on_battery: false, battery: None, critical: false, thermal: Thermal::Nominal }
    }
}

/// At or below this, on battery, Finish Work lets go and won't start.
pub const BATTERY_FLOOR: u8 = 10;

/// How often the power state is asked for while holding.
const POLL: Duration = Duration::from_secs(5);

/// How long a safety stop stays on the icon.
const STOP_SHOWN: Duration = Duration::from_secs(12);

/// A held wake assertion. Dropping it releases it.
pub trait Lease {}

/// The platform's power controls (finish_work_native.rs; a fake in the tests).
pub trait Platform {
    fn capability(&self) -> Capability;
    fn acquire(&mut self, reason: &str) -> Result<Box<dyn Lease>, String>;
    fn power_state(&mut self) -> PowerState;
}

/// Why Finish Work let go on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Battery(u8),
    CriticalBattery,
    Thermal,
    /// The platform refused the assertion.
    Unavailable,
}

/// Why Finish Work would not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NothingToFinish,
    Battery(u8),
    CriticalBattery,
    Thermal,
    Unavailable,
}

/// What changed, for a notice. Reference-count changes say nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Engaged(usize),
    Complete,
    TurnedOff,
    Stopped(Stop),
    Refused(Refusal),
}

/// What the header shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nothing eligible, or nothing the platform can do.
    Unavailable,
    /// Eligible work exists: a click protects it.
    Ready(usize),
    /// Holding for this many tasks.
    Holding(usize),
    /// Let go for safety, a moment ago.
    SafetyReleased(Stop),
}

fn safety(p: &PowerState) -> Option<Stop> {
    if p.thermal == Thermal::Critical {
        return Some(Stop::Thermal);
    }
    if p.on_battery && p.critical {
        return Some(Stop::CriticalBattery);
    }
    match p.battery {
        Some(b) if p.on_battery && b <= BATTERY_FLOOR => Some(Stop::Battery(b)),
        _ => None,
    }
}

/// The reason the OS shows for the assertion (pmset -g assertions, etc.).
pub fn reason(tasks: usize) -> String {
    match tasks {
        1 => "nus is finishing 1 user-started task".into(),
        n => format!("nus is finishing {n} user-started tasks"),
    }
}

pub struct FinishWork {
    platform: Box<dyn Platform>,
    engaged: bool,
    held: BTreeSet<WorkId>,
    lease: Option<Box<dyn Lease>>,
    stopped: Option<(Stop, Instant)>,
    power: PowerState,
    polled: Option<Instant>,
}

impl FinishWork {
    pub fn new(platform: Box<dyn Platform>) -> Self {
        FinishWork { platform, engaged: false, held: BTreeSet::new(), lease: None, stopped: None, power: PowerState::default(), polled: None }
    }

    pub fn capability(&self) -> Capability {
        self.platform.capability()
    }

    #[cfg(test)]
    pub fn engaged(&self) -> bool {
        self.engaged
    }

    #[cfg(test)]
    pub fn held(&self) -> usize {
        self.held.len()
    }

    #[cfg(test)]
    pub fn holding_lease(&self) -> bool {
        self.lease.is_some()
    }

    fn poll(&mut self, now: Instant) {
        self.power = self.platform.power_state();
        self.polled = Some(now);
    }

    fn release(&mut self) {
        self.lease = None;
        self.held.clear();
        self.engaged = false;
    }

    /// The click: protect the eligible work that exists now.
    pub fn engage(&mut self, live: &[Work], now: Instant) -> Event {
        if self.engaged {
            return Event::Engaged(self.held.len());
        }
        if self.platform.capability() == Capability::None {
            return Event::Refused(Refusal::Unavailable);
        }
        self.poll(now);
        match safety(&self.power) {
            Some(Stop::Thermal) => return Event::Refused(Refusal::Thermal),
            Some(Stop::CriticalBattery) => return Event::Refused(Refusal::CriticalBattery),
            Some(Stop::Battery(b)) => return Event::Refused(Refusal::Battery(b)),
            _ => {}
        }
        let eligible: BTreeSet<WorkId> = live.iter().filter(|w| w.eligible()).map(|w| w.id).collect();
        if eligible.is_empty() {
            return Event::Refused(Refusal::NothingToFinish);
        }
        match self.platform.acquire(&reason(eligible.len())) {
            Ok(lease) => {
                self.lease = Some(lease);
                self.held = eligible;
                self.engaged = true;
                self.stopped = None;
                Event::Engaged(self.held.len())
            }
            Err(e) => {
                tracing::warn!("Finish Work: the platform refused the assertion: {e}");
                Event::Refused(Refusal::Unavailable)
            }
        }
    }

    /// The click again, while holding: give it back now.
    pub fn disengage(&mut self) -> Event {
        let was = self.engaged;
        self.release();
        if was { Event::TurnedOff } else { Event::Complete }
    }

    pub fn toggle(&mut self, live: &[Work], now: Instant) -> Event {
        if self.engaged { self.disengage() } else { self.engage(live, now) }
    }

    /// Once a turn, with the work that is live right now. Completed and
    /// vanished work leaves; new user-started work joins; safety wins.
    pub fn sync(&mut self, live: &[Work], now: Instant) -> Option<Event> {
        if !self.engaged {
            return None;
        }
        if self.polled.is_none_or(|t| now.saturating_duration_since(t) >= POLL) {
            self.poll(now);
        }
        if let Some(stop) = safety(&self.power) {
            self.release();
            self.stopped = Some((stop, now));
            return Some(Event::Stopped(stop));
        }
        let eligible: BTreeSet<WorkId> = live.iter().filter(|w| w.eligible()).map(|w| w.id).collect();
        // Leave: finished, cancelled, gone with its pane or its window.
        self.held.retain(|id| eligible.contains(id));
        // Join: user-started work that began while holding.
        self.held.extend(eligible);
        if self.held.is_empty() {
            self.release();
            return Some(Event::Complete);
        }
        if self.lease.is_none() {
            match self.platform.acquire(&reason(self.held.len())) {
                Ok(lease) => self.lease = Some(lease),
                Err(_) => {
                    self.release();
                    self.stopped = Some((Stop::Unavailable, now));
                    return Some(Event::Stopped(Stop::Unavailable));
                }
            }
        }
        None
    }

    /// Quitting, or the OS asking to: let go, and say nothing.
    pub fn shutdown(&mut self) {
        self.release();
    }

    pub fn phase(&self, live: &[Work], now: Instant) -> Phase {
        if self.engaged {
            return Phase::Holding(self.held.len());
        }
        if let Some((stop, at)) = self.stopped {
            if now.saturating_duration_since(at) < STOP_SHOWN {
                return Phase::SafetyReleased(stop);
            }
        }
        let n = live.iter().filter(|w| w.eligible()).count();
        if n == 0 || self.platform.capability() == Capability::None {
            Phase::Unavailable
        } else {
            Phase::Ready(n)
        }
    }
}

impl Drop for FinishWork {
    fn drop(&mut self) {
        self.release();
    }
}

// ── Across windows ───────────────────────────────────────────────────────
//
// The lease is the process's, not a window's: every window's shells and
// the process-wide downloads feed one FinishWork, owned by the host
// (main.rs). A window asks by setting a flag; the host decides on its next
// turn and publishes what the header should show.

static TOGGLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The coffee, clicked: the host toggles on its next turn.
pub fn request_toggle() {
    TOGGLE.store(true, Ordering::Relaxed);
}

fn take_toggle() -> bool {
    TOGGLE.swap(false, Ordering::Relaxed)
}

/// What every header draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub phase: Phase,
    pub capability: Capability,
}

static VIEW: std::sync::Mutex<View> = std::sync::Mutex::new(View { phase: Phase::Unavailable, capability: Capability::None });

pub fn view() -> View {
    *VIEW.lock().unwrap_or_else(|e| e.into_inner())
}

fn publish(v: View) -> bool {
    let mut g = VIEW.lock().unwrap_or_else(|e| e.into_inner());
    let changed = *g != v;
    *g = v;
    changed
}

/// Whether the header shows the control at all: laptops, regular windows.
pub fn offered() -> bool {
    crate::finish_work_native::portable() && !crate::private::enabled()
}

impl crate::Host {
    /// Once a turn: gather the live work of every window and the downloads,
    /// answer a click, keep the lease honest, and tell the headers.
    pub(crate) fn refresh_finish_work(&mut self) {
        if !offered() {
            return;
        }
        let mut live: Vec<Work> = self.apps.iter().flat_map(|a| a.finish_work_items()).collect();
        live.extend(crate::downloads::finish_work_items());
        let now = Instant::now();
        let mut events: Vec<Event> = Vec::new();
        if take_toggle() {
            events.push(self.finish.toggle(&live, now));
        }
        if let Some(e) = self.finish.sync(&live, now) {
            events.push(e);
        }
        let view = View { phase: self.finish.phase(&live, now), capability: self.finish.capability() };
        if publish(view) {
            for a in self.apps.iter_mut() {
                a.dirty = true;
            }
        }
        if events.is_empty() {
            return;
        }
        let target = self.focused.and_then(|id| self.apps.iter().position(|a| a.window.id() == id)).unwrap_or(0);
        if let Some(a) = self.apps.get_mut(target) {
            for e in events {
                if let Some((words, detail)) = notice(e) {
                    a.notice(nus_render::text::icons::COFFEE, words, detail);
                }
            }
        }
    }

    /// Quitting: let go before anything else does.
    pub(crate) fn release_finish_work(&mut self) {
        self.finish.shutdown();
        publish(View { phase: Phase::Unavailable, capability: self.finish.capability() });
    }
}

/// The header's words, true to what the platform can promise.
pub fn tooltip(phase: Phase, capability: Capability) -> String {
    let lid = match capability {
        Capability::IdleSleep => "Prevents idle sleep. Closing the lid still follows system power settings.",
        Capability::ClosedLidInhibitor => "Keeps these tasks running with the lid closed.",
        Capability::None => "Keeping the computer awake isn't available on this system.",
    };
    let tasks = |n: usize| if n == 1 { "1 task".to_string() } else { format!("{n} tasks") };
    match phase {
        Phase::Unavailable if capability == Capability::None => format!("Finish Work. {lid}"),
        Phase::Unavailable => "Finish Work. Keep user-started work running while you're away. Start a command or a download first.".into(),
        Phase::Ready(n) => format!("Finish Work · {}. Keep them running while you're away, then let the computer sleep. {lid}", tasks(n)),
        Phase::Holding(n) => format!("Finish Work · {}. Keeping the computer awake until they finish. {lid} Click to stop.", tasks(n)),
        Phase::SafetyReleased(stop) => stop_words(stop),
    }
}

pub fn stop_words(stop: Stop) -> String {
    match stop {
        Stop::Battery(b) => format!("Finish Work stopped · battery {b}%"),
        Stop::CriticalBattery => "Finish Work stopped · battery critical".into(),
        Stop::Thermal => "Finish Work stopped · thermal safety".into(),
        Stop::Unavailable => "Finish Work stopped · the system refused to stay awake".into(),
    }
}

/// A notice for an event, as the toast's words and detail.
pub fn notice(event: Event) -> Option<(&'static str, String)> {
    let tasks = |n: usize| if n == 1 { "1 task".to_string() } else { format!("{n} tasks") };
    let why = |stop: Stop| stop_words(stop).split_once(" · ").map(|(_, d)| d.to_string()).unwrap_or_default();
    match event {
        Event::Engaged(n) => Some(("Finish Work On", tasks(n))),
        Event::Complete => Some(("Finish Work Complete", String::new())),
        Event::TurnedOff => Some(("Finish Work Off", String::new())),
        Event::Stopped(stop) => Some(("Finish Work Stopped", why(stop))),
        Event::Refused(Refusal::NothingToFinish) => Some(("Nothing To Finish", "start a command or a download first".into())),
        Event::Refused(Refusal::Battery(b)) => Some(("Finish Work Can't Start", format!("battery {b}%"))),
        Event::Refused(Refusal::CriticalBattery) => Some(("Finish Work Can't Start", "battery critical".into())),
        Event::Refused(Refusal::Thermal) => Some(("Finish Work Can't Start", "thermal safety".into())),
        Event::Refused(Refusal::Unavailable) => Some(("Finish Work Unavailable", "not on this system".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[derive(Default)]
    struct Counts {
        acquired: Cell<u32>,
        released: Cell<u32>,
        reasons: RefCell<Vec<String>>,
    }

    struct FakeLease(Rc<Counts>);
    impl Lease for FakeLease {}
    impl Drop for FakeLease {
        fn drop(&mut self) {
            self.0.released.set(self.0.released.get() + 1);
        }
    }

    struct Fake {
        counts: Rc<Counts>,
        power: Rc<Cell<PowerState>>,
        capability: Capability,
        refuse: bool,
    }
    impl Platform for Fake {
        fn capability(&self) -> Capability {
            self.capability
        }
        fn acquire(&mut self, reason: &str) -> Result<Box<dyn Lease>, String> {
            if self.refuse {
                return Err("no".into());
            }
            self.counts.acquired.set(self.counts.acquired.get() + 1);
            self.counts.reasons.borrow_mut().push(reason.to_string());
            Ok(Box::new(FakeLease(self.counts.clone())))
        }
        fn power_state(&mut self) -> PowerState {
            self.power.get()
        }
    }

    fn setup() -> (FinishWork, Rc<Counts>, Rc<Cell<PowerState>>) {
        let counts = Rc::new(Counts::default());
        let power = Rc::new(Cell::new(PowerState::default()));
        let fw = FinishWork::new(Box::new(Fake { counts: counts.clone(), power: power.clone(), capability: Capability::IdleSleep, refuse: false }));
        (fw, counts, power)
    }

    fn shell(n: u64) -> Work {
        Work { id: WorkId::Shell(n), origin: Some(Origin::ShellInput) }
    }
    fn user_download(n: u64) -> Work {
        Work { id: WorkId::Download(n), origin: Some(Origin::BrowserGesture) }
    }
    fn background_download(n: u64) -> Work {
        Work { id: WorkId::Download(n), origin: None }
    }
    fn battery(pct: u8) -> PowerState {
        PowerState { on_battery: true, battery: Some(pct), ..Default::default() }
    }

    #[test]
    fn zero_to_one_job_acquires_once() {
        let (mut fw, c, _) = setup();
        let now = Instant::now();
        assert_eq!(fw.engage(&[shell(1)], now), Event::Engaged(1));
        assert_eq!(c.acquired.get(), 1);
        assert!(fw.holding_lease());
        assert_eq!(c.reasons.borrow()[0], "nus is finishing 1 user-started task");
    }

    #[test]
    fn one_to_two_does_not_acquire_again_and_two_to_one_keeps_it() {
        let (mut fw, c, _) = setup();
        let now = Instant::now();
        fw.engage(&[shell(1)], now);
        assert_eq!(fw.sync(&[shell(1), shell(2)], now), None);
        assert_eq!(fw.held(), 2);
        assert_eq!(c.acquired.get(), 1);
        assert_eq!(fw.sync(&[shell(2)], now), None);
        assert_eq!(fw.held(), 1);
        assert_eq!(c.released.get(), 0);
        assert!(fw.holding_lease());
    }

    #[test]
    fn one_to_zero_releases_and_turns_off() {
        let (mut fw, c, _) = setup();
        let now = Instant::now();
        fw.engage(&[shell(1), user_download(7)], now);
        fw.sync(&[user_download(7)], now);
        assert_eq!(fw.sync(&[], now), Some(Event::Complete));
        assert_eq!(c.released.get(), 1);
        assert!(!fw.engaged() && !fw.holding_lease());
        // Nothing comes back on its own afterwards.
        assert_eq!(fw.sync(&[shell(9)], now), None);
        assert_eq!(c.acquired.get(), 1);
    }

    #[test]
    fn background_browser_activity_never_counts() {
        let (mut fw, c, _) = setup();
        let now = Instant::now();
        assert_eq!(fw.engage(&[background_download(1)], now), Event::Refused(Refusal::NothingToFinish));
        assert_eq!(c.acquired.get(), 0);
        assert_eq!(fw.phase(&[background_download(1)], now), Phase::Unavailable);
    }

    #[test]
    fn user_started_downloads_and_commands_count() {
        let (mut fw, _, _) = setup();
        let now = Instant::now();
        assert_eq!(fw.phase(&[user_download(1)], now), Phase::Ready(1));
        assert_eq!(fw.phase(&[shell(1), user_download(1)], now), Phase::Ready(2));
        assert_eq!(fw.engage(&[shell(1), user_download(1), background_download(2)], now), Event::Engaged(2));
    }

    #[test]
    fn new_user_work_joins_while_engaged_but_background_does_not() {
        let (mut fw, c, _) = setup();
        let now = Instant::now();
        fw.engage(&[shell(1)], now);
        fw.sync(&[shell(1), shell(2), background_download(3)], now);
        assert_eq!(fw.held(), 2);
        // The first finishes; the joiner still holds; the background download never did.
        assert_eq!(fw.sync(&[shell(2), background_download(3)], now), None);
        assert_eq!(fw.held(), 1);
        assert_eq!(fw.sync(&[background_download(3)], now), Some(Event::Complete));
        assert_eq!(c.acquired.get(), 1);
        assert_eq!(c.released.get(), 1);
    }

    #[test]
    fn battery_eleven_holds_ten_releases_and_below_cannot_start() {
        let (mut fw, c, power) = setup();
        let now = Instant::now();
        power.set(battery(11));
        assert_eq!(fw.engage(&[shell(1)], now), Event::Engaged(1));
        assert_eq!(fw.sync(&[shell(1)], now + POLL), None);
        power.set(battery(10));
        assert_eq!(fw.sync(&[shell(1)], now + POLL * 2), Some(Event::Stopped(Stop::Battery(10))));
        assert_eq!(c.released.get(), 1);
        assert_eq!(fw.phase(&[shell(1)], now + POLL * 2), Phase::SafetyReleased(Stop::Battery(10)));
        // It won't come back when the battery does; a click is needed, and
        // at the floor that click is refused.
        power.set(battery(9));
        assert_eq!(fw.engage(&[shell(1)], now + POLL * 3), Event::Refused(Refusal::Battery(9)));
        power.set(battery(40));
        assert_eq!(fw.sync(&[shell(1)], now + POLL * 4), None);
        assert!(!fw.engaged());
        assert_eq!(c.acquired.get(), 1);
    }

    #[test]
    fn low_charge_on_external_power_is_not_a_reason_to_stop() {
        let (mut fw, _, power) = setup();
        let now = Instant::now();
        power.set(PowerState { on_battery: false, battery: Some(4), ..Default::default() });
        assert_eq!(fw.engage(&[shell(1)], now), Event::Engaged(1));
        assert_eq!(fw.sync(&[shell(1)], now + POLL), None);
    }

    #[test]
    fn critical_thermal_state_and_critical_battery_release() {
        let (mut fw, c, power) = setup();
        let now = Instant::now();
        fw.engage(&[shell(1)], now);
        power.set(PowerState { thermal: Thermal::Critical, ..Default::default() });
        assert_eq!(fw.sync(&[shell(1)], now + POLL), Some(Event::Stopped(Stop::Thermal)));
        assert_eq!(c.released.get(), 1);
        power.set(PowerState { on_battery: true, battery: Some(30), critical: true, ..Default::default() });
        assert_eq!(fw.engage(&[shell(1)], now + POLL * 2), Event::Refused(Refusal::CriticalBattery));
        power.set(PowerState { thermal: Thermal::Elevated, ..Default::default() });
        assert_eq!(fw.engage(&[shell(1)], now + POLL * 3), Event::Engaged(1));
    }

    #[test]
    fn shutdown_and_drop_release_the_lease() {
        let (mut fw, c, _) = setup();
        let now = Instant::now();
        fw.engage(&[shell(1)], now);
        fw.shutdown();
        assert_eq!(c.released.get(), 1);
        fw.engage(&[shell(2)], now);
        drop(fw);
        assert_eq!(c.released.get(), 2);
    }

    #[test]
    fn a_second_click_gives_it_back() {
        let (mut fw, c, _) = setup();
        let now = Instant::now();
        fw.toggle(&[shell(1)], now);
        assert_eq!(fw.toggle(&[shell(1)], now), Event::TurnedOff);
        assert_eq!(c.released.get(), 1);
        assert!(!fw.engaged());
    }

    #[test]
    fn a_refused_platform_never_claims_to_hold() {
        let counts = Rc::new(Counts::default());
        let power = Rc::new(Cell::new(PowerState::default()));
        let mut fw = FinishWork::new(Box::new(Fake { counts, power, capability: Capability::IdleSleep, refuse: true }));
        assert_eq!(fw.engage(&[shell(1)], Instant::now()), Event::Refused(Refusal::Unavailable));
        assert!(!fw.engaged());
        let counts = Rc::new(Counts::default());
        let power = Rc::new(Cell::new(PowerState::default()));
        let mut fw = FinishWork::new(Box::new(Fake { counts, power, capability: Capability::None, refuse: false }));
        assert_eq!(fw.engage(&[shell(1)], Instant::now()), Event::Refused(Refusal::Unavailable));
        assert_eq!(fw.phase(&[shell(1)], Instant::now()), Phase::Unavailable);
    }

    #[test]
    fn tooltips_are_honest_about_the_lid() {
        assert!(tooltip(Phase::Holding(2), Capability::IdleSleep).contains("Closing the lid still follows system power settings"));
        assert!(tooltip(Phase::Holding(2), Capability::ClosedLidInhibitor).contains("with the lid closed"));
        assert!(tooltip(Phase::Holding(2), Capability::IdleSleep).starts_with("Finish Work · 2 tasks"));
        assert_eq!(tooltip(Phase::SafetyReleased(Stop::Battery(10)), Capability::IdleSleep), "Finish Work stopped · battery 10%");
    }

    #[test]
    fn command_generations_are_unique() {
        let a = next_command();
        let b = next_command();
        assert_ne!(a, b);
    }
}

//! What a site has to tell you, said where nus already says things.
//!
//! A page you keep open speaks for itself (design page 8, "A · Listening").
//! CEF has no Notification API, so every page gets one from nus: `NOTIFY_JS`
//! defines `Notification`, a service worker registration's
//! `showNotification` and `navigator.setAppBadge`, and each calls the
//! `nusNotify` binding. A notification is "to you": it fills the tab's
//! cell with signal, its words become the row's second line until the tab
//! is in front, and it rings the Dock while nus is behind. A badge, or a
//! count in the title ("(14) Slack", "Inbox (12) - … - Gmail"), is only a
//! number in the cell. What the page may do is the site's: ASK, COUNT ONLY,
//! SHOW or OFF (`Mode`, sites.json), asked once on the band.
//!
//! A site you listen to never sleeps and never archives, so it can keep
//! telling you; four at most, since each is a live renderer. When a
//! listening page goes to a sign-in, its last count stays in the cell,
//! dim and marked, rather than falling silent as if nothing were new.
//!
//! Sources with no page say things too (`nus notify`, GitHub): a notice
//! about a site that has a tab joins that tab; otherwise it is a toast and
//! a row in WHILE YOU WERE AWAY.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How much a site may say. Kept per host in sites.json.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// Not asked yet: the page's `Notification.permission` is "default".
    #[default]
    Ask,
    /// Counts in the cell; notifications are swallowed.
    Count,
    /// Counts, and notifications to you.
    Show,
    /// Nothing: no counts, no notifications.
    Off,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Ask, Mode::Count, Mode::Show, Mode::Off];

    /// What the page's `Notification.permission` reads.
    pub fn permission(self) -> &'static str {
        match self {
            Mode::Ask => "default",
            Mode::Count | Mode::Show => "granted",
            Mode::Off => "denied",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Mode::Ask => "ASK",
            Mode::Count => "COUNT",
            Mode::Show => "SHOW",
            Mode::Off => "OFF",
        }
    }

    /// Counts show unless the site is off.
    pub fn counts(self) -> bool {
        self != Mode::Off
    }
}

/// The site's mode, for a page at this address.
pub fn mode_for(url: &str) -> Mode {
    crate::sites::prefs(&crate::sites::host_of(url)).notices
}

/// Whether a page at this address is listened to.
pub fn listening(url: &str) -> bool {
    let host = crate::sites::host_of(url);
    !host.is_empty() && crate::sites::prefs(&host).listen
}

/// Sites that may listen at once: each is a live renderer, never asleep.
pub const MAX_LISTENING: usize = 4;

/// One thing a page or a source said.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub tag: String,
    pub at: Instant,
    /// Unix seconds, for WHILE YOU WERE AWAY.
    pub wall: u64,
}

impl Notice {
    pub fn new(title: &str, body: &str, tag: &str) -> Self {
        Notice { title: clip(title, 120), body: clip(body, 240), tag: clip(tag, 100), at: crate::clock::now(), wall: crate::journal::now() }
    }

    /// The notice in one line: "ana · can you look at #412".
    pub fn line(&self) -> String {
        match (self.title.is_empty(), self.body.is_empty()) {
            (false, false) => format!("{} · {}", self.title, self.body),
            (false, true) => self.title.clone(),
            (true, false) => self.body.clone(),
            (true, true) => String::new(),
        }
    }
}

/// Words from a page or a source: one line, no controls, bounded.
pub fn clip(s: &str, n: usize) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= n {
        flat
    } else {
        flat.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    }
}

/// A count a page shows: a number, or only that there is something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    N(u32),
    Dot,
}

/// The count a title carries, as mail and chat sites write it: "(14) Slack",
/// "Inbox (12) - me@x - Gmail", "* Slack | general". Years in a title
/// ("Matrix (1999) - Wikipedia") are not counts.
pub fn title_count(title: &str) -> Option<Count> {
    let t = title.trim_start();
    if let Some(rest) = t.strip_prefix('(') {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let after = &rest[digits.len()..];
        if !digits.is_empty() && (after.starts_with(')') || after.starts_with("+)")) {
            return digits.parse().ok().map(|n: u32| Count::N(n.min(99_999)));
        }
    }
    if t.starts_with("* ") || t.starts_with("*") && t.chars().nth(1).is_some_and(|c| c.is_alphanumeric()) {
        return Some(Count::Dot);
    }
    // "Inbox (12) - …": a short first part, then the number, then a dash.
    let open = t.find(" (")?;
    if open > 30 {
        return None;
    }
    let rest = &t[open + 2..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 5 {
        return None;
    }
    let tail = rest[digits.len()..].strip_prefix(')')?.trim_start();
    if !tail.starts_with(['-', '–', '—', '|', '·']) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    if (1900..=2100).contains(&n) {
        return None;
    }
    Some(Count::N(n))
}

/// The address a listening page was counting at went to a sign-in.
pub fn sign_in(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    ["login", "signin", "sign_in", "sign-in", "logon", "/auth", "accounts.", "sso.", "/sso"].iter().any(|w| u.contains(w))
}

/// What a page has said, kept on its `Shared`.
#[derive(Default)]
pub struct PageNotices {
    /// Notifications to you since the tab was last in front.
    pub unseen: u32,
    /// The newest notification to you.
    pub latest: Option<Notice>,
    /// Recent notifications, newest last, for WHILE YOU WERE AWAY.
    pub recent: VecDeque<Notice>,
    /// `navigator.setAppBadge`: a number, or Dot for none given.
    pub badge: Option<Count>,
    /// A source's count for this site (GitHub's unread).
    pub source: Option<u32>,
    /// The last count read while the page was on its own site: (host, n,
    /// unix seconds), for saying it went stale.
    pub counted: Option<(String, u32, u64)>,
    /// A new notification nus hasn't rung the Dock for.
    pub ring: bool,
    last_ring: Option<Instant>,
    /// Page worlds waiting to hear their permission: execution context ids.
    pub tell: Vec<i64>,
}

impl PageNotices {
    /// The count to show: the page's badge, then its title, then a source's.
    pub fn count(&self, title: &str) -> Option<Count> {
        self.badge.filter(|b| *b != Count::N(0)).or_else(|| title_count(title)).or(self.source.filter(|n| *n > 0).map(Count::N))
    }

    /// A notification to you. A repeat with the same tag replaces the last
    /// rather than counting again.
    pub fn receive(&mut self, n: Notice) {
        let same = !n.tag.is_empty() && self.latest.as_ref().is_some_and(|l| l.tag == n.tag);
        if same {
            if let Some(last) = self.recent.back_mut().filter(|l| l.tag == n.tag) {
                *last = n.clone();
            } else {
                self.recent.push_back(n.clone());
            }
            if self.unseen == 0 {
                self.unseen = 1;
            }
        } else {
            self.recent.push_back(n.clone());
            self.unseen = (self.unseen + 1).min(99);
        }
        while self.recent.len() > 20 {
            self.recent.pop_front();
        }
        // The Dock rings once for a burst, not once per message.
        if self.last_ring.is_none_or(|t| crate::clock::since(t) > Duration::from_secs(10)) {
            self.ring = true;
            self.last_ring = Some(n.at);
        }
        self.latest = Some(n);
    }

    /// The tab was in front: what it said has been seen.
    pub fn seen(&mut self) {
        self.unseen = 0;
    }
}

/// The page's side: `Notification`, `showNotification` and the badge,
/// each saying it through the `nusNotify` binding. The permission is the
/// site's, told to each page world when it asks ("hi") and again when it
/// changes, through `__nusNotifyPerm`.
pub const NOTIFY_JS: &str = r#"(()=>{
if(window.__nusNotify)return;Object.defineProperty(window,'__nusNotify',{value:true});
const say=o=>{try{nusNotify(JSON.stringify(o))}catch(e){}};
const str=(v,n)=>String(v==null?'':v).slice(0,n);
let perm='default';let waiting=[];
Object.defineProperty(window,'__nusNotifyPerm',{configurable:false,get(){return perm},set(v){if(v!=='granted'&&v!=='denied'&&v!=='default')return;perm=v;if(v!=='default'){const w=waiting;waiting=[];w.forEach(f=>{try{f(v)}catch(e){}})}}});
class Notification extends EventTarget{
 constructor(title,o){super();o=o||{};
  this.title=str(title,200);this.body=str(o.body,400);this.tag=str(o.tag,100);this.data=o.data===undefined?null:o.data;
  this.icon=str(o.icon,500);this.badge=str(o.badge,500);this.image=str(o.image,500);this.lang=str(o.lang,20);this.dir=o.dir||'auto';
  this.silent=!!o.silent;this.renotify=!!o.renotify;this.requireInteraction=!!o.requireInteraction;this.timestamp=Date.now();this.actions=[];this.vibrate=[];
  this.onclick=null;this.onshow=null;this.onerror=null;this.onclose=null;
  const ok=perm==='granted';
  if(ok)say({k:'n',title:this.title,body:this.body,tag:this.tag});
  setTimeout(()=>{const e=new Event(ok?'show':'error');try{this.dispatchEvent(e)}catch(_){}const h=ok?this.onshow:this.onerror;if(typeof h==='function'){try{h.call(this,e)}catch(_){}}},0);
 }
 close(){setTimeout(()=>{const e=new Event('close');try{this.dispatchEvent(e)}catch(_){}if(typeof this.onclose==='function'){try{this.onclose.call(this,e)}catch(_){}}},0)}
 static get permission(){return perm}
 static get maxActions(){return 2}
 static requestPermission(cb){
  const p=new Promise(res=>{if(perm!=='default'){res(perm);return}waiting.push(res);say({k:'ask'})});
  if(typeof cb==='function')p.then(cb);return p;
 }
}
Object.defineProperty(window,'Notification',{value:Notification,writable:true,configurable:true});
if(window.ServiceWorkerRegistration){
 ServiceWorkerRegistration.prototype.showNotification=function(title,o){new Notification(title,o);return Promise.resolve()};
 ServiceWorkerRegistration.prototype.getNotifications=function(){return Promise.resolve([])};
}
if(window.Navigator){
 Navigator.prototype.setAppBadge=function(n){say({k:'b',n:n===undefined?-1:Math.max(0,Math.floor(Number(n)||0))});return Promise.resolve()};
 Navigator.prototype.clearAppBadge=function(){say({k:'b',n:0});return Promise.resolve()};
}
if(window.navigator&&navigator.permissions&&navigator.permissions.query){
 const q=navigator.permissions.query.bind(navigator.permissions);
 navigator.permissions.query=function(d){if(d&&d.name==='notifications'){return Promise.resolve({state:perm==='default'?'prompt':perm,name:'notifications',onchange:null,addEventListener(){},removeEventListener(){}})}return q(d)};
}
say({k:'hi'});
})()"#;

/// The words that tell a page world its permission.
pub fn perm_js(mode: Mode) -> String {
    format!("window.__nusNotifyPerm={:?}", mode.permission())
}

/// A notice from a source with no page of its own (`nus notify`, GitHub).
#[derive(Clone, Debug)]
pub struct SourceNotice {
    /// Who sent it: "github", "deploy", a shell's program.
    pub source: String,
    pub notice: Notice,
    /// Where it leads, when it leads somewhere.
    pub open: Option<String>,
    /// Addressed to you (a review asked of you), or only news.
    pub to_you: bool,
}

/// What came in from sources, newest last, for WHILE YOU WERE AWAY.
#[derive(Default)]
pub struct Sources {
    pub recent: VecDeque<SourceNotice>,
}

impl Sources {
    pub fn push(&mut self, n: SourceNotice) {
        self.recent.push_back(n);
        while self.recent.len() > 40 {
            self.recent.pop_front();
        }
    }
}

/// Only http(s) addresses lead anywhere.
pub fn openable(url: &str) -> Option<String> {
    let u = url::Url::parse(url.trim()).ok()?;
    matches!(u.scheme(), "http" | "https").then(|| u.to_string())
}

use crate::app::{App, Pane};

impl App {
    /// Each frame: tell new page worlds their permission, keep what the tab
    /// in front said as seen, remember counts for going stale, ring for
    /// what came while nus was behind, and keep listening pages awake.
    pub(crate) fn tend_notices(&mut self) {
        self.tend_github();
        let focused = self.window_focused;
        let active = self.active;
        let mut wake = None;
        let mut ring = false;
        for (i, tab) in self.tabs.iter().enumerate() {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                let Pane::Web(w) = p else { continue };
                if let Some(url) = w.asleep.as_deref() {
                    if wake.is_none() && listening(url) {
                        wake = Some(i);
                    }
                    continue;
                }
                let tell = std::mem::take(&mut w.tab.shared.borrow_mut().notices.tell);
                if !tell.is_empty() {
                    let js = perm_js(mode_for(&w.tab.shared.borrow().url));
                    for context in tell {
                        w.tab.devtools("Runtime.evaluate", serde_json::json!({ "expression": js, "contextId": context, "silent": true }));
                    }
                }
                let mut s = w.tab.shared.borrow_mut();
                if i == active && focused && s.notices.unseen > 0 {
                    s.notices.seen();
                    self.dirty = true;
                }
                if std::mem::take(&mut s.notices.ring) && !(i == active && focused) {
                    ring = true;
                }
                let host = crate::sites::host_of(&s.url);
                if !host.is_empty() && mode_for(&s.url).counts() {
                    if let Some(Count::N(n)) = s.notices.count(&s.title) {
                        if s.notices.counted.as_ref().is_none_or(|(h, m, _)| *h != host || *m != n) {
                            s.notices.counted = Some((host, n, crate::journal::now()));
                        }
                    }
                }
            }
        }
        if ring {
            self.notice_ring = true;
            self.play_event("bell");
        }
        if let Some(i) = wake {
            self.wake_tab(i);
            self.dirty = true;
        }
    }

    /// What a tab's page has said, for its cell's tooltip.
    pub(crate) fn notice_words(&self, tab: &crate::app::Tab) -> Option<String> {
        let (main, _) = tab.panes();
        let Pane::Web(w) = main else { return None };
        let s = w.tab.shared.borrow();
        let mut bits = Vec::new();
        if s.notices.unseen > 0 {
            bits.push(format!("{} to you", s.notices.unseen));
        }
        match mode_for(&s.url).counts().then(|| s.notices.count(&s.title)).flatten() {
            Some(Count::N(n)) if n > 0 => bits.push(format!("{n} unread")),
            Some(Count::Dot) => bits.push("something new".into()),
            _ => {}
        }
        if let Some(n) = crate::tab_state::stale(&s) {
            if let Some((_, _, when)) = s.notices.counted.as_ref() {
                bits.push(format!("signed out · {n} unread at {}", crate::journal::when(*when)));
            }
        }
        if let Some(n) = s.notices.latest.as_ref().filter(|_| s.notices.unseen > 0) {
            bits.push(format!("last: {}", n.line()));
        }
        if listening(&s.url) {
            bits.push("listening · never sleeps".into());
        }
        (!bits.is_empty()).then(|| bits.join(" · "))
    }

    /// A notice from a source with no page (`nus notify`, GitHub). About a
    /// site that has a tab, it joins that tab as the page's own would;
    /// otherwise it is a toast and a row in WHILE YOU WERE AWAY.
    pub(crate) fn source_notice(&mut self, n: SourceNotice) {
        if crate::private::enabled() {
            return;
        }
        let host = n.open.as_deref().map(crate::sites::host_of).unwrap_or_default();
        let tab = (!host.is_empty()).then(|| {
            self.tabs.iter().position(|t| t.peek.is_none() && matches!(&t.left, Pane::Web(w) if crate::sites::host_of(&w.tab.shared.borrow().url) == host))
        }).flatten();
        let looked = |i: usize, me: &Self| i == me.active && me.window_focused;
        match tab {
            Some(i) if n.to_you && !looked(i, self) => {
                if let Pane::Web(w) = &self.tabs[i].left {
                    w.tab.shared.borrow_mut().notices.receive(n.notice.clone());
                }
            }
            Some(_) => {}
            None if n.source == "github" && !n.to_you => {}
            None => {
                let words = n.notice.line();
                let detail = if words.is_empty() { n.source.clone() } else { format!("{} · {words}", n.source) };
                let act = n.open.clone().map(crate::toast::Act::OpenUrl);
                self.toast(if n.source == "github" { nus_render::text::icons::GITHUB } else { nus_render::text::icons::BELL }, "New Notice", detail, act);
                if n.to_you && !self.window_focused {
                    self.notice_ring = true;
                }
            }
        }
        self.sources.push(n);
        self.dirty = true;
    }
}

/// `nus notify`: to the shell it ran in (its NUS_PANE), unless it leads
/// somewhere; otherwise to the window in front, as a source's notice.
pub fn route(apps: &mut [App], front: Option<usize>, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let text = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let (title, words) = (clip(&text("title"), 120), clip(&text("words"), 240));
    if title.is_empty() && words.is_empty() {
        return Err("nothing to say: nus notify <words…> [--title T] [--to-you] [--open URL] [--source NAME]".into());
    }
    let open = match text("open") {
        o if o.is_empty() => None,
        o => Some(openable(&o).ok_or("--open takes an http or https address")?),
    };
    let to_you = args.get("to_you").and_then(|v| v.as_bool()).unwrap_or(false);
    let pane = text("pane");
    if open.is_none() && !pane.is_empty() {
        let line = Notice::new(&title, &words, "").line();
        for a in apps.iter_mut() {
            if a.shell_notice(&pane, line.clone()) {
                return Ok(serde_json::json!({ "to": "shell" }));
            }
        }
    }
    let source = match clip(&text("source"), 24) {
        s if s.is_empty() => "notify".to_string(),
        s => s.to_lowercase(),
    };
    let i = front.filter(|i| *i < apps.len()).unwrap_or(0);
    let a = apps.get_mut(i).ok_or("no nus window")?;
    a.source_notice(SourceNotice { source, notice: Notice::new(&title, &words, ""), open, to_you });
    Ok(serde_json::json!({ "to": "window" }))
}

/// GitHub's notifications, through the forge sign-in (SYNC · FORGE): a
/// worker asks at the pace GitHub sets (`X-Poll-Interval`; a 304 when
/// nothing is new costs nothing) and hands what is new to the window.
/// The token goes to curl on stdin, never in its arguments.
pub mod github {
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    /// One notification thread, as the window needs it.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Item {
        pub id: String,
        pub repo: String,
        pub title: String,
        pub reason: String,
        pub url: String,
    }

    impl Item {
        /// Asked of you, rather than only something you follow.
        pub fn to_you(&self) -> bool {
            matches!(self.reason.as_str(), "review_requested" | "mention" | "team_mention" | "assign" | "ci_activity" | "security_alert" | "approval_requested")
        }

        /// The reason in words.
        pub fn why(&self) -> &'static str {
            match self.reason.as_str() {
                "review_requested" => "review requested",
                "mention" | "team_mention" => "mentioned you",
                "assign" => "assigned to you",
                "ci_activity" => "a workflow run",
                "security_alert" => "a security alert",
                "approval_requested" => "approval requested",
                "author" => "on your thread",
                "comment" => "a comment",
                "state_change" => "state changed",
                _ => "news",
            }
        }
    }

    #[derive(Default)]
    struct Shared {
        new: Vec<Item>,
        unread: Option<u32>,
        status: String,
    }

    static ON: AtomicBool = AtomicBool::new(false);
    static STARTED: OnceLock<()> = OnceLock::new();
    static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

    fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
        let mut g = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        f(g.get_or_insert_with(Default::default))
    }

    /// GITHUB NOTIFICATIONS on or off; the worker starts the first time.
    pub fn set(on: bool) {
        let on = on && !crate::private::enabled();
        if ON.swap(on, Ordering::Relaxed) == on {
            return;
        }
        if on {
            STARTED.get_or_init(|| {
                let _ = std::thread::Builder::new().name("nus-github-notices".into()).spawn(worker);
            });
        } else {
            with(|s| {
                s.new.clear();
                s.unread = None;
                s.status.clear();
            });
        }
    }

    /// What arrived since the window last asked, and the unread count.
    pub fn take() -> (Vec<Item>, Option<u32>) {
        with(|s| (std::mem::take(&mut s.new), s.unread))
    }

    /// How the worker is doing, in words, for the settings page.
    pub fn status() -> String {
        with(|s| s.status.clone())
    }

    fn say(status: &str) {
        with(|s| s.status = status.to_string());
    }

    fn worker() {
        let mut since: Option<String> = None;
        let mut seen: HashSet<String> = HashSet::new();
        let mut first = true;
        loop {
            if !ON.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
            let forge = crate::forge::load().filter(|f| f.kind == crate::forge::Kind::GitHub && f.host.trim_end_matches('/') == "https://github.com");
            let (Some(_), Some(token)) = (forge, crate::forge::token()) else {
                say("sign in to GitHub under SYNC · FORGE");
                std::thread::sleep(Duration::from_secs(30));
                continue;
            };
            let wait = match fetch(&token, since.as_deref()) {
                Ok(Answer::Same(poll)) => {
                    if !first {
                        say("up to date");
                    }
                    poll
                }
                Ok(Answer::New { items, modified, poll }) => {
                    since = modified;
                    let unread = items.len() as u32;
                    let fresh: Vec<Item> = items.into_iter().filter(|i| seen.insert(i.id.clone())).collect();
                    let announce = !first;
                    with(|s| {
                        s.unread = Some(unread);
                        // The first answer is what was already there: counted, not announced.
                        if announce {
                            s.new.extend(fresh);
                            s.new.truncate(50);
                        }
                    });
                    first = false;
                    say(&format!("{unread} unread"));
                    poll
                }
                Err(Refused) => {
                    say("GitHub refused the sign-in: notifications need the repo or notifications scope");
                    600
                }
            };
            crate::browser_runtime::wake();
            std::thread::sleep(Duration::from_secs(wait.clamp(60, 3600)));
        }
    }

    enum Answer {
        Same(u64),
        New { items: Vec<Item>, modified: Option<String>, poll: u64 },
    }

    struct Refused;

    fn fetch(token: &str, since: Option<&str>) -> Result<Answer, Refused> {
        use std::io::Write;
        let mut c = nus_compat::command("curl");
        c.args(["-sS", "-i", "--max-time", "30", "-A", "nus", "-H", "@-", "-H", "Accept: application/vnd.github+json", "-H", "X-GitHub-Api-Version: 2022-11-28"]);
        if let Some(since) = since {
            c.args(["-H", &format!("If-Modified-Since: {since}")]);
        }
        c.arg("https://api.github.com/notifications?per_page=50");
        c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        let Ok(mut child) = c.spawn() else { return Ok(Answer::Same(300)) };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "Authorization: Bearer {token}");
        }
        let Ok(out) = child.wait_with_output() else { return Ok(Answer::Same(300)) };
        parse(&String::from_utf8_lossy(&out.stdout))
    }

    /// curl -i's answer: the last header block (after any 100 Continue),
    /// then the body.
    fn parse(text: &str) -> Result<Answer, Refused> {
        let text = text.replace("\r\n", "\n");
        let mut rest = text.as_str();
        let (head, body) = loop {
            let Some((head, body)) = rest.split_once("\n\n") else { return Ok(Answer::Same(300)) };
            if head.starts_with("HTTP/") && body.starts_with("HTTP/") {
                rest = body;
                continue;
            }
            break (head, body);
        };
        let code: u16 = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
        let header = |name: &str| head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()));
        let poll = header("x-poll-interval").and_then(|p| p.parse().ok()).unwrap_or(60);
        match code {
            304 => Ok(Answer::Same(poll)),
            200 => {
                let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
                let items = v.as_array().map(|a| a.iter().filter_map(item).collect()).unwrap_or_default();
                Ok(Answer::New { items, modified: header("last-modified"), poll })
            }
            401 | 403 => Err(Refused),
            _ => Ok(Answer::Same(poll.max(300))),
        }
    }

    fn item(v: &serde_json::Value) -> Option<Item> {
        let s = |p: &str| v.pointer(p).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let id = s("/id");
        if id.is_empty() || v.get("unread").and_then(|u| u.as_bool()) == Some(false) {
            return None;
        }
        let repo = s("/repository/full_name");
        Some(Item { url: html(&s("/subject/url"), &repo), id, repo: super::clip(&repo, 80), title: super::clip(&s("/subject/title"), 200), reason: s("/reason") })
    }

    /// The page for a thread: the API's address turned into the site's.
    pub fn html(api: &str, repo: &str) -> String {
        if let Some(path) = api.strip_prefix("https://api.github.com/repos/") {
            if !path.contains("/releases/") {
                let path = path.replacen("/pulls/", "/pull/", 1).replacen("/commits/", "/commit/", 1);
                return format!("https://github.com/{path}");
            }
        }
        if !repo.is_empty() {
            return format!("https://github.com/{repo}");
        }
        "https://github.com/notifications".into()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn answers_are_read_from_curl() {
            let ok = "HTTP/2 200\r\nlast-modified: Mon, 05 Oct 2026 14:20:00 GMT\r\nx-poll-interval: 90\r\n\r\n[{\"id\":\"1\",\"unread\":true,\"reason\":\"review_requested\",\"subject\":{\"title\":\"Fix sleep\",\"url\":\"https://api.github.com/repos/cbassuarez/nus/pulls/412\"},\"repository\":{\"full_name\":\"cbassuarez/nus\"}}]";
            let Ok(Answer::New { items, modified, poll }) = parse(ok) else { panic!("200 is news") };
            assert_eq!(poll, 90);
            assert_eq!(modified.as_deref(), Some("Mon, 05 Oct 2026 14:20:00 GMT"));
            assert_eq!(items[0].url, "https://github.com/cbassuarez/nus/pull/412");
            assert!(items[0].to_you());
            assert!(matches!(parse("HTTP/2 304\r\nx-poll-interval: 60\r\n\r\n"), Ok(Answer::Same(60))));
            assert!(parse("HTTP/2 401\r\n\r\n{}").is_err());
            assert!(matches!(parse("HTTP/1.1 100 Continue\r\n\r\nHTTP/2 304\r\n\r\n"), Ok(Answer::Same(60))));
        }

        #[test]
        fn threads_lead_to_their_pages() {
            assert_eq!(html("https://api.github.com/repos/o/r/issues/7", "o/r"), "https://github.com/o/r/issues/7");
            assert_eq!(html("https://api.github.com/repos/o/r/commits/abc", "o/r"), "https://github.com/o/r/commit/abc");
            assert_eq!(html("https://api.github.com/repos/o/r/releases/1", "o/r"), "https://github.com/o/r");
            assert_eq!(html("", ""), "https://github.com/notifications");
        }
    }
}

impl App {
    /// GitHub's notifications into the window: new threads as notices (to
    /// you when asked of you), the unread count on every github.com tab.
    pub(crate) fn tend_github(&mut self) {
        github::set(self.behavior.github_notices);
        let (items, unread) = if self.behavior.github_notices { github::take() } else { (Vec::new(), None) };
        for t in &self.tabs {
            for p in std::iter::once(&t.left).chain(t.right.as_ref()) {
                if let Pane::Web(w) = p {
                    let mut s = w.tab.shared.borrow_mut();
                    let want = if crate::sites::host_of(&s.url) == "github.com" { unread } else { None };
                    if s.notices.source != want {
                        s.notices.source = want;
                        self.dirty = true;
                    }
                }
            }
        }
        for i in items {
            let to_you = i.to_you();
            let notice = Notice::new(&format!("{} · {}", i.repo, i.why()), &i.title, &i.id);
            self.source_notice(SourceNotice { source: "github".into(), notice, open: Some(i.url), to_you });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_come_from_the_title_as_sites_write_them() {
        assert_eq!(title_count("(14) Slack | general"), Some(Count::N(14)));
        assert_eq!(title_count("(3) WhatsApp"), Some(Count::N(3)));
        assert_eq!(title_count("(99+) X"), Some(Count::N(99)));
        assert_eq!(title_count("Inbox (12) - seb@example.com - Gmail"), Some(Count::N(12)));
        assert_eq!(title_count("* Slack | nus-dev"), Some(Count::Dot));
        assert_eq!(title_count("*nus-dev - Slack"), Some(Count::Dot));
        assert_eq!(title_count("Matrix (1999) - Wikipedia"), None);
        assert_eq!(title_count("Inbox - seb@example.com - Gmail"), None);
        assert_eq!(title_count("wgpu — Rust"), None);
        assert_eq!(title_count("A very long title about something (4) - x"), None);
        assert_eq!(title_count("**bold** markdown"), None);
    }

    #[test]
    fn a_badge_beats_the_title_and_zero_clears_it() {
        let mut p = PageNotices { badge: Some(Count::N(3)), ..Default::default() };
        assert_eq!(p.count("(14) Slack"), Some(Count::N(3)));
        p.badge = Some(Count::N(0));
        assert_eq!(p.count("(14) Slack"), Some(Count::N(14)));
        p.source = Some(5);
        assert_eq!(p.count("GitHub"), Some(Count::N(5)));
    }

    #[test]
    fn a_repeat_with_its_tag_replaces_rather_than_counts() {
        let mut p = PageNotices::default();
        p.receive(Notice::new("ana", "one", "D1"));
        p.receive(Notice::new("ana", "two", "D1"));
        assert_eq!(p.unseen, 1);
        assert_eq!(p.recent.len(), 1);
        assert_eq!(p.latest.as_ref().unwrap().body, "two");
        p.receive(Notice::new("ben", "three", ""));
        assert_eq!(p.unseen, 2);
        assert!(p.ring, "the first of a burst rings");
        p.seen();
        assert_eq!(p.unseen, 0);
    }

    #[test]
    fn words_from_a_page_are_one_bounded_line() {
        assert_eq!(clip("a\nb\t c", 20), "a b c");
        assert_eq!(clip(&"x".repeat(300), 10).chars().count(), 10);
        assert_eq!(Notice::new("ana", "hi", "").line(), "ana · hi");
    }

    #[test]
    fn sign_ins_and_openable_addresses() {
        assert!(sign_in("https://accounts.google.com/v3/signin"));
        assert!(sign_in("https://login.microsoftonline.com/"));
        assert!(!sign_in("https://app.slack.com/client"));
        assert_eq!(openable("https://github.com/x").as_deref(), Some("https://github.com/x"));
        assert_eq!(openable("javascript:alert(1)"), None);
        assert_eq!(openable("file:///etc/passwd"), None);
    }

    #[test]
    fn modes_say_what_the_page_reads() {
        assert_eq!(Mode::Ask.permission(), "default");
        assert_eq!(Mode::Count.permission(), "granted");
        assert_eq!(Mode::Off.permission(), "denied");
        assert!(perm_js(Mode::Show).contains("\"granted\""));
        assert!(!Mode::Off.counts());
    }
}

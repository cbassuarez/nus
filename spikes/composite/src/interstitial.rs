//! The pages nus shows in place of a site, or over one: a connection
//! that isn't private, a reported site, a clock that's off, a page that
//! crashed or ran out of memory, a form that would be sent again, a Wi-Fi
//! network that wants a sign-in, a site that can't be reached; and over a
//! live page, a hung renderer, a tab waking from sleep, a microphone or
//! camera macOS won't give, a download that was blocked.
//!
//! All of them read as a shell transcript (the "Transcript" direction):
//! what nus tried, what happened, then the next commands. A rule down the
//! left marks how serious it is: signal for danger, ink for a problem,
//! none at rest. ↵ always takes the safe command; going ahead anyway is a
//! dim command at the end, never the default.
//!
//! Pages that stand in for a site are HTML, written over Chromium's own
//! error document (or a blank one), so the address bar keeps the address
//! you asked for. Their commands come back through the `nusInterstitial`
//! binding with a token only that document knows. Overlays (`Page::native`)
//! are drawn by nus over the page, which stays alive underneath.
//!
//! `nus://interstitials` lists every page and the `nus://` commands that
//! make the real thing happen (`nus://crash`, `nus://hang`, …).

use std::collections::HashSet;
use std::sync::{Mutex, RwLock};

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cert,
    Malware,
    Clock,
    Oom,
    Crash,
    Resubmit,
    Portal,
    Unreachable,
    Index,
    // Overlays, over a page that is still there.
    Hung,
    Sleep,
    Permission,
    File,
    /// A page's own question: alert, confirm, prompt, leave-page, sign-in.
    Dialog,
}

impl Kind {
    pub const ALL: [Kind; 14] = [Kind::Cert, Kind::Malware, Kind::Clock, Kind::Oom, Kind::Crash, Kind::Permission, Kind::File, Kind::Resubmit, Kind::Sleep, Kind::Portal, Kind::Unreachable, Kind::Hung, Kind::Dialog, Kind::Index];
    pub fn slug(self) -> &'static str {
        match self {
            Kind::Cert => "cert",
            Kind::Malware => "malware",
            Kind::Clock => "clock",
            Kind::Oom => "oom",
            Kind::Crash => "crash",
            Kind::Resubmit => "resubmit",
            Kind::Portal => "portal",
            Kind::Unreachable => "unreachable",
            Kind::Index => "index",
            Kind::Hung => "hung",
            Kind::Sleep => "sleep",
            Kind::Permission => "permission",
            Kind::File => "file",
            Kind::Dialog => "dialog",
        }
    }
    pub fn from_slug(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.slug() == s)
    }
    /// Drawn by nus over the page rather than in place of it.
    pub fn native(self) -> bool {
        matches!(self, Kind::Hung | Kind::Sleep | Kind::Permission | Kind::File | Kind::Dialog)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sev {
    Danger,
    Problem,
    Rest,
}

/// One next command: the verb nus runs, what it does in words, its key.
#[derive(Clone, Debug, PartialEq)]
pub struct Act {
    pub verb: String,
    pub label: String,
    pub key: &'static str,
    /// Going ahead against the advice: dim, last, never the default.
    pub unsafe_: bool,
}

fn act(verb: &str, label: &str, key: &'static str) -> Act {
    Act { verb: verb.into(), label: label.into(), key, unsafe_: false }
}
fn risky(verb: &str, label: &str) -> Act {
    Act { verb: verb.into(), label: label.into(), key: "", unsafe_: true }
}

/// A line to type into, on an overlay that asks for words (a prompt, a
/// sign-in). A secret one shows dots.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub label: String,
    pub value: String,
    pub secret: bool,
}

/// A page, ready to show either way.
#[derive(Clone, Debug)]
pub struct Page {
    pub kind: Kind,
    pub sev: Sev,
    /// The address this is about: the one you asked for.
    pub url: String,
    /// The first line of the transcript, after `»`.
    pub command: String,
    /// What happened, a line at a time; the first is the verdict.
    pub log: Vec<String>,
    pub head: String,
    pub body: String,
    pub acts: Vec<Act>,
    /// Proves a command came from this document and not from a site.
    pub token: String,
    /// What the overlay asks you to type, and which line has the caret.
    pub fields: Vec<Field>,
    pub field: usize,
}

impl Page {
    fn new(kind: Kind, sev: Sev, url: &str, log: Vec<String>, head: String, body: String, mut acts: Vec<Act>) -> Page {
        // ↵ belongs to the first safe command alone.
        let default = acts.iter().position(|a| !a.unsafe_);
        for (i, a) in acts.iter_mut().enumerate() {
            if a.key == "↵" && Some(i) != default {
                a.key = "";
            }
        }
        Page { kind, sev, url: url.into(), command: format!("open {url}"), log, head, body, acts, token: crate::remote::new_token(), fields: Vec::new(), field: 0 }
    }

    /// The safe way out: back, or when there is nowhere to go back to,
    /// closing the tab.
    fn away(can_back: bool) -> Act {
        if can_back { act("back", "Go back", "↵") } else { act("close", "Close this tab", "↵") }
    }

    pub fn cert(url: &str, code: &str, can_back: bool) -> Page {
        let host = host(url);
        Page::new(Kind::Cert, Sev::Danger, url,
            vec![format!("✕ {}", cert_words(code)), format!("  error   {code}"), format!("  site    {host}")],
            "This connection isn't private".into(),
            format!("{host} couldn't prove it is {host}. Someone could be reading or changing what you send, such as passwords or card numbers."),
            vec![Page::away(can_back), act("retry", "Try again", "⌘R"), risky("proceed", &format!("Continue to {host} (unsafe)"))])
    }

    /// The clock, when it explains a certificate that isn't valid yet or
    /// any more. `behind` is how far, in days (negative: ahead).
    pub fn clock(url: &str, behind_days: i64, can_back: bool) -> Page {
        let host = host(url);
        let (way, n) = if behind_days >= 0 { ("behind", behind_days) } else { ("ahead", -behind_days) };
        let days = if n == 1 { "1 day".to_string() } else { format!("{n} days") };
        Page::new(Kind::Clock, Sev::Problem, url,
            vec![format!("✕ your clock is {days} {way}"), format!("  clock   {}", clock_now()), "  error   NET::ERR_CERT_DATE_INVALID".into()],
            format!("Your computer's clock is {days} {way}"),
            format!("Secure sites check the date, so {host} can't be verified while your clock is wrong. Set the date and time, then try again."),
            vec![act("settings:date-time", "Date & time settings", "↵"), act("retry", "Try again", "⌘R"), Page::away(can_back)])
    }

    pub fn malware(url: &str, list: &str, can_back: bool) -> Page {
        let host = host(url);
        Page::new(Kind::Malware, Sev::Danger, url,
            vec![format!("✕ {host} is on your list of dangerous sites"), format!("  list    {list}"), "  nus stopped before anything loaded".into()],
            "This site is reported as dangerous".into(),
            format!("{host} is on a list of sites that steal passwords or install harmful software. Nothing from it has loaded."),
            vec![Page::away(can_back), risky("proceed", &format!("Visit {host} anyway (unsafe)"))])
    }

    pub fn crashed(url: &str, oom: bool, code: &str, killed: bool) -> Page {
        let host = host(url);
        if oom {
            return Page::new(Kind::Oom, Sev::Problem, url,
                vec!["✕ the page ran out of memory".into(), format!("  site    {host}")],
                "This page ran out of memory".into(),
                format!("{host} used more memory than it could have and stopped. Your other tabs are fine. Putting idle tabs to sleep frees room before you reload."),
                vec![act("retry", "Reload", "↵"), act("sleep-idle", "Sleep idle tabs", "S")]);
        }
        let why = if killed { "the page's process was stopped".to_string() } else { format!("the page's process stopped · {code}") };
        Page::new(Kind::Crash, Sev::Problem, url,
            vec![format!("✕ {why}"), format!("  site    {host}")],
            "This page can't be opened".into(),
            if killed { "The page stopped because you ended it, or the system did. Reloading starts it again.".into() } else { "The page's process stopped unexpectedly. Reloading usually works.".into() },
            vec![act("retry", "Reload", "↵")])
    }

    pub fn resubmit(url: &str, can_back: bool) -> Page {
        let host = host(url);
        Page::new(Kind::Resubmit, Sev::Problem, url,
            vec!["✕ this page came from a form you sent · ERR_CACHE_MISS".into(), format!("  form    to {host}")],
            "Send this form again?".into(),
            format!("Loading this page again sends the form to {host} again, which could, for example, place the same order twice."),
            vec![Page::away(can_back), act("resubmit", "Send again", "⌘↵")])
    }

    pub fn portal(url: &str, network: &str) -> Page {
        let host = host(url);
        Page::new(Kind::Portal, Sev::Problem, url,
            vec!["✕ the network answered instead of the site".into(), format!("  wi-fi   {network} wants a sign-in")],
            "Sign in to this Wi-Fi network".into(),
            format!("This network needs you to sign in or accept its terms before other pages load. Once you have, {host} will load."),
            vec![act("open-portal", "Open the sign-in page", "↵"), act("retry", "Try again", "⌘R")])
    }

    pub fn unreachable(url: &str, code: &str, can_back: bool) -> Page {
        let host = host(url);
        let (verdict, head, body) = match code {
            "ERR_NAME_NOT_RESOLVED" => (format!("✕ no address found for {host}"), format!("{host} can't be found"), "Check the spelling. If it's right, the site may be gone or your DNS isn't answering.".to_string()),
            "ERR_INTERNET_DISCONNECTED" => ("✕ you're offline".into(), "You're offline".into(), "Connect to a network and nus will try again.".into()),
            "ERR_CONNECTION_REFUSED" => (format!("✕ nothing is listening at {host}"), format!("{host} refused to connect"), if is_local(url) { "If this is your server, check that it's running and on this port.".into() } else { "The site may be down, or a firewall is blocking it.".into() }),
            "ERR_CONNECTION_TIMED_OUT" | "ERR_TIMED_OUT" => (format!("✕ {host} took too long to answer"), format!("{host} took too long to respond"), "The site may be busy or down. Try again in a moment.".into()),
            "ERR_BLOCKED_BY_CLIENT" => (format!("✕ blocked by content blocking · {host}"), format!("{host} was blocked"), "It's on nus's list of ad and tracking hosts. You can turn blocking off for a site in its panel (the gear in the address row).".into()),
            "ERR_TOO_MANY_REDIRECTS" => (format!("✕ {host} kept sending the page elsewhere"), format!("{host} redirected too many times"), "The site sends the request round in a loop and never arrives. Clearing this site's cookies often fixes it.".into()),
            "ERR_EMPTY_RESPONSE" => (format!("✕ {host} answered with nothing"), format!("{host} sent an empty response"), "The server closed the connection without sending a page. Try again in a moment.".into()),
            "ERR_INVALID_AUTH_CREDENTIALS" => (format!("✕ {host} didn't accept the sign-in"), format!("{host} didn't accept the sign-in"), "The username or password wasn't accepted. Try again to enter them again.".into()),
            "ERR_INVALID_RESPONSE" => (format!("✕ {host} sent something that isn't a page"), format!("{host} sent an invalid response"), "The server's answer couldn't be read. It may be misconfigured.".into()),
            "ERR_NETWORK_CHANGED" => ("✕ your network changed".into(), "Your network changed".into(), "The connection dropped while switching networks. Try again.".into()),
            "ERR_CONNECTION_RESET" | "ERR_CONNECTION_CLOSED" => (format!("✕ {host} dropped the connection"), format!("{host} closed the connection"), "The connection was cut before the page arrived. Try again in a moment.".into()),
            _ => (format!("✕ couldn't load {host}"), format!("{host} can't be reached"), "The connection failed before the page arrived.".into()),
        };
        Page::new(Kind::Unreachable, Sev::Problem, url,
            vec![verdict, format!("  error   {code}")], head, body,
            vec![act("retry", "Try again", "↵"), Page::away(can_back)])
    }

    pub fn hung(url: &str, secs: u64) -> Page {
        let host = host(url);
        Page::new(Kind::Hung, Sev::Problem, url,
            vec![format!("✕ {host} hasn't answered for {secs}s")],
            "This page isn't responding".into(),
            "You can wait for it, or stop it. Stopping loses anything unsaved on the page.".into(),
            vec![act("wait", "Wait", "↵"), act("stop", "Stop the page", "S")])
    }

    pub fn sleep(url: &str, asleep_for: std::time::Duration, waking: bool) -> Page {
        let host = host(url);
        let mins = (asleep_for.as_secs() / 60).max(1);
        let mut p = Page::new(Kind::Sleep, Sev::Rest, url,
            vec![format!("· asleep {} · its memory was given back", if mins >= 60 { format!("{}h {}m", mins / 60, mins % 60) } else { format!("{mins} min") }), "  place   kept".into()],
            format!("{host} is asleep"),
            if waking { "Waking it now. Your place on the page comes back with it.".into() } else { "nus put this tab to sleep while it was idle. Your place on the page is kept.".into() },
            if waking { vec![] } else { vec![act("wake", "Wake", "↵")] });
        p.command = format!("wake {url}");
        p
    }

    pub fn permission(url: &str, what: &str) -> Page {
        let host = host(url);
        Page::new(Kind::Permission, Sev::Problem, url,
            vec![format!("✕ macOS denied nus the {what}"), format!("  site    {host} asked for it")],
            format!("{host} can't use your {what}"),
            format!("macOS hasn't given nus access to your {what}. Turn nus on in System Settings under Privacy & Security, then quit and reopen nus."),
            vec![act("settings:privacy", "Open System Settings", "↵"), act("dismiss", "Back to the page", "Esc")])
    }

    pub fn file(url: &str, name: &str, why: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::File, Sev::Danger, url,
            vec![format!("✕ blocked {name}"), format!("  why     {why}"), "  saved   nothing".into()],
            format!("{name} was blocked"),
            format!("It came from {host}. nus stopped the download, so nothing was saved."),
            vec![act("dismiss", "Back to the page", "↵"), act("downloads", "Downloads", "⌘J"), risky("keep", "Keep the file anyway (unsafe)")]);
        p.command = format!("download {url}");
        p
    }

    /// A page's `alert()`: what it says, and a way on.
    pub fn alert(url: &str, message: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Dialog, Sev::Rest, url, vec![format!("· {host} says")], String::new(), message.into(), vec![act("ok", "OK", "↵")]);
        p.command = format!("alert · {host}");
        p
    }

    /// A page's `confirm()`. Cancel is the safe answer, so it takes ↵;
    /// saying yes is a deliberate ⌘↵.
    pub fn confirm(url: &str, message: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Dialog, Sev::Rest, url, vec![format!("· {host} asks")], String::new(), message.into(), vec![act("cancel", "Cancel", "↵"), act("ok", "OK", "⌘↵")]);
        p.command = format!("confirm · {host}");
        p
    }

    /// A page's `prompt()`: the question, a line to answer on.
    pub fn prompt(url: &str, message: &str, default: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Dialog, Sev::Rest, url, vec![format!("· {host} asks")], String::new(), message.into(), vec![act("ok", "OK", "↵"), act("cancel", "Cancel", "Esc")]);
        p.command = format!("prompt · {host}");
        p.fields = vec![Field { label: "answer".into(), value: default.into(), secret: false }];
        p
    }

    /// Leaving (or reloading) a page that says it holds unsaved work.
    pub fn leave(url: &str, reload: bool) -> Page {
        let host = host(url);
        let (verb, what) = if reload { ("reload", "Reload") } else { ("leave", "Leave") };
        let mut p = Page::new(Kind::Dialog, Sev::Problem, url,
            vec![format!("✕ {host} has changes that may not be saved")],
            format!("{what} this page?"),
            "Changes you made may not be saved.".into(),
            vec![act("stay", "Stay on the page", "↵"), act(verb, what, "⌘↵")]);
        p.command = format!("{verb} {url}");
        p
    }

    /// A site (or a proxy) wants a username and password. Over plain HTTP
    /// they travel readable, and the page says so.
    pub fn signin(url: &str, host_name: &str, realm: &str, proxy: bool) -> Page {
        let plain = url.starts_with("http:");
        let who = if proxy { format!("the proxy {host_name}") } else { host_name.to_string() };
        let mut log = vec![format!("✕ {who} wants a sign-in")];
        if !realm.is_empty() {
            log.push(format!("  realm   {realm}"));
        }
        let body = if plain {
            format!("Your username and password go only to {who}, but this connection isn't private, so anyone on the network can read them.")
        } else {
            format!("Your username and password go only to {who}.")
        };
        let mut p = Page::new(Kind::Dialog, if plain { Sev::Danger } else { Sev::Problem }, url, log,
            format!("Sign in to {who}"), body,
            vec![act("signin", "Sign in", "↵"), act("cancel", "Cancel", "Esc")]);
        p.command = format!("sign in · {host_name}");
        p.fields = vec![
            Field { label: "username".into(), value: String::new(), secret: false },
            Field { label: "password".into(), value: String::new(), secret: true },
        ];
        p
    }

    /// `nus://interstitials`: every page, and the commands that cause the real thing.
    pub fn index() -> Page {
        let mut acts: Vec<Act> = Kind::ALL.iter().filter(|k| **k != Kind::Index).map(|k| act(&format!("open:nus://interstitial/{}", k.slug()), &format!("the {} page", k.slug()), "")).collect();
        for (url, what) in DEBUG_URLS {
            acts.push(act(&format!("open:{url}"), what, ""));
        }
        let mut p = Page::new(Kind::Index, Sev::Rest, "nus://interstitials",
            vec![format!("· {} pages, {} commands", Kind::ALL.len() - 1, DEBUG_URLS.len())],
            "Pages nus shows when a site can't".into(),
            "Each nus://interstitial/… address shows a page with sample facts. The commands below it make the real thing happen to the page you're on.".into(),
            acts);
        p.command = "ls nus://interstitials".into();
        p
    }

    /// A page with sample facts, for `nus://interstitial/<kind>`.
    pub fn sample(kind: Kind) -> Page {
        match kind {
            Kind::Cert => Page::cert("https://expired.badssl.com/", "NET::ERR_CERT_DATE_INVALID", true),
            Kind::Malware => Page::malware("http://login-paypa1-secure.example/", "profile/dangerous.txt", true),
            Kind::Clock => Page::clock("https://nus.dev/", 3, true),
            Kind::Oom => Page::crashed("https://figma.com/file/8Hq2/Plot", true, "", false),
            Kind::Crash => Page::crashed("https://maps.example.com/", false, "STATUS_BREAKPOINT", false),
            Kind::Resubmit => Page::resubmit("https://shop.example.com/checkout", true),
            Kind::Portal => Page::portal("https://news.ycombinator.com/", "this network"),
            Kind::Unreachable => Page::unreachable("http://localhost:3000/", "ERR_CONNECTION_REFUSED", true),
            Kind::Index => Page::index(),
            Kind::Hung => Page::hung("https://maps.example.com/", 12),
            Kind::Sleep => Page::sleep("https://docs.rs/tokio/latest/tokio/", std::time::Duration::from_secs(14 * 60), false),
            Kind::Permission => Page::permission("https://meet.example.com/abc-defg", "microphone"),
            Kind::File => Page::file("https://files.example.net/invoice.pdf.exe", "invoice.pdf.exe", "a program named like a document"),
            Kind::Dialog => Page::confirm("https://mail.example.com/", "Delete 3 conversations?"),
        }
    }

    /// Written over the current document, built node by node: Chrome's
    /// own error pages enforce Trusted Types, which refuse any HTML string
    /// (document.write, innerHTML, DOMParser), but not DOM made by hand.
    /// The listeners come from this script, which the page's CSP does not
    /// govern either.
    pub fn script(&self) -> String {
        let c = *COLORS.read().unwrap_or_else(|e| e.into_inner());
        let data = serde_json::json!({
            "title": self.head,
            "sev": match self.sev { Sev::Danger => "danger", Sev::Problem => "", Sev::Rest => "rest" },
            "command": self.command,
            "log": self.log,
            "head": self.head,
            "body": self.body,
            "acts": self.acts.iter().map(|a| serde_json::json!({ "verb": a.verb, "shown": shown_verb(&a.verb), "what": if a.key.is_empty() { a.label.clone() } else { format!("{} · {}", a.label, a.key) }, "unsafe": a.unsafe_ })).collect::<Vec<_>>(),
            "token": self.token,
            "css": self.css(c),
            "scheme": if c.dark { "dark" } else { "light" },
        });
        format!("({})({});", BUILD_JS, data)
    }

    fn css(&self, c: Colors) -> String {
        format!(r#"{fonts}
:root{{--paper:{paper};--ink:{ink};--dim:{dim};--signal:{signal};color-scheme:{scheme}}}
html,body{{margin:0;background:var(--paper);color:var(--ink)}}
body{{font:14px/1.6 "nus mono",ui-monospace,Menlo,Consolas,monospace;-webkit-font-smoothing:antialiased}}
main{{display:grid;grid-template-columns:3px minmax(0,1fr);gap:0 22px;padding:44px 48px;max-width:880px}}
.gut{{background:var(--ink)}}.gut.danger{{background:var(--signal)}}.gut.rest{{background:transparent}}
.line{{white-space:pre-wrap;overflow-wrap:anywhere}}.p{{color:var(--signal)}}.verdict{{font-weight:500}}
.gap{{height:18px}}.head{{font-weight:500;font-size:16px}}.body{{max-width:68ch}}
.cap{{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:var(--dim);margin-bottom:4px}}
.cmd{{display:grid;grid-template-columns:minmax(18ch,max-content) 1fr;gap:18px;cursor:pointer;padding:1px 6px;margin-left:-6px}}
.cmd .what{{color:var(--dim)}}.cmd.unsafe .verb{{color:var(--dim)}}
.cmd:hover{{outline:1px solid var(--ink);outline-offset:-1px}}
.cmd[aria-selected=true]{{background:var(--ink);color:var(--paper)}}.cmd[aria-selected=true] .what,.cmd[aria-selected=true] .verb{{color:var(--paper)}}
.prompt{{display:flex;gap:1ch;align-items:baseline}}
.prompt input{{flex:1;min-width:0;font:inherit;color:var(--ink);background:transparent;border:0;outline:0;padding:0;caret-color:var(--ink)}}
@media (max-width:560px){{main{{padding:24px 18px}}}}"#,
            fonts = font_css(), paper = css(c.paper), ink = css(c.ink), dim = css(c.dim), signal = css(c.signal), scheme = if c.dark { "dark" } else { "light" })
    }

    /// The first safe command, which ↵ takes.
    pub fn default_act(&self) -> Option<&Act> {
        self.acts.iter().find(|a| !a.unsafe_)
    }
}

/// The verb as the transcript shows it: `open:nus://x` reads `open nus://x`.
fn shown_verb(verb: &str) -> String {
    verb.replacen(':', " ", usize::from(verb.starts_with("open:")))
}

/// Builds the transcript from the page's data (see `Page::script`).
const BUILD_JS: &str = r#"(P)=>{
const d=document;d.open();d.close();
const el=(tag,cls,text)=>{const e=d.createElement(tag);if(cls)e.className=cls;if(text!=null)e.textContent=text;return e};
const root=d.documentElement||d.appendChild(d.createElement('html'));
const head=d.head||root.appendChild(d.createElement('head'));
const body=d.body||root.appendChild(d.createElement('body'));
d.title=P.title;
const meta=el('meta');meta.name='color-scheme';meta.content=P.scheme;head.appendChild(meta);
try{const s=new CSSStyleSheet();s.replaceSync(P.css);d.adoptedStyleSheets=[s]}catch(e){head.appendChild(el('style',null,P.css))}
const main=el('main');main.appendChild(el('div','gut '+P.sev));const col=el('div');main.appendChild(col);body.appendChild(main);
const line=(cls,text)=>col.appendChild(el('div','line'+(cls?' '+cls:''),text));
const first=el('div','line');first.append(el('span','p','»'),' '+P.command);col.appendChild(first);
P.log.forEach((l,i)=>line(i===0?'verdict':'',l));
col.appendChild(el('div','gap'));line('head',P.head);line('body',P.body);
const run=v=>{try{window.nusInterstitial(JSON.stringify({token:P.token,verb:v}))}catch(e){}};
const rows=[];
if(P.acts.length){col.appendChild(el('div','gap'));col.appendChild(el('div','cap','Next'));
 const next=el('div');next.setAttribute('role','listbox');col.appendChild(next);
 P.acts.forEach(a=>{const r=el('div','cmd'+(a.unsafe?' unsafe':''));r.setAttribute('role','option');r.dataset.verb=a.verb;
  r.append(el('span','verb','» '+a.shown),el('span','what',a.what));r.onclick=()=>run(a.verb);next.appendChild(r);rows.push(r)})}
let sel=Math.max(0,P.acts.findIndex(a=>!a.unsafe));
const mark=()=>rows.forEach((r,i)=>r.setAttribute('aria-selected',String(i===sel)));
rows.forEach((r,i)=>r.onmouseenter=()=>{sel=i;mark()});
col.appendChild(el('div','gap'));const pr=el('div','prompt');pr.appendChild(el('span','p','»'));
const input=el('input');input.autocomplete='off';input.spellcheck=false;input.setAttribute('aria-label','Type a command, or press Enter for the highlighted one');pr.appendChild(input);col.appendChild(pr);
d.addEventListener('keydown',e=>{
 if(rows.length&&(e.key==='ArrowDown'||e.key==='ArrowUp')){sel=(sel+(e.key==='ArrowDown'?1:rows.length-1))%rows.length;mark();e.preventDefault();return}
 if(e.key==='Enter'){const typed=input.value.trim().toLowerCase();
  if(!typed){if(rows[sel])run(rows[sel].dataset.verb);return}
  const hit=P.acts.find(a=>a.shown.toLowerCase()===typed||a.shown.toLowerCase().split(' ')[0]===typed);
  if(hit)run(hit.verb);else{input.value='';input.placeholder='no such command · try one of the above'}}
 if(e.key==='Escape')input.value=''});
mark();input.focus();
}"#;

/// `nus://` commands that make the real thing happen to the page you're
/// on, so each page can be seen for real.
pub const DEBUG_URLS: [(&str, &str); 4] = [
    ("nus://crash", "crash this page's process"),
    ("nus://oom", "run this page out of memory"),
    ("nus://hang", "hang this page"),
    ("nus://block-download", "download a disguised program"),
];

/// What a `nus://` address asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Internal {
    Show(Page),
    Crash,
    Oom,
    Hang,
    BlockDownload,
}

pub fn internal(url: &str) -> Option<Internal> {
    let rest = url.strip_prefix("nus://")?.trim_end_matches('/');
    Some(match rest {
        "interstitials" | "interstitial" | "" => Internal::Show(Page::index()),
        "crash" => Internal::Crash,
        "oom" => Internal::Oom,
        "hang" => Internal::Hang,
        "block-download" => Internal::BlockDownload,
        _ => Internal::Show(Page::sample(Kind::from_slug(rest.strip_prefix("interstitial/")?)?)),
    })
}

impl PartialEq for Page {
    fn eq(&self, other: &Page) -> bool {
        self.token == other.token
    }
}

// ── Theme, fonts, escaping ───────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub paper: [f32; 4],
    pub ink: [f32; 4],
    pub dim: [f32; 4],
    pub signal: [f32; 4],
    pub dark: bool,
}

static COLORS: RwLock<Colors> = RwLock::new(Colors {
    paper: [1.0, 1.0, 1.0, 1.0],
    ink: [0.0784, 0.0784, 0.0784, 1.0],
    dim: [0.541, 0.522, 0.478, 1.0],
    signal: [0.784, 0.063, 0.180, 1.0],
    dark: false,
});

/// The window's look, for pages written from Chromium's callbacks.
pub fn set_colors(c: Colors) {
    *COLORS.write().unwrap_or_else(|e| e.into_inner()) = c;
}

fn css(c: [f32; 4]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("rgba({},{},{},{})", b(c[0]), b(c[1]), b(c[2]), c[3])
}

/// Plex Mono, the app's own, inlined: the page can't reach nus's files.
fn font_css() -> &'static str {
    static CSS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CSS.get_or_init(|| {
        let face = |bytes: &[u8], weight: u16| format!("@font-face{{font-family:\"nus mono\";font-weight:{weight};src:url(data:font/ttf;base64,{}) format(\"truetype\")}}", base64(bytes));
        face(nus_render::text::bundled::PLEX_MONO, 400) + &face(nus_render::text::bundled::PLEX_MONO_MEDIUM, 500)
    })
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn host(url: &str) -> String {
    let h = crate::sites::host_of(url);
    if h.is_empty() { url.to_string() } else { h }
}

fn is_local(url: &str) -> bool {
    let h = host(url);
    h.starts_with("localhost") || h.starts_with("127.") || h == "[::1]" || h.starts_with("0.0.0.0")
}

fn clock_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil(secs as i64 / 86400);
    format!("{y}-{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600 % 24, secs / 60 % 60)
}

/// Days since 1970-01-01 → (year, month, day).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

// ── Deciding which page ──────────────────────────────────────────────

fn cert_words(code: &str) -> &'static str {
    match code {
        "NET::ERR_CERT_DATE_INVALID" => "the certificate has expired, or isn't valid yet",
        "NET::ERR_CERT_AUTHORITY_INVALID" => "the certificate isn't from an authority nus trusts",
        "NET::ERR_CERT_COMMON_NAME_INVALID" => "the certificate is for a different site",
        "NET::ERR_CERT_REVOKED" => "the certificate was revoked",
        _ => "the certificate couldn't be checked",
    }
}

/// Chromium's error number → its name, for the ones that get a page of
/// their own or a clearer line.
pub fn error_name(code: i32) -> String {
    match code {
        -105 => "ERR_NAME_NOT_RESOLVED".into(),
        -106 => "ERR_INTERNET_DISCONNECTED".into(),
        -102 => "ERR_CONNECTION_REFUSED".into(),
        -118 => "ERR_CONNECTION_TIMED_OUT".into(),
        -7 => "ERR_TIMED_OUT".into(),
        -101 => "ERR_CONNECTION_RESET".into(),
        -100 => "ERR_CONNECTION_CLOSED".into(),
        -109 => "ERR_ADDRESS_UNREACHABLE".into(),
        -20 => "ERR_BLOCKED_BY_CLIENT".into(),
        -21 => "ERR_NETWORK_CHANGED".into(),
        -310 => "ERR_TOO_MANY_REDIRECTS".into(),
        -324 => "ERR_EMPTY_RESPONSE".into(),
        -338 => "ERR_INVALID_AUTH_CREDENTIALS".into(),
        -320 => "ERR_INVALID_RESPONSE".into(),
        -137 => "ERR_NAME_RESOLUTION_FAILED".into(),
        -400 => "ERR_CACHE_MISS".into(),
        -107 => "ERR_SSL_PROTOCOL_ERROR".into(),
        -200 => "NET::ERR_CERT_COMMON_NAME_INVALID".into(),
        -201 => "NET::ERR_CERT_DATE_INVALID".into(),
        -202 => "NET::ERR_CERT_AUTHORITY_INVALID".into(),
        -206 => "NET::ERR_CERT_REVOKED".into(),
        c if (-299..=-200).contains(&c) => format!("NET::ERR_CERT ({c})"),
        c => format!("ERR {c}"),
    }
}

/// The page for a main-frame load error. `now` and `built` are Unix
/// seconds: a date error while the clock reads before this build existed
/// (or years after) is the clock's fault.
pub fn for_error(url: &str, code: i32, can_back: bool, now: i64, built: i64) -> Page {
    let name = error_name(code);
    if code == -201 {
        let behind = (built - now) / 86400;
        if behind >= 1 {
            return Page::clock(url, behind, can_back);
        }
        let ahead = (now - built) / 86400;
        if ahead > 3 * 365 {
            return Page::clock(url, -(ahead), can_back);
        }
    }
    if (-299..=-200).contains(&code) {
        return Page::cert(url, &name, can_back);
    }
    if code == -400 {
        return Page::resubmit(url, can_back);
    }
    Page::unreachable(url, &name, can_back)
}

/// Errors a captive portal could be behind: worth asking the network.
pub fn portal_suspect(code: i32) -> bool {
    matches!(code, -105 | -118 | -7 | -101 | -100 | -109 | -107 | -200 | -202)
}

/// When this build was made (its commit), Unix seconds.
pub fn built() -> i64 {
    option_env!("NUS_BUILD_EPOCH").and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Ask Apple's plain-HTTP probe whether this network intercepts pages.
/// Some(network) when it does. Blocks up to a few seconds; run it off
/// the UI thread.
pub fn probe_portal() -> Option<String> {
    use std::io::{Read, Write};
    use std::net::ToSocketAddrs;
    let addr = ("captive.apple.com", 80).to_socket_addrs().ok()?.next()?;
    let mut s = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3)).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(3))).ok()?;
    s.write_all(b"GET /hotspot-detect.html HTTP/1.0\r\nHost: captive.apple.com\r\nUser-Agent: CaptiveNetworkSupport\r\n\r\n").ok()?;
    let mut body = Vec::new();
    let _ = s.take(64 * 1024).read_to_end(&mut body);
    let text = String::from_utf8_lossy(&body);
    if text.is_empty() || text.contains("<BODY>Success</BODY>") || text.contains("Success") {
        return None;
    }
    // Where the network wanted to send us, when it said.
    let to = text.lines().find_map(|l| l.strip_prefix("Location:").or_else(|| l.strip_prefix("location:"))).map(|l| host(l.trim()));
    Some(to.filter(|h| !h.is_empty()).unwrap_or_else(|| "this network".into()))
}

/// A camera or microphone macOS itself refuses nus (denied or restricted
/// in Privacy & Security), in words; None when it would let nus ask.
#[cfg(target_os = "macos")]
pub fn os_denied(audio: bool, video: bool) -> Option<&'static str> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVMediaTypeAudio: &'static AnyObject;
        static AVMediaTypeVideo: &'static AnyObject;
    }
    // AVAuthorizationStatus: 1 restricted, 2 denied.
    let refused = |t: &AnyObject| -> bool {
        let status: isize = unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: t] };
        matches!(status, 1 | 2)
    };
    let a = audio && refused(unsafe { AVMediaTypeAudio });
    let v = video && refused(unsafe { AVMediaTypeVideo });
    match (v, a) {
        (true, true) => Some("camera and microphone"),
        (true, false) => Some("camera"),
        (false, true) => Some("microphone"),
        _ => None,
    }
}
#[cfg(not(target_os = "macos"))]
pub fn os_denied(_audio: bool, _video: bool) -> Option<&'static str> {
    None
}

// ── Lists: hosts you've said are dangerous, and what you let through ──

static DANGEROUS: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
static ALLOWED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// profile/dangerous.txt: one host per line (`||host^` works too). nus
/// has no Safe Browsing service; this list is yours.
pub fn dangerous(url: &str) -> bool {
    let set = DANGEROUS.get_or_init(|| {
        let path = std::env::current_dir().unwrap_or_default().join("profile").join("dangerous.txt");
        std::fs::read_to_string(path).unwrap_or_default().lines().map(|l| l.trim().trim_start_matches("||").split(['^', '/', '$']).next().unwrap_or("").trim().to_lowercase()).filter(|h| !h.is_empty() && !h.starts_with(['#', '!'])).collect()
    });
    let h = host(url).to_lowercase();
    let mut part = h.as_str();
    loop {
        if set.contains(part) {
            return !allowed(&format!("site:{h}"));
        }
        match part.find('.') {
            Some(i) => part = &part[i + 1..],
            None => return false,
        }
    }
}

/// You went ahead: this session only.
pub fn allow(key: String) {
    ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashSet::new).insert(key);
}

pub fn allowed(key: &str) -> bool {
    ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| s.contains(key))
}

/// A download name that is a program in disguise, or a kind that is
/// only ever used to attack: Some(why).
pub fn dangerous_file(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    let mut parts = lower.rsplit('.');
    let last = parts.next().unwrap_or("");
    let inner = parts.next().unwrap_or("");
    const PROGRAM: [&str; 14] = ["exe", "scr", "bat", "cmd", "com", "pif", "msi", "vbs", "vbe", "js", "jse", "wsf", "ps1", "command"];
    const DOCUMENT: [&str; 14] = ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "jpg", "jpeg", "png", "gif", "mp3", "mp4"];
    if PROGRAM.contains(&last) && DOCUMENT.contains(&inner) {
        return Some("a program named like a document");
    }
    if matches!(last, "scr" | "pif" | "vbe" | "jse") {
        return Some("a kind of file used almost only to attack");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_pick_their_page() {
        let built = 1_790_000_000;
        assert_eq!(for_error("https://a.test/", -201, true, built, built).kind, Kind::Cert);
        assert_eq!(for_error("https://a.test/", -201, true, built - 3 * 86400, built).kind, Kind::Clock);
        assert_eq!(for_error("https://a.test/", -202, true, built, built).kind, Kind::Cert);
        assert_eq!(for_error("https://a.test/", -400, true, built, built).kind, Kind::Resubmit);
        let p = for_error("http://localhost:3000/", -102, false, built, built);
        assert_eq!(p.kind, Kind::Unreachable);
        assert!(p.head.contains("refused"));
        // Close isn't first here, so ↵ stays with Try again.
        assert_eq!(p.acts.iter().find(|a| a.verb == "close").map(|a| a.key), Some(""));
        assert_eq!(p.default_act().map(|a| a.key), Some("↵"));
    }

    #[test]
    fn going_ahead_is_never_the_default() {
        for k in Kind::ALL {
            let p = Page::sample(k);
            if let Some(a) = p.default_act() {
                assert!(!a.unsafe_, "{k:?}");
            }
            let first_unsafe = p.acts.iter().position(|a| a.unsafe_);
            if let Some(i) = first_unsafe {
                assert!(p.acts[i..].iter().all(|a| a.unsafe_), "{k:?}: unsafe commands come last");
            }
        }
    }

    #[test]
    fn internal_addresses() {
        assert!(matches!(internal("nus://interstitials"), Some(Internal::Show(p)) if p.kind == Kind::Index));
        assert!(matches!(internal("nus://interstitial/cert"), Some(Internal::Show(p)) if p.kind == Kind::Cert));
        assert_eq!(internal("nus://crash"), Some(Internal::Crash));
        assert_eq!(internal("nus://interstitial/nope"), None);
        assert_eq!(internal("https://nus.dev"), None);
    }

    #[test]
    fn disguised_programs() {
        assert!(dangerous_file("invoice.pdf.exe").is_some());
        assert!(dangerous_file("Invoice.PDF.EXE").is_some());
        assert!(dangerous_file("screensaver.scr").is_some());
        assert!(dangerous_file("setup.exe").is_none());
        assert!(dangerous_file("report.pdf").is_none());
        assert!(dangerous_file("nus.dmg").is_none());
    }

    #[test]
    fn html_carries_the_token_and_escapes() {
        let mut p = Page::unreachable("https://a.test/<script>", "ERR_NAME_NOT_RESOLVED", true);
        p.token = "tok".into();
        let h = p.script();
        assert!(h.contains("\"tok\""));
        // Text goes in as JSON data, set with textContent: never as markup.
        assert!(h.contains("https://a.test/<script>"));
        assert!(!h.contains("innerHTML") && !h.contains("document.write"));
        assert_eq!(base64(b"nus"), "bnVz");
        assert_eq!(base64(b"nu"), "bnU=");
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20720), (2026, 9, 24));
    }
}

//! Hands: the assistant acting on the page beside its shell, in sight.
//!
//! `nus mcp` (crates/cli) turns `nus_page_click` and friends into the
//! `hands` verb of the instance protocol. Every hand goes through the
//! policy first — ASSISTANTS · HANDS: ASK (default) · ALWAYS · NEVER, and
//! the hosts you allowed for good — and, when it needs asking, draws a band
//! over the page: *claude wants to click Submit · ALLOW · DENY · ALLOW ON
//! THIS HOST*. Allowed, it runs by CDP on the pane (`Input.dispatch…`,
//! `Input.insertText`, `Page.navigate`). Each one leaves a chip under the
//! URL row for a while — an icon with the words in its tooltip — and a
//! line in the page's log. Any click or key of yours on that pane while a
//! hand is waiting takes over: the tool is told *taken over*.

use std::sync::mpsc::Sender;
use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene};
use serde_json::{json, Value};

use crate::app::{fade, App, Pane, WebPane};
use nus_render::theme::metric as m;

/// What a hand does.
#[derive(Clone, Debug, PartialEq)]
pub enum Do {
    Click { selector: Option<String>, x: Option<f64>, y: Option<f64> },
    Type { text: String, enter: bool },
    Scroll { dy: f64 },
    Navigate { url: String },
}

impl Do {
    /// The words for the band and the tooltip: *click "Submit"*, *type 12 characters*.
    pub fn label(&self) -> String {
        match self {
            Do::Click { selector: Some(s), .. } => format!("click {s}"),
            Do::Click { x: Some(x), y: Some(y), .. } => format!("click at {x:.0}, {y:.0}"),
            Do::Click { .. } => "click".into(),
            Do::Type { text, enter } => format!("type {} character{}{}", text.chars().count(), if text.chars().count() == 1 { "" } else { "s" }, if *enter { " and enter" } else { "" }),
            Do::Scroll { dy } => format!("scroll {}", if *dy < 0.0 { "up" } else { "down" }),
            Do::Navigate { url } => format!("go to {url}"),
        }
    }

    /// Where it is going: a navigation's host, else the page's.
    fn is_submit(&self) -> bool {
        match self {
            Do::Type { enter, .. } => *enter,
            Do::Click { selector: Some(s), .. } => {
                let s = s.to_lowercase();
                s.contains("submit") || s.contains("button") || s.contains("[type=submit]")
            }
            Do::Navigate { .. } => true,
            _ => false,
        }
    }
}

/// A hand waiting on you.
pub struct Ask {
    pub who: String,
    pub what: Do,
    pub reply: Sender<Value>,
    pub since: Instant,
}

/// A hand that ran (or did not), shown as a chip for a while.
#[derive(Clone, Debug)]
pub struct Done {
    pub who: String,
    pub what: Do,
    pub outcome: Outcome,
    pub at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ran,
    Denied,
    TakenOver,
}

/// Per-pane hands state.
#[derive(Default)]
pub struct Hands {
    pub ask: Option<Ask>,
    pub done: Vec<Done>,
    pub hits: Vec<(Rect, Answer)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Allow,
    Deny,
    AllowHost,
}

fn parse(args: &Value) -> Result<Do, String> {
    let s = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
    let f = |k: &str| args.get(k).and_then(Value::as_f64);
    match s("what").as_deref().unwrap_or("") {
        "click" => {
            let selector = s("selector").filter(|x| !x.trim().is_empty());
            let (x, y) = (f("x"), f("y"));
            if selector.is_none() && (x.is_none() || y.is_none()) {
                return Err("click needs a selector or x and y".into());
            }
            Ok(Do::Click { selector, x, y })
        }
        "type" => Ok(Do::Type { text: s("text").ok_or("type needs text")?, enter: args.get("enter").and_then(Value::as_bool).unwrap_or(false) }),
        "scroll" => Ok(Do::Scroll { dy: f("dy").ok_or("scroll needs dy")? }),
        "navigate" => Ok(Do::Navigate { url: s("url").ok_or("navigate needs a url")? }),
        other => Err(format!("hands: click · type · scroll · navigate, not {other}")),
    }
}

impl App {
    /// The `hands` verb: policy, then the band or the act.
    pub(crate) fn hands_request(&mut self, args: &Value, reply: Sender<Value>) {
        let answer = |reply: &Sender<Value>, v: Value| {
            let _ = reply.send(v);
        };
        let what = match parse(args) {
            Ok(w) => w,
            Err(e) => return answer(&reply, json!({ "ok": false, "error": e })),
        };
        let who = args.get("who").and_then(Value::as_str).unwrap_or("the assistant").to_string();
        let (tab, right) = match self.page_pane_for(args) {
            Ok(p) => p,
            Err(e) => return answer(&reply, json!({ "ok": false, "error": e })),
        };
        use crate::settings::HandsMode;
        let mode = self.behavior.hands;
        let host = self.web_pane_ref(tab, right).map(|w| crate::sites::host_of(&w.tab.shared.borrow().url)).unwrap_or_default();
        let allowed_host = self.behavior.hands_hosts.iter().any(|h| h.eq_ignore_ascii_case(&host));
        let must_ask = match mode {
            HandsMode::Never => return answer(&reply, json!({ "ok": false, "error": "hands are off (ASSISTANTS · HANDS)" })),
            HandsMode::Always => false,
            HandsMode::Ask => !allowed_host || what.is_submit() && self.behavior.hands_confirm_submit,
        };
        if let Some(w) = self.web_pane_mut(tab, right) {
            if w.hands.ask.is_some() {
                return answer(&reply, json!({ "ok": false, "error": "a hand is already waiting on the user" }));
            }
        }
        if must_ask {
            if let Some(w) = self.web_pane_mut(tab, right) {
                w.hands.ask = Some(Ask { who, what, reply, since: crate::clock::now() });
                self.band_anim.replay(0.0, 1.0, self.motion.dur(crate::anim::base::BAND));
                self.play_event("toggle");
                self.dirty = true;
            }
            return;
        }
        let outcome = self.hands_do(tab, right, &who, &what);
        answer(&reply, outcome);
    }

    /// The band's answer, from a chip or a key.
    pub(crate) fn hands_answer(&mut self, tab: usize, right: bool, a: Answer) {
        let host = self.web_pane_ref(tab, right).map(|w| crate::sites::host_of(&w.tab.shared.borrow().url)).unwrap_or_default();
        let Some(ask) = self.web_pane_mut(tab, right).and_then(|w| w.hands.ask.take()) else { return };
        match a {
            Answer::Deny => {
                let _ = ask.reply.send(json!({ "ok": false, "error": "denied by the user" }));
                if let Some(w) = self.web_pane_mut(tab, right) {
                    w.hands.done.push(Done { who: ask.who, what: ask.what, outcome: Outcome::Denied, at: crate::clock::now() });
                }
            }
            Answer::Allow | Answer::AllowHost => {
                if a == Answer::AllowHost && !host.is_empty() && !self.behavior.hands_hosts.iter().any(|h| h == &host) {
                    self.behavior.hands_hosts.push(host);
                    self.save_prefs();
                }
                let v = self.hands_do(tab, right, &ask.who, &ask.what);
                let _ = ask.reply.send(v);
            }
        }
        self.play_event("toggle");
        self.dirty = true;
    }

    /// Your input on a pane with a hand waiting: the hand is off.
    pub(crate) fn hands_taken_over(&mut self, tab: usize, right: bool) {
        let Some(ask) = self.web_pane_mut(tab, right).and_then(|w| w.hands.ask.take()) else { return };
        let _ = ask.reply.send(json!({ "ok": false, "error": "taken over by the user" }));
        if let Some(w) = self.web_pane_mut(tab, right) {
            w.hands.done.push(Done { who: ask.who, what: ask.what, outcome: Outcome::TakenOver, at: crate::clock::now() });
        }
        self.dirty = true;
    }

    /// Do it, by CDP on the pane, and leave a chip and a log line.
    fn hands_do(&mut self, tab: usize, right: bool, who: &str, what: &Do) -> Value {
        let scale = self.scale;
        let Some(w) = self.web_pane_mut(tab, right) else { return json!({ "ok": false, "error": "no page" }) };
        let (pw, ph) = (w.page.w / scale, w.page.h / scale);
        match what {
            Do::Click { selector: Some(sel), .. } => {
                // The element's centre, then a real click there; a JS click as the fallback.
                let expr = format!(
                    "(function(){{ const n = document.querySelector({sel}); if (!n) return null; n.scrollIntoView({{ block: 'center', inline: 'center' }}); const r = n.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; }})()",
                    sel = serde_json::to_string(sel).unwrap_or_default()
                );
                w.tab.eval(&format!("(function(){{ const n = document.querySelector({}); if (n) n.click(); }})()", serde_json::to_string(sel).unwrap_or_default()));
                let _ = expr;
            }
            Do::Click { x: Some(x), y: Some(y), .. } => {
                let (x, y) = (x.clamp(0.0, pw as f64), y.clamp(0.0, ph as f64));
                w.tab.devtools("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y }));
                w.tab.devtools("Input.dispatchMouseEvent", json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 }));
                w.tab.devtools("Input.dispatchMouseEvent", json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 }));
            }
            Do::Click { .. } => {}
            Do::Type { text, enter } => {
                w.tab.devtools("Input.insertText", json!({ "text": text }));
                if *enter {
                    w.tab.devtools("Input.dispatchKeyEvent", json!({ "type": "keyDown", "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "text": "\r" }));
                    w.tab.devtools("Input.dispatchKeyEvent", json!({ "type": "keyUp", "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13 }));
                }
            }
            Do::Scroll { dy } => {
                w.tab.eval(&format!("window.scrollBy({{ top: {dy}, behavior: 'smooth' }})"));
            }
            Do::Navigate { url } => {
                let url = if url.contains("://") { url.clone() } else { format!("https://{url}") };
                w.tab.load(&url);
            }
        }
        let entry = json!({ "kind": "hand", "who": who, "what": what.label(), "at": crate::journal::now() });
        {
            let mut s = w.tab.shared.borrow_mut();
            if s.log.len() >= 400 {
                s.log.remove(0);
            }
            s.log.push(entry);
        }
        w.hands.done.push(Done { who: who.to_string(), what: what.clone(), outcome: Outcome::Ran, at: crate::clock::now() });
        w.hands.done.retain(|d| crate::clock::since(d.at).as_secs() < 60);
        self.dirty = true;
        json!({ "ok": true, "result": { "did": what.label() } })
    }

    fn page_pane_for(&self, args: &Value) -> Result<(usize, bool), String> {
        let i = args.get("tab").and_then(|t| t.as_u64()).map(|t| (t as usize).saturating_sub(1)).unwrap_or(self.active);
        let tab = self.tabs.get(i).ok_or("no such tab")?;
        if matches!(tab.left, Pane::Web(_)) {
            return Ok((i, false));
        }
        if matches!(tab.right, Some(Pane::Web(_))) {
            return Ok((i, true));
        }
        Err("no page on that tab".into())
    }

    pub(crate) fn web_pane_ref(&self, tab: usize, right: bool) -> Option<&WebPane> {
        self.tabs.get(tab).and_then(|t| if right { t.right.as_ref() } else { Some(&t.left) }).and_then(|p| match p {
            Pane::Web(w) => Some(w),
            _ => None,
        })
    }

    pub(crate) fn web_pane_mut(&mut self, tab: usize, right: bool) -> Option<&mut WebPane> {
        self.tabs.get_mut(tab).and_then(|t| if right { t.right.as_mut() } else { Some(&mut t.left) }).and_then(|p| match p {
            Pane::Web(w) => Some(w),
            _ => None,
        })
    }

    /// The band over the page while a hand waits, and the chips of recent
    /// hands under the URL row. Drawn with the web overlays.
    pub(crate) fn draw_hands(&mut self, scene: &mut Scene, w: &mut WebPane) {
        let t = self.theme.clone();
        let ink = t.ink;
        let page = w.page;
        let strong = self.label_strong();
        let label = self.label();
        w.hands.hits.clear();
        if let Some(ask) = w.hands.ask.as_ref() {
            let bh = self.header_h();
            let drop = self.band_anim.value();
            let br = Rect::new(page.x, page.y - (1.0 - drop) * bh, page.w, bh);
            scene.layer(Some(Rect::new(page.x, page.y, page.w, bh)));
            scene.rect(br, ink);
            let inv = Style { color: t.paper, ..strong };
            let inv_l = Style { color: t.paper, ..label };
            let by = br.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
            let mut x = br.x + self.px(m::HEADER_PAD_X);
            let isz = self.px(13.0);
            self.fonts.draw_icon(scene, nus_render::text::icons::ASSISTANT, isz, x, br.y + (bh - isz) / 2.0, t.paper);
            x += isz + self.px(8.0);
            x += self.fonts.draw(scene, inv, x, by, &ask.who.to_uppercase()) + self.px(8.0);
            x += self.fonts.draw(scene, inv_l, x, by, &format!("WANTS TO {}", ask.what.label().to_uppercase())) + self.px(18.0);
            for (word, a) in [("ALLOW", Answer::Allow), ("DENY", Answer::Deny), ("ALLOW ON THIS HOST", Answer::AllowHost)] {
                let ww = self.fonts.measure(inv, word) + self.px(20.0);
                if x + ww > br.right() - self.px(8.0) {
                    break;
                }
                let chip = Rect::new(x, br.y + self.px(6.0), ww, bh - self.px(12.0));
                scene.outline(chip, self.px(m::HAIRLINE), t.paper);
                if a == Answer::Allow {
                    scene.rect(Rect::new(chip.x + self.px(1.0), chip.y + self.px(1.0), chip.w - self.px(2.0), chip.h - self.px(2.0)), self.surface.signal);
                }
                self.fonts.draw(scene, inv, x + self.px(10.0), by, word);
                w.hands.hits.push((chip, a));
                x += ww + self.px(8.0);
            }
            scene.layer(None);
            self.dirty = true;
        }
        // Recent hands: an icon chip each, newest first, fading out over a minute.
        w.hands.done.retain(|d| crate::clock::since(d.at).as_secs() < 60);
        if !w.hands.done.is_empty() && w.hands.ask.is_none() {
            let isz = self.px(12.0);
            let pad = self.px(4.0);
            let mut x = page.right() - self.px(10.0) - isz;
            let y = page.y + self.px(8.0);
            let (mx, my) = self.mouse;
            for d in w.hands.done.iter().rev().take(6) {
                let age = crate::clock::since(d.at).as_secs_f32();
                let alpha = (1.0 - (age - 45.0).max(0.0) / 15.0).clamp(0.0, 1.0);
                let chip = Rect::new(x - pad, y - pad, isz + pad * 2.0, isz + pad * 2.0);
                scene.rect(chip, fade(self.paper(), 0.9 * alpha));
                scene.outline(chip, self.px(m::HAIRLINE), fade(ink, 0.5 * alpha));
                let color = match d.outcome {
                    Outcome::Ran => fade(ink, alpha),
                    Outcome::Denied | Outcome::TakenOver => fade(self.surface.signal, alpha),
                };
                let icon = match d.what {
                    Do::Click { .. } => nus_render::text::icons::CURSOR,
                    Do::Type { .. } => nus_render::text::icons::KEYBOARD,
                    Do::Scroll { .. } => nus_render::text::icons::CARET_DOWN,
                    Do::Navigate { .. } => nus_render::text::icons::GLOBE,
                };
                self.fonts.draw_icon(scene, icon, isz, x, y, color);
                if chip.contains(mx, my) {
                    let words = match d.outcome {
                        Outcome::Ran => format!("{} · {} · {}s ago", d.who, d.what.label(), age as u32),
                        Outcome::Denied => format!("{} · {} · denied", d.who, d.what.label()),
                        Outcome::TakenOver => format!("{} · {} · you took over", d.who, d.what.label()),
                    };
                    self.tip_words(chip, &words);
                }
                x -= isz + pad * 2.0 + self.px(4.0);
            }
            if w.hands.done.iter().any(|d| crate::clock::since(d.at).as_secs() >= 44) {
                self.dirty = true;
            }
        }
    }

    /// A click on the hands band. Returns true when it was one.
    pub(crate) fn hands_click(&mut self, x: f32, y: f32) -> bool {
        let i = self.active;
        // Decide first, act after: the panes are borrowed while deciding.
        let mut decision: Option<(bool, Option<Answer>)> = None;
        if let Some(tab) = self.tabs.get(i) {
            for (right, p) in [(false, Some(&tab.left)), (true, tab.right.as_ref())] {
                let Some(Pane::Web(w)) = p else { continue };
                if let Some(&(_, a)) = w.hands.hits.iter().find(|(r, _)| r.contains(x, y)) {
                    decision = Some((right, Some(a)));
                    break;
                }
                // Your click on a page with a hand waiting: you took over.
                if w.hands.ask.is_some() && w.page.contains(x, y) {
                    decision = Some((right, None));
                    break;
                }
            }
        }
        match decision {
            Some((right, Some(a))) => {
                self.hands_answer(i, right, a);
                true
            }
            Some((right, None)) => {
                self.hands_taken_over(i, right);
                true
            }
            None => false,
        }
    }

    /// A key while a hand waits on the focused page: y/enter · n/esc · h.
    /// Returns true when it was one of those.
    pub(crate) fn hands_key(&mut self, key: &winit::keyboard::Key) -> bool {
        use winit::keyboard::{Key as K, NamedKey};
        let i = self.active;
        let Some(tab) = self.tabs.get(i) else { return false };
        let right = tab.focus_right;
        let Some(w) = self.web_pane_ref(i, right) else { return false };
        if w.hands.ask.is_none() {
            return false;
        }
        let a = match key {
            // Modifiers on their own are not a take-over.
            K::Named(NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Super | NamedKey::Meta | NamedKey::CapsLock | NamedKey::Fn) => return false,
            K::Named(NamedKey::Enter) => Answer::Allow,
            K::Named(NamedKey::Escape) => Answer::Deny,
            K::Character(c) if c.eq_ignore_ascii_case("y") => Answer::Allow,
            K::Character(c) if c.eq_ignore_ascii_case("n") => Answer::Deny,
            K::Character(c) if c.eq_ignore_ascii_case("h") => Answer::AllowHost,
            _ => {
                self.hands_taken_over(i, right);
                return true;
            }
        };
        self.hands_answer(i, right, a);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_label() {
        let d = parse(&json!({ "what": "click", "selector": "#go" })).unwrap();
        assert_eq!(d.label(), "click #go");
        let d = parse(&json!({ "what": "type", "text": "hello", "enter": true })).unwrap();
        assert_eq!(d.label(), "type 5 characters and enter");
        assert!(d.is_submit());
        assert!(parse(&json!({ "what": "click" })).is_err());
        assert!(parse(&json!({ "what": "scroll", "dy": -300 })).unwrap().label().contains("up"));
    }
}

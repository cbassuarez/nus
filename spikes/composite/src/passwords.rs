//! Saved sign-ins for pages: offered when a login form is sent, filled on
//! request when one comes back.
//!
//! The page script only reports: a password form was sent (what was in it),
//! or a password field is on the page. Which site it was comes from the
//! JavaScript context Chromium names for the call, never from the page's
//! word, so a page can't file a password under another site or ask for
//! one. Nothing is saved without Save, nothing filled without Fill, and a
//! fill goes only to the context of the site it was saved for. Kept in
//! profile/passwords.json, sealed with the profile's keychain key like the
//! rest of nus's sensitive state; never in incognito.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Injected into every document: reports sent password forms and password
/// fields, and defines the fill (which page script can't replace).
pub const JS: &str = r#"(()=>{try{
if(window.__nusPw)return;Object.defineProperty(window,'__nusPw',{value:true});
const send=o=>{try{nusPassword(JSON.stringify(o))}catch(_){}};
const shown=el=>!!el&&el.getClientRects().length>0;
const pws=()=>[...document.querySelectorAll('input[type=password]')].filter(shown);
const userFor=pw=>{const scope=pw.form||document;let best=null;
 for(const i of scope.querySelectorAll('input')){if(i===pw||!shown(i))continue;
  if(!/^(text|email|tel|)$/.test(i.type))continue;
  if(i.compareDocumentPosition(pw)&Node.DOCUMENT_POSITION_FOLLOWING)best=i;}
 return best||scope.querySelector('input[autocomplete~=username]')};
let last='';
const capture=()=>{const f=pws().filter(p=>p.value);if(!f.length)return;
 let pw=f[0];if(f.length>1&&f[f.length-1].value===f[f.length-2].value)pw=f[f.length-1];
 const u=userFor(f[0]);const key=(u?u.value:'')+'\n'+pw.value;if(key===last)return;last=key;
 send({kind:'sent',user:u?u.value:'',pass:pw.value})};
addEventListener('submit',capture,true);
addEventListener('click',e=>{const t=e.target;if(t&&t.closest&&t.closest('button,input[type=submit],input[type=image],[role=button]'))capture()},true);
addEventListener('keydown',e=>{if(e.key==='Enter'&&e.target&&e.target.tagName==='INPUT')capture()},true);
addEventListener('pagehide',capture,true);
let told=false;const look=()=>{if(!told&&pws().length){told=true;send({kind:'form'})}};
const set=(el,v)=>{const d=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');d.set.call(el,v);
 el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}))};
Object.defineProperty(window,'__nusFill',{value:(user,pass)=>{const pw=pws()[0];if(!pw)return false;
 const u=userFor(pw);if(u&&user)set(u,user);set(pw,pass);return true}});
new MutationObserver(look).observe(document,{subtree:true,childList:true});
addEventListener('DOMContentLoaded',look);look();
}catch(_){}})()"#;

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Login {
    pub origin: String,
    pub user: String,
    pub pass: String,
    pub saved: u64,
    pub used: u64,
}

/// What a page reported, with the site Chromium says it came from.
#[derive(Clone, Debug)]
pub enum Report {
    Sent { origin: String, user: String, pass: String },
    Form { origin: String, context: i64 },
}

fn path() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile/passwords.json")
}

pub fn load() -> Vec<Login> {
    if crate::private::enabled() {
        return Vec::new();
    }
    crate::protected_state::read_text(&path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn store(list: &[Login]) -> std::io::Result<()> {
    if crate::private::enabled() {
        return Ok(());
    }
    crate::protected_state::write_json(&path(), &list)
}

/// Sites a password may be saved for: https, or this machine.
pub fn savable(origin: &str) -> bool {
    origin.starts_with("https://") || origin.starts_with("http://localhost") || origin.starts_with("http://127.0.0.1")
}

pub fn host(origin: &str) -> &str {
    origin.split("://").nth(1).unwrap_or(origin)
}

/// What to offer for a sent form: None when it's already saved as is.
pub fn offer(list: &[Login], origin: &str, user: &str, pass: &str) -> Option<bool> {
    match list.iter().find(|l| l.origin == origin && l.user == user) {
        Some(l) if l.pass == pass => None,
        Some(_) => Some(true),
        None => Some(false),
    }
}

/// Save or replace the sign-in for (origin, user).
pub fn upsert(list: &mut Vec<Login>, origin: &str, user: &str, pass: &str, now: u64) {
    if let Some(l) = list.iter_mut().find(|l| l.origin == origin && l.user == user) {
        l.pass = pass.into();
        l.saved = now;
    } else {
        list.push(Login { origin: origin.into(), user: user.into(), pass: pass.into(), saved: now, used: 0 });
    }
}

/// The sign-in to fill for a site: the one used most recently.
pub fn best<'a>(list: &'a [Login], origin: &str) -> Option<&'a Login> {
    list.iter().filter(|l| l.origin == origin).max_by_key(|l| l.used.max(l.saved))
}

/// The page-side fill call, with the values as JSON string literals.
pub fn fill_js(user: &str, pass: &str) -> String {
    format!(
        "window.__nusFill&&window.__nusFill({},{})",
        serde_json::Value::String(user.into()),
        serde_json::Value::String(pass.into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_save_then_update_then_nothing() {
        let mut list = Vec::new();
        assert_eq!(offer(&list, "https://a.test", "me", "one"), Some(false));
        upsert(&mut list, "https://a.test", "me", "one", 1);
        assert_eq!(offer(&list, "https://a.test", "me", "one"), None);
        assert_eq!(offer(&list, "https://a.test", "me", "two"), Some(true));
        upsert(&mut list, "https://a.test", "me", "two", 2);
        assert_eq!(list.len(), 1);
        assert_eq!(best(&list, "https://a.test").unwrap().pass, "two");
        assert!(best(&list, "https://b.test").is_none());
    }

    #[test]
    fn only_secure_sites_and_values_stay_quoted() {
        assert!(savable("https://a.test"));
        assert!(savable("http://localhost:3000"));
        assert!(!savable("http://a.test"));
        assert_eq!(fill_js("a\"b", "c'</script>"), r#"window.__nusFill&&window.__nusFill("a\"b","c'</script>")"#);
    }
}

/// A sign-in waiting on the user's Save or Fill: in memory only, and only
/// the latest of each.
#[derive(Default)]
pub struct Offers {
    pub save: Option<(String, String, String)>,
    pub fill: Option<(u64, bool, i64, String)>,
}

impl crate::app::App {
    /// Once a loop: what pages reported, as a Save or a Fill offer.
    pub(crate) fn tend_passwords(&mut self) {
        if crate::private::enabled() {
            return;
        }
        let mut reports = Vec::new();
        for tab in &self.tabs {
            for (right, pane) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|p| (true, p))) {
                if let crate::app::Pane::Web(w) = pane {
                    for r in std::mem::take(&mut w.tab.shared.borrow_mut().passwords) {
                        reports.push((tab.id, right, r));
                    }
                }
            }
        }
        if reports.is_empty() {
            return;
        }
        let list = load();
        use crate::toast::Act;
        use nus_render::text::icons;
        for (tab, right, report) in reports {
            match report {
                Report::Sent { origin, user, pass } => {
                    if !savable(&origin) {
                        continue;
                    }
                    let Some(update) = offer(&list, &origin, &user, &pass) else { continue };
                    let who = if user.is_empty() { host(&origin).to_string() } else { format!("{user} · {}", host(&origin)) };
                    let words = if update { "Update Password?" } else { "Save Password?" };
                    self.passwords.save = Some((origin, user, pass));
                    self.toast(icons::LOCK_KEY, words, format!("{who} · kept in this profile, encrypted"), Some(Act::SavePassword));
                }
                Report::Form { origin, context } => {
                    let Some(login) = best(&list, &origin) else { continue };
                    let n = list.iter().filter(|l| l.origin == origin).count();
                    let who = if login.user.is_empty() { host(&origin).to_string() } else { login.user.clone() };
                    let more = if n > 1 { format!(" · {} saved", n) } else { String::new() };
                    self.passwords.fill = Some((tab, right, context, origin.clone()));
                    self.toast(icons::LOCK_KEY, "Sign In", format!("{who} · {}{more}", host(&origin)), Some(Act::FillPassword));
                }
            }
        }
    }

    pub(crate) fn save_offered_password(&mut self) {
        let Some((origin, user, pass)) = self.passwords.save.take() else { return };
        let mut list = load();
        upsert(&mut list, &origin, &user, &pass, crate::journal::now());
        match store(&list) {
            Ok(()) => self.toast(nus_render::text::icons::CHECK, "Password Saved", host(&origin).to_string(), None),
            Err(e) => self.toast_problem("Could Not Save Password", e.to_string(), None),
        }
    }

    pub(crate) fn fill_offered_password(&mut self) {
        let Some((tab, right, context, origin)) = self.passwords.fill.take() else { return };
        let mut list = load();
        let Some(login) = best(&list, &origin).cloned() else { return };
        let Some(t) = self.tabs.iter().find(|t| t.id == tab) else { return };
        let Some(crate::app::Pane::Web(w)) = (if right { t.right.as_ref() } else { Some(&t.left) }) else { return };
        // Only into the world that site's page asked from, and only if
        // Chromium still says it is that site.
        if w.tab.shared.borrow().contexts.get(&context) != Some(&origin) {
            return;
        }
        w.tab.fill_password(context, &login.user, &login.pass);
        if let Some(l) = list.iter_mut().find(|l| l.origin == origin && l.user == login.user) {
            l.used = crate::journal::now();
        }
        let _ = store(&list);
    }

    /// Settings · Browser: asks first, with how many would go.
    pub(crate) fn ask_forget_passwords(&mut self) {
        let n = load().len();
        if n == 0 {
            self.toast(nus_render::text::icons::LOCK_KEY, "No Saved Passwords", "nothing to forget", None);
            return;
        }
        self.toast(nus_render::text::icons::WARNING, "Forget Passwords?", format!("{n} saved sign-in{} · this can't be undone", if n == 1 { "" } else { "s" }), Some(crate::toast::Act::ForgetPasswords));
    }

    pub(crate) fn forget_passwords(&mut self) {
        match store(&[]) {
            Ok(()) => self.toast(nus_render::text::icons::CHECK, "Passwords Forgotten", "none are kept in this profile now", None),
            Err(e) => self.toast_problem("Could Not Forget Passwords", e.to_string(), None),
        }
    }
}

//! Autofill under the field, as a browser does it: saved sign-ins, a
//! strong password for a new account, cards and addresses. Chromium's own
//! dropdown is a native popup that a windowless page never gets, so the
//! page script says which kind of field took focus and where it is, and
//! nus draws the list (page_menu.rs) under it.
//!
//! As with passwords.rs, the site is the JavaScript context Chromium names
//! for the report, never the page's say, and a fill goes only into that
//! context while Chromium still says it is that site. Nothing is filled
//! without a pick, a card's security code is never kept, and cards and
//! addresses are sealed in profile/autofill.json with the profile's key;
//! never in incognito.
//!
//! Payments: Chromium's Payment Request has no sheet in an embedded
//! browser, so a page that asks for one waits forever. The script takes it
//! away, which is what sends Google Pay to its own sign-in window (a popup
//! nus joins to its opener). Apple Pay exists only in Safari's WebKit, so
//! on a Mac a card field offers the checkout in Safari.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Injected into every document beside passwords::JS: reports the field
/// that took focus (its kind, the kinds around it, where it is), that it
/// left, and card or address forms sent; defines the form fill.
pub const JS: &str = r##"(()=>{try{
if(window.__nusAf)return;Object.defineProperty(window,'__nusAf',{value:true});
try{delete window.PaymentRequest}catch(_){}
const send=o=>{try{nusPassword(JSON.stringify(o))}catch(_){}};
const shown=el=>!!el&&el.getClientRects().length>0;
const AC={'username':'username','email':'email','current-password':'password','new-password':'new-password',
 'cc-name':'cc-name','cc-number':'cc-number','cc-exp':'cc-exp','cc-exp-month':'cc-exp-month','cc-exp-year':'cc-exp-year',
 'cc-csc':'cc-csc','name':'name','given-name':'given-name','family-name':'family-name','organization':'org',
 'street-address':'street','address-line1':'street','address-line2':'line2','address-level2':'city',
 'address-level1':'region','postal-code':'postal','country':'country','country-name':'country','tel':'tel','tel-national':'tel'};
const R=[[/card.?holder|name.?on.?card|cc.?name/,'cc-name'],[/card.?num|cc.?num|credit.?card|\bpan\b/,'cc-number'],
 [/cvc|cvv|csc|security.?code|card.?code/,'cc-csc'],[/exp\w*.?(month|mm\b)|cc.?month/,'cc-exp-month'],
 [/exp\w*.?(year|yy)|cc.?year/,'cc-exp-year'],[/expir|exp.?date|mm.?\/.?yy/,'cc-exp'],[/e.?mail/,'email'],
 [/user.?name|login|user.?id/,'username'],[/first.?name|given.?name|fname/,'given-name'],
 [/last.?name|family.?name|surname|lname/,'family-name'],[/company|organi[sz]ation/,'org'],
 [/address.?(line)?.?2|\bapt\b|apartment|suite/,'line2'],[/address|street/,'street'],[/city|town|locality/,'city'],
 [/\bstate\b|province|region|county/,'region'],[/zip|postal|post.?code/,'postal'],[/country/,'country'],
 [/phone|\btel\b|mobile/,'tel'],[/full.?name|your.?name|\bname\b/,'name']];
const scopeOf=el=>el.form||document;
const fields=s=>[...s.querySelectorAll('input,select')].filter(shown);
const pwsIn=s=>fields(s).filter(i=>i.type==='password');
const words=el=>{const t=[el.name,el.id,el.placeholder,el.getAttribute('aria-label')];
 if(el.labels)for(const l of el.labels)t.push(l.textContent);return t.filter(Boolean).join(' ').toLowerCase()};
const userOf=s=>{const p=pwsIn(s)[0];if(!p)return null;let best=null;
 for(const i of fields(s)){if(i===p||i.tagName!=='INPUT'||!/^(text|email|tel|)$/.test(i.type))continue;
  if(i.compareDocumentPosition(p)&Node.DOCUMENT_POSITION_FOLLOWING)best=i}return best};
const kind=el=>{
 if(!el||!(el instanceof HTMLInputElement||el instanceof HTMLSelectElement))return '';
 const t=(el.type||'').toLowerCase();
 if(/^(hidden|submit|button|checkbox|radio|file|image|reset|range|color|search)$/.test(t))return '';
 const ac=(el.getAttribute('autocomplete')||'').toLowerCase().split(/\s+/).filter(Boolean).pop()||'';
 if(AC[ac])return AC[ac];
 const s=scopeOf(el);
 if(t==='password'){const all=pwsIn(s);
  if(/new|create|confirm|repeat|again|choose/.test(words(el)))return 'new-password';
  if(all.length===2||(all.length>2&&all[0]!==el))return 'new-password';return 'password'}
 if(el===userOf(s))return 'username';
 if(t==='email')return 'email';if(t==='tel')return 'tel';
 const w=words(el);for(const[r,k]of R)if(r.test(w))return k;return ''};
const rectOf=el=>{const r=el.getBoundingClientRect();let x=r.left,y=r.top,w=window;
 try{while(w!==w.top){const f=w.frameElement;if(!f)return null;const b=f.getBoundingClientRect();
  x+=b.left+f.clientLeft;y+=b.top+f.clientTop;w=w.parent}}catch(_){return null}
 return[x,y,r.width,r.height]};
let cur=null,told=false;
const tell=el=>{const k=kind(el);if(!k){if(cur){cur=null;told=false;send({kind:'away'})}return}
 const r=rectOf(el);if(!r)return;cur=el;told=true;
 send({kind:'focus',field:k,kinds:[...new Set(fields(scopeOf(el)).map(kind).filter(Boolean))],rect:r,
  dpr:devicePixelRatio||1,value:el.type==='password'?'':String(el.value||'').slice(0,64)})};
addEventListener('focusin',e=>tell(e.target),true);
addEventListener('focusout',e=>{if(e.target===cur){cur=null;told=false;send({kind:'away'})}},true);
addEventListener('mousedown',e=>{if(e.target===cur&&document.activeElement===cur)setTimeout(()=>tell(cur),0)},true);
addEventListener('input',e=>{if(e.isTrusted&&e.target===cur&&cur.type!=='password')tell(cur)},true);
const gone=()=>{if(cur&&told){told=false;send({kind:'away'})}};
addEventListener('scroll',gone,{capture:true,passive:true});addEventListener('resize',gone);
let last='';
const capture=e=>{const a=document.activeElement,t=e&&e.target;
 const s=(t&&(t.tagName==='FORM'?t:t.form))||(a&&a.form)||(cur&&scopeOf(cur));if(!s)return;
 const o={};for(const f of fields(s)){const k=kind(f);if(k&&k!=='cc-csc'&&!/password/.test(k)&&f.value)o[k]=String(f.value).slice(0,256)}
 if(!o['cc-number']&&!(o.street&&(o.postal||o.city)))return;const key=JSON.stringify(o);if(key===last)return;last=key;
 send({kind:'filled',fields:o})};
addEventListener('submit',capture,true);
addEventListener('click',e=>{const t=e.target;if(t&&t.closest&&t.closest('button,input[type=submit],[role=button]'))capture(e)},true);
const set=(el,v)=>{if(el.tagName==='SELECT'){const l=v.toLowerCase();
  const o=[...el.options].find(o=>o.value.toLowerCase()===l||o.text.trim().toLowerCase()===l)
   ||[...el.options].find(o=>/^\d+$/.test(l)&&/^\d+$/.test(o.value)&&+o.value===+l)
   ||[...el.options].find(o=>l.length>2&&o.text.trim().toLowerCase().startsWith(l));
  if(!o)return;el.value=o.value}
 else{const d=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');d.set.call(el,v)}
 el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}))};
Object.defineProperty(window,'__nusFillForm',{value:m=>{const a=cur||document.activeElement;if(!a)return 0;let n=0;
 for(const f of fields(scopeOf(a))){const k=kind(f);let v=m[k];if(v==null)continue;
  if(k==='cc-exp-year'&&f.maxLength===2)v=v.slice(-2);
  if(k==='cc-exp-year'&&f.tagName==='SELECT'&&[...f.options].some(o=>/^\d\d$/.test(o.value)))v=v.slice(-2);
  if(f===a||!f.value||f.tagName==='SELECT'){set(f,v);n++}}return n}});
if(document.activeElement&&document.activeElement!==document.body)tell(document.activeElement);
}catch(_){}})()"##;

/// A card or an address, as the kinds of field it fills.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug, Default)]
pub struct Entry {
    pub values: BTreeMap<String, String>,
    pub saved: u64,
    pub used: u64,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug, Default)]
pub struct Wallet {
    #[serde(default)]
    pub cards: Vec<Entry>,
    #[serde(default)]
    pub addresses: Vec<Entry>,
}

fn path() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile/autofill.json")
}

pub fn load() -> Wallet {
    if crate::private::enabled() {
        return Wallet::default();
    }
    crate::protected_state::read_text(&path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn store(w: &Wallet) -> std::io::Result<()> {
    if crate::private::enabled() {
        return Ok(());
    }
    crate::protected_state::write_json(&path(), w)
}

/// What the page's focus asked for, and what picking a row does.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Login(String),
    Strong(String),
    Card(usize),
    Address(usize),
    Safari,
}

/// One row of the list: what it says, the detail on the right, its icon.
pub struct Row {
    pub label: String,
    pub detail: String,
    pub icon: (&'static str, &'static str),
    pub pick: Pick,
}

const CARD_KINDS: &[&str] = &["cc-name", "cc-number", "cc-exp", "cc-exp-month", "cc-exp-year", "cc-csc"];
const ADDRESS_KINDS: &[&str] = &["name", "given-name", "family-name", "org", "street", "line2", "city", "region", "postal", "country", "tel", "email"];

pub fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// A card number that could be one: 12 to 19 digits that pass Luhn.
pub fn card_number(s: &str) -> bool {
    let d = digits(s);
    if !(12..=19).contains(&d.len()) || s.chars().any(|c| !(c.is_ascii_digit() || c == ' ' || c == '-')) {
        return false;
    }
    let sum: u32 = d.bytes().rev().enumerate().map(|(i, b)| {
        let n = (b - b'0') as u32;
        if i % 2 == 1 { if n * 2 > 9 { n * 2 - 9 } else { n * 2 } } else { n }
    }).sum();
    sum % 10 == 0
}

pub fn brand(number: &str) -> &'static str {
    let d = digits(number);
    let two: u32 = d.get(..2).and_then(|p| p.parse().ok()).unwrap_or(0);
    let four: u32 = d.get(..4).and_then(|p| p.parse().ok()).unwrap_or(0);
    match () {
        _ if d.starts_with('4') => "VISA",
        _ if (51..=55).contains(&two) || (2221..=2720).contains(&four) => "MASTERCARD",
        _ if two == 34 || two == 37 => "AMEX",
        _ if d.starts_with("6011") || two == 65 => "DISCOVER",
        _ if (3528..=3589).contains(&four) => "JCB",
        _ => "CARD",
    }
}

/// "04/29" from what the page had: a joined expiry or a month and year.
fn expiry(v: &BTreeMap<String, String>) -> (String, String) {
    if let Some(e) = v.get("cc-exp") {
        let parts: Vec<String> = e.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(str::to_string).collect();
        if parts.len() == 2 {
            return (parts[0].clone(), parts[1].clone());
        }
    }
    (v.get("cc-exp-month").cloned().unwrap_or_default(), v.get("cc-exp-year").cloned().unwrap_or_default())
}

/// The card to keep from a sent form, with the security code never in it.
pub fn card_from(fields: &BTreeMap<String, String>) -> Option<Entry> {
    let number = fields.get("cc-number").filter(|n| card_number(n))?;
    let (mut month, mut year) = expiry(fields);
    if month.len() == 1 { month.insert(0, '0'); }
    if year.len() == 2 { year.insert_str(0, "20"); }
    let mut values = BTreeMap::new();
    values.insert("cc-number".into(), digits(number));
    if let Some(n) = fields.get("cc-name").filter(|n| !n.trim().is_empty()) { values.insert("cc-name".into(), n.trim().into()); }
    if !month.is_empty() { values.insert("cc-exp-month".into(), month); }
    if !year.is_empty() { values.insert("cc-exp-year".into(), year); }
    Some(Entry { values, saved: 0, used: 0 })
}

/// The address to keep from a sent form: a street and a city or a code.
pub fn address_from(fields: &BTreeMap<String, String>) -> Option<Entry> {
    let mut values: BTreeMap<String, String> = fields.iter()
        .filter(|(k, v)| ADDRESS_KINDS.contains(&k.as_str()) && !v.trim().is_empty())
        .map(|(k, v)| (k.clone(), v.trim().to_string())).collect();
    if !values.contains_key("street") || !(values.contains_key("city") || values.contains_key("postal")) {
        return None;
    }
    if !values.contains_key("name") {
        let joined = [values.get("given-name"), values.get("family-name")].into_iter().flatten().cloned().collect::<Vec<_>>().join(" ");
        if !joined.is_empty() { values.insert("name".into(), joined); }
    }
    values.remove("given-name");
    values.remove("family-name");
    Some(Entry { values, saved: 0, used: 0 })
}

/// Whether the wallet already holds this: the same number, or the same
/// street and code.
fn same(a: &Entry, b: &Entry) -> bool {
    let k = |e: &Entry, f: &str| e.values.get(f).map(|v| v.to_lowercase());
    if a.values.contains_key("cc-number") {
        return k(a, "cc-number") == k(b, "cc-number");
    }
    k(a, "street") == k(b, "street") && k(a, "postal") == k(b, "postal")
}

/// None when it's kept as is; Some(true) when it replaces one.
pub fn offer(list: &[Entry], e: &Entry) -> Option<bool> {
    match list.iter().find(|x| same(x, e)) {
        Some(x) if x.values == e.values => None,
        Some(_) => Some(true),
        None => Some(false),
    }
}

pub fn upsert(list: &mut Vec<Entry>, mut e: Entry, now: u64) {
    e.saved = now;
    if let Some(x) = list.iter_mut().find(|x| same(x, &e)) {
        e.used = x.used;
        *x = e;
    } else {
        list.push(e);
    }
}

pub fn card_label(e: &Entry) -> (String, String) {
    let n = e.values.get("cc-number").map(String::as_str).unwrap_or("");
    let last = &n[n.len().saturating_sub(4)..];
    let (m, y) = (e.values.get("cc-exp-month").cloned().unwrap_or_default(), e.values.get("cc-exp-year").cloned().unwrap_or_default());
    let exp = if m.is_empty() { String::new() } else { format!("{m}/{}", &y[y.len().saturating_sub(2)..]) };
    (format!("{} ···· {last}", brand(n)), [e.values.get("cc-name").cloned().unwrap_or_default(), exp].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "))
}

pub fn address_label(e: &Entry) -> (String, String) {
    let g = |k: &str| e.values.get(k).cloned().unwrap_or_default();
    let place = [g("city"), g("postal")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    (g("street"), [g("name"), place].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "))
}

/// What goes into the form for a card or an address, in every shape a
/// field might ask for it (joined expiry, split name).
pub fn fill_values(e: &Entry) -> BTreeMap<String, String> {
    let mut v = e.values.clone();
    if let (Some(m), Some(y)) = (v.get("cc-exp-month").cloned(), v.get("cc-exp-year").cloned()) {
        v.insert("cc-exp".into(), format!("{m}/{}", &y[y.len().saturating_sub(2)..]));
    }
    if let Some(name) = v.get("name").cloned() {
        let mut parts = name.rsplitn(2, ' ');
        let family = parts.next().unwrap_or("").to_string();
        let given = parts.next().unwrap_or("").to_string();
        if !given.is_empty() {
            v.insert("given-name".into(), given);
            v.insert("family-name".into(), family);
        }
        v.entry("cc-name".into()).or_insert(name);
    }
    v
}

pub fn fill_js(values: &BTreeMap<String, String>) -> String {
    format!("window.__nusFillForm&&window.__nusFillForm({})", serde_json::to_string(values).unwrap_or_else(|_| "{}".into()))
}

/// A password for a new account: 3 groups of 6 from letters and digits
/// that don't look alike, at least one upper case and one digit.
pub fn strong_password() -> String {
    const LOWER: &[u8] = b"abcdefghijkmnopqrstuvwxyz";
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const DIGIT: &[u8] = b"23456789";
    loop {
        let mut bytes = [0u8; 18];
        if getrandom::fill(&mut bytes).is_err() {
            continue;
        }
        let pick = |b: u8| -> char {
            let all = [LOWER, LOWER, LOWER, UPPER, DIGIT];
            let set = all[(b % 5) as usize];
            set[(b as usize / 5) % set.len()] as char
        };
        let chars: Vec<char> = bytes.iter().map(|b| pick(*b)).collect();
        if chars.iter().any(|c| c.is_ascii_uppercase()) && chars.iter().any(|c| c.is_ascii_digit()) {
            return chars.chunks(6).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-");
        }
    }
}

/// The list for a field that took focus, or nothing to show.
pub fn rows(field: &str, kinds: &[String], typed: &str, origin: &str, logins: &[crate::passwords::Login], wallet: &Wallet, mac: bool) -> Vec<Row> {
    use nus_render::text::icons;
    let mut out = Vec::new();
    let has = |k: &str| kinds.iter().any(|x| x == k);
    let secure = crate::passwords::savable(origin);
    let host = crate::passwords::host(origin).to_string();
    let login_form = has("password") || has("new-password");
    if field == "new-password" && secure {
        out.push(Row { label: "USE A STRONG PASSWORD".into(), detail: "SAVED WHEN YOU SIGN UP".into(), icon: icons::KEY, pick: Pick::Strong(strong_password()) });
    }
    if field == "password" || (login_form && matches!(field, "username" | "email")) {
        let typed = typed.to_lowercase();
        let mut mine: Vec<_> = logins.iter().filter(|l| l.origin == origin && l.user.to_lowercase().starts_with(&typed)).collect();
        mine.sort_by_key(|l| std::cmp::Reverse(l.used.max(l.saved)));
        for l in mine.into_iter().take(8) {
            let label = if l.user.is_empty() { "SAVED PASSWORD".into() } else { l.user.clone() };
            out.push(Row { label, detail: host.clone(), icon: icons::PASSWORD, pick: Pick::Login(l.user.clone()) });
        }
        return out;
    }
    if CARD_KINDS.contains(&field) && secure {
        for (i, c) in wallet.cards.iter().enumerate() {
            let (label, detail) = card_label(c);
            out.push(Row { label, detail, icon: icons::CREDIT_CARD, pick: Pick::Card(i) });
        }
        if mac {
            out.push(Row { label: "PAY WITH APPLE PAY".into(), detail: "OPENS THIS PAGE IN SAFARI".into(), icon: icons::OPEN_EXTERNAL, pick: Pick::Safari });
        }
        return out;
    }
    if ADDRESS_KINDS.contains(&field) && secure && !login_form {
        let typed = typed.to_lowercase();
        for (i, a) in wallet.addresses.iter().enumerate() {
            let own = fill_values(a);
            if !own.get(field).is_some_and(|v| v.to_lowercase().starts_with(&typed)) {
                continue;
            }
            let (label, detail) = address_label(a);
            out.push(Row { label, detail, icon: icons::MAP_PIN, pick: Pick::Address(i) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn cards_pass_luhn_and_never_keep_the_code() {
        assert!(card_number("4242 4242 4242 4242"));
        assert!(!card_number("4242 4242 4242 4241"));
        assert!(!card_number("12345"));
        let c = card_from(&map(&[("cc-number", "4242-4242-4242-4242"), ("cc-exp", "4 / 29"), ("cc-csc", "123"), ("cc-name", "Ada L")])).unwrap();
        assert_eq!(c.values, map(&[("cc-number", "4242424242424242"), ("cc-exp-month", "04"), ("cc-exp-year", "2029"), ("cc-name", "Ada L")]));
        assert_eq!(card_label(&c), ("VISA ···· 4242".into(), "Ada L · 04/29".into()));
        let f = fill_values(&c);
        assert_eq!(f.get("cc-exp").unwrap(), "04/29");
        assert!(!f.contains_key("cc-csc"));
        assert!(card_from(&map(&[("cc-number", "1111")])).is_none());
    }

    #[test]
    fn addresses_need_a_street_and_join_names() {
        assert!(address_from(&map(&[("street", "1 Main")])).is_none());
        let a = address_from(&map(&[("street", "1 Main"), ("postal", "87102"), ("given-name", "Ada"), ("family-name", "Lovelace")])).unwrap();
        assert_eq!(a.values.get("name").unwrap(), "Ada Lovelace");
        let f = fill_values(&a);
        assert_eq!((f["given-name"].as_str(), f["family-name"].as_str()), ("Ada", "Lovelace"));
        let mut list = Vec::new();
        assert_eq!(offer(&list, &a), Some(false));
        upsert(&mut list, a.clone(), 1);
        assert_eq!(offer(&list, &a), None);
        let mut moved = a.clone();
        moved.values.insert("tel".into(), "555".into());
        assert_eq!(offer(&list, &moved), Some(true));
        upsert(&mut list, moved, 2);
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn strong_passwords_are_grouped_and_mixed() {
        let p = strong_password();
        assert_eq!(p.len(), 20);
        assert_eq!(p.split('-').count(), 3);
        assert!(p.chars().any(|c| c.is_ascii_uppercase()) && p.chars().any(|c| c.is_ascii_digit()));
        assert_ne!(p, strong_password());
    }

    #[test]
    fn rows_fit_the_field() {
        let logins = vec![crate::passwords::Login { origin: "https://a.test".into(), user: "ada".into(), pass: "x".into(), saved: 1, used: 0 }];
        let mut wallet = Wallet::default();
        wallet.cards.push(card_from(&map(&[("cc-number", "4242424242424242")])).unwrap());
        let k = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let r = rows("username", &k(&["username", "password"]), "", "https://a.test", &logins, &wallet, false);
        assert_eq!(r.iter().map(|r| r.pick.clone()).collect::<Vec<_>>(), vec![Pick::Login("ada".into())]);
        assert!(rows("username", &k(&["username", "password"]), "b", "https://a.test", &logins, &wallet, false).is_empty());
        assert!(rows("password", &k(&["password"]), "", "https://b.test", &logins, &wallet, false).is_empty());
        let r = rows("new-password", &k(&["email", "new-password"]), "", "https://b.test", &logins, &wallet, false);
        assert!(matches!(r[0].pick, Pick::Strong(_)));
        let r = rows("cc-number", &k(&["cc-number"]), "", "https://shop.test", &logins, &wallet, true);
        assert_eq!(r.iter().map(|r| r.pick.clone()).collect::<Vec<_>>(), vec![Pick::Card(0), Pick::Safari]);
        assert!(rows("cc-number", &k(&["cc-number"]), "", "http://shop.test", &logins, &wallet, true).is_empty());
        assert_eq!(fill_js(&map(&[("a", "\"</script>")])), r#"window.__nusFillForm&&window.__nusFillForm({"a":"\"</script>"})"#);
    }
}

impl crate::app::App {
    fn autofill_pane(&self, tab: u64, right: bool) -> Option<&crate::app::WebPane> {
        let t = self.tabs.iter().find(|t| t.id == tab)?;
        match if right { t.right.as_ref() } else { Some(&t.left) } {
            Some(crate::app::Pane::Web(w)) => Some(w),
            _ => None,
        }
    }

    /// A field took focus, left, or a card or address form went: the list
    /// under the field, or an offer to keep what was sent.
    pub(crate) fn autofill_report(&mut self, tab: u64, right: bool, report: crate::passwords::Report) {
        use crate::passwords::Report;
        match report {
            Report::Focus { origin, context, field, kinds, rect, dpr, value } => {
                let active = self.tabs.get(self.active).map(|t| (t.id, t.focus_right && t.right.is_some()));
                let Some(w) = self.autofill_pane(tab, right) else { return };
                if active != Some((tab, right)) || w.reader.is_some() {
                    return;
                }
                let page = w.page;
                let logins = crate::passwords::load();
                let list = rows(&field, &kinds, &value, &origin, &logins, &load(), cfg!(target_os = "macos"));
                if list.is_empty() {
                    self.close_autofill(Some(context));
                    return;
                }
                let [x, y, _, h] = rect;
                let at = (page.x + x * dpr, page.y + (y + h) * dpr + self.px(2.0));
                if !page.contains(at.0, at.1 - self.px(3.0)) {
                    return;
                }
                self.open_autofill_menu(tab, right, at, context, origin, list);
            }
            Report::Away { context } => self.close_autofill(Some(context)),
            Report::Filled { origin, fields } => {
                if !crate::passwords::savable(&origin) {
                    return;
                }
                let wallet = load();
                use nus_render::text::icons;
                let (card, entry) = match card_from(&fields) {
                    Some(c) => (true, c),
                    None => match address_from(&fields) {
                        Some(a) => (false, a),
                        None => return,
                    },
                };
                let Some(update) = offer(if card { &wallet.cards } else { &wallet.addresses }, &entry) else { return };
                let (label, detail) = if card { card_label(&entry) } else { address_label(&entry) };
                let words = match (card, update) {
                    (true, false) => "Save Card?",
                    (true, true) => "Update Card?",
                    (false, false) => "Save Address?",
                    (false, true) => "Update Address?",
                };
                let what = if card { "security code not kept" } else { "kept in this profile, encrypted" };
                let detail = [label, detail, what.to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                self.passwords.wallet = Some((card, entry));
                self.toast(if card { icons::CREDIT_CARD } else { icons::MAP_PIN }, words, detail, Some(crate::toast::Act::SaveAutofill));
            }
            _ => {}
        }
    }

    /// The Save on a card or address offer.
    pub(crate) fn save_offered_autofill(&mut self) {
        let Some((card, entry)) = self.passwords.wallet.take() else { return };
        let mut wallet = load();
        upsert(if card { &mut wallet.cards } else { &mut wallet.addresses }, entry, crate::journal::now());
        match store(&wallet) {
            Ok(()) => self.toast(nus_render::text::icons::CHECK, if card { "Card Saved" } else { "Address Saved" }, "offered under the field next time", None),
            Err(e) => self.toast_problem("Could Not Save", e.to_string(), None),
        }
    }

    /// Settings · Browser: asks first, with how many would go.
    pub(crate) fn ask_forget_wallet(&mut self) {
        let w = load();
        let n = w.cards.len() + w.addresses.len();
        if n == 0 {
            self.toast(nus_render::text::icons::CREDIT_CARD, "Nothing Kept", "no cards or addresses in this profile", None);
            return;
        }
        self.toast(nus_render::text::icons::WARNING, "Forget Cards And Addresses?", format!("{} card{} · {} address{} · this can't be undone", w.cards.len(), if w.cards.len() == 1 { "" } else { "s" }, w.addresses.len(), if w.addresses.len() == 1 { "" } else { "es" }), Some(crate::toast::Act::ForgetWallet));
    }

    pub(crate) fn forget_wallet(&mut self) {
        match store(&Wallet::default()) {
            Ok(()) => self.toast(nus_render::text::icons::CHECK, "Cards And Addresses Forgotten", "none are kept in this profile now", None),
            Err(e) => self.toast_problem("Could Not Forget", e.to_string(), None),
        }
    }

    /// A row picked from the list, into the page world that asked, while
    /// Chromium still says it is that site.
    pub(crate) fn autofill_pick(&mut self, tab: u64, right: bool, context: i64, origin: &str, pick: Pick) {
        let Some(w) = self.autofill_pane(tab, right) else { return };
        if w.tab.shared.borrow().contexts.get(&context).map(String::as_str) != Some(origin) {
            return;
        }
        match pick {
            Pick::Login(user) => {
                let mut list = crate::passwords::load();
                let Some(l) = list.iter_mut().find(|l| l.origin == origin && l.user == user) else { return };
                w.tab.fill_password(context, &l.user, &l.pass);
                l.used = crate::journal::now();
                let _ = crate::passwords::store(&list);
            }
            Pick::Strong(pass) => {
                // Offered to keep when the form is sent, as any typed one is.
                w.tab.fill_form(context, &BTreeMap::from([("new-password".to_string(), pass)]));
            }
            Pick::Card(i) | Pick::Address(i) => {
                let card = matches!(pick, Pick::Card(_));
                let mut wallet = load();
                let list = if card { &mut wallet.cards } else { &mut wallet.addresses };
                let Some(e) = list.get_mut(i) else { return };
                w.tab.fill_form(context, &fill_values(e));
                e.used = crate::journal::now();
                let _ = store(&wallet);
            }
            Pick::Safari => {
                let url = w.tab.shared.borrow().url.clone();
                if url.starts_with("https://") {
                    let _ = std::process::Command::new("open").args(["-a", "Safari", &url]).spawn();
                    self.toast(nus_render::text::icons::OPEN_EXTERNAL, "Opened In Safari", "Apple Pay is Safari's; you may need to sign in there", None);
                }
            }
        }
    }
}

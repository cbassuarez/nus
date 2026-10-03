//! The hatch hotkey on Wayland, where no app may grab keys itself: the
//! desktop's GlobalShortcuts portal (org.freedesktop.portal.GlobalShortcuts,
//! GNOME 48+, KDE Plasma 6). nus asks for one shortcut with the chord it
//! would like; the desktop asks the person once, may let them pick another
//! chord, and from then on says when it is pressed. D-Bus work stays on its
//! own thread; a press arrives as `UserEvent::Hatch`.
//!
//! When the desktop has no such portal, or the person declines, the chord
//! still works whenever a nus window has the keyboard (app.rs).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use winit::event_loop::EventLoopProxy;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::hotkey::{Chord, KEYS, M_ALT, M_CTRL, M_SHIFT, M_SUPER};
use crate::UserEvent;

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// The chord in the shortcuts spec's words: `CTRL+SHIFT+space`.
pub fn trigger(chord: Chord) -> String {
    let (m, row) = chord.parts();
    let mut s = String::new();
    for (bit, word) in [(M_CTRL, "CTRL+"), (M_ALT, "ALT+"), (M_SHIFT, "SHIFT+"), (M_SUPER, "LOGO+")] {
        if m & bit != 0 {
            s.push_str(word);
        }
    }
    let label = KEYS[row].2;
    let key = match label {
        "`" => "grave".to_string(),
        "SPACE" => "space".into(),
        "-" => "minus".into(),
        "=" => "equal".into(),
        "[" => "bracketleft".into(),
        "]" => "bracketright".into(),
        "\\" => "backslash".into(),
        ";" => "semicolon".into(),
        "'" => "apostrophe".into(),
        "," => "comma".into(),
        "." => "period".into(),
        "/" => "slash".into(),
        "RETURN" => "Return".into(),
        "TAB" => "Tab".into(),
        f if f.starts_with('F') && f.len() > 1 => f.into(),
        other => other.to_lowercase(),
    };
    s.push_str(&key);
    s
}

/// A shortcut asked of the portal; dropping it stops listening.
pub struct Portal {
    alive: Arc<AtomicBool>,
}

impl Drop for Portal {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

/// Ask for the shortcut. Returns at once; the desktop's question and the
/// presses are handled on a thread.
pub fn bind(chord: Chord, proxy: EventLoopProxy<UserEvent>) -> Portal {
    let alive = Arc::new(AtomicBool::new(true));
    let live = alive.clone();
    let _ = std::thread::Builder::new().name("hotkey portal".into()).spawn(move || {
        if let Err(e) = listen(chord, &proxy, &live) {
            tracing::info!("hatch hotkey: no desktop shortcut ({e}); it works while nus has the keyboard");
        }
    });
    Portal { alive }
}

/// The Request object a portal call answers on, subscribed before the call
/// so its Response cannot be missed.
fn request<'a>(conn: &'a zbus::blocking::Connection, token: &str) -> zbus::Result<zbus::blocking::proxy::SignalIterator<'a>> {
    let sender = conn.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
    let path = format!("{PATH}/request/{sender}/{token}");
    let req = zbus::blocking::Proxy::new(conn, DEST, path, "org.freedesktop.portal.Request")?;
    req.receive_signal("Response")
}

fn answer(it: &mut zbus::blocking::proxy::SignalIterator<'_>) -> Result<HashMap<String, OwnedValue>, String> {
    let msg = it.next().ok_or("the portal went away")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize().map_err(|e| e.to_string())?;
    match code {
        0 => Ok(results),
        1 => Err("declined".into()),
        _ => Err("the desktop could not".into()),
    }
}

fn listen(chord: Chord, proxy: &EventLoopProxy<UserEvent>, alive: &AtomicBool) -> Result<(), String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let portal = zbus::blocking::Proxy::new(&conn, DEST, PATH, "org.freedesktop.portal.GlobalShortcuts").map_err(|e| e.to_string())?;
    let n = std::process::id();
    let token = format!("nus_session_{n}");
    let mut it = request(&conn, &token).map_err(|e| e.to_string())?;
    let options: HashMap<&str, Value> = HashMap::from([("handle_token", Value::from(token.as_str())), ("session_handle_token", Value::from(format!("nus_hatch_{n}")))]);
    portal.call_method("CreateSession", &(options,)).map_err(|e| e.to_string())?;
    let created = answer(&mut it)?;
    let session: OwnedObjectPath = created
        .get("session_handle")
        .and_then(|v| {
            OwnedObjectPath::try_from(v.try_clone().ok()?).ok().or_else(|| {
                let s: String = v.try_clone().ok()?.try_into().ok()?;
                OwnedObjectPath::try_from(s).ok()
            })
        })
        .ok_or("no session")?;

    let token = format!("nus_bind_{n}");
    let mut it = request(&conn, &token).map_err(|e| e.to_string())?;
    let wanted = trigger(chord);
    let shortcut: HashMap<&str, Value> = HashMap::from([("description", Value::from("Show or hide the nus hatch")), ("preferred_trigger", Value::from(wanted.as_str()))]);
    let shortcuts = vec![("hatch", shortcut)];
    let options: HashMap<&str, Value> = HashMap::from([("handle_token", Value::from(token.as_str()))]);
    portal.call_method("BindShortcuts", &(&session, shortcuts, "", options)).map_err(|e| e.to_string())?;
    answer(&mut it)?;
    tracing::info!("hatch hotkey: bound through the desktop (asked for {wanted})");

    for msg in portal.receive_signal("Activated").map_err(|e| e.to_string())? {
        if !alive.load(Ordering::Relaxed) {
            break;
        }
        let Ok((from, id, _, _)) = msg.body().deserialize::<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>)>() else { continue };
        if from == session && id == "hatch" {
            let _ = proxy.send_event(UserEvent::Hatch);
        }
    }
    let _ = conn.call_method(Some(DEST), session.as_str(), Some("org.freedesktop.portal.Session"), "Close", &());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_read_as_the_spec_writes_them() {
        assert_eq!(trigger(Chord::CtrlGrave), "CTRL+grave");
        assert_eq!(trigger(Chord::CtrlShiftSpace), "CTRL+SHIFT+space");
        assert_eq!(trigger(Chord::AltSpace), "ALT+space");
        assert_eq!(trigger(Chord::SuperGrave), "LOGO+grave");
        let k = KEYS.iter().position(|k| k.2 == "K").unwrap() as u8;
        assert_eq!(trigger(Chord::Custom { mods: M_CTRL | M_ALT, key: k }), "CTRL+ALT+k");
    }
}

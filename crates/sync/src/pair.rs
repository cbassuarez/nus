//! Pairing: the key from a device that has it to one that doesn't, across the
//! local network, with no server of ours or anyone's. The device with the key
//! shows a short code (`7-orbit-ledger`); the other types it.
//!
//! The code's number finds the two devices on the network: the offer is
//! announced by UDP broadcast and answered over TCP. Its words are a password
//! for SPAKE2, which turns it into a secret only these two devices hold:
//! someone watching the network learns nothing, and someone guessing gets one
//! try per code. Each side then proves it holds the same secret, and the key
//! crosses sealed under it. This is Magic Wormhole's shape without its relay.
//!
//! Nothing is announced unless asked: by default the offering device only
//! listens, on its own LAN address, and the other connects to the address
//! it shows. Broadcast discovery is opt-in, for a home network. One code
//! gets one attempt: whoever connects first, right or wrong, ends the
//! offer. Either side can be cancelled; an offer lapses on its own.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::{open, seal, KEY_LEN};

/// Where offers are announced.
pub const PORT: u16 = 51807;
const HELLO: &str = "NUSPAIR1";
const IDENTITY: &[u8] = b"nus pair v1";
const MAX_FRAME: usize = 8192;

/// A new code: a number to find each other by, and two words to prove it.
pub fn new_code() -> String {
    let words = bip39::Language::English.word_list();
    let mut r = [0u8; 6];
    getrandom::getrandom(&mut r).expect("randomness");
    let n = 1 + (u16::from_le_bytes([r[0], r[1]]) % 99);
    let a = words[u16::from_le_bytes([r[2], r[3]]) as usize % words.len()];
    let b = words[u16::from_le_bytes([r[4], r[5]]) as usize % words.len()];
    format!("{n}-{a}-{b}")
}

/// The code's number: how the two find each other on the network.
pub fn nameplate(code: &str) -> Option<u16> {
    let (n, rest) = code.trim().split_once('-')?;
    (!rest.is_empty()).then_some(())?;
    n.parse().ok()
}

fn frame_out(s: &mut TcpStream, bytes: &[u8]) -> std::io::Result<()> {
    s.write_all(&(bytes.len() as u32).to_be_bytes())?;
    s.write_all(bytes)
}

fn frame_in(s: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(std::io::Error::other("frame too large"));
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf)?;
    Ok(buf)
}

/// Run SPAKE2 over `s`, then prove to each other that both hold the same
/// secret. The secret, as a sealing key.
fn agree(s: &mut TcpStream, code: &str, offering: bool) -> Result<[u8; KEY_LEN], String> {
    let words = code.trim().to_lowercase();
    let (state, mine) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(words.as_bytes()),
        &Identity::new(IDENTITY),
    );
    frame_out(s, &mine).map_err(|e| e.to_string())?;
    let theirs = frame_in(s).map_err(|e| e.to_string())?;
    let shared = state
        .finish(&theirs)
        .map_err(|_| "the other device did not speak nus pairing".to_string())?;
    let key = blake3::derive_key("nus pair v1 key", &shared);
    let (say, expect) = if offering {
        ("offer", "receive")
    } else {
        ("receive", "offer")
    };
    let proof = |who: &str| blake3::keyed_hash(&key, format!("nus pair {who}").as_bytes());
    frame_out(s, proof(say).as_bytes()).map_err(|e| e.to_string())?;
    let got = frame_in(s).map_err(|e| e.to_string())?;
    if got != proof(expect).as_bytes() {
        return Err("the code did not match: check it and start again on both devices".into());
    }
    Ok(key)
}

/// This machine's address on the local network, for the other device to type
/// when broadcast does not reach it. (Connecting a UDP socket sends nothing.)
pub fn local_address() -> Option<std::net::IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

/// Wait for the other device and hand it the key. `listener` is already
/// bound (its port goes in the announcement and in the fallback line).
/// `waiting` is called about twice a second, for a spinner. The other
/// device's name, when it took the key.
pub fn offer(
    listener: &TcpListener,
    code: &str,
    key_word: &str,
    device: &str,
    until: Duration,
    announce: bool,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let n = nameplate(code).ok_or("that is not a pairing code")?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let shout = if announce {
        UdpSocket::bind("0.0.0.0:0")
            .and_then(|u| u.set_broadcast(true).map(|_| u))
            .ok()
    } else {
        None
    };
    let hello = format!("{HELLO} {n} {port}");
    let deadline = Instant::now() + until;
    let mut last = Instant::now() - Duration::from_secs(5);
    let mut stream = loop {
        if Instant::now() > deadline {
            return Err("no device came for the code in time".into());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if last.elapsed() >= Duration::from_secs(1) {
            if let Some(u) = &shout {
                let _ = u.send_to(hello.as_bytes(), ("255.255.255.255", PORT));
            }
            last = Instant::now();
        }
        match listener.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    // The one attempt this code gets: a wrong code ends the offer here.
    let secret = agree(&mut stream, code, true).map_err(|e| {
        if e.contains("did not match") {
            "a device answered with the wrong code; nothing was sent. Start again with a new code"
                .to_string()
        } else {
            e
        }
    })?;
    let parcel = serde_json::json!({ "key": key_word, "device": device }).to_string();
    frame_out(&mut stream, &seal(&secret, "pair", parcel.as_bytes())).map_err(|e| e.to_string())?;
    let ack = frame_in(&mut stream).map_err(|e| e.to_string())?;
    let ack = open(&secret, "pair-ack", &ack).ok_or("the other device did not confirm")?;
    let v: serde_json::Value = serde_json::from_slice(&ack).map_err(|e| e.to_string())?;
    Ok(v["device"]
        .as_str()
        .unwrap_or("the other device")
        .to_string())
}

/// Find the offer for `code` (or connect straight to `direct`) and take the
/// key. The key's word and the offering device's name.
pub fn receive(
    code: &str,
    device: &str,
    direct: Option<SocketAddr>,
    until: Duration,
    discover: bool,
    cancel: &AtomicBool,
) -> Result<(String, String), String> {
    let n = nameplate(code).ok_or("that is not a pairing code: it looks like 7-orbit-ledger")?;
    let deadline = Instant::now() + until;
    let peer = match direct {
        Some(a) => a,
        None if !discover => {
            return Err("give the address the other device shows (--to, like 192.168.1.20:51807), or --discover on a home network".into());
        }
        None => {
            let listen = UdpSocket::bind(("0.0.0.0", PORT)).map_err(|e| format!("could not listen for the other device ({e}); use --to with the address it shows"))?;
            listen
                .set_read_timeout(Some(Duration::from_millis(500)))
                .map_err(|e| e.to_string())?;
            let mut buf = [0u8; 128];
            loop {
                if Instant::now() > deadline {
                    return Err("no device is offering that code on this network; use --to with the address it shows".into());
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                if let Ok((len, from)) = listen.recv_from(&mut buf) {
                    let text = String::from_utf8_lossy(&buf[..len]);
                    let mut parts = text.split_whitespace();
                    if parts.next() == Some(HELLO)
                        && parts.next().and_then(|p| p.parse::<u16>().ok()) == Some(n)
                    {
                        if let Some(port) = parts.next().and_then(|p| p.parse::<u16>().ok()) {
                            break SocketAddr::new(from.ip(), port);
                        }
                    }
                }
            }
        }
    };
    let mut stream = TcpStream::connect_timeout(&peer, Duration::from_secs(10))
        .map_err(|e| format!("could not reach {peer}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    let secret = agree(&mut stream, code, false)?;
    let parcel = frame_in(&mut stream).map_err(|e| e.to_string())?;
    let parcel = open(&secret, "pair", &parcel).ok_or("the key did not arrive intact")?;
    let v: serde_json::Value = serde_json::from_slice(&parcel).map_err(|e| e.to_string())?;
    let word = v["key"].as_str().ok_or("no key in the parcel")?.to_string();
    let ack = serde_json::json!({ "device": device }).to_string();
    frame_out(&mut stream, &seal(&secret, "pair-ack", ack.as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok((
        word,
        v["device"]
            .as_str()
            .unwrap_or("the other device")
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_have_a_number_and_two_words() {
        let c = new_code();
        let parts: Vec<&str> = c.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert!(nameplate(&c).is_some_and(|n| (1..=99).contains(&n)));
        assert_eq!(nameplate("7-orbit-ledger"), Some(7));
        assert_eq!(nameplate("orbit"), None);
        assert_eq!(nameplate("7-"), None);
    }

    fn pair(
        offered: &'static str,
        typed: &'static str,
    ) -> (Result<String, String>, Result<(String, String), String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let offer = std::thread::spawn(move || {
            offer(
                &listener,
                offered,
                "nus5-abcd",
                "desk",
                Duration::from_secs(10),
                false,
                &AtomicBool::new(false),
            )
        });
        let got = receive(
            typed,
            "laptop",
            Some(addr),
            Duration::from_secs(10),
            false,
            &AtomicBool::new(false),
        );
        (offer.join().unwrap(), got)
    }

    #[test]
    fn the_key_crosses_when_the_codes_match() {
        let (offered, received) = pair("7-orbit-ledger", "7-Orbit-Ledger");
        assert_eq!(offered.unwrap(), "laptop");
        assert_eq!(
            received.unwrap(),
            ("nus5-abcd".to_string(), "desk".to_string())
        );
    }

    #[test]
    fn a_wrong_code_learns_nothing() {
        let (offered, received) = pair("7-orbit-ledger", "7-orbit-legend");
        assert!(offered.is_err());
        let err = received.unwrap_err();
        assert!(err.contains("did not match"), "{err}");
    }

    #[test]
    fn nothing_is_announced_or_heard_unless_asked() {
        // Without an address and without discovery, receiving refuses at
        // once rather than listening on the network.
        let err = receive(
            "7-orbit-ledger",
            "laptop",
            None,
            Duration::from_secs(1),
            false,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(err.contains("--to"), "{err}");
        // A cancelled offer lets go of its listener.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cancel = AtomicBool::new(true);
        let err = offer(
            &listener,
            "7-orbit-ledger",
            "nus5-abcd",
            "desk",
            Duration::from_secs(5),
            false,
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err, "cancelled");
    }
}

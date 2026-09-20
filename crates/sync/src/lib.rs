//! The profile on more than one device, with no account and nothing
//! readable in flight or at rest anywhere but your machines.
//!
//! **The key.** 32 random bytes, made once (`nus sync key`), shown as a
//! word you copy to the next device (`nus sync join <key>`). It never
//! leaves your devices; nothing derives it from a password. Every file
//! is sealed with XChaCha20-Poly1305 under that key, the file's own path
//! as associated data so a blob can't be moved to another slot; names on
//! the carrier are hashes of the key and the label, so nothing readable
//! is there at all. See docs/SYNC.md.
//!
//! **The carriers.** A folder — one your OS or Syncthing already moves
//! (iCloud Drive, OneDrive, Dropbox, a USB stick) — and/or a git remote.
//! Each carrier holds, per device, an encrypted manifest and the sealed
//! files. Both can be on.
//!
//! **The merge.** Last writer wins, per file, by the writer's clock; the
//! losing version is kept beside the winner as `<file>.<device>.lost` so
//! nothing is ever silently gone. Google-Docs-level: no locks, no
//! prompts, the newest edit stands.
//!
//! **What syncs.** The profile's own files: settings, me, rules, layouts, folders,
//! ports names, memory, site rules — and the session (open tabs) when
//! the setting says so. Never cookies, caches, downloads or history.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};

pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const MAGIC: &[u8; 4] = b"NUS1";

/// The files that travel, relative to the profile. `session.json` only
/// when asked.
pub const ALWAYS: &[&str] = &[
    "settings.json",
    "me.json",
    "rules.luau",
    "folders.json",
    "ports.json",
    "memory.md",
    "sites.json",
    "containers.json",
    "blocklist.txt",
    "avatar.png",
];
pub const DIRS: &[&str] = &["layouts", "themes", "surfaces"];
pub const SESSION: &str = "session.json";
/// Files that only grow — an assistant's memory, written from wherever
/// it worked — merge as the union of their lines instead of one side
/// losing: theirs in their order, then whatever of ours they lacked.
pub const UNION: &[&str] = &["memory.md"];

/// The union of two line-files: `theirs` in order, then the lines of
/// `ours` they don't have, in ours' order. Blank lines are kept as they
/// come in theirs and dropped from the tail, so a merge never doubles
/// the spacing.
pub fn union_lines(theirs: &str, ours: &str) -> String {
    let mut out: Vec<&str> = theirs.lines().collect();
    let have: std::collections::HashSet<&str> =
        theirs.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut added = false;
    for l in ours.lines() {
        if l.trim().is_empty() || have.contains(l) {
            continue;
        }
        out.push(l);
        added = true;
    }
    let mut s = out.join("\n");
    if theirs.ends_with('\n') || (added && !s.is_empty()) {
        s.push('\n');
    }
    s
}

// --- the key ---

/// A new key from the OS's randomness.
pub fn new_key() -> [u8; KEY_LEN] {
    let mut k = [0u8; KEY_LEN];
    getrandom::getrandom(&mut k).expect("randomness");
    k
}

/// The key as the word you copy: base32, lower case, no padding, in
/// groups of four. `nus5-…`.
pub fn encode_key(k: &[u8; KEY_LEN]) -> String {
    let s = base32_encode(k);
    let groups: Vec<String> = s
        .as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect();
    format!("nus5-{}", groups.join("-"))
}

pub fn decode_key(word: &str) -> Option<[u8; KEY_LEN]> {
    let w = word.trim().to_ascii_lowercase();
    let w = w.strip_prefix("nus5-")?;
    let s: String = w.chars().filter(|c| *c != '-').collect();
    let bytes = base32_decode(&s)?;
    if bytes.len() != KEY_LEN {
        return None;
    }
    let mut k = [0u8; KEY_LEN];
    k.copy_from_slice(&bytes);
    Some(k)
}

const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

fn base32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut buf: u64 = 0;
    let mut bits = 0;
    for &b in data {
        buf = (buf << 8) | b as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf: u64 = 0;
    let mut bits = 0;
    for c in s.bytes() {
        let v = ALPHABET.iter().position(|&a| a == c)? as u64;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

// --- sealing ---

/// Seal `plain` for `path` (the associated data): magic · nonce · box.
pub fn seal(key: &[u8; KEY_LEN], path: &str, plain: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).expect("randomness");
    let boxed = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: path.as_bytes(),
            },
        )
        .expect("seal");
    let mut out = Vec::with_capacity(4 + NONCE_LEN + boxed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&boxed);
    out
}

/// Open a sealed blob for `path`; None when the key or the path is wrong.
pub fn open(key: &[u8; KEY_LEN], path: &str, sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 4 + NONCE_LEN || &sealed[..4] != MAGIC {
        return None;
    }
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XNonce::from_slice(&sealed[4..4 + NONCE_LEN]);
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &sealed[4 + NONCE_LEN..],
                aad: path.as_bytes(),
            },
        )
        .ok()
}

// --- the manifest ---

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// blake3 of the plaintext, hex.
    pub hash: String,
    /// The writer's clock when it last changed, seconds since the epoch.
    pub written: u64,
    pub size: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub device: String,
    /// When this manifest was written.
    pub at: u64,
    pub files: BTreeMap<String, Entry>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The files to sync under `profile`, relative paths, sorted.
pub fn files_to_sync(profile: &Path, session: bool) -> Vec<String> {
    let mut v: Vec<String> = ALWAYS
        .iter()
        .filter(|f| profile.join(f).is_file())
        .map(|f| f.to_string())
        .collect();
    for d in DIRS {
        if let Ok(rd) = std::fs::read_dir(profile.join(d)) {
            for e in rd.flatten() {
                if e.path().is_file() {
                    v.push(format!("{d}/{}", e.file_name().to_string_lossy()));
                }
            }
        }
    }
    if session && profile.join(SESSION).is_file() {
        v.push(SESSION.into());
    }
    v.sort();
    v
}

/// This device's manifest of the profile as it is on disk.
pub fn local_manifest(profile: &Path, device: &str, session: bool) -> Manifest {
    let mut m = Manifest {
        device: device.into(),
        at: now(),
        files: BTreeMap::new(),
    };
    for rel in files_to_sync(profile, session) {
        let p = profile.join(&rel);
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        let written = std::fs::metadata(&p)
            .and_then(|md| md.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        m.files.insert(
            rel,
            Entry {
                hash: blake3::hash(&bytes).to_hex().to_string(),
                written,
                size: bytes.len() as u64,
            },
        );
    }
    m
}

// --- carriers ---

/// Where sealed files live: a folder (one your OS syncs) or a git work
/// tree (pushed and pulled around the exchange).
pub trait Carrier {
    fn name(&self) -> String;
    /// Every device's manifest the carrier holds.
    fn manifests(&self, key: &[u8; KEY_LEN]) -> Vec<Manifest>;
    fn read(&self, key: &[u8; KEY_LEN], device: &str, rel: &str) -> Option<Vec<u8>>;
    fn write(
        &self,
        key: &[u8; KEY_LEN],
        device: &str,
        rel: &str,
        plain: &[u8],
    ) -> anyhow::Result<()>;
    fn write_manifest(&self, key: &[u8; KEY_LEN], m: &Manifest) -> anyhow::Result<()>;
    /// Before reading (a pull) and after writing (a push); the folder does nothing.
    fn before(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn after(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A folder: `<root>/<device>/<rel>.enc` and `<root>/<device>/manifest.enc`.
pub struct Folder {
    pub root: PathBuf,
}

/// A name on the carrier that says nothing: the key and the label, hashed.
/// A stranger holding the folder sees neither your hostnames nor which
/// files exist, only how many and how big.
fn slot(key: &[u8; KEY_LEN], label: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(key);
    h.update(label.as_bytes());
    h.finalize().to_hex()[..16].to_string()
}

fn device_dir(key: &[u8; KEY_LEN], device: &str) -> String {
    slot(key, &format!("device:{device}"))
}

fn enc_name(key: &[u8; KEY_LEN], rel: &str) -> String {
    format!("{}.enc", slot(key, &format!("file:{rel}")))
}

impl Carrier for Folder {
    fn name(&self) -> String {
        format!("folder {}", self.root.display())
    }
    fn manifests(&self, key: &[u8; KEY_LEN]) -> Vec<Manifest> {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.root) {
            for e in rd.flatten() {
                let p = e.path().join("manifest.enc");
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Some(plain) = open(key, "manifest", &bytes) {
                        if let Ok(m) = serde_json::from_slice::<Manifest>(&plain) {
                            v.push(m);
                        }
                    }
                }
            }
        }
        v
    }
    fn read(&self, key: &[u8; KEY_LEN], device: &str, rel: &str) -> Option<Vec<u8>> {
        let bytes = std::fs::read(
            self.root
                .join(device_dir(key, device))
                .join(enc_name(key, rel)),
        )
        .ok()?;
        open(key, rel, &bytes)
    }
    fn write(
        &self,
        key: &[u8; KEY_LEN],
        device: &str,
        rel: &str,
        plain: &[u8],
    ) -> anyhow::Result<()> {
        let dir = self.root.join(device_dir(key, device));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(enc_name(key, rel)), seal(key, rel, plain))?;
        Ok(())
    }
    fn write_manifest(&self, key: &[u8; KEY_LEN], m: &Manifest) -> anyhow::Result<()> {
        let dir = self.root.join(device_dir(key, &m.device));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("manifest.enc"),
            seal(key, "manifest", &serde_json::to_vec(m)?),
        )?;
        Ok(())
    }
}

/// A git remote: a clone under `work`, pulled before and pushed after.
/// With `auth`, an `Authorization` header value goes on every fetch and
/// push (a forge's token, kept off the URL and out of git's config).
pub struct Git {
    pub remote: String,
    pub work: PathBuf,
    pub folder: Folder,
    pub auth: Option<String>,
}

impl Git {
    pub fn new(remote: &str, work: &Path) -> Git {
        Git {
            remote: remote.into(),
            work: work.to_path_buf(),
            folder: Folder {
                root: work.to_path_buf(),
            },
            auth: None,
        }
    }
    pub fn with_auth(remote: &str, work: &Path, auth: Option<String>) -> Git {
        let mut g = Git::new(remote, work);
        g.auth = auth;
        g
    }
    /// The header, as a `-c` pair for the command line.
    fn header_args(&self) -> Vec<String> {
        match &self.auth {
            Some(h) => vec!["-c".into(), format!("http.extraheader=Authorization: {h}")],
            None => Vec::new(),
        }
    }
    fn git(&self, args: &[&str]) -> anyhow::Result<String> {
        let out = std::process::Command::new("git")
            .args(self.header_args())
            .args(args)
            .current_dir(&self.work)
            .output()
            .map_err(|e| anyhow::anyhow!("git: {e}"))?;
        if !out.status.success() {
            anyhow::bail!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

impl Carrier for Git {
    fn name(&self) -> String {
        format!("git {}", self.remote)
    }
    fn manifests(&self, key: &[u8; KEY_LEN]) -> Vec<Manifest> {
        self.folder.manifests(key)
    }
    fn read(&self, key: &[u8; KEY_LEN], device: &str, rel: &str) -> Option<Vec<u8>> {
        self.folder.read(key, device, rel)
    }
    fn write(
        &self,
        key: &[u8; KEY_LEN],
        device: &str,
        rel: &str,
        plain: &[u8],
    ) -> anyhow::Result<()> {
        self.folder.write(key, device, rel, plain)
    }
    fn write_manifest(&self, key: &[u8; KEY_LEN], m: &Manifest) -> anyhow::Result<()> {
        self.folder.write_manifest(key, m)
    }
    fn before(&self) -> anyhow::Result<()> {
        if !self.work.join(".git").exists() {
            std::fs::create_dir_all(&self.work)?;
            let out = std::process::Command::new("git")
                .args(self.header_args())
                .args(["clone", "--quiet", &self.remote, "."])
                .current_dir(&self.work)
                .output()?;
            if !out.status.success() {
                // A fresh remote: start it.
                self.git(&["init", "--quiet"])?;
                self.git(&["remote", "add", "origin", &self.remote])?;
            }
        } else {
            // Nothing to pull on a brand-new remote is fine.
            let _ = self.git(&["pull", "--quiet", "--rebase", "origin", "HEAD"]);
        }
        Ok(())
    }
    fn after(&self) -> anyhow::Result<()> {
        self.git(&["add", "-A"])?;
        let status = self.git(&["status", "--porcelain"])?;
        if status.trim().is_empty() {
            return Ok(());
        }
        self.git(&[
            "-c",
            "user.name=nus",
            "-c",
            "user.email=nus@localhost",
            "commit",
            "--quiet",
            "-m",
            "nus sync",
        ])?;
        let branch = self
            .git(&["rev-parse", "--abbrev-ref", "HEAD"])
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "main".into());
        let branch = if branch == "HEAD" {
            "main".to_string()
        } else {
            branch
        };
        self.git(&["push", "--quiet", "-u", "origin", &branch])?;
        Ok(())
    }
}

// --- the exchange ---

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub pushed: Vec<String>,
    pub pulled: Vec<(String, String)>, // (file, from device)
    pub kept: Vec<String>,             // losers kept as .lost
    pub errors: Vec<String>,
}

/// One round: pull what's newer elsewhere (keeping what it replaces),
/// push what's newer here, write this device's manifest.
pub fn exchange(
    profile: &Path,
    device: &str,
    key: &[u8; KEY_LEN],
    session: bool,
    carriers: &[&dyn Carrier],
) -> Report {
    let mut rep = Report::default();
    for c in carriers {
        if let Err(e) = c.before() {
            rep.errors.push(format!("{}: {e}", c.name()));
        }
    }
    let mut local = local_manifest(profile, device, session);
    // The newest version of every file anyone has.
    let mut best: BTreeMap<String, (u64, String, usize)> = BTreeMap::new(); // rel → (written, device, carrier index)
    for (ci, c) in carriers.iter().enumerate() {
        for m in c.manifests(key) {
            if m.device == device {
                continue;
            }
            for (rel, e) in &m.files {
                let newer = best
                    .get(rel)
                    .map(|(w, _, _)| e.written > *w)
                    .unwrap_or(true);
                if newer {
                    best.insert(rel.clone(), (e.written, m.device.clone(), ci));
                }
            }
        }
    }
    // Pull: a file newer elsewhere replaces ours; ours is kept as .lost.
    for (rel, (written, from, ci)) in &best {
        let mine = local.files.get(rel);
        let take = match mine {
            None => true,
            Some(e) => *written > e.written,
        };
        if !take {
            continue;
        }
        let Some(plain) = carriers[*ci].read(key, from, rel) else {
            continue;
        };
        if mine.is_some_and(|e| e.hash == blake3::hash(&plain).to_hex().to_string()) {
            continue; // same bytes, just a newer clock
        }
        let dest = profile.join(rel);
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // A file that only grows: theirs and ours become one, and ours
        // goes back out on the push as the newer of the two.
        let union = UNION.contains(&rel.as_str()) && dest.is_file();
        let plain = if union {
            let ours = std::fs::read(&dest).unwrap_or_default();
            union_lines(
                &String::from_utf8_lossy(&plain),
                &String::from_utf8_lossy(&ours),
            )
            .into_bytes()
        } else {
            plain
        };
        if dest.is_file() && !union {
            let lost = profile.join(format!("{rel}.{device}.lost"));
            if std::fs::rename(&dest, &lost).is_ok() {
                rep.kept.push(rel.clone());
            }
        }
        match std::fs::write(&dest, &plain) {
            Ok(()) => {
                // Keep the writer's clock, so the next round agrees; a
                // union is newer than both, so it pushes.
                if !union {
                    let t = UNIX_EPOCH + std::time::Duration::from_secs(*written);
                    if let Ok(f) = std::fs::File::options().write(true).open(&dest) {
                        let _ = f.set_modified(t);
                    }
                }
                rep.pulled.push((rel.clone(), from.clone()));
            }
            Err(e) => rep.errors.push(format!("{rel}: {e}")),
        }
    }
    // Push: everything of ours that's newer than (or unknown to) the carriers.
    local = local_manifest(profile, device, session);
    for c in carriers {
        let theirs: BTreeMap<String, Entry> = c
            .manifests(key)
            .into_iter()
            .find(|m| m.device == device)
            .map(|m| m.files)
            .unwrap_or_default();
        for (rel, e) in &local.files {
            let same = theirs.get(rel).is_some_and(|t| t.hash == e.hash);
            if same {
                continue;
            }
            let Ok(bytes) = std::fs::read(profile.join(rel)) else {
                continue;
            };
            match c.write(key, device, rel, &bytes) {
                Ok(()) => {
                    if !rep.pushed.contains(rel) {
                        rep.pushed.push(rel.clone());
                    }
                }
                Err(err) => rep.errors.push(format!("{}: {rel}: {err}", c.name())),
            }
        }
        if let Err(err) = c.write_manifest(key, &local) {
            rep.errors.push(format!("{}: manifest: {err}", c.name()));
        }
        if let Err(err) = c.after() {
            rep.errors.push(format!("{}: {err}", c.name()));
        }
    }
    rep
}

/// A device name: the hostname, or what the user set.
pub fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "this-device".into())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_file_merges_as_a_union() {
        let theirs = "# memory
- a
- b
";
        let ours = "# memory
- a
- c
";
        assert_eq!(
            union_lines(theirs, ours),
            "# memory
- a
- b
- c
"
        );
        // Nothing of ours missing: theirs, untouched.
        assert_eq!(
            union_lines(
                theirs, "- a
"
            ),
            theirs
        );
        // Blank lines of ours never pile up.
        assert_eq!(
            union_lines(
                "- a
", "

- z

"
            ),
            "- a
- z
"
        );
    }

    #[test]
    fn key_round_trip() {
        let k = new_key();
        let w = encode_key(&k);
        assert!(w.starts_with("nus5-"));
        assert_eq!(decode_key(&w), Some(k));
        assert_eq!(decode_key(&w.to_uppercase()), Some(k));
        assert_eq!(decode_key("nus5-nope"), None);
    }

    #[test]
    fn seal_is_bound_to_the_path() {
        let k = new_key();
        let s = seal(&k, "settings.json", b"hello");
        assert_eq!(
            open(&k, "settings.json", &s).as_deref(),
            Some(&b"hello"[..])
        );
        assert_eq!(open(&k, "rules.luau", &s), None);
        assert_eq!(open(&new_key(), "settings.json", &s), None);
    }

    #[test]
    fn two_devices_exchange_last_writer_wins() {
        let base = std::env::temp_dir().join(format!("nus-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, b, carrier) = (base.join("a"), base.join("b"), base.join("carrier"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let k = new_key();
        let f = Folder {
            root: carrier.clone(),
        };
        // A has prefs; B has nothing.
        std::fs::write(a.join("settings.json"), "{\"a\":1}").unwrap();
        let r = exchange(&a, "alpha", &k, false, &[&f]);
        assert_eq!(r.pushed, vec!["settings.json"]);
        let r = exchange(&b, "beta", &k, false, &[&f]);
        assert_eq!(
            r.pulled,
            vec![("settings.json".to_string(), "alpha".to_string())]
        );
        assert_eq!(
            std::fs::read_to_string(b.join("settings.json")).unwrap(),
            "{\"a\":1}"
        );
        // B edits later: A pulls it and keeps its own as .lost.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(b.join("settings.json"), "{\"b\":2}").unwrap();
        exchange(&b, "beta", &k, false, &[&f]);
        let r = exchange(&a, "alpha", &k, false, &[&f]);
        assert_eq!(r.pulled.len(), 1);
        assert_eq!(
            std::fs::read_to_string(a.join("settings.json")).unwrap(),
            "{\"b\":2}"
        );
        assert_eq!(
            std::fs::read_to_string(a.join("settings.json.alpha.lost")).unwrap(),
            "{\"a\":1}"
        );
        // Session only when asked.
        std::fs::write(a.join("session.json"), "{}").unwrap();
        let r = exchange(&a, "alpha", &k, false, &[&f]);
        assert!(!r.pushed.contains(&"session.json".to_string()));
        let r = exchange(&a, "alpha", &k, true, &[&f]);
        assert!(r.pushed.contains(&"session.json".to_string()));
        // The carrier holds only ciphertext, under names that say nothing.
        let dir = carrier.join(device_dir(&k, "alpha"));
        let blob = std::fs::read(dir.join(enc_name(&k, "settings.json"))).unwrap();
        assert!(!blob.windows(5).any(|w| w == b"\"b\":2"));
        assert!(!carrier.join("alpha").exists());
        assert!(!dir.join("settings.json.enc").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}

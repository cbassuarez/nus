//! XChaCha20-Poly1305 local storage. The only persistent key is held by the OS
//! credential store; disk holds a public key identifier, nonces and ciphertext.
//! Locked/missing stores and failed authentication never fall back to plaintext.
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"NUSENC01";
const STREAM: &[u8; 8] = b"NUSLOG01";
const MAX: usize = 64 * 1024 * 1024;
const SERVICE: &str = "dev.nus.local-state.v1";
type VaultKey = Arc<Zeroizing<[u8; 32]>>;
static KEYS: LazyLock<Mutex<HashMap<PathBuf, VaultKey>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
fn error(text: &str) -> io::Error {
    io::Error::other(text)
}

/// A short-lived key handoff to a child that already owns the same protected
/// profile. Send only over that child's anonymous stdin pipe, never argv/env.
pub struct ChildKey(VaultKey);
impl ChildKey {
    pub fn write_to(self, mut pipe: impl Write) -> io::Result<()> {
        pipe.write_all(b"NUSKEY01")?;
        pipe.write_all(self.0.as_ref().as_ref())
    }
}
pub fn key_for_child(profile: &Path) -> io::Result<ChildKey> {
    key(profile).map(ChildKey)
}

/// The holder's explicit internal startup path. The bounded temporary buffer
/// is zeroized, and the key stays in the existing process-local vault cache.
pub fn receive_child_key(profile: &Path, mut pipe: impl Read) -> io::Result<()> {
    let mut header = [0u8; 8];
    pipe.read_exact(&mut header)?;
    if &header != b"NUSKEY01" {
        return Err(error("Invalid vault handoff"));
    }
    let mut bytes = Zeroizing::new([0u8; 32]);
    pipe.read_exact(bytes.as_mut())?;
    let profile = profile.canonicalize()?;
    KEYS.lock()
        .map_err(|_| error("Local vault unavailable"))?
        .insert(profile, Arc::new(bytes));
    Ok(())
}
fn random<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| error("Secure random source unavailable"))?;
    Ok(bytes)
}

pub fn profile_for(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "profile"))
        .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")))
        .to_path_buf()
}

fn key(profile: &Path) -> io::Result<Arc<Zeroizing<[u8; 32]>>> {
    std::fs::create_dir_all(profile)?;
    let profile = profile.canonicalize()?;
    let mut keys = KEYS.lock().map_err(|_| error("Local vault unavailable"))?;
    if let Some(key) = keys.get(&profile) {
        return Ok(key.clone());
    }
    #[cfg(test)]
    let bytes = Zeroizing::new(random()?);
    #[cfg(not(test))]
    let bytes = {
        let lock_path = profile.join(".vault-lock");
        if lock_path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(error("Invalid vault lock"));
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(lock_path)?;
        lock.lock()?;
        let marker = profile.join(".vault-id");
        let id = match std::fs::read_to_string(&marker) {
            Ok(id) => {
                if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(error("Invalid vault identifier"));
                }
                id
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let id: String = random::<16>()?.iter().map(|b| format!("{b:02x}")).collect();
                let key = Zeroizing::new(random::<32>()?);
                let entry = keyring::Entry::new(SERVICE, &id)
                    .map_err(|_| error("OS credential store unavailable"))?;
                entry.set_secret(key.as_slice()).map_err(|_| {
                    error("OS credential store could not protect the local state key")
                })?;
                // Verify before committing the public identifier. A crash can
                // leave an unused credential, never an unreadable saved state.
                let check = Zeroizing::new(entry.get_secret().map_err(|_| {
                    error("OS credential store could not retrieve the local state key")
                })?);
                if check.as_slice() != key.as_slice() {
                    return Err(error("Local state key verification failed"));
                }
                replace(&marker, id.as_bytes())?;
                id
            }
            Err(e) => return Err(e),
        };
        let entry = keyring::Entry::new(SERVICE, &id)
            .map_err(|_| error("OS credential store unavailable"))?;
        let stored = Zeroizing::new(
            entry
                .get_secret()
                .map_err(|_| error("Local state is locked: its OS credential is unavailable"))?,
        );
        let mut bytes = Zeroizing::new([0; 32]);
        if stored.len() != 32 {
            return Err(error("Invalid local state key"));
        }
        bytes.copy_from_slice(&stored);
        bytes
    };
    let bytes = Arc::new(bytes);
    keys.insert(profile, bytes.clone());
    Ok(bytes)
}

/// Explicit fixture injection; merely enabling the feature never changes key
/// selection. Only isolated unit-test setup calls this, never app startup.
#[cfg(feature = "test-vault")]
pub fn install_test_key(profile: &Path) -> io::Result<()> {
    std::fs::create_dir_all(profile)?;
    KEYS.lock()
        .map_err(|_| error("Test vault unavailable"))?
        .insert(profile.canonicalize()?, Arc::new(Zeroizing::new(random()?)));
    Ok(())
}

/// Cleanup for a caller-owned disposable fixture. Never used by app code.
#[cfg(feature = "test-vault")]
pub fn remove_test_key(profile: &Path) -> io::Result<()> {
    let profile = profile.canonicalize()?;
    KEYS.lock()
        .map_err(|_| error("Test vault unavailable"))?
        .remove(&profile);
    let marker = profile.join(".vault-id");
    if marker.is_file() {
        let id = std::fs::read_to_string(&marker)?;
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(error("Invalid test vault identifier"));
        }
        keyring::Entry::new(SERVICE, &id)
            .map_err(|_| error("Test keychain unavailable"))?
            .delete_credential()
            .map_err(|_| error("Test credential cleanup failed"))?;
        std::fs::remove_file(marker)?;
    }
    Ok(())
}

pub fn available(profile: &Path) -> io::Result<()> {
    key(profile).map(|_| ())
}
/// Once recognized legacy records have migrated, reject plaintext on every
/// subsequent read. A damaged magic header must not become a legacy import.
pub fn finish_migration(profile: &Path) -> io::Result<()> {
    available(profile)?;
    replace(&profile.join(".vault-format"), b"1")
}

fn seal_with(key: &[u8; 32], context: &[u8], plain: &[u8]) -> io::Result<Vec<u8>> {
    if plain.len() > MAX {
        return Err(error("Local state exceeds the encryption limit"));
    }
    let nonce = random::<24>()?;
    let cipher = XChaCha20Poly1305::new_from_slice(key).unwrap();
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: context,
            },
        )
        .map_err(|_| error("Local state encryption failed"))?;
    let mut out = Vec::with_capacity(32 + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend(ciphertext);
    Ok(out)
}
fn open_with(key: &[u8; 32], context: &[u8], bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 48 || bytes.len() > MAX + 48 || !bytes.starts_with(MAGIC) {
        return Err(error("Invalid encrypted local state"));
    }
    XChaCha20Poly1305::new_from_slice(key)
        .unwrap()
        .decrypt(
            XNonce::from_slice(&bytes[8..32]),
            Payload {
                msg: &bytes[32..],
                aad: context,
            },
        )
        .map_err(|_| error("Local state authentication failed; original preserved"))
}

fn context(profile: &Path, path: &Path) -> io::Result<String> {
    let relative = path
        .strip_prefix(profile)
        .map_err(|_| error("State path is outside its profile"))?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(error("Invalid local state path"));
    }
    // A lexical prefix alone does not contain ../ paths or symlinked folders.
    // Reject links before a legacy read can replace an external plaintext file.
    let mut checked = profile.to_path_buf();
    for part in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(part) = part {
            checked.push(part.as_os_str());
        }
        match checked.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(error("Local state path contains a symbolic link"))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(format!(
        "nus-local-state-v1:{}",
        relative
            .to_string_lossy()
            .replace('\\', "/")
            .replace(".previous.cast", ".cast")
    ))
}

pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| error("Missing local state directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn write_at(profile: &Path, path: &Path, plain: &[u8]) -> io::Result<()> {
    let context = context(profile, path)?;
    let key = key(profile)?;
    // Never overwrite corrupt or locked ciphertext with a new default value.
    if path.is_file() {
        let _ = read_at(profile, path)?;
    }
    replace(path, &seal_with(&key, context.as_bytes(), plain)?)
}
pub fn read_at(profile: &Path, path: &Path) -> io::Result<Vec<u8>> {
    let context = context(profile, path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX + 49) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX + 48 {
        return Err(error("Local state exceeds the read limit"));
    }
    let key = key(profile)?;
    if bytes.starts_with(STREAM) {
        open_stream(&key, context.as_bytes(), &bytes)
    } else if bytes.starts_with(MAGIC) {
        open_with(&key, context.as_bytes(), &bytes)
    } else {
        // Legacy plaintext is migrated atomically before it is handed back.
        // No plaintext backup/temp file is created.
        if profile.join(".vault-format").exists()
            || bytes.starts_with(b"NUS")
            || (std::str::from_utf8(&bytes).is_err() && !bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
        {
            return Err(error("Unsupported or damaged encrypted state"));
        }
        let modified = std::fs::metadata(path)?.modified().ok();
        replace(path, &seal_with(&key, context.as_bytes(), &bytes)?)?;
        if let Some(at) = modified {
            std::fs::File::options()
                .write(true)
                .open(path)?
                .set_modified(at)?;
        }
        Ok(bytes)
    }
}
pub fn write(path: &Path, plain: &[u8]) -> io::Result<()> {
    write_at(&profile_for(path), path, plain)
}
/// Authenticate the old value once, transform it, and atomically replace it.
/// Like `write`, callers serialize updates to the same file. This avoids a
/// second full decrypt when the new value is derived from the old one.
pub fn update(path: &Path, transform: impl FnOnce(&[u8]) -> io::Result<Vec<u8>>) -> io::Result<()> {
    let profile = profile_for(path);
    let context = context(&profile, path)?;
    let key = key(&profile)?;
    let old = match read_at(&profile, path) {
        Ok(bytes) => Zeroizing::new(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Zeroizing::new(Vec::new()),
        Err(e) => return Err(e),
    };
    let next = Zeroizing::new(transform(&old)?);
    replace(path, &seal_with(&key, context.as_bytes(), &next)?)
}
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    read_at(&profile_for(path), path)
}
pub fn read_text(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|_| error("State is not UTF-8"))
}

fn record_context(context: &[u8], id: &[u8], sequence: u64) -> Vec<u8> {
    let mut aad = context.to_vec();
    aad.extend_from_slice(id);
    aad.extend_from_slice(&sequence.to_le_bytes());
    aad
}
fn open_stream(key: &[u8; 32], context: &[u8], bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 80 {
        return Err(error("Truncated encrypted history"));
    }
    if !open_with(
        key,
        &record_context(context, &bytes[8..32], u64::MAX),
        &bytes[32..80],
    )?
    .is_empty()
    {
        return Err(error("Invalid history header"));
    }
    let mut at = 80;
    let mut sequence = 0;
    let mut out = Vec::new();
    while at < bytes.len() {
        let len = bytes
            .get(at..at + 4)
            .ok_or_else(|| error("Truncated encrypted history"))?;
        let len = u32::from_le_bytes(len.try_into().unwrap()) as usize;
        at += 4;
        let frame = bytes
            .get(at..at + len)
            .ok_or_else(|| error("Truncated encrypted history"))?;
        out.extend(open_with(
            key,
            &record_context(context, &bytes[8..32], sequence),
            frame,
        )?);
        if out.len() > MAX {
            return Err(error("Encrypted history exceeds limit"));
        }
        at += len;
        sequence += 1;
    }
    Ok(out)
}
/// Each buffered history record is independently authenticated, bound to its
/// stream and sequence. Plaintext never touches a staging file.
pub struct StreamWriter {
    file: std::io::BufWriter<std::fs::File>,
    key: Arc<Zeroizing<[u8; 32]>>,
    context: Vec<u8>,
    id: [u8; 24],
    sequence: u64,
}
impl StreamWriter {
    pub fn create(path: &Path) -> io::Result<Self> {
        let profile = profile_for(path);
        let key = key(&profile)?;
        let context = context(&profile, path)?.into_bytes();
        let id = random()?;
        let mut header = STREAM.to_vec();
        header.extend_from_slice(&id);
        header.extend(seal_with(
            &key,
            &record_context(&context, &id, u64::MAX),
            b"",
        )?);
        if path.is_file() {
            read_at(&profile, path)?;
        }
        replace(path, &header)?;
        let file = std::fs::OpenOptions::new().append(true).open(path)?;
        Ok(Self {
            file: std::io::BufWriter::new(file),
            key,
            context,
            id,
            sequence: 0,
        })
    }
}
impl Write for StreamWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let frame = seal_with(
            &self.key,
            &record_context(&self.context, &self.id, self.sequence),
            bytes,
        )?;
        self.file.write_all(&(frame.len() as u32).to_le_bytes())?;
        self.file.write_all(&frame)?;
        self.sequence += 1;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_handoff_is_bounded_and_unlocks_existing_ciphertext() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        write(&path, b"saved work").unwrap();
        let mut pipe = Zeroizing::new(Vec::new());
        key_for_child(dir.path())
            .unwrap()
            .write_to(&mut *pipe)
            .unwrap();
        assert_eq!(pipe.len(), 40);
        let root = dir.path().canonicalize().unwrap();
        KEYS.lock().unwrap().remove(&root);
        assert!(receive_child_key(dir.path(), &pipe[..30]).is_err());
        assert!(!KEYS.lock().unwrap().contains_key(&root));
        receive_child_key(dir.path(), &pipe[..]).unwrap();
        assert_eq!(read_text(&path).unwrap(), "saved work");
        assert!(receive_child_key(dir.path(), &b"BADKEY01"[..]).is_err());
    }
    #[test]
    fn update_authenticates_once_and_preserves_original_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        update(&path, |old| {
            assert!(old.is_empty());
            Ok(b"first\n".to_vec())
        })
        .unwrap();
        update(&path, |old| {
            let mut next = old.to_vec();
            next.extend(b"second\n");
            Ok(next)
        })
        .unwrap();
        assert_eq!(read(&path).unwrap(), b"first\nsecond\n");
        let bytes = std::fs::read(&path).unwrap();
        assert!(update(&path, |_| Err(error("cancelled"))).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let mut corrupt = bytes;
        *corrupt.last_mut().unwrap() ^= 1;
        std::fs::write(&path, &corrupt).unwrap();
        assert!(update(&path, |_| panic!(
            "corrupt input must never reach the transform"
        ))
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    }
    #[test]
    fn parent_traversal_cannot_migrate_or_overwrite_external_files() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profile");
        std::fs::create_dir(&profile).unwrap();
        let outside = dir.path().join("outside.json");
        std::fs::write(&outside, b"keep me").unwrap();
        let path = profile.join("../outside.json");
        assert!(read_at(&profile, &path).is_err());
        assert!(write_at(&profile, &path, b"replacement").is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"keep me");
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_files_and_directories_are_not_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profile");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&profile).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let original = outside.join("state.json");
        std::fs::write(&original, b"keep me").unwrap();
        std::os::unix::fs::symlink(&outside, profile.join("linked-dir")).unwrap();
        std::os::unix::fs::symlink(&original, profile.join("linked.json")).unwrap();
        for path in [
            profile.join("linked-dir/state.json"),
            profile.join("linked.json"),
        ] {
            assert!(read_at(&profile, &path).is_err());
            assert!(write_at(&profile, &path, b"replacement").is_err());
            assert!(StreamWriter::create(&path).is_err());
        }
        assert_eq!(std::fs::read(&original).unwrap(), b"keep me");
    }
    #[test]
    fn nonce_uniqueness_tampering_wrong_key_and_context() {
        let key = [4; 32];
        let a = seal_with(&key, b"session", b"secret-source-token").unwrap();
        let b = seal_with(&key, b"session", b"secret-source-token").unwrap();
        assert_ne!(a, b);
        assert!(!a.windows(6).any(|w| w == b"secret"));
        assert_eq!(
            open_with(&key, b"session", &a).unwrap(),
            b"secret-source-token"
        );
        assert!(open_with(&[5; 32], b"session", &a).is_err());
        assert!(open_with(&key, b"other", &a).is_err());
        for i in [0, 9, 33, a.len() - 1] {
            let mut bad = a.clone();
            bad[i] ^= 1;
            assert!(open_with(&key, b"session", &bad).is_err());
        }
        assert!(open_with(&key, b"session", &a[..a.len() - 1]).is_err());
    }
    #[test]
    fn legacy_migrates_and_corruption_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        std::fs::write(&path, b"legacy-secret").unwrap();
        assert_eq!(read(&path).unwrap(), b"legacy-secret");
        assert!(std::fs::read(&path).unwrap().starts_with(MAGIC));
        let mut bytes = std::fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(write(&path, b"empty default").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    #[test]
    fn completed_migration_rejects_plaintext_and_damaged_headers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        write(&path, b"synthetic payload").unwrap();
        finish_migration(dir.path()).unwrap();
        std::fs::write(&path, b"valid utf8 but not ciphertext").unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(read(&path).is_err());
        assert!(write(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn history_frames_preserve_bytes_and_reject_reordering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tab.cast");
        let mut writer = StreamWriter::create(&path).unwrap();
        writer.write_all(b"first secret\n").unwrap();
        writer.write_all(b"second secret\n").unwrap();
        writer.flush().unwrap();
        assert_eq!(read(&path).unwrap(), b"first secret\nsecond secret\n");
        let bytes = std::fs::read(&path).unwrap();
        let first = 4 + u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        let mut bad = bytes[..80].to_vec();
        bad.extend_from_slice(&bytes[80 + first..]);
        bad.extend_from_slice(&bytes[80..80 + first]);
        std::fs::write(&path, &bad).unwrap();
        assert!(read(&path).is_err());
    }
}

//! WebKit cookie jars follow CEF's profile and container boundaries.
//! Named persistent stores are public API on macOS 14+. Older systems keep
//! an isolated in-memory jar for each context instead of using the app-wide
//! default store. Private contexts are always memory-only.

/// A random identity belongs to the CEF profile's lifetime, not its path.
/// Removing/recreating the profile cannot reopen its old WebKit sign-ins.
/// An absent/invalid cache path must never persist an off-the-record context.
#[cfg(any(target_os = "macos", test))]
fn persistent_identifier(cache_path: &str, private: bool) -> std::io::Result<Option<String>> {
    if private || !std::path::Path::new(cache_path).is_absolute() {
        return Ok(None);
    }
    let marker = std::path::Path::new(cache_path).join("nus-webkit-store-id");
    match std::fs::read_to_string(&marker) {
        Ok(text) => {
            let id = text.trim();
            if valid_uuid(id) {
                return Ok(Some(scoped_identifier(
                    cache_path,
                    &id.to_ascii_lowercase(),
                )));
            }
            // Preserve corrupt identities instead of disconnecting the
            // profile from its previous native jar by silently replacing one.
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid WebKit profile identity",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|error| std::io::Error::other(error.to_string()))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let id = uuid_text(bytes);
    // CEF owns this profile's process lock; WebKit creation runs on the main
    // thread. Commit the identity before creating or caching its native jar.
    crate::store::write_atomic(&marker, format!("{id}\n").as_bytes())?;
    Ok(Some(scoped_identifier(cache_path, &id)))
}

/// A copied profile gets a separate native jar; its random marker still
/// makes deleting/recreating that same path start with a fresh identity.
#[cfg(any(target_os = "macos", test))]
fn scoped_identifier(cache_path: &str, profile_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"nus.webkit.profile.v1\0");
    hash.update(cache_path.as_bytes());
    hash.update(b"\0");
    hash.update(profile_id.as_bytes());
    let mut bytes: [u8; 16] = hash.finalize()[..16].try_into().unwrap();
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid_text(bytes)
}

#[cfg(any(target_os = "macos", test))]
fn uuid_text(bytes: [u8; 16]) -> String {
    let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[cfg(any(target_os = "macos", test))]
fn valid_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, byte)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
        && text.bytes().any(|byte| byte != b'0' && byte != b'-')
}

/// Empty CEF paths identify the process's off-the-record context. Include
/// its profile namespace so even an in-process profile change stays isolated.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, PartialEq, Eq, Hash)]
struct StoreKey {
    private: bool,
    explicit_cache_path: bool,
    path: std::path::PathBuf,
}

#[cfg(any(target_os = "macos", test))]
fn memory_key(cache_path: &str, private: bool, profile: &std::path::Path) -> StoreKey {
    let path = std::path::Path::new(cache_path);
    StoreKey {
        private,
        explicit_cache_path: path.is_absolute(),
        path: if path.is_absolute() {
            path.to_path_buf()
        } else {
            profile.join(path)
        },
    }
}

#[cfg(target_os = "macos")]
pub fn store(
    cache_path: &str,
    private: bool,
    mtm: objc2::MainThreadMarker,
) -> objc2::rc::Retained<objc2_web_kit::WKWebsiteDataStore> {
    use objc2::rc::Retained;
    use objc2::{AnyThread, ClassType, msg_send, sel};
    use objc2_foundation::{NSString, NSUUID};
    use objc2_web_kit::WKWebsiteDataStore;
    use std::cell::RefCell;
    use std::collections::HashMap;

    // Disposable native fixtures must not leave named stores in the user's
    // system WebKit Library, even when their CEF fixture has a cache path.
    let private = private || std::env::var_os("NUS_SHOT").is_some();
    thread_local! {
        // Retain memory-only jars across tabs for this process. Persistent
        // stores use the same cache, but remain recoverable by UUID at launch.
        static STORES: RefCell<HashMap<(StoreKey, Option<String>), Retained<WKWebsiteDataStore>>> = RefCell::new(HashMap::new());
    }
    let profile = std::env::current_dir().unwrap_or_default().join("profile");
    // Resolve the marker before the cache, so resetting a profile also
    // changes its jar within this process. Older WebKit never writes a marker.
    let named_stores: bool = unsafe {
        msg_send![WKWebsiteDataStore::class(), respondsToSelector: sel!(dataStoreForIdentifier:)]
    };
    let id = if named_stores {
        match persistent_identifier(cache_path, private) {
            Ok(id) => id,
            Err(_) => {
                tracing::warn!("WebKit profile identity unavailable; using memory-only storage");
                None
            }
        }
    } else {
        None
    };
    let key = (memory_key(cache_path, private, &profile), id.clone());
    STORES.with(|stores| {
        if let Some(store) = stores.borrow().get(&key) {
            return store.clone();
        }
        let store = if let Some(id) = id {
            let id = NSUUID::initWithUUIDString(NSUUID::alloc(), &NSString::from_str(&id))
                .expect("valid NUS store UUID");
            unsafe { WKWebsiteDataStore::dataStoreForIdentifier(&id, mtm) }
        } else {
            unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) }
        };
        stores.borrow_mut().insert(key, store.clone());
        store
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_identity_survives_restart_but_not_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy();
        let first = persistent_identifier(&path, false).unwrap().unwrap();
        assert!(valid_uuid(&first));
        assert_eq!(first.as_bytes()[14], b'8');
        assert_eq!(
            std::fs::read_to_string(dir.path().join("nus-webkit-store-id"))
                .unwrap()
                .as_bytes()[14],
            b'4'
        );
        assert_eq!(
            persistent_identifier(&path, false).unwrap().as_ref(),
            Some(&first)
        );
        std::fs::remove_file(dir.path().join("nus-webkit-store-id")).unwrap();
        assert_ne!(
            persistent_identifier(&path, false).unwrap().as_ref(),
            Some(&first)
        );
    }

    #[test]
    fn profiles_and_containers_have_distinct_stores() {
        let dir = tempfile::tempdir().unwrap();
        let id = |path: &str| {
            persistent_identifier(&dir.path().join(path).to_string_lossy(), false).unwrap()
        };
        let personal = id("one/profile");
        assert_ne!(personal, id("two/profile"));
        assert_ne!(personal, id("one/profile/container-work"));
    }

    #[test]
    fn copying_a_profile_does_not_share_its_native_jar() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original");
        let copy = dir.path().join("copy");
        let first = persistent_identifier(&original.to_string_lossy(), false).unwrap();
        std::fs::create_dir(&copy).unwrap();
        std::fs::copy(
            original.join("nus-webkit-store-id"),
            copy.join("nus-webkit-store-id"),
        )
        .unwrap();
        let copied = persistent_identifier(&copy.to_string_lossy(), false).unwrap();
        assert_ne!(first, copied);
        assert_eq!(
            first,
            persistent_identifier(&original.to_string_lossy(), false).unwrap()
        );
    }

    #[test]
    fn private_and_unknown_contexts_never_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private");
        assert_eq!(
            persistent_identifier(&path.to_string_lossy(), true).unwrap(),
            None
        );
        assert!(!path.exists());
        assert_eq!(persistent_identifier("", false).unwrap(), None);
        assert_eq!(
            persistent_identifier("profile/container-work", false).unwrap(),
            None
        );
    }

    #[test]
    fn corrupt_or_unreadable_identity_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("nus-webkit-store-id");
        std::fs::write(&marker, "damaged identity\n").unwrap();
        assert!(persistent_identifier(&dir.path().to_string_lossy(), false).is_err());
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "damaged identity\n"
        );
        std::fs::remove_file(&marker).unwrap();
        std::fs::create_dir(&marker).unwrap();
        assert!(persistent_identifier(&dir.path().to_string_lossy(), false).is_err());
        assert!(marker.is_dir());
    }

    #[test]
    fn native_store_ids_must_be_nonzero_uuids() {
        assert!(!valid_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(!valid_uuid("not a UUID"));
        assert!(!valid_uuid("dff77d63_4007-87c0-992f-de9ffb41448f"));
        assert!(valid_uuid("DFF77D63-4007-87C0-992F-DE9FFB41448F"));
    }

    #[test]
    fn memory_jars_share_only_their_context() {
        let root = std::path::Path::new(if cfg!(windows) { "C:/" } else { "/" });
        let profile = root.join("profiles/one/profile");
        let path = profile.to_string_lossy();
        let personal = memory_key(&path, false, &profile);
        assert_eq!(personal, memory_key(&path, false, &profile));
        assert_ne!(
            personal,
            memory_key(
                &profile.join("container-work").to_string_lossy(),
                false,
                &profile
            )
        );
        assert_ne!(personal, memory_key(&path, true, &profile));
        assert_ne!(personal, memory_key("", false, &profile));
        assert_ne!(
            memory_key("", true, &profile),
            memory_key("", true, &root.join("profiles/two/profile"))
        );
    }
}

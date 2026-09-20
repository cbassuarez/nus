//! Shared boundaries for the loopback CLI and opt-in phone server.
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
pub const MAX_REQUEST: usize = 1024 * 1024;
pub const IO_TIMEOUT: Duration = Duration::from_secs(5);
static CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
pub struct Connection;
impl Connection {
    pub fn acquire() -> Option<Self> {
        CONNECTIONS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 32).then_some(n + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Bound both bytes and total time: read timeouts alone allow slow trickles.
pub fn line(
    reader: &mut impl BufRead,
    max: usize,
    deadline: Instant,
) -> io::Result<Option<String>> {
    let mut out = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            return if out.is_empty() {
                Ok(None)
            } else {
                Err(io::ErrorKind::UnexpectedEof.into())
            };
        }
        let take = bytes
            .iter()
            .position(|b| *b == b'\n')
            .map(|n| n + 1)
            .unwrap_or(bytes.len());
        if out.len() + take > max {
            return Err(io::ErrorKind::InvalidData.into());
        }
        out.extend_from_slice(&bytes[..take]);
        reader.consume(take);
        if out.last() == Some(&b'\n') {
            out.pop();
            if out.last() == Some(&b'\r') {
                out.pop();
            }
            return String::from_utf8(out)
                .map(Some)
                .map_err(|_| io::ErrorKind::InvalidData.into());
        }
    }
}
pub fn token_matches(expected: &str, supplied: &str) -> bool {
    expected.len() == supplied.len()
        && expected
            .bytes()
            .zip(supplied.bytes())
            .fold(0u8, |v, (a, b)| v | (a ^ b))
            == 0
}
/// Atomically replace without following a pre-existing symlink. Unix temp
/// files are 0600; Windows removes inherited access before writing secrets.
pub fn write_secret(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
    #[cfg(windows)]
    owner_only(file.path())?;
    file.write_all(bytes)?;
    file.flush()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
#[cfg(windows)]
fn owner_only(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::{
        Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SetFileSecurityW,
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sddl: Vec<u16> = "D:P(A;;GA;;;OW)".encode_utf16().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let ok = SetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        let error = if ok == 0 {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        windows_sys::Win32::Foundation::LocalFree(descriptor);
        if let Some(error) = error {
            return Err(error);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_lines_reject_oversize_truncation_and_expiry() {
        let deadline = Instant::now() + IO_TIMEOUT;
        assert_eq!(
            line(&mut &b"ok\r\nnext\n"[..], 4, deadline).unwrap(),
            Some("ok".into())
        );
        assert!(line(&mut &b"12345\n"[..], 5, deadline).is_err());
        assert!(line(&mut &b"partial"[..], 20, deadline).is_err());
        assert!(line(&mut &b"ok\n"[..], 20, Instant::now()).is_err());
        assert_eq!(line(&mut &b""[..], 20, deadline).unwrap(), None);
    }
    #[test]
    fn secrets_replace_symlinks_without_touching_targets() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let path = dir.path().join("instance");
        std::fs::write(&target, "unchanged").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &path).unwrap();
        write_secret(&path, b"secret").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"unchanged");
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn tokens_are_unpredictable_and_compared_in_full() {
        let a = crate::remote::new_token();
        let b = crate::remote::new_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(token_matches(&a, &a));
        assert!(!token_matches(&a, &b));
        assert!(!token_matches(&a, &a[..63]));
    }
}

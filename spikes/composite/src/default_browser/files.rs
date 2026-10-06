//! Documents the system opens in nus: Markdown, PDF, JSON, CSV and plain
//! text, the kinds nus reads (its viewers, and Chromium's for PDF).
//!
//! Apart from the browser registration on purpose: being the default
//! browser never takes over documents, and nothing here happens at install.
//! Each kind is asked for from FILE VIEWERS. Windows lets only the person
//! choose a default (Default Apps opens on nus's page); macOS and Linux set
//! it when asked.
//!
//! The system hands a document over as `--open-file -- <path>` (a file URL
//! from a Linux desktop entry, a path from Windows), or on macOS as an
//! open-documents event; either way it arrives as a `file://` URL.

use super::Identity;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Doc {
    Markdown,
    Pdf,
    Json,
    Csv,
    Text,
}

impl Doc {
    pub const ALL: [Doc; 5] = [Doc::Markdown, Doc::Pdf, Doc::Json, Doc::Csv, Doc::Text];
    pub fn label(self) -> &'static str {
        match self {
            Doc::Markdown => "MARKDOWN",
            Doc::Pdf => "PDF",
            Doc::Json => "JSON",
            Doc::Csv => "CSV",
            Doc::Text => "TEXT",
        }
    }
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Doc::Markdown => &["md", "markdown", "mdown"],
            Doc::Pdf => &["pdf"],
            Doc::Json => &["json"],
            Doc::Csv => &["csv", "tsv"],
            Doc::Text => &["txt", "log"],
        }
    }
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn mimes(self) -> &'static [&'static str] {
        match self {
            Doc::Markdown => &["text/markdown", "text/x-markdown"],
            Doc::Pdf => &["application/pdf"],
            Doc::Json => &["application/json"],
            Doc::Csv => &["text/csv", "text/tab-separated-values"],
            Doc::Text => &["text/plain"],
        }
    }
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn uti(self) -> &'static str {
        match self {
            Doc::Markdown => "net.daringfireball.markdown",
            Doc::Pdf => "com.adobe.pdf",
            Doc::Json => "public.json",
            Doc::Csv => "public.comma-separated-values-text",
            Doc::Text => "public.plain-text",
        }
    }
    pub fn of(path: &std::path::Path) -> Option<Doc> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Doc::ALL.into_iter().find(|d| d.extensions().contains(&ext.as_str()))
    }
    fn index(self) -> usize {
        Doc::ALL.iter().position(|d| *d == self).unwrap_or(0)
    }
}

/// What the system says for each kind: Some(true) opens in this nus.
#[derive(Default)]
struct State {
    opens: [Option<bool>; 5],
    note: String,
    revision: u64,
    busy: bool,
}
static STATE: Mutex<State> = Mutex::new(State { opens: [None; 5], note: String::new(), revision: 0, busy: false });
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// (revision, which kinds open in nus, a note when there is one).
pub fn status() -> (u64, [Option<bool>; 5], String) {
    let s = state();
    (s.revision, s.opens, s.note.clone())
}

fn finish(opens: Result<[Option<bool>; 5], String>, note: Option<String>) {
    let mut s = state();
    s.busy = false;
    match opens {
        Ok(o) => {
            s.opens = o;
            s.note = note.unwrap_or_default();
        }
        Err(e) => s.note = note.unwrap_or(e),
    }
    s.revision += 1;
    drop(s);
    crate::browser_runtime::wake();
}

fn begin() -> Option<Identity> {
    let mut s = state();
    if s.busy {
        return None;
    }
    match Identity::discover() {
        Ok(i) => {
            s.busy = true;
            Some(i)
        }
        Err(e) => {
            s.note = e;
            s.revision += 1;
            None
        }
    }
}

/// Look again at what opens each kind.
pub fn refresh() {
    if crate::private::enabled() {
        return;
    }
    let Some(identity) = begin() else { return };
    #[cfg(target_os = "macos")]
    finish(macos::query(&identity), None);
    #[cfg(not(target_os = "macos"))]
    std::thread::spawn(move || finish(query(&identity), None));
}

/// Ask for `doc` to open in nus. Called only from native controls.
pub fn request(doc: Doc) {
    if crate::private::enabled() || std::env::var_os("NUS_SHOT").is_some() {
        let mut s = state();
        s.note = "Choose what opens files from a regular, non-test window.".into();
        s.revision += 1;
        return;
    }
    let Some(identity) = begin() else { return };
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = macos::request(&identity, doc) {
            finish(macos::query(&identity), Some(e));
        }
    }
    #[cfg(not(target_os = "macos"))]
    std::thread::spawn(move || {
        let note = platform_request(&identity, doc).err();
        let waiting = cfg!(windows) && note.is_none();
        let note = note.or_else(|| waiting.then(|| format!("Choose nus for .{} in Default Apps, then return here.", doc.extensions()[0])));
        finish(query(&identity), note);
    });
}

#[cfg(target_os = "macos")]
pub(super) fn mac_done(identity: Identity, error: Option<isize>) {
    let note = error.map(|code| format!("The system did not make the change (code {code})."));
    finish(macos::query(&identity), note);
}

#[cfg(not(target_os = "macos"))]
fn query(identity: &Identity) -> Result<[Option<bool>; 5], String> {
    #[cfg(windows)]
    return windows::query(identity);
    #[cfg(target_os = "linux")]
    return linux::query(identity);
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = identity;
        Err("Opening files is unavailable on this platform.".into())
    }
}
#[cfg(not(target_os = "macos"))]
fn platform_request(identity: &Identity, doc: Doc) -> Result<(), String> {
    #[cfg(windows)]
    return windows::request(identity, doc);
    #[cfg(target_os = "linux")]
    return linux::request(identity, doc);
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (identity, doc);
        Err("Opening files is unavailable on this platform.".into())
    }
}

/// A document the system handed over (`--open-file -- …`), as the file
/// URL nus opens: an absolute path or a `file://` URL to an existing file
/// of a kind nus reads. Anything else is refused.
pub fn accept(arg: &str) -> Result<String, String> {
    if arg.len() > 32 * 1024 || arg.chars().any(|c| c.is_control()) {
        return Err("The file path is invalid or too long.".into());
    }
    let path = match url::Url::parse(arg) {
        Ok(u) if u.scheme() == "file" => u.to_file_path().map_err(|_| "The file URL is not a local path.".to_string())?,
        _ => std::path::PathBuf::from(arg),
    };
    if !path.is_absolute() {
        return Err("Files are opened by their full path.".into());
    }
    let path = path.canonicalize().map_err(|_| "The file does not exist.".to_string())?;
    if !path.is_file() {
        return Err("Only files can be opened this way.".into());
    }
    if Doc::of(&path).is_none() {
        return Err("nus opens Markdown, PDF, JSON, CSV and text files this way.".into());
    }
    url::Url::from_file_path(&path).map(|u| u.to_string()).map_err(|_| "The file path cannot be opened.".into())
}

#[cfg(windows)]
mod windows {
    use super::{Doc, Identity};
    use std::{ffi::c_void, path::Path, ptr};
    use windows_sys::Win32::{System::Registry::*, UI::Shell::ShellExecuteW, UI::WindowsAndMessaging::SW_SHOWNORMAL};

    #[link(name = "shlwapi")]
    extern "system" {
        fn AssocQueryStringW(flags: u32, kind: i32, association: *const u16, extra: *const u16, out: *mut u16, length: *mut u32) -> i32;
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn write(path: &str, name: &str, text: &str) -> Result<(), String> {
        let (path, name, text) = (wide(path), wide(name), wide(text));
        let mut key = ptr::null_mut();
        let status = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, ptr::null(), 0, KEY_SET_VALUE, ptr::null(), &mut key, ptr::null_mut()) };
        if status != 0 {
            return Err(format!("Could not create the file registration ({status})."));
        }
        let status = unsafe { RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, text.as_ptr().cast(), (text.len() * 2) as u32) };
        unsafe { RegCloseKey(key) };
        if status == 0 { Ok(()) } else { Err(format!("Could not write the file registration ({status}).")) }
    }
    fn prog_id(identity: &Identity) -> String {
        format!("{}.file", identity.key)
    }
    fn association(ext: &str, kind: i32) -> Option<String> {
        let ext = wide(&format!(".{ext}"));
        let mut len = 0;
        let flags = 0x20; // ASSOCF_NOTRUNCATE
        unsafe { AssocQueryStringW(flags, kind, ext.as_ptr(), ptr::null(), ptr::null_mut(), &mut len) };
        if len == 0 || len > 32768 {
            return None;
        }
        let mut value = vec![0u16; len as usize];
        if unsafe { AssocQueryStringW(flags, kind, ext.as_ptr(), ptr::null(), value.as_mut_ptr(), &mut len) } != 0 {
            return None;
        }
        value.truncate(len as usize);
        if value.last() == Some(&0) {
            value.pop();
        }
        String::from_utf16(&value).ok()
    }
    pub fn query(identity: &Identity) -> Result<[Option<bool>; 5], String> {
        let id = prog_id(identity);
        let mut out = [None; 5];
        for doc in Doc::ALL {
            let ext = doc.extensions()[0];
            out[doc.index()] = Some(match association(ext, 20) {
                // ASSOCSTR_PROGID, then ASSOCSTR_EXECUTABLE.
                Some(p) if p.eq_ignore_ascii_case(&id) => association(ext, 2).is_some_and(|exe| super::super::identity::same_path(Path::new(&exe), &identity.gui)),
                _ => false,
            });
        }
        Ok(out)
    }
    /// Register nus for every kind (Default Apps lists what an app can
    /// open), then open Default Apps on nus's page: Windows lets only the
    /// person choose.
    pub fn request(identity: &Identity, _doc: Doc) -> Result<(), String> {
        super::super::register_browser(identity)?;
        let id = prog_id(identity);
        let classes = format!(r"Software\Classes\{id}");
        let gui = format!("\"{}\"", identity.gui.display());
        write(&classes, "", &format!("{} Document", identity.title))?;
        write(&format!(r"{classes}\DefaultIcon"), "", &format!("{gui},0"))?;
        write(&format!(r"{classes}\shell\open\command"), "", &format!("{gui} --open-file -- \"%1\""))?;
        let capabilities = format!(r"Software\Clients\StartMenuInternet\{}\Capabilities", identity.key);
        for doc in Doc::ALL {
            for ext in doc.extensions() {
                write(&format!(r"{capabilities}\FileAssociations"), &format!(".{ext}"), &id)?;
                write(&format!(r"Software\Classes\.{ext}\OpenWithProgids"), &id, "")?;
            }
        }
        let open = wide("open");
        for uri in [format!("ms-settings:defaultapps?registeredAppUser={}", identity.key), "ms-settings:defaultapps".into()] {
            let uri = wide(&uri);
            let result = unsafe { ShellExecuteW(ptr::null_mut::<c_void>(), open.as_ptr(), uri.as_ptr(), ptr::null(), ptr::null(), SW_SHOWNORMAL) };
            if result as isize > 32 {
                return Ok(());
            }
        }
        Err("Could not open Default Apps. Open Windows Settings and choose nus there.".into())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{Doc, Identity};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObjectProtocol};
    use objc2::{class, msg_send, sel};
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    fn content_type(doc: Doc) -> Option<Retained<AnyObject>> {
        let cls = AnyClass::get(c"UTType")?;
        let id = NSString::from_str(doc.uti());
        unsafe { msg_send![cls, typeWithIdentifier: &*id] }
    }
    pub fn query(identity: &Identity) -> Result<[Option<bool>; 5], String> {
        let workspace = NSWorkspace::sharedWorkspace();
        if !workspace.respondsToSelector(sel!(URLForApplicationToOpenContentType:)) {
            return Err("This macOS version can't be asked what opens files.".into());
        }
        let mut out = [None; 5];
        for doc in Doc::ALL {
            let Some(t) = content_type(doc) else { continue };
            let app: Option<Retained<NSURL>> = unsafe { msg_send![&*workspace, URLForApplicationToOpenContentType: &*t] };
            let path: Option<Retained<NSString>> = app.and_then(|a| unsafe { msg_send![&*a, path] });
            out[doc.index()] = Some(path.is_some_and(|p| super::super::identity::same_path(std::path::Path::new(&p.to_string()), &identity.root)));
        }
        Ok(out)
    }
    pub fn request(identity: &Identity, doc: Doc) -> Result<(), String> {
        let workspace = NSWorkspace::sharedWorkspace();
        let selector = sel!(setDefaultApplicationAtURL:toOpenContentType:completionHandler:);
        if !workspace.respondsToSelector(selector) {
            return Err("On this macOS version, choose nus with Finder's Get Info › Open With.".into());
        }
        let Some(t) = content_type(doc) else { return Err("This macOS version doesn't know that kind of file.".into()) };
        let path = identity.root.to_str().ok_or("The application path is not valid Unicode.")?;
        let path = NSString::from_str(path);
        let app: Retained<NSURL> = unsafe { msg_send![class!(NSURL), fileURLWithPath: &*path] };
        let who = identity.clone();
        let completion = block2::RcBlock::new(move |error: *mut AnyObject| {
            let code = if error.is_null() { None } else { Some(unsafe { msg_send![error, code] }) };
            super::mac_done(who.clone(), code);
        });
        unsafe {
            let _: () = msg_send![&*workspace, setDefaultApplicationAtURL: &*app, toOpenContentType: &*t, completionHandler: &*completion];
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{Doc, Identity};
    use std::io::Write;
    use std::path::Path;
    use std::time::Duration;

    /// Its own entry, apart from the browser's: hidden from menus, it only
    /// names nus as able to open these kinds.
    fn desktop_id(identity: &Identity) -> String {
        identity.desktop_id.replace(".desktop", ".files.desktop")
    }
    fn run(program: &str, args: &[&str]) -> Result<String, String> {
        let out = super::super::process::run(Path::new(program), args, Duration::from_secs(4))?;
        String::from_utf8(out).map(|s| s.trim().to_string()).map_err(|_| "The desktop returned invalid association data.".into())
    }
    fn entry(identity: &Identity) -> Result<String, String> {
        let mimes: Vec<&str> = Doc::ALL.iter().flat_map(|d| d.mimes().iter().copied()).collect();
        Ok(format!(
            "[Desktop Entry]\nType=Application\nName={}\nComment=Opens documents in nus\nExec={} --open-file -- %F\nTerminal=false\nNoDisplay=true\nMimeType={};\nX-Nus-Owner=files\n",
            identity.title,
            super::super::linux_exec(&identity.gui)?,
            mimes.join(";")
        ))
    }
    pub fn query(identity: &Identity) -> Result<[Option<bool>; 5], String> {
        let id = desktop_id(identity);
        let mut out = [None; 5];
        for doc in Doc::ALL {
            out[doc.index()] = run("xdg-mime", &["query", "default", doc.mimes()[0]]).ok().map(|v| v == id);
        }
        Ok(out)
    }
    pub fn request(identity: &Identity, doc: Doc) -> Result<(), String> {
        let dir = super::super::linux_applications()?;
        std::fs::create_dir_all(&dir).map_err(|_| "Could not create the user applications directory.")?;
        let id = desktop_id(identity);
        let mut staged = tempfile::NamedTempFile::new_in(&dir).map_err(|_| "Could not stage the desktop entry.")?;
        staged.write_all(entry(identity)?.as_bytes()).and_then(|_| staged.as_file().sync_all()).map_err(|_| "Could not write the desktop entry.")?;
        staged.persist(dir.join(&id)).map_err(|_| "Could not install the desktop entry.")?;
        if let Some(d) = dir.to_str() {
            let _ = run("update-desktop-database", &[d]);
        }
        for mime in doc.mimes() {
            run("xdg-mime", &["default", &id, mime])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kinds_by_extension() {
        assert_eq!(Doc::of(std::path::Path::new("/a/README.MD")), Some(Doc::Markdown));
        assert_eq!(Doc::of(std::path::Path::new("/a/b.pdf")), Some(Doc::Pdf));
        assert_eq!(Doc::of(std::path::Path::new("/a/b.tsv")), Some(Doc::Csv));
        assert_eq!(Doc::of(std::path::Path::new("/a/b.exe")), None);
        assert_eq!(Doc::of(std::path::Path::new("/a/md")), None);
    }
    #[test]
    fn only_existing_documents_are_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("a b.md");
        std::fs::write(&md, "# hi").unwrap();
        let url = accept(md.to_str().unwrap()).unwrap();
        assert!(url.starts_with("file://") && url.ends_with("a%20b.md"), "{url}");
        assert_eq!(accept(&url).unwrap(), url);
        let exe = dir.path().join("x.exe");
        std::fs::write(&exe, "MZ").unwrap();
        assert!(accept(exe.to_str().unwrap()).is_err());
        assert!(accept("a.md").is_err());
        assert!(accept(dir.path().join("gone.md").to_str().unwrap()).is_err());
        assert!(accept(dir.path().to_str().unwrap()).is_err());
    }
}

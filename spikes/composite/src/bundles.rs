//! Optional tools, fetched on request: language servers, tree-sitter
//! grammars, assistant CLIs. The list is assets/bundles.json (a
//! profile/bundles.json beside it adds or overrides entries); each entry
//! names a download per platform and where it unpacks under profile/,
//! or a command that installs itself. Nothing arrives unless the user
//! presses GET on the welcome page's OPTIONAL TOOLS — the first boot's
//! question — and a bundle is a folder you can delete.

use crate::app::App;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Platform {
    pub url: String,
    #[serde(default)]
    pub unpack: String,
    /// For a single-file download: the name to give it.
    #[serde(default)]
    pub file: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Bundle {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub about: String,
    #[serde(default)]
    pub size_mb: u32,
    /// Where it lands, under profile/ ("bin", "grammars/rust", "" for a command).
    #[serde(default)]
    pub into: String,
    #[serde(default)]
    pub platforms: HashMap<String, Platform>,
    /// A command that installs the tool itself (no download here).
    #[serde(default)]
    pub command: Vec<String>,
    /// Not published yet.
    #[serde(default)]
    pub soon: bool,
    /// Executable or script paths required before an install is complete.
    #[serde(default)]
    pub entrypoints: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Absent,
    Soon,
    NoPlatform,
    Fetching,
    Installed,
    External,
    Failed(String),
}

pub struct Jobs {
    pub rx: Receiver<(String, Result<(), String>)>,
    pub tx: std::sync::mpsc::Sender<(String, Result<(), String>)>,
    pub running: Vec<String>,
    pub failed: HashMap<String, String>,
}

impl Jobs {
    pub fn new() -> Jobs {
        let (tx, rx) = channel();
        Jobs { rx, tx, running: Vec::new(), failed: HashMap::new() }
    }
}

impl Default for Jobs {
    fn default() -> Self {
        Self::new()
    }
}

pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        _ => "other",
    }
}

fn profile_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile")
}

/// The bundle list: the built-in one, then profile/bundles.json's entries
/// (by id) on top.
pub fn list() -> Vec<Bundle> {
    let mut v: Vec<Bundle> = serde_json::from_str(include_str!("../assets/bundles.json")).unwrap_or_default();
    if let Ok(text) = std::fs::read_to_string(profile_dir().join("bundles.json")) {
        if let Ok(mine) = serde_json::from_str::<Vec<Bundle>>(&text) {
            for b in mine {
                v.retain(|x| x.id != b.id);
                v.push(b);
            }
        }
    }
    v
}

/// Each tool owns a directory; removing one can never delete another tool.
pub fn dir_of(b: &Bundle) -> Option<std::path::PathBuf> {
    valid_id(&b.id).then(|| profile_dir().join("tools").join(&b.id))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn entrypoints(b: &Bundle) -> Vec<String> {
    if !b.entrypoints.is_empty() { return b.entrypoints.clone(); }
    vec![b.platforms.get(platform()).filter(|p|!p.file.is_empty()).map(|p|p.file.clone()).unwrap_or_else(||format!("{}{}",b.id,if cfg!(windows){".exe"}else{""}))]
}
fn safe_relative(s: &str) -> bool {
    let p=std::path::Path::new(s);
    !s.is_empty() && !s.contains('\\') && p.components().all(|c|matches!(c,std::path::Component::Normal(_)))
}
fn entry_path(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    if cfg!(windows) && name.starts_with("bin/") && !name.ends_with(".exe") {
        root.join(format!("{}.cmd",name.trim_start_matches("bin/")))
    } else {root.join(name)}
}
fn callable(path:&std::path::Path)->bool {
    let Ok(meta)=std::fs::metadata(path) else{return false;};
    if !meta.is_file() || meta.len()==0{return false;}
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;meta.permissions().mode()&0o111!=0}
    #[cfg(not(unix))] {true}
}
fn installed_at(b: &Bundle, root: &std::path::Path) -> bool {
    let names=entrypoints(b);
    !names.is_empty() && names.iter().all(|name|safe_relative(name) && entry_path(root,name).is_file())
}
pub fn resolve(command: &str) -> Option<std::path::PathBuf> {
    for b in list() {
        let Some(dir)=dir_of(&b) else {continue};
        if !dir.join(".installed").is_file() || !installed_at(&b,&dir) {continue;}
        for name in entrypoints(&b) {
            let stem=std::path::Path::new(&name).file_stem().and_then(|s|s.to_str()).unwrap_or("");
            if stem==command {return Some(entry_path(&dir,&name));}
        }
    }
    nus_lsp::registry::resolve(command,Some(&crate::lsp_host::bin_dir()))
}
fn no_window(c: &mut std::process::Command) {
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))] let _=c;
}
fn run(mut c: std::process::Command) -> Result<(), String> {
    use std::io::{Read,Seek,SeekFrom};
    use std::process::Stdio;
    no_window(&mut c);
    // Files avoid pipe deadlocks and unbounded output buffers during npm/go builds.
    let mut log=tempfile::tempfile().map_err(|e|e.to_string())?;
    c.stdin(Stdio::null()).stdout(log.try_clone().map_err(|e|e.to_string())?).stderr(log.try_clone().map_err(|e|e.to_string())?);
    let program=c.get_program().to_string_lossy().into_owned();
    let mut child=c.spawn().map_err(|e|format!("Could not start {program}: {e}"))?;
    let start=std::time::Instant::now();
    let status=loop {
        match child.try_wait().map_err(|e|e.to_string())? {
            Some(s)=>break s,
            None if start.elapsed().as_secs()>600=>{let _=child.kill();let _=child.wait();return Err(format!("{program} timed out after 10 minutes. Try again."));},
            None=>std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    };
    if status.success(){return Ok(());}
    let end=log.metadata().map_err(|e|e.to_string())?.len();
    log.seek(SeekFrom::Start(end.saturating_sub(4096))).map_err(|e|e.to_string())?;
    let mut bytes=Vec::new();log.read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    let text=String::from_utf8_lossy(&bytes);
    let line=text.lines().find(|l|l.to_ascii_lowercase().contains("error")).or_else(||text.lines().find(|l|!l.trim().is_empty())).unwrap_or("No diagnostic output");
    Err(format!("{program} exited {status}: {}",line.chars().take(300).collect::<String>()))
}
fn executable(path: &std::path::Path) -> Result<(),String> {
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;std::fs::set_permissions(path,std::fs::Permissions::from_mode(0o755)).map_err(|e|e.to_string())?;}
    #[cfg(not(unix))] let _=path;
    Ok(())
}
fn unpack(b:&Bundle,p:&Platform,archive:&std::path::Path,dir:&std::path::Path)->Result<(),String> {
    use std::io::{Read,Write};
    let io=|e:std::io::Error|e.to_string();
    let input=std::fs::File::open(archive).map_err(io)?;
    match p.unpack.as_str() {
        "zip"=>{
            let mut zip=zip::ZipArchive::new(input).map_err(|e|e.to_string())?;
            let mut expanded=0u64;
            for i in 0..zip.len() {
                let mut file=zip.by_index(i).map_err(|e|e.to_string())?;
                let relative=file.enclosed_name().ok_or("Unsafe archive path")?;
                if file.unix_mode().is_some_and(|m|m & 0o170000==0o120000){return Err("Archive symlinks are not supported".into());}
                expanded=expanded.saturating_add(file.size());if expanded>1024*1024*1024{return Err("Archive exceeds 1 GiB".into());}
                let path=dir.join(relative);
                if file.is_dir(){std::fs::create_dir_all(&path).map_err(io)?;continue;}
                std::fs::create_dir_all(path.parent().unwrap()).map_err(io)?;
                let mut out=std::fs::File::create(path).map_err(io)?;std::io::copy(&mut file,&mut out).map_err(io)?;
            }
        },
        "gz"|"none"|""=>{
            let name=if p.file.is_empty(){b.id.as_str()}else{p.file.as_str()};
            if !safe_relative(name){return Err("Unsafe filename".into());}
            let path=dir.join(name);std::fs::create_dir_all(path.parent().unwrap()).map_err(io)?;
            let mut out=std::fs::File::create(&path).map_err(io)?;
            let mut reader:Box<dyn std::io::Read>=if p.unpack=="gz"{Box::new(flate2::read::GzDecoder::new(input))}else{Box::new(input)};
            let count=std::io::copy(&mut reader.by_ref().take(1024*1024*1024+1),&mut out).map_err(io)?;
            if count>1024*1024*1024{return Err("Download expands beyond 1 GiB".into());}out.flush().map_err(io)?;
        },
        _=>return Err("Unsupported archive format".into()),
    }
    for name in entrypoints(b) {let path=entry_path(dir,&name);if path.is_file(){executable(&path)?;}}
    Ok(())
}
fn install(b:&Bundle)->Result<(),String> {
    let dir=dir_of(b).ok_or("Invalid tool identifier")?;
    if entrypoints(b).iter().any(|p|!safe_relative(p)){return Err("Unsafe executable path".into());}
    std::fs::create_dir_all(dir.parent().unwrap()).map_err(|e|e.to_string())?;
    let stage=tempfile::tempdir_in(dir.parent().unwrap()).map_err(|e|e.to_string())?;
    if !b.command.is_empty() {
        let program=&b.command[0];
        let bin=nus_lsp::registry::resolve(program,None).ok_or_else(||format!("Install {} first, then try again.",match program.as_str(){"npm"=>"Node.js (includes npm)","go"=>"Go","gh"=>"GitHub CLI",_=>program}))?;
        if program=="npm" && nus_lsp::registry::resolve("node",None).is_none(){return Err("Install Node.js first, then try again.".into());}
        let mut c=std::process::Command::new(bin);
        c.args(&b.command[1..]);
        match program.as_str() {
            "npm"=>{c.arg("--prefix").arg(stage.path()).args(["--no-audit","--no-fund","--ignore-scripts"]);},
            "go"=>{c.env("GOBIN",stage.path());},
            _=>return Err("This tool needs its publisher's installer; no managed installation is available.".into()),
        }
        run(c)?;
    } else {
        let p=b.platforms.get(platform()).ok_or("Not available for this platform")?;
        let url=url::Url::parse(&p.url).map_err(|e|e.to_string())?;
        if url.scheme()!="https"{return Err("Tool downloads require HTTPS".into());}
        let archive=tempfile::NamedTempFile::new_in(dir.parent().unwrap()).map_err(|e|e.to_string())?;
        let mut curl=std::process::Command::new("curl");
        curl.args(["--fail","--location","--silent","--show-error","--proto","=https","--proto-redir","=https","--connect-timeout","20","--max-time","300","--retry","2","--output"]).arg(archive.path()).arg(&p.url);
        run(curl)?;unpack(b,p,archive.path(),stage.path())?;
    }
    if !installed_at(b,stage.path()){return Err("The download did not contain the expected executable. Nothing was installed.".into());}
    std::fs::write(stage.path().join(".installed"),b.id.as_bytes()).map_err(|e|e.to_string())?;
    // Only replace this tool's own directory; unrelated and legacy bin files stay.
    if dir.exists(){std::fs::remove_dir_all(&dir).map_err(|e|e.to_string())?;}
    std::fs::rename(stage.path(),&dir).map_err(|e|e.to_string())?;
    Ok(())
}
fn fetch(b:Bundle,tx:std::sync::mpsc::Sender<(String,Result<(),String>)>,proxy:winit::event_loop::EventLoopProxy<crate::UserEvent>) {
    std::thread::spawn(move||{let r=install(&b);let _=tx.send((b.id,r));let _=proxy.send_event(crate::UserEvent::Wake);});
}

impl App {
    /// A bundle's state: on disk, fetching, failed, not for here, soon.
    pub(crate) fn bundle_state(&self, b: &Bundle) -> State {
        if self.jobs.running.contains(&b.id) {
            return State::Fetching;
        }
        if let Some(e) = self.jobs.failed.get(&b.id) {
            return State::Failed(e.clone());
        }
        if b.soon {
            return State::Soon;
        }
        if let Some(dir)=dir_of(b) {
            if dir.join(".installed").is_file() && installed_at(b,&dir){return State::Installed;}
        }
        if b.command.is_empty() && !b.platforms.contains_key(platform()){return State::NoPlatform;}
        if b.id=="powershell-editor-services" {
            if crate::lsp_host::bin_dir().join("pses/PowerShellEditorServices/Start-EditorServices.ps1").is_file(){return State::External;}
        } else if entrypoints(b).iter().all(|name| {
            let stem=std::path::Path::new(name).file_stem().and_then(|s|s.to_str()).unwrap_or("");
            nus_lsp::registry::resolve(stem,Some(&crate::lsp_host::bin_dir())).is_some_and(|p|callable(&p))
        }) {return State::External;}
        State::Absent
    }

    /// GET: fetch a bundle; REMOVE: delete its folder.
    pub(crate) fn bundle_toggle(&mut self, id: &str) {
        let Some(b) = list().into_iter().find(|b| b.id == id) else { return };
        match self.bundle_state(&b) {
            State::Installed => {
                if let Some(d) = dir_of(&b) {
                    match std::fs::remove_dir_all(d) {
                        Ok(())=>self.toast_with(None,format!("{} removed",b.name),"",None),
                        Err(e)=>self.toast_with(None,"Could not remove tool",e.to_string(),None),
                    }
                }
            }
            State::Absent | State::Failed(_) => {
                self.jobs.failed.remove(id);
                self.jobs.running.push(id.to_string());
                self.toast_with(None,format!("Installing {}",b.name),"You can keep working",None);
                fetch(b, self.jobs.tx.clone(),self.proxy.clone());
            }
            _ => {}
        }
        self.play_event("control.press");
        self.dirty = true;
    }

    /// Once a loop: finished fetches.
    pub(crate) fn tend_bundles(&mut self) {
        while let Ok((id, r)) = self.jobs.rx.try_recv() {
            self.jobs.running.retain(|x| x != &id);
            match r {
                Ok(()) => {
                    self.play_event("page.ready");
                    self.lsp.failed.clear();
                    for tab in &mut self.tabs {for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                        if let crate::app::Pane::Term(t)=pane {if t.plsp.is_none(){t.plsp_tried=false;}}
                    }}
                    let mut buffers=Vec::new();
                    for (ti,tab) in self.tabs.iter().enumerate(){for (right,pane) in std::iter::once((false,&tab.left)).chain(tab.right.as_ref().map(|p|(true,p))) {if let crate::app::Pane::Editor(e)=pane{for (bi,b) in e.buffers.iter().enumerate(){if b.ready()&&!b.in_lsp{buffers.push((ti,right,bi));}}}}}
                    for (ti,right,bi) in buffers{self.lsp_open_buffer(ti,right,bi);}
                    self.toast_with(None,format!("{id} installed"),"Ready to use",None);
                }
                Err(e) => {
                    self.toast_with(None,format!("{id} failed"),&e,None);
                    self.jobs.failed.insert(id, e);
                }
            }
            self.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bundle(id:&str)->Bundle {serde_json::from_value(serde_json::json!({"id":id,"name":id,"kind":"lsp","about":"fixture","into":"bin"})).unwrap()}
    #[test]
    fn installed_detection_requires_this_tools_executable() {
        let tmp=tempfile::tempdir().unwrap();
        let a=bundle("alpha");let b=bundle("beta");
        let name=entrypoints(&a).remove(0);std::fs::write(tmp.path().join(name),b"a").unwrap();
        assert!(installed_at(&a,tmp.path()));assert!(!installed_at(&b,tmp.path()));
        assert_ne!(dir_of(&a),dir_of(&b));assert!(dir_of(&bundle("../bin")).is_none());
    }
    #[test]
    fn gzip_and_raw_files_are_executable_in_paths_with_quotes() {
        use std::io::Write;
        let tmp=tempfile::tempdir().unwrap();let root=tmp.path().join("a ' quoted path");std::fs::create_dir(&root).unwrap();
        let b=bundle("fixture");let archive=tmp.path().join("archive");
        let mut gz=flate2::write::GzEncoder::new(Vec::new(),flate2::Compression::default());gz.write_all(b"#!/bin/sh\nexit 0\n").unwrap();std::fs::write(&archive,gz.finish().unwrap()).unwrap();
        let name=entrypoints(&b).remove(0);unpack(&b,&Platform{url:String::new(),unpack:"gz".into(),file:name.clone()},&archive,&root).unwrap();
        assert!(installed_at(&b,&root));
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;assert_ne!(std::fs::metadata(root.join(&name)).unwrap().permissions().mode()&0o111,0);}
        assert!(unpack(&b,&Platform{url:String::new(),unpack:"none".into(),file:"../escape".into()},&archive,&root).is_err());
    }
    #[test]
    fn zip_rejects_traversal_and_missing_entry_is_not_installed() {
        use std::io::Write;
        let tmp=tempfile::tempdir().unwrap();let path=tmp.path().join("bad.zip");let mut zip=zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("../escape",zip::write::SimpleFileOptions::default()).unwrap();zip.write_all(b"bad").unwrap();zip.finish().unwrap();
        assert!(unpack(&bundle("a"),&Platform{url:String::new(),unpack:"zip".into(),file:String::new()},&path,tmp.path()).is_err());
        assert!(!installed_at(&bundle("a"),tmp.path()));
    }
    #[test]
    fn command_failures_always_include_exit_status() {
        let mut c=std::process::Command::new(if cfg!(windows){"cmd"}else{"sh"});
        if cfg!(windows){c.args(["/C","exit 7"]);}else{c.args(["-c","exit 7"]);}
        let error=run(c).unwrap_err();assert!(error.contains('7'));assert!(error.contains("No diagnostic output"));
    }
}

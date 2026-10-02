use super::{Identity,Observation,process};
use std::{path::{Path,PathBuf},time::Duration,io::Write};

fn applications()->Result<PathBuf,String> {
    let data=std::env::var_os("XDG_DATA_HOME").filter(|s|!s.is_empty()).map(PathBuf::from)
        .or_else(||std::env::var_os("HOME").map(|p|PathBuf::from(p).join(".local/share")))
        .ok_or("Could not locate the user applications directory.")?;
    if !data.is_absolute(){return Err("The applications directory must be absolute.".into());}
    Ok(data.join("applications"))
}
fn session()->Result<(),String> {
    if unsafe{libc::geteuid()}==0 {return Err("Browser integration must run as the desktop user, not root.".into());}
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err("No desktop session is available for browser integration.".into());
    }
    Ok(())
}
fn run(program:&str,args:&[&str])->Result<String,String> {
    let out=process::run(Path::new(program),args,Duration::from_secs(4))?;
    String::from_utf8(out).map(|s|s.trim().to_string()).map_err(|_|"The desktop returned invalid association data.".into())
}
fn marker(identity:&Identity)->String {
    use sha2::{Digest,Sha256};
    format!("{:x}",Sha256::digest(identity.root.to_string_lossy().as_bytes()))
}
/// Desktop Entry string escaping is a different layer from Exec quoting.
fn value(text:&str)->Result<String,String> {
    if text.chars().any(|c|c.is_control()) {return Err("The installation path contains a control character.".into());}
    Ok(text.replace('\\',"\\\\"))
}
fn executable(path:&Path)->Result<String,String> {
    let s=path.to_str().ok_or("The launcher path is not valid Unicode.")?;
    if !path.is_absolute() || s.contains('=') {return Err("Install nus at an absolute path without '=' before creating its desktop entry.".into());}
    let mut out=String::from("\"");
    for ch in s.chars() {
        if ch.is_control(){return Err("The launcher path contains a control character.".into());}
        match ch {
            '\\'=>out.push_str("\\\\\\\\"),
            '"'|'`'|'$'=>{out.push_str("\\\\");out.push(ch);},
            '%'=>out.push_str("%%"),
            _=>out.push(ch),
        }
    }
    out.push('"');Ok(out)
}
fn desktop(identity:&Identity)->Result<String,String> {
    let icon=value(identity.root.join("nus.png").to_str().ok_or("The icon path is not valid Unicode.")?)?;
    Ok(format!("[Desktop Entry]\nType=Application\nName={}\nComment=A terminal and browser\nExec={} --open-external -- %U\nIcon={}\nTerminal=false\nCategories=Development;TerminalEmulator;WebBrowser;\nMimeType=x-scheme-handler/http;x-scheme-handler/https;\nStartupWMClass=nus\nX-Nus-Owner={}\n",
        identity.title,executable(&identity.gui)?,icon,marker(identity)))
}
/// The lines that make an entry this installation's registration. The Dock
/// publisher may update Icon without changing registration.
fn critical(text:&str)->Vec<&str> {
    text.lines().filter(|l|l.starts_with("Exec=")||l.starts_with("MimeType=")||l.starts_with("X-Nus-Owner=")||l.starts_with("Type=")).collect()
}
/// A system package's entry, which a user entry of the same id overrides.
fn system_entry(identity:&Identity)->Option<String> {
    let dirs=std::env::var_os("XDG_DATA_DIRS").filter(|s|!s.is_empty()).unwrap_or_else(||"/usr/local/share:/usr/share".into());
    std::env::split_paths(&dirs).filter(|p|p.is_absolute())
        .find_map(|p|std::fs::read_to_string(p.join("applications").join(identity.desktop_id)).ok())
}
pub fn register(identity:&Identity)->Result<(),String> {
    session()?;
    // A system package installs its own entry; a user copy would only shadow it.
    if crate::distribution::managed().is_some() {
        let expected=desktop(identity)?;
        if system_entry(identity).is_some_and(|s|critical(&s)==critical(&expected)) {return Ok(());}
    }
    let dir=applications()?;
    std::fs::create_dir_all(&dir).map_err(|_|"Could not create the user applications directory.")?;
    let path=dir.join(identity.desktop_id);
    if path.exists() {
        let old=std::fs::read_to_string(&path).map_err(|_|"Could not read the existing browser entry.")?;
        let marked=old.lines().any(|l|l==format!("X-Nus-Owner={}",marker(identity)));
        let legacy= !old.lines().any(|l|l.starts_with("X-Nus-Owner="))
            && old.lines().filter(|l|l.starts_with("Exec=")).count()==1
            && [&identity.gui, &identity.root.join("nus-desktop")].iter().any(|exe|
                executable(exe).is_ok_and(|cmd|old.lines().any(|l|l==format!("Exec={cmd} %U"))));
        if !marked && !legacy {
            return Err("Another installation owns this channel's desktop entry. Remove or repair that entry before registering this installation.".into());
        }
    }
    let text=desktop(identity)?;
    let mut temporary=tempfile::NamedTempFile::new_in(&dir).map_err(|_|"Could not stage the desktop entry.")?;
    temporary.write_all(text.as_bytes()).and_then(|_|temporary.as_file().sync_all()).map_err(|_|"Could not write the desktop entry.")?;
    temporary.persist(&path).map_err(|_|"Could not install the desktop entry.")?;
    // Refresh is optional; xdg-mime also understands the installed entry.
    let _=process::run(Path::new("update-desktop-database"),&[dir.to_str().ok_or("The applications directory is not valid Unicode.")?],Duration::from_secs(3));
    Ok(())
}
pub fn query(identity:&Identity)->Result<Observation,String> {
    session()?;
    if std::env::var_os("BROWSER").is_some_and(|v|!v.is_empty()) {
        return Err("The BROWSER environment variable overrides desktop launch policy. This session’s link handling cannot be verified.".into());
    }
    let expected=desktop(identity)?;
    let entry=std::fs::read_to_string(applications()?.join(identity.desktop_id)).ok().or_else(||system_entry(identity));
    let registered=entry.is_some_and(|s|critical(&s)==critical(&expected));
    let handler=|scheme:&str| {
        run("xdg-mime",&["query","default",&format!("x-scheme-handler/{scheme}")]).ok().map(|v|v==identity.desktop_id && registered)
    };
    Ok(Observation{http:handler("http"),https:handler("https"),registered})
}
pub fn request(identity:&Identity)->Result<(),String> {
    register(identity)?;
    for scheme in ["http","https"] {
        // Keep scope to the two URL schemes. Do not take over HTML/PDF files.
        if run("xdg-settings",&["set","default-url-scheme-handler",scheme,identity.desktop_id]).is_err() {
            run("xdg-mime",&["default",identity.desktop_id,&format!("x-scheme-handler/{scheme}")])?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn exec_argument_is_not_a_shell_command() {
        assert_eq!(executable(Path::new("/opt/nus/nus")).unwrap(),"\"/opt/nus/nus\"");
        assert!(executable(Path::new("/opt/a=b/nus")).is_err());
        assert!(executable(Path::new("/opt/a\nnus")).is_err());
        let q=executable(Path::new("/opt/a $`\"%\\/nus")).unwrap();
        assert!(q.contains("%%"));assert!(q.contains("\\\\$"));assert!(q.contains("\\\\\\\\"));
    }
    /// scripts/package-linux.py writes the system entry; registration accepts
    /// it only while these lines agree.
    #[test] fn system_package_entry_matches_registration() {
        let identity=Identity{gui:"/opt/nus-preview/nus".into(),root:"/opt/nus-preview".into(),key:"nus-preview",
            prog_id:"nus-preview.url",desktop_id:"dev.nus.app.preview.desktop",title:"nus Preview"};
        let text=desktop(&identity).unwrap();
        assert_eq!(critical(&text),["Type=Application","Exec=\"/opt/nus-preview/nus\" --open-external -- %U",
            "MimeType=x-scheme-handler/http;x-scheme-handler/https;",
            "X-Nus-Owner=c0a7538a52d859fa78cb2dcb63be773388bc8867ba8bf92c466a265eb082b8e2"]);
    }
}

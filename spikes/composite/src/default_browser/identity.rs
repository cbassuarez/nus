use std::path::{Path,PathBuf};

#[derive(Clone,Debug)]
pub struct Identity {
    pub gui:PathBuf,
    pub root:PathBuf,
    pub key:&'static str,
    pub prog_id:&'static str,
    pub desktop_id:&'static str,
    pub title:&'static str,
}
pub fn preview()->bool {env!("NUS_BUILD_VERSION").contains("preview")}
pub fn app_id()->&'static str {if preview(){"dev.nus.app.preview"}else{"dev.nus.app"}}
impl Identity {
    pub fn discover()->Result<Self,String> {
        let exe=std::env::current_exe().and_then(std::fs::canonicalize).map_err(|_|"Could not identify the installed application.".to_string())?;
        let parent=exe.parent().ok_or("Could not locate the application folder.")?;
        let mut root=parent.to_path_buf();
        let mut gui=exe.clone();
        #[cfg(target_os="macos")]
        {
            root=parent.parent().and_then(Path::parent).filter(|p|p.extension().is_some_and(|e|e=="app"))
                .ok_or("Install the nus.app bundle before choosing a default browser.")?.to_path_buf();
            if !root.join("Contents/Info.plist").is_file()
                || root.to_string_lossy().contains("/AppTranslocation/")
                || root.starts_with("/Volumes") || root.starts_with(std::env::temp_dir()) {
                return Err("Move nus.app to a stable Applications folder, reopen it, then choose Make Default.".into());
            }
        }
        #[cfg(target_os="linux")]
        {
            gui=root.join("nus");
            if !root.join("nus-desktop").is_file() || !gui.is_file() || !root.join("nus-package.json").is_file() {
                return Err("Use the complete installed Linux package before choosing Make Default.".into());
            }
            if root.starts_with(std::env::temp_dir()) || std::env::var_os("FLATPAK_ID").is_some() || std::env::var_os("SNAP").is_some() {
                return Err("This temporary or sandboxed installation cannot register a durable browser target.".into());
            }
        }
        #[cfg(windows)]
        {
            let base=std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or("LOCALAPPDATA is unavailable.")?;
            let expected=base.join("Programs/nus").join(if preview(){"preview"}else{"release"}).join("nus.exe");
            if !same_path(&exe,&expected) || !root.join("nus.dll").is_file() {
                return Err("Install this channel with the nus installer before choosing Make Default. Portable copies do not replace installed registrations.".into());
            }
        }
        Ok(Self {gui,root,key:if preview(){"nus-preview"}else{"nus"},
            prog_id:if preview(){"nus-preview.url"}else{"nus.url"},
            desktop_id:if preview(){"dev.nus.app.preview.desktop"}else{"dev.nus.app.desktop"},
            title:if preview(){"nus Preview"}else{"nus"}})
    }
    pub fn dismissal(&self)->Option<PathBuf> {
        #[cfg(target_os="macos")]
        let base=std::env::var_os("HOME").map(PathBuf::from).filter(|p|p.is_absolute())?.join("Library/Application Support");
        #[cfg(windows)]
        let base=std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|p|p.is_absolute())?;
        #[cfg(not(any(windows,target_os="macos")))]
        let base=std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p|p.is_absolute()).or_else(||std::env::var_os("HOME").map(PathBuf::from).filter(|p|p.is_absolute()).map(|p|p.join(".local/share")))?;
        // Stable installation-scoped marker, outside synced/ephemeral profiles.
        use sha2::{Digest,Sha256};
        let id=format!("{:x}",Sha256::digest(self.root.to_string_lossy().as_bytes()));
        Some(base.join("nus/browser-integration").join(self.key).join(&id[..16]).join("offer-dismissed"))
    }
}
pub fn same_path(a:&Path,b:&Path)->bool {
    let (Ok(a),Ok(b))=(std::fs::canonicalize(a),std::fs::canonicalize(b)) else{return false};
    #[cfg(windows)] {a.to_string_lossy().to_lowercase()==b.to_string_lossy().to_lowercase()}
    #[cfg(not(windows))] {a==b}
}

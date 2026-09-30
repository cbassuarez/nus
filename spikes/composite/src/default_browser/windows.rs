use super::{Identity,Observation,identity::same_path};
use std::{ffi::c_void,path::Path,ptr};
use windows_sys::Win32::{System::Registry::*,Foundation::ERROR_FILE_NOT_FOUND,UI::{Shell::ShellExecuteW,WindowsAndMessaging::SW_SHOWNORMAL}};

// Stable Shlwapi ABI. Avoid creating or retaining apartment-bound COM objects.
#[link(name="shlwapi")]
extern "system" {
    fn AssocQueryStringW(flags:u32,kind:i32,association:*const u16,extra:*const u16,out:*mut u16,length:*mut u32)->i32;
}
fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
fn read(path:&str,name:&str)->Result<Option<String>,String> {
    let (path,name)=(wide(path),wide(name));let mut key=ptr::null_mut();
    let status=unsafe{RegOpenKeyExW(HKEY_CURRENT_USER,path.as_ptr(),0,KEY_READ,&mut key)};
    if status==ERROR_FILE_NOT_FOUND{return Ok(None);}
    if status!=0{return Err(format!("Could not read browser registration ({status})."));}
    struct Key(HKEY);impl Drop for Key{fn drop(&mut self){unsafe{RegCloseKey(self.0);}}}
    let key=Key(key);let mut len=0;let mut ty=0;
    let status=unsafe{RegQueryValueExW(key.0,name.as_ptr(),ptr::null(),&mut ty,ptr::null_mut(),&mut len)};
    if status==ERROR_FILE_NOT_FOUND{return Ok(None);}
    if status!=0 || ty!=REG_SZ || len>65536 || len%2!=0{return Err("Invalid browser registration value.".into());}
    let mut data=vec![0u16;len as usize/2];
    let status=unsafe{RegQueryValueExW(key.0,name.as_ptr(),ptr::null(),&mut ty,data.as_mut_ptr().cast(),&mut len)};
    if status!=0{return Err("Browser registration changed during inspection.".into());}
    if data.last()==Some(&0){data.pop();}
    String::from_utf16(&data).map(Some).map_err(|_|"Invalid browser registration text.".into())
}
fn write(path:&str,name:&str,text:&str)->Result<(),String> {
    let (path,name,text)=(wide(path),wide(name),wide(text));let mut key=ptr::null_mut();
    let status=unsafe{RegCreateKeyExW(HKEY_CURRENT_USER,path.as_ptr(),0,ptr::null(),0,KEY_SET_VALUE,ptr::null(),&mut key,ptr::null_mut())};
    if status!=0{return Err(format!("Could not create browser registration ({status})."));}
    let status=unsafe{RegSetValueExW(key,name.as_ptr(),0,REG_SZ,text.as_ptr().cast(),(text.len()*2)as u32)};
    unsafe{RegCloseKey(key);}
    if status==0{Ok(())}else{Err(format!("Could not write browser registration ({status})."))}
}
fn command(identity:&Identity)->String {format!("\"{}\" --open-external -- \"%1\"",identity.gui.display())}
fn owned_command(command:&str,identity:&Identity)->bool {
    let Some(tail)=command.strip_prefix('"') else{return false};
    let Some((target,_))=tail.split_once('"') else{return false};
    same_path(Path::new(target),&identity.gui)
}
fn register(identity:&Identity)->Result<(),String> {
    let classes=format!(r"Software\Classes\{}",identity.prog_id);
    let old=read(&format!(r"{classes}\shell\open\command"),"")?;
    if old.as_ref().is_some_and(|cmd|!owned_command(cmd,identity)) {
        return Err("Another installation owns this browser registration. Repair the installed channel before continuing.".into());
    }
    let browser=format!(r"Software\Clients\StartMenuInternet\{}",identity.key);
    if read(&format!(r"{browser}\shell\open\command"), "")?.as_ref()
        .is_some_and(|cmd|!owned_command(cmd,identity)) {
        return Err("Another installation owns this channel's browser capabilities.".into());
    }
    let capabilities=format!(r"{browser}\Capabilities");
    let gui=format!("\"{}\"",identity.gui.display());let icon=format!("{gui},0");
    let entries=vec![
        (classes.clone(),"",format!("{} URL",identity.title)),
        (classes.clone(),"URL Protocol",String::new()),
        (format!(r"{classes}\DefaultIcon"),"",icon.clone()),
        (format!(r"{classes}\shell\open\command"),"",command(identity)),
        (browser.clone(),"",identity.title.into()),
        (format!(r"{browser}\DefaultIcon"),"",icon.clone()),
        (format!(r"{browser}\shell\open\command"),"",gui),
        (capabilities.clone(),"ApplicationName",identity.title.into()),
        (capabilities.clone(),"ApplicationDescription","A terminal and browser".into()),
        (capabilities.clone(),"ApplicationIcon",icon),
        (format!(r"{capabilities}\StartMenu"),"StartMenuInternet",identity.key.into()),
        (format!(r"{capabilities}\URLAssociations"),"http",identity.prog_id.into()),
        (format!(r"{capabilities}\URLAssociations"),"https",identity.prog_id.into()),
        (r"Software\RegisteredApplications".into(),identity.key,capabilities),
    ];
    for (path,name,value) in entries {write(&path,name,&value)?;}
    Ok(())
}
fn association(scheme:&str,kind:i32)->Option<String> {
    let scheme=wide(scheme);let mut len=0;
    let flags=0x1000|0x20; // ASSOCF_IS_PROTOCOL | ASSOCF_NOTRUNCATE
    unsafe{AssocQueryStringW(flags,kind,scheme.as_ptr(),ptr::null(),ptr::null_mut(),&mut len);}
    if len==0 || len>32768{return None;}
    let mut value=vec![0u16;len as usize];
    let status=unsafe{AssocQueryStringW(flags,kind,scheme.as_ptr(),ptr::null(),value.as_mut_ptr(),&mut len)};
    if status!=0{return None;}
    value.truncate(len as usize);if value.last()==Some(&0){value.pop();}
    String::from_utf16(&value).ok()
}
pub fn query(identity:&Identity)->Result<Observation,String> {
    let path=format!(r"Software\Classes\{}\shell\open\command",identity.prog_id);
    let cmd=read(&path,"")?;
    let registered=cmd.as_deref().is_some_and(|c|owned_command(c,identity));
    let handler=|s:&str|->Option<bool>{
        let id=association(s,20)?; // ASSOCSTR_PROGID: effective user association
        if !id.eq_ignore_ascii_case(identity.prog_id){return Some(false);}
        let executable=association(s,2)?; // ASSOCSTR_EXECUTABLE
        Some(registered && same_path(Path::new(&executable),&identity.gui))
    };
    Ok(Observation{http:handler("http"),https:handler("https"),registered})
}
pub fn request(identity:&Identity)->Result<(),String> {
    register(identity)?;
    let open=wide("open");
    for uri in [format!("ms-settings:defaultapps?registeredAppUser={}",identity.key),"ms-settings:defaultapps".into()] {
        let uri=wide(&uri);
        let result=unsafe{ShellExecuteW(ptr::null_mut::<c_void>(),open.as_ptr(),uri.as_ptr(),ptr::null(),ptr::null(),SW_SHOWNORMAL)};
        if result as isize>32{return Ok(());}
    }
    Err("Could not open Default Apps. Open Windows Settings and choose nus there.".into())
}

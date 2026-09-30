use super::{identity::same_path,Identity,Observation};
use objc2::{class,msg_send,sel,MainThreadMarker};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject,NSObjectProtocol};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString,NSURL};

fn file_url(path:&std::path::Path)->Result<Retained<NSURL>,String> {
    let path=path.to_str().ok_or("The application path is not valid Unicode.")?;
    let value=NSString::from_str(path);
    Ok(unsafe {msg_send![class!(NSURL),fileURLWithPath:&*value]})
}
pub fn query(identity:&Identity)->Result<Observation,String> {
    let _mtm=MainThreadMarker::new().ok_or("Default-browser lookup must run on the native main thread.")?;
    let workspace=NSWorkspace::sharedWorkspace();
    let handler=|scheme:&str|->Option<bool> {
        let text=NSString::from_str(&format!("{scheme}://nus-association-check.invalid/"));
        let url:Option<Retained<NSURL>>=unsafe{msg_send![class!(NSURL),URLWithString:&*text]};
        let url=url?;
        // A Launch Services lookup only. This does not open/fetch the probe URL.
        let app:Option<Retained<NSURL>>=unsafe{msg_send![&*workspace,URLForApplicationToOpenURL:&*url]};
        match app {
            Some(app)=>{
                let path:Option<Retained<NSString>>=unsafe{msg_send![&*app,path]};
                path.map(|p|same_path(std::path::Path::new(&p.to_string()),&identity.root))
            }
            None=>Some(false),
        }
    };
    Ok(Observation{http:handler("http"),https:handler("https"),registered:identity.root.join("Contents/Info.plist").is_file()})
}
pub fn request_scheme(identity:&Identity,ticket:u64,scheme:u8)->Result<(),String> {
    let _mtm=MainThreadMarker::new().ok_or("The system browser request must run on the native main thread.")?;
    let workspace=NSWorkspace::sharedWorkspace();
    let selector=sel!(setDefaultApplicationAtURL:toOpenURLsWithScheme:completionHandler:);
    if !workspace.respondsToSelector(selector) {
        // Older supported systems retain a usable, user-controlled route.
        // Open the system application itself, not an undocumented pane URL.
        if let Ok(settings)=file_url(std::path::Path::new("/System/Applications/System Preferences.app")) {
            let _:bool=unsafe{msg_send![&*workspace,openURL:&*settings]};
        }
        return Err("Choose nus in System Settings (System Preferences on older macOS), then use Check Again. This macOS version requires the system-settings route.".into());
    }
    let app=file_url(&identity.root)?;
    let scheme_name=NSString::from_str(if scheme==0{"http"}else{"https"});
    let completion=block2::RcBlock::new(move |error:*mut AnyObject| {
        let code=if error.is_null(){None}else{Some(unsafe{msg_send![error,code]})};
        super::mac_result(ticket,scheme,code);
    });
    // NSWorkspace copies the completion block. No App/window reference crosses
    // this boundary; closing the originating window cannot leave a dangling UI.
    unsafe {
        let _:()=msg_send![&*workspace,setDefaultApplicationAtURL:&*app,
            toOpenURLsWithScheme:&*scheme_name,completionHandler:&*completion];
    }
    Ok(())
}

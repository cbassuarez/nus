//! Strict HTTP(S) activation. Native URLs never become CLI switches or shell
//! input. The early macOS queue is memory-only and bounded until Host exists.
use std::{collections::VecDeque,sync::{Mutex,atomic::{AtomicU64,Ordering}}};
const MAX_COUNT:usize=128;
const MAX_URL:usize=64*1024;
const MAX_BYTES:usize=1024*1024;
static NEXT:AtomicU64=AtomicU64::new(1);
static QUEUE:Mutex<VecDeque<(u64,String)>>=Mutex::new(VecDeque::new());

pub(crate) fn validate(value:&str)->Result<(),String> {
    if value.len()>MAX_URL || value.chars().any(|c|c.is_control() || c=='"') {
        return Err("The external web link is invalid or too long.".into());
    }
    let u=url::Url::parse(value).map_err(|_|"The external web link is not a valid URL.")?;
    if !matches!(u.scheme(),"http"|"https") || u.host_str().is_none() {
        return Err("External browser activation accepts only HTTP and HTTPS URLs.".into());
    }
    Ok(())
}
/// None is an ordinary invocation. An external mode is parsed before any
/// scanning for CEF --type switches, and all items after -- remain URL data.
/// A desktop launcher's `%U` expands to nothing when its icon is clicked: an
/// empty batch opens nus with no links.
pub fn arguments(args:&[String])->Result<Option<Vec<String>>,String> {
    if args.first().map(String::as_str)!=Some("--open-external") {return Ok(None);}
    if args.get(1).map(String::as_str)!=Some("--") {return Err("External activation requires '--open-external -- <URL>'.".into());}
    let urls=&args[2..];
    if urls.is_empty() {return Ok(Some(Vec::new()));}
    if urls.len()>MAX_COUNT || urls.iter().map(String::len).sum::<usize>()>MAX_BYTES {
        return Err("The external activation batch is too large.".into());
    }
    for u in urls {validate(u)?;}
    Ok(Some(urls.to_vec())) // Preserve original escaping, query and fragment.
}
#[cfg(target_os="macos")]
pub fn enqueue(url:String)->Result<u64,String> {
    validate(&url)?;
    if crate::private::enabled() {
        let exe=std::env::current_exe().map_err(|_|"Could not locate the regular nus application.")?;
        let mut command=nus_compat::command(exe);
        command.args(["--open-external","--",&url]).stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        for key in ["NUS_PRIVATE_LOOK","NUS_SHOT","NUS_SHOT2","NUS_SHOT_DIR","NUS_SHOT_OUT"] {command.env_remove(key);}
        if let Some(home)=std::env::var_os("HOME") {command.current_dir(home);}
        let mut child=command.spawn().map_err(|_|"Could not open the web link in a regular nus window.")?;
        std::thread::spawn(move||{let _=child.wait();});
        return Ok(NEXT.fetch_add(1,Ordering::Relaxed));
    }
    let mut q=QUEUE.lock().unwrap_or_else(|e|e.into_inner());
    if q.len()>=MAX_COUNT || q.iter().map(|(_,u)|u.len()).sum::<usize>()+url.len()>MAX_BYTES {
        return Err("Too many web links are waiting for nus to start.".into());
    }
    let id=NEXT.fetch_add(1,Ordering::Relaxed);
    q.push_back((id,url));drop(q);crate::browser_runtime::wake();Ok(id)
}
/// Host invokes this only after a destination window exists. A missing window
/// never drains the queue. Separate identical activations are not deduplicated.
pub fn pump(sender:&std::sync::mpsc::Sender<crate::little::Inbound>) {
    let mut q=QUEUE.lock().unwrap_or_else(|e|e.into_inner());
    while let Some((_,url))=q.front() {
        if sender.send(crate::little::Inbound::Url(url.clone())).is_err() {break;}
        q.pop_front();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(a:&[&str])->Vec<String>{a.iter().map(|v|v.to_string()).collect()}
    #[test]fn preserving_signed_link(){
        let u="https://example.test/a%2Fb?q=%2B&sig=a%3Db#frag";
        assert_eq!(arguments(&args(&["--open-external","--",u])).unwrap(),Some(vec![u.into()]));
    }
    #[test]fn external_tail_cannot_become_a_child_switch(){
        for u in ["--type=renderer","file:///tmp/x","nus://crash","javascript:alert(1)","https://x/\" --type=renderer"] {
            assert!(arguments(&args(&["--open-external","--",u])).is_err());
        }
    }
    #[test]fn ordinary_child_dispatch_untouched(){assert_eq!(arguments(&args(&["--type=renderer"])).unwrap(),None);}
    #[test]fn launcher_without_links_is_an_ordinary_launch(){
        assert_eq!(arguments(&args(&["--open-external","--"])).unwrap(),Some(Vec::new()));
    }
    #[test]fn malformed_and_large_batches_fail(){
        assert!(arguments(&args(&["--open-external","https://x/"])).is_err());
        assert!(validate(&format!("https://x/{}","x".repeat(MAX_URL))).is_err());
    }
}

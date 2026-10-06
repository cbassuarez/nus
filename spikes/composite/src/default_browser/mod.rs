//! Process-wide default-browser integration. Registration, dispatch, and the
//! effective OS handlers are separate facts. Never initialized by a CEF child.
mod identity;
#[cfg(target_os="macos")] mod macos;
#[cfg(windows)] mod windows;
#[cfg(target_os="linux")] mod linux;
mod process;
pub mod files;

use crate::startup_policy::{default_state,DefaultState,Handler,Requests};
use std::{collections::VecDeque,sync::{Mutex,OnceLock},time::{Duration,Instant}};
pub use identity::{Identity,app_id};

#[derive(Clone,Debug,serde::Serialize,serde::Deserialize)]
pub struct Observation { pub http:Option<bool>, pub https:Option<bool>, pub registered:bool }
impl Observation {
    pub fn state(&self)->DefaultState {
        let h=|v|match v {Some(true)=>Handler::ThisInstall,Some(false)=>Handler::Other,None=>Handler::Unknown};
        default_state(h(self.http),h(self.https))
    }
}
impl Default for Observation {
    fn default()->Self {Self{http:None,https:None,registered:false}}
}
#[derive(Default)]
struct State {
    identity:Option<Result<Identity,String>>,
    requests:Requests,
    revision:u64,
    observation:Observation,
    note:String,
    checked:Option<Instant>,
    dismissed:bool,
    awaiting_choice:bool,
    pending_click:bool,
    active:Option<(u64,Identity)>,
}
static STATE:OnceLock<Mutex<State>>=OnceLock::new();
fn state()->std::sync::MutexGuard<'static,State> {
    STATE.get_or_init(||Mutex::new(State::default())).lock().unwrap_or_else(|e|e.into_inner())
}
#[cfg(target_os="macos")]
static MAC_RESULTS:Mutex<VecDeque<(u64,u8,Option<isize>)>>=Mutex::new(VecDeque::new());

#[cfg(target_os="macos")]
static MAC_QUERIES:Mutex<VecDeque<(u64,u8,Option<isize>,Result<Observation,String>)>>=Mutex::new(VecDeque::new());

pub fn init() {
    let identity=Identity::discover();
    // A menu entry an older or deleted copy left must not keep nus from opening.
    #[cfg(target_os="linux")]
    if let Ok(i)=&identity { linux::repair(i); }
    let dismissed=identity.as_ref().is_ok_and(|i|i.dismissal().is_some_and(|p|p.is_file()));
    let mut s=state();
    if s.identity.is_some() {return;}
    s.dismissed=dismissed; s.identity=Some(identity); s.revision=1;
    s.note="Checking default browser…".into(); drop(s);
    refresh();
}

pub fn status()->(u64,String) {
    let s=state();
    (s.revision,if !s.note.is_empty(){s.note.clone()}else{words(&s.observation).into()})
}
fn words(o:&Observation)->&'static str {
    match o.state() {
        DefaultState::Default=>"nus opens HTTP and HTTPS links.",
        DefaultState::Partial=>"Only one web-link scheme opens in nus. Choose Make Default to finish setup.",
        DefaultState::NotDefault=>"Web links open in another application.",
        DefaultState::Unknown=>"Could not verify the current default. Choose Check Again.",
    }
}
pub fn offer_visible()->bool {
    if crate::private::enabled() {return false;}
    let s=state();
    !s.dismissed && s.identity.as_ref().is_some_and(|i|i.is_ok())
        && s.observation.state()!=DefaultState::Default && !s.requests.busy
}
pub fn dismiss_offer() {
    let path={let mut s=state();s.dismissed=true;s.revision+=1;
        s.identity.as_ref().and_then(|i|i.as_ref().ok()).and_then(Identity::dismissal)};
    if let Some(path)=path { std::thread::spawn(move|| {
        if let Some(dir)=path.parent() {if std::fs::create_dir_all(dir).is_err(){return;}}
        let _=std::fs::write(path,b"1\n");
    }); }
}

pub fn refresh() {
    let (ticket,identity)={
        let mut s=state();
        if s.requests.busy || s.checked.is_some_and(|t|t.elapsed()<Duration::from_millis(500)) {return;}
        let identity=match s.identity.clone() {Some(Ok(i))=>i,Some(Err(e))=>{s.note=e;s.revision+=1;return;},None=>return};
        let Some(ticket)=s.requests.begin() else{return};
        (ticket,identity)
    };
    std::thread::spawn(move|| {
        let result=query_worker(&identity);
        complete(ticket,result,None,false);
    });
}

/// Called only from native controls. No URL command or JS binding reaches it.
pub fn request() {
    if crate::private::enabled() || std::env::var_os("NUS_SHOT").is_some() {
        let mut s=state();s.note="Choose the default browser from a regular, non-test window.".into();s.revision+=1;return;
    }
    let needs_init={state().identity.is_none()};
    if needs_init {init();}
    let (ticket,identity)={
        let mut s=state();
        if s.requests.busy {s.pending_click=s.active.is_none();return;}
        let identity=match s.identity.clone(){Some(Ok(i))=>i,Some(Err(e))=>{s.note=e;s.revision+=1;return;},None=>return};
        let Some(ticket)=s.requests.begin() else{return};
        s.active=Some((ticket,identity.clone()));s.pending_click=false;
        s.note="Waiting for the system’s default-browser choice…".into();s.revision+=1;
        (ticket,identity)
    };
    #[cfg(target_os="macos")]
    {
        let scheme=if state().observation.http==Some(true){1}else{0};
        if let Err(e)=macos::request_scheme(&identity,ticket,scheme) {
            complete(ticket,Err(e),None,false);
        }
    }
    #[cfg(not(target_os="macos"))]
    std::thread::spawn(move|| {
        let result=platform_request(&identity);
        let waiting=cfg!(windows) && result.is_ok();
        let note=result.err();
        let query=query_worker(&identity);
        complete(ticket,query,note,waiting);
    });
}

fn complete(ticket:u64,result:Result<Observation,String>,note:Option<String>,waiting:bool) {
    let mut s=state();
    if !s.requests.complete(ticket) {return;}
    s.active=None;s.checked=Some(Instant::now());s.awaiting_choice=waiting;
    match result {
        Ok(o)=>{s.observation=o;s.note=note.unwrap_or_else(|| {
            if waiting && s.observation.state()!=DefaultState::Default {
                "Choose nus in Default Apps, then return here or use Check Again.".into()
            }else{words(&s.observation).into()}
        });}
        Err(e)=>{s.observation=Observation::default();s.note=note.unwrap_or(e);}
    }
    s.revision+=1;drop(s);crate::browser_runtime::wake();
}

/// Native callback results are drained on the main thread, outside locks.
/// A successful first-scheme request is verified before requesting the second.
pub fn poll() {
    #[cfg(target_os="macos")]
    loop {
        let result=MAC_RESULTS.lock().unwrap_or_else(|e|e.into_inner()).pop_front();
        let Some((ticket,scheme,error))=result else{break};
        let identity={let s=state();s.active.as_ref().filter(|(t,_)|*t==ticket).map(|(_,i)|i.clone())};
        let Some(identity)=identity else{continue};
        std::thread::spawn(move|| {
            let query=query_worker(&identity);
            MAC_QUERIES.lock().unwrap_or_else(|e|e.into_inner()).push_back((ticket,scheme,error,query));
            crate::browser_runtime::wake();
        });
    }
    #[cfg(target_os="macos")]
    loop {
        let result=MAC_QUERIES.lock().unwrap_or_else(|e|e.into_inner()).pop_front();
        let Some((ticket,scheme,error,query))=result else{break};
        let identity={let s=state();s.active.as_ref().filter(|(t,_)|*t==ticket).map(|(_,i)|i.clone())};
        let Some(identity)=identity else{continue};
        if error.is_none() && scheme==0 && query.as_ref().is_ok_and(|o|o.http==Some(true)&&o.https==Some(false)) {
            if let Err(e)=macos::request_scheme(&identity,ticket,1) {complete(ticket,query,Some(e),false);}
        } else {
            let note=error.map(|code|format!("The system did not complete the default-browser change (code {code}). Check the current associations."));
            complete(ticket,query,note,false);
        }
    }
    let pending={let mut s=state();if !s.requests.busy {std::mem::take(&mut s.pending_click)}else{false}};
    if pending {request();}
}

#[cfg(target_os="macos")]
pub(super) fn mac_result(ticket:u64,scheme:u8,error:Option<isize>) {
    MAC_RESULTS.lock().unwrap_or_else(|e|e.into_inner()).push_back((ticket,scheme,error));
    crate::browser_runtime::wake();
}

fn query_worker(identity:&Identity)->Result<Observation,String> {
    #[cfg(target_os="linux")] {return linux::query(identity);}
    #[cfg(any(windows,target_os="macos"))] {
        // Bounds a stuck native association query without blocking the host.
        let bytes=process::run(&identity.gui,&["--nus-browser-query"],Duration::from_secs(5))?;
        serde_json::from_slice(&bytes).map_err(|_|"The default-browser query returned an invalid result.".into())
    }
    #[cfg(not(any(windows,target_os="macos",target_os="linux")))]
    {let _=identity;Err("Default-browser integration is unavailable on this platform.".into())}
}
fn platform_query(identity:&Identity)->Result<Observation,String> {
    #[cfg(target_os="macos")] {return macos::query(identity);}
    #[cfg(windows)] {return windows::query(identity);}
    #[cfg(target_os="linux")] {return linux::query(identity);}
    #[cfg(not(any(windows,target_os="macos",target_os="linux")))]
    {let _=identity;Err("Unsupported platform.".into())}
}
#[cfg(not(target_os="macos"))]
fn platform_request(identity:&Identity)->Result<(),String> {
    #[cfg(windows)] {return windows::request(identity);}
    #[cfg(target_os="linux")] {return linux::request(identity);}
    #[cfg(not(any(windows,target_os="linux")))]
    {let _=identity;Err("Unsupported platform.".into())}
}

#[cfg(windows)]
fn register_browser(identity:&Identity)->Result<(),String> {windows::register(identity)}
#[cfg(target_os="linux")]
fn linux_exec(path:&std::path::Path)->Result<String,String> {linux::executable(path)}
#[cfg(target_os="linux")]
fn linux_applications()->Result<std::path::PathBuf,String> {linux::applications()}

/// Exact maintenance invocations, before profiles, URL intake and CEF startup.
/// Only the entry installer writes registration; it never changes defaults.
pub fn maintenance(args:&[String])->Option<i32> {
    if args.len()!=1 {return None;}
    match args[0].as_str() {
        "--nus-browser-query"=>{
            let result=Identity::discover().and_then(|i|platform_query(&i));
            match result {Ok(o)=>{println!("{}",serde_json::to_string(&o).unwrap());Some(0)},Err(e)=>{eprintln!("{e}");Some(1)}}
        }
        "--install-browser-entry"=>{
            #[cfg(target_os="linux")]
            let result=Identity::discover().and_then(|i|linux::register(&i));
            #[cfg(not(target_os="linux"))]
            let result:Result<(),String>=Err("Use the application installer on this platform.".into());
            match result {Ok(())=>{println!("Browser entry installed. Your defaults were not changed.");Some(0)},Err(e)=>{eprintln!("{e}");Some(1)}}
        }
        _=>None,
    }
}

//! CEF starts on first use. Its requested deadlines wake the native loop.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};
use winit::event_loop::EventLoopProxy;

static READY: AtomicBool = AtomicBool::new(false);
static ATTEMPTED: AtomicBool = AtomicBool::new(false);
static PROXY: OnceLock<EventLoopProxy<crate::UserEvent>> = OnceLock::new();
static DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);
static FAILURE: Mutex<Option<String>> = Mutex::new(None);
#[cfg(target_os = "windows")]
static SANDBOX: std::sync::atomic::AtomicPtr<u8> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[cfg(target_os = "windows")]
pub fn set_sandbox_info(info: *mut u8) { SANDBOX.store(info, Ordering::Release); }
pub fn sandbox_info() -> *mut u8 {
    #[cfg(target_os = "windows")]
    { SANDBOX.load(Ordering::Acquire) }
    #[cfg(not(target_os = "windows"))]
    { std::ptr::null_mut() }
}
pub fn failure() -> String {
    FAILURE.lock().unwrap_or_else(|e| e.into_inner()).clone()
        .unwrap_or_else(|| "The browser engine could not start.".into())
}
fn fail(message: String) -> bool {
    tracing::error!("{message}");
    *FAILURE.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
    false
}

/// Local scripted checks only: a debug build, run by the NUS_SHOT harness
/// in an isolated profile, asked explicitly. A development binary is not the
/// installed one its AppArmor policy names, so without this a fixture page
/// on 127.0.0.1 cannot render there. Release builds never have it.
/// A debug build run by the NUS_SHOT harness in an isolated profile.
pub(crate) fn test_harness() -> bool {
    cfg!(debug_assertions) && std::env::var_os("NUS_SHOT").is_some() && std::env::var_os("NUS_SHOT_DIR").is_some()
}

pub(crate) fn test_unsandboxed() -> bool {
    cfg!(debug_assertions)
        && std::env::var_os("NUS_TEST_UNSANDBOXED").is_some_and(|v| v == "1")
        && std::env::var_os("NUS_SHOT").is_some()
        && std::env::var_os("NUS_SHOT_DIR").is_some()
}

#[cfg(target_os = "linux")]
fn linux_sandbox() -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if test_unsandboxed() {
        tracing::warn!("browser sandbox off: debug build under the scripted harness");
        return Ok(());
    }
    static CHECK: OnceLock<Result<(), String>> = OnceLock::new();
    CHECK.get_or_init(|| {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let helper = std::env::var_os("CHROME_DEVEL_SANDBOX").map(std::path::PathBuf::from)
            .unwrap_or_else(|| exe.with_file_name("chrome-sandbox"));
        if std::fs::metadata(helper).is_ok_and(|m| m.is_file() && m.uid() == 0 && m.mode() & 0o4777 == 0o4755) {
            return Ok(());
        }
        // Chromium needs user, PID and network namespaces. Test in a short-lived
        // child, with the installed executable's own AppArmor policy.
        let mut child = nus_compat::command(exe).arg("--nus-check-userns")
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()).spawn().map_err(|e| e.to_string())?;
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(10)),
                _ => { let _ = child.kill(); let _ = child.wait(); break; }
            }
        }
        Err("Linux blocked the browser sandbox. This installation needs permission to create user namespaces (including its AppArmor policy), or a correctly installed chrome-sandbox helper. Ask your system administrator to configure the sandbox, then restart nus.".into())
    }).clone()
}
pub fn ready() -> bool {
    READY.load(Ordering::Acquire)
}
pub fn set_proxy(proxy: EventLoopProxy<crate::UserEvent>) {
    let _ = PROXY.set(proxy);
}
pub fn redirect(browser:i32,from:String,to:String) {
    if let Some(proxy)=PROXY.get(){let _=proxy.send_event(crate::UserEvent::BrowserRedirect(browser,from,to));}
}
pub fn wake() {
    if let Some(proxy) = PROXY.get() {
        let _ = proxy.send_event(crate::UserEvent::BrowserWork);
    }
}
pub fn schedule(delay_ms: i64) {
    let at = Instant::now() + Duration::from_millis(delay_ms.clamp(0, 86_400_000) as u64);
    let earlier = {
        let mut next = DEADLINE.lock().unwrap_or_else(|e| e.into_inner());
        if next.is_none_or(|old| at < old) {
            *next = Some(at);
            true
        } else {
            false
        }
    };
    if earlier {
        wake();
    }
}
pub fn wait(maximum: Duration) -> Duration {
    let maximum = if ready() { maximum.min(MAX_IDLE) } else { maximum };
    DEADLINE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .map_or(maximum, |at| {
            at.saturating_duration_since(Instant::now()).min(maximum)
        })
}
/// The longest Chromium goes without a turn, asked for or not. It does not
/// ask for every task it queues on this thread, so waiting only on its
/// deadlines can leave a navigation queued behind nothing, for good. CEF's
/// own external pump (cefclient) keeps the same 30 Hz floor.
const MAX_IDLE: Duration = Duration::from_millis(33);
static LAST_WORK: Mutex<Option<Instant>> = Mutex::new(None);
pub fn pump() {
    if !ready() {
        return;
    }
    let now = Instant::now();
    let due = {
        let mut next = DEADLINE.lock().unwrap_or_else(|e| e.into_inner());
        if next.is_some_and(|at| at <= now) {
            *next = None;
            true
        } else {
            false
        }
    };
    let idle = LAST_WORK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_none_or(|at| now.duration_since(at) >= MAX_IDLE);
    if due || idle {
        *LAST_WORK.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
        cef::do_message_loop_work();
    }
}
/// Keep the framework mapped through CEF shutdown and any remaining wrappers.
/// Native-only windows never load it. Helpers still load it before dispatch.
pub fn load_library() {
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            static LIBRARY: OnceLock<cef::library_loader::LibraryLoader> = OnceLock::new();
            LIBRARY.get_or_init(|| {
                let loader = cef::library_loader::LibraryLoader::new(
                    &std::env::current_exe().unwrap(),
                    false,
                );
                assert!(
                    loader.load(),
                    "could not load the CEF framework from the bundle"
                );
                loader
            });
        }
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    });
}

/// UI-thread only, before any browser/request-context/cookie-manager operation.
pub fn ensure() -> bool {
    if ready() {
        return true;
    }
    #[cfg(target_os = "windows")]
    if sandbox_info().is_null() {
        return fail("The Windows browser sandbox is missing. Open nus.exe from the complete installed package, with nus.dll beside it. Reinstall nus if either file is missing.".into());
    }
    #[cfg(target_os = "linux")]
    if let Err(message) = linux_sandbox() { return fail(message); }
    // CEF initialization is process-wide and must not be retried after a
    // partial failure. Keep the native explanation available until restart.
    if ATTEMPTED.swap(true, Ordering::AcqRel) { return false; }
    let started = Instant::now();
    self::load_library();
    use cef::*;
    let args = cef::args::Args::new();
    let mut app = crate::browser::AppBuilder::new(crate::browser::AppHandler);
    let profile = std::env::current_dir().unwrap_or_default().join("profile");
    let settings = Settings {
        command_line_args_disabled: 1,
        no_sandbox: test_unsandboxed() as i32,
        windowless_rendering_enabled: 1,
        external_message_pump: 1,
        // Where a page paints no background, white, as in Chrome. CEF's
        // default (alpha 0) makes an off-screen browser paint nothing there,
        // so pages that assume the white canvas (Google's sign-in) showed
        // the pane, or the desktop, through. Every browser inherits it.
        background_color: 0xFFFF_FFFF,
        user_agent_product: format!("Chrome/{}", crate::chromium_version())
            .as_str()
            .into(),
        // Chromium's log is off; NUS_CEF_LOG=<file> turns it on (verbose),
        // for diagnosing what the browser will not say on screen, such as
        // why a DRM module did not load.
        log_severity: if cef_log().is_some() { cef::LogSeverity::VERBOSE } else { cef::LogSeverity::DISABLE },
        log_file: cef_log().unwrap_or_default().as_str().into(),
        root_cache_path: profile.to_string_lossy().as_ref().into(),
        cache_path: if crate::private::enabled() {
            "".into()
        } else {
            profile.to_string_lossy().as_ref().into()
        },
        ..Default::default()
    };
    let ok = initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        sandbox_info(),
    ) == 1;
    if ok {
        READY.store(true, Ordering::Release);
        // Streaming sites need the Widevine CDM, which Chromium installs on
        // its own schedule minutes after launch; ask for it now.
        crate::widevine::fetch();
        crate::perf::startup(crate::perf::StartupMark::CefReady);
        crate::perf::record(
            "browser_initialization",
            started.elapsed().as_secs_f64() * 1000.0,
        );
        schedule(0);
    } else {
        return fail(format!("The browser engine could not start (CEF exit code {}). Restart nus; if this continues, reinstall the complete package.", cef::get_exit_code()));
    }
    ok
}

/// Where Chromium should write its log, when asked to (NUS_CEF_LOG).
fn cef_log() -> Option<String> {
    std::env::var("NUS_CEF_LOG").ok().filter(|p| !p.trim().is_empty())
}

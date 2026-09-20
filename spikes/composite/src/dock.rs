//! One Dock tile per process, following the most recently focused window.
//! AppKit owns the physical bounce; we own the short promo font sequence.
#[cfg(target_os = "macos")]
use {
    nus_render::{
        dock_icon::{self, Face},
        Color,
    },
    std::time::Instant,
};

#[derive(Default)]
pub struct Dock {
    quitting: bool,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    signal: Option<nus_render::Color>,
    #[cfg(target_os = "macos")]
    images: Vec<objc2::rc::Retained<objc2_app_kit::NSImage>>,
    #[cfg(target_os = "macos")]
    shown: Option<Face>,
    #[cfg(target_os = "macos")]
    sequence: Option<(Instant, bool)>, // true = attention; focus cancels it
    #[cfg(target_os = "macos")]
    request: Option<isize>,
    #[cfg(target_os = "macos")]
    last_attention: Option<Instant>,
    #[cfg(target_os = "macos")]
    renderer: Option<Renderer>,
    #[cfg(target_os = "macos")]
    launch: Option<launch::Launch>,
    #[cfg(target_os = "linux")]
    publisher: Option<linux::Publisher>,
}

impl Dock {
    /// Install the correct, pre-rendered icon before PATH, CEF and GPU setup.
    /// Launch Services uses the same artwork from the bundle before main runs.
    pub fn bootstrap() -> Self {
        let mut dock = Self::default();
        #[cfg(target_os = "macos")]
        if objc2::MainThreadMarker::new().is_some() {
            let launch = launch::Launch::new();
            dock.images = launch.images();
            dock.shown = Some(Face::Newsreader);
            dock.launch = Some(launch);
        }
        dock
    }

    pub fn begin_launch(&mut self, reduced: bool) {
        #[cfg(target_os = "macos")]
        if let Some(launch) = &mut self.launch {
            launch.begin(reduced);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = reduced;
    }

    /// The first window has actually been drawn. Do not wait for the worker or
    /// request another native bounce: settle while AppKit completes its launch.
    pub fn finish_launch(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(mut launch) = self.launch.take() {
            launch.finish();
            self.shown = Some(Face::Newsreader);
            register_bundle();
        }
    }

    /// Release the temporary running icon while AppKit is still alive. Leaving
    /// it installed until process teardown can expose the Dock's cached fallback.
    /// `None` restores the signed bundle's canonical, multi-resolution artwork.
    pub fn prepare_quit(&mut self) {
        if std::mem::replace(&mut self.quitting, true) {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            self.launch = None;
            self.stop();
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                unsafe {
                    objc2_app_kit::NSApplication::sharedApplication(mtm)
                        .setApplicationIconImage(None);
                }
                trace(
                    "quit-default",
                    Some(Face::Newsreader),
                    nus_render::theme::signal::RED,
                );
            }
        }
    }

    /// Called on the event-loop thread. No new icon work while idle.
    pub fn tick(&mut self, signal: nus_render::Color, reduced: bool) {
        if self.quitting {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            let Some(mtm) = objc2::MainThreadMarker::new() else {
                return;
            };
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
            if self.signal != Some(signal) {
                let worker = self.renderer.get_or_insert_with(Renderer::new);
                let _ = worker.send.send(signal);
                self.signal = Some(signal);
            }
            if let Some(worker) = &self.renderer {
                for (palette, frames) in worker.receive.try_iter() {
                    if palette != signal {
                        continue;
                    }
                    let first = self.images.is_empty();
                    self.images = frames.iter().map(|png| image(png)).collect();
                    self.shown = None;
                    if first {
                        register_bundle();
                    }
                    // Theme rasterization must never restart the launch sequence.
                }
            }
            if self.images.is_empty() {
                return;
            }
            if reduced || (app.isActive() && self.sequence.is_some_and(|(_, attention)| attention))
            {
                self.stop();
            }
            let face = self
                .sequence
                .map(|(at, _)| dock_icon::face_at(at.elapsed().as_secs_f32()))
                .unwrap_or(Face::Newsreader);
            if self.shown != Some(face) {
                unsafe {
                    app.setApplicationIconImage(Some(&self.images[face as usize]));
                }
                self.shown = Some(face);
                tracing::debug!("dock face: {}", face.name());
                trace("frame", Some(face), signal);
            }
            if face == Face::Newsreader {
                self.sequence = None;
            }
            // Informational bounces are bounded even if the application stays behind.
            if self.request.is_some()
                && (app.isActive()
                    || self
                        .last_attention
                        .is_some_and(|t| t.elapsed().as_secs_f32() > 1.5))
            {
                self.stop();
            }
        }
        #[cfg(target_os = "linux")]
        if self.signal != Some(signal) {
            self.signal = Some(signal);
            self.publisher
                .get_or_insert_with(linux::Publisher::new)
                .update(signal);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (signal, reduced);
    }

    /// Existing completion notifications may request one informational bounce.
    /// Never steal focus or repeatedly restart for a burst of completions.
    pub fn attention(&mut self, reduced: bool) {
        if self.quitting {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            let Some(mtm) = objc2::MainThreadMarker::new() else {
                return;
            };
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
            if reduced
                || app.isActive()
                || self.images.is_empty()
                || self
                    .last_attention
                    .is_some_and(|at| at.elapsed().as_secs_f32() < 2.0)
            {
                return;
            }
            self.stop();
            self.request = Some(app.requestUserAttention(
                objc2_app_kit::NSRequestUserAttentionType::InformationalRequest,
            ));
            let now = Instant::now();
            self.sequence = Some((now, true));
            self.last_attention = Some(now);
            trace("attention", None, self.signal.unwrap_or([0.0; 4]));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = reduced;
    }

    #[cfg(target_os = "macos")]
    fn stop(&mut self) {
        self.sequence = None;
        if let Some(request) = self.request.take() {
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                objc2_app_kit::NSApplication::sharedApplication(mtm)
                    .cancelUserAttentionRequest(request);
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[path = "dock_launch.rs"]
mod launch;

/// Service only the native run loop while awaiting the login shell, before
/// winit's event loop exists. No UI or AppKit work is moved to a worker thread.
#[cfg(target_os = "macos")]
pub fn pump_launch() {
    launch::pump();
}

/// Give directly launched development builds a valid Launch Services identity.
/// Stage Manager and system badges use the signed, multi-resolution bundle icon.
/// Finder custom-icon metadata fails strict signing: never mutate it at runtime.
#[cfg(target_os = "macos")]
fn register_bundle() {
    use objc2_foundation::{NSString, NSURL};
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(bundle) = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
    else {
        return;
    };
    let path = NSString::from_str(&bundle.to_string_lossy());
    // A directly launched development bundle still needs a Launch Services identity.
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        #[link(name = "CoreServices", kind = "framework")]
        extern "C" {
            fn LSRegisterURL(url: *const std::ffi::c_void, update: bool) -> i32;
        }
        let url = NSURL::fileURLWithPath_isDirectory(&path, true);
        unsafe {
            let status = LSRegisterURL((&*url as *const NSURL).cast(), true);
            if status != 0 {
                tracing::warn!("register app icon: Launch Services {status}");
            }
        }
    });
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    trace("bundle", None, nus_render::theme::signal::RED);
    if std::env::var_os("NUS_SHOT").is_some() {
        if let Some(dir) = std::env::var_os("NUS_DOCK_TRACE").map(std::path::PathBuf::from) {
            if let Some(data) = workspace.iconForFile(&path).TIFFRepresentation() {
                let _ = std::fs::write(dir.join("workspace.tiff"), data.to_vec());
            }
        }
    }
}

/// Isolated native checks read back AppKit's actual image, not just our raster.
#[cfg(target_os = "macos")]
fn trace(event: &str, face: Option<Face>, signal: Color) {
    if std::env::var_os("NUS_SHOT").is_none() {
        return;
    }
    let Some(dir) = std::env::var_os("NUS_DOCK_TRACE").map(std::path::PathBuf::from) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let launched =
        unsafe { objc2_app_kit::NSRunningApplication::currentApplication().isFinishedLaunching() };
    let row = serde_json::json!({"event":event,"face":face.map(Face::name),"signal":signal,"ms":ms,"native_launch_finished":launched});
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("events.jsonl"))
    {
        let _ = writeln!(file, "{row}");
    }
    if let (Some(face), Some(mtm)) = (face, objc2::MainThreadMarker::new()) {
        if let Some(data) = objc2_app_kit::NSApplication::sharedApplication(mtm)
            .applicationIconImage()
            .and_then(|img| img.TIFFRepresentation())
        {
            let _ = std::fs::write(
                dir.join(format!("{ms}-{}.tiff", face as usize)),
                data.to_vec(),
            );
        }
    }
}

#[cfg(any(target_os = "linux", test))]
#[path = "dock_linux.rs"]
mod linux;

#[cfg(target_os = "macos")]
fn image(png: &[u8]) -> objc2::rc::Retained<objc2_app_kit::NSImage> {
    use objc2::AnyThread;
    let data = objc2_foundation::NSData::with_bytes(png);
    let image = objc2_app_kit::NSImage::initWithData(objc2_app_kit::NSImage::alloc(), &data)
        .expect("generated Dock PNG");
    image.setSize(objc2_foundation::NSSize::new(128.0, 128.0));
    image
}

/// Rasterization stays off the UI thread; rapid theme previews coalesce.
#[cfg(target_os = "macos")]
struct Renderer {
    send: std::sync::mpsc::Sender<Color>,
    receive: std::sync::mpsc::Receiver<(Color, Vec<Vec<u8>>)>,
}
#[cfg(target_os = "macos")]
impl Renderer {
    fn new() -> Self {
        let (send, jobs) = std::sync::mpsc::channel();
        let (results, receive) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("dock artwork".into())
            .spawn(move || {
                let fields: Vec<_> = Face::ALL
                    .into_iter()
                    .map(|f| dock_icon::Field::new(256, f))
                    .collect();
                while let Ok(mut signal) = jobs.recv() {
                    for newer in jobs.try_iter() {
                        signal = newer;
                    }
                    let frames = fields
                        .iter()
                        .map(|f| nus_render::icon::png(&f.frame(signal), 256, 256))
                        .collect();
                    if results.send((signal, frames)).is_err() {
                        break;
                    }
                }
            })
            .expect("dock renderer thread");
        Self { send, receive }
    }
}

impl Drop for Dock {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        self.stop();
    }
}

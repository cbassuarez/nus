//! A main-run-loop launch loop, available before CEF, fonts and the first window.
//! Pre-rendered frames share the desktop icon's clipping and contrast treatment.
use super::{image, trace};
use nus_render::{
    dock_icon::{self, Face},
    theme::signal::RED,
};
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSImage};
use std::{cell::RefCell, ffi::c_void, ptr, time::Instant};

const FRAMES: [&[u8]; 6] = [
    include_bytes!("../../../assets/icon/dock/face-0.png"),
    include_bytes!("../../../assets/icon/dock/face-1.png"),
    include_bytes!("../../../assets/icon/dock/face-2.png"),
    include_bytes!("../../../assets/icon/dock/face-3.png"),
    include_bytes!("../../../assets/icon/dock/face-4.png"),
    include_bytes!("../../../assets/icon/dock/face-5.png"),
];

struct State {
    images: Vec<Retained<NSImage>>,
    started: Option<Instant>,
    shown: Option<Face>,
}
impl State {
    fn show(&mut self, face: Face, event: &str) {
        if self.shown == Some(face) {
            return;
        }
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        unsafe {
            NSApplication::sharedApplication(mtm)
                .setApplicationIconImage(Some(&self.images[face as usize]));
        }
        self.shown = Some(face);
        trace(event, Some(face), RED);
    }
    fn tick(&mut self) {
        if let Some(at) = self.started {
            // AppKit ends the native launch bounce at launch completion. Settle
            // on the first main-loop opportunity, even if window setup follows.
            if unsafe {
                objc2_app_kit::NSRunningApplication::currentApplication().isFinishedLaunching()
            } {
                self.started = None;
                self.show(Face::Newsreader, "launch-settled");
                return;
            }
            self.show(
                dock_icon::launch_face_at(crate::clock::since(at).as_secs_f32()),
                "launch-frame",
            );
        }
    }
}

pub(super) struct Launch {
    // Box keeps the callback address stable when Dock moves into Host.
    state: Box<RefCell<State>>,
    timer: *mut c_void,
}
impl Launch {
    pub(super) fn new() -> Self {
        let mut state = State {
            images: FRAMES.iter().map(|png| image(png)).collect(),
            started: None,
            shown: None,
        };
        state.show(Face::Newsreader, "bootstrap");
        Self {
            state: Box::new(RefCell::new(state)),
            timer: ptr::null_mut(),
        }
    }
    pub(super) fn images(&self) -> Vec<Retained<NSImage>> {
        self.state.borrow().images.clone()
    }
    pub(super) fn begin(&mut self, reduced: bool) {
        self.invalidate();
        if reduced {
            return;
        }
        {
            let mut state = self.state.borrow_mut();
            state.started = Some(crate::clock::now());
            state.tick();
        }
        let mut context = TimerContext {
            version: 0,
            info: (&*self.state as *const RefCell<State>).cast_mut().cast(),
            retain: None,
            release: None,
            description: None,
        };
        // All registration, callbacks and invalidation stay on the main thread.
        // No callback may retain the context; invalidate before the Box is freed.
        unsafe {
            self.timer = CFRunLoopTimerCreate(
                ptr::null(),
                CFAbsoluteTimeGetCurrent() + 0.016,
                0.016,
                0,
                0,
                timer_fired,
                &mut context,
            );
            if !self.timer.is_null() {
                CFRunLoopAddTimer(CFRunLoopGetMain(), self.timer, kCFRunLoopCommonModes);
            }
        }
    }
    pub(super) fn finish(&mut self) {
        self.invalidate();
        let mut state = self.state.borrow_mut();
        state.started = None;
        state.show(Face::Newsreader, "launch-settled");
        trace("ready", Some(Face::Newsreader), RED);
    }
    fn invalidate(&mut self) {
        if !self.timer.is_null() {
            unsafe {
                CFRunLoopTimerInvalidate(self.timer);
                CFRelease(self.timer);
            }
            self.timer = ptr::null_mut();
        }
    }
}
impl Drop for Launch {
    fn drop(&mut self) {
        self.invalidate();
    }
}

unsafe extern "C" fn timer_fired(_: *mut c_void, info: *mut c_void) {
    if objc2::MainThreadMarker::new().is_none() {
        return;
    }
    let cell = unsafe { &*info.cast::<RefCell<State>>() };
    if let Ok(mut state) = cell.try_borrow_mut() {
        state.tick();
    }
}

pub(super) fn pump() {
    unsafe {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, true);
    }
}

#[repr(C)]
struct TimerContext {
    version: isize,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<unsafe extern "C" fn(*const c_void)>,
    description: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopCommonModes: *const c_void;
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFAbsoluteTimeGetCurrent() -> f64;
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopTimerCreate(
        allocator: *const c_void,
        date: f64,
        interval: f64,
        flags: usize,
        order: isize,
        callback: unsafe extern "C" fn(*mut c_void, *mut c_void),
        context: *mut TimerContext,
    ) -> *mut c_void;
    fn CFRunLoopAddTimer(run_loop: *mut c_void, timer: *mut c_void, mode: *const c_void);
    fn CFRunLoopTimerInvalidate(timer: *mut c_void);
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source: bool) -> i32;
    fn CFRelease(value: *const c_void);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_frames_are_the_current_clipped_desktop_artwork() {
        for (png, face) in FRAMES.into_iter().zip(Face::ALL) {
            let mut reader = png::Decoder::new(png).read_info().unwrap();
            let mut bytes = vec![0; reader.output_buffer_size()];
            let info = reader.next_frame(&mut bytes).unwrap();
            assert_eq!((info.width, info.height), (256, 256));
            assert_eq!(info.color_type, png::ColorType::Rgba);
            assert_eq!(
                &bytes[..info.buffer_size()],
                dock_icon::render(256, RED, face),
                "Regenerate assets/icon/dock for {}",
                face.name()
            );
        }
    }
}

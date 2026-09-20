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
    sequence: Option<Sequence>,
    signal: nus_render::Color,
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
        trace(event, Some(face), self.signal);
    }
    fn tick(&mut self) {
        if let Some(sequence) = &mut self.sequence {
            let (face, done) = sequence.tick(crate::clock::now());
            if done {
                trace("launch-settled", Some(face), self.signal);
                self.sequence = None;
            } else {
                self.show(face, "launch-frame");
            }
        }
    }
}

// Advance by visible frames, not elapsed phase: CEF/GPU initialization can
// block the main loop. A delayed callback must not skip the entire sequence.
struct Sequence {
    next: Instant,
    index: usize,
    ready: bool,
}
impl Sequence {
    fn new(now: Instant) -> Self {
        Self {
            next: now + std::time::Duration::from_secs_f32(dock_icon::STEP_SECONDS),
            index: 0,
            ready: false,
        }
    }
    fn tick(&mut self, now: Instant) -> (Face, bool) {
        if now >= self.next {
            if self.ready && self.index == Face::ALL.len() - 1 {
                return (Face::Newsreader, true);
            }
            self.index = (self.index + 1) % Face::ALL.len();
            self.next = now + std::time::Duration::from_secs_f32(dock_icon::STEP_SECONDS);
        }
        (Face::ALL[self.index], false)
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
            sequence: None,
            signal: RED,
            shown: None,
        };
        state.show(Face::Newsreader, "bootstrap");
        Self {
            state: Box::new(RefCell::new(state)),
            timer: ptr::null_mut(),
        }
    }
    pub(super) fn signal(&self) -> nus_render::Color {
        self.state.borrow().signal
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
            state.sequence = Some(Sequence::new(crate::clock::now()));
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
    pub(super) fn ready(&mut self) {
        let mut state = self.state.borrow_mut();
        if let Some(sequence) = &mut state.sequence {
            sequence.ready = true;
        }
        trace("ready", state.shown, state.signal);
    }
    pub(super) fn update(
        &mut self,
        images: &[Retained<NSImage>],
        signal: nus_render::Color,
        reduced: bool,
    ) -> bool {
        let mut state = self.state.borrow_mut();
        if state.signal != signal {
            state.images = images.to_vec();
            state.signal = signal;
            state.shown = None;
        }
        if reduced {
            state.sequence = None;
        }
        state.tick();
        state.sequence.is_some()
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
    fn fast_launch_and_stalled_callbacks_show_every_face_before_settling() {
        let now = Instant::now();
        let mut seq = Sequence::new(now);
        seq.ready = true;
        assert_eq!(seq.tick(now), (Face::Plex, false));
        // A long synchronous launch block still advances just one frame.
        let mut at = now + std::time::Duration::from_secs(4);
        for face in Face::ALL.into_iter().skip(1) {
            assert_eq!(seq.tick(at), (face, false));
            at += std::time::Duration::from_secs_f32(dock_icon::STEP_SECONDS + 0.001);
        }
        assert_eq!(seq.tick(at), (Face::Newsreader, true));
    }
    #[test]
    fn unfinished_launch_keeps_looping() {
        let now = Instant::now();
        let mut seq = Sequence::new(now);
        for i in 0..18 {
            assert_eq!(
                seq.tick(
                    now + std::time::Duration::from_secs_f32(
                        i as f32 * (dock_icon::STEP_SECONDS + 0.001)
                    )
                ),
                (Face::ALL[i % 6], false)
            );
        }
    }
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

//! Process-wide Mercury animation on the main run loop. The OS owns bouncing.
use super::{image, trace};
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSCompositingOperation, NSImage};
use objc2_foundation::{NSRect, NSSize};
use std::{
    cell::RefCell,
    ffi::c_void,
    ptr,
    time::{Duration, Instant},
};

struct State {
    // PNG bytes remain in the executable; only the visible frame is decoded.
    intro: Vec<&'static [u8]>,
    base: Option<Retained<NSImage>>,
    still: Retained<NSImage>,
    reduced: bool,
    finished: bool,
    index: usize,
    next: Instant,
    started: Instant,
}
impl State {
    fn tick(&mut self) {
        if self.finished || Instant::now() < self.next {
            return;
        }
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        self.index =
            (self.started.elapsed().as_secs_f64() * nus_render::mercury::INTRO_FPS as f64) as usize;
        if self.reduced || self.index >= self.intro.len() {
            unsafe {
                app.setApplicationIconImage(Some(&self.still));
            }
            trace("mercury-still", None, [0.0; 4]);
            self.intro = Vec::new();
            self.base = None;
            self.finished = true;
            return;
        }
        let metal = image(self.intro[self.index]);
        let frame = match self.base.as_ref() {
            Some(base) => shell_frame(base.clone(), metal, self.index, self.intro.len()),
            None => metal,
        };
        unsafe {
            app.setApplicationIconImage(Some(&frame));
        }
        if self.index % 16 == 0 {
            trace(&format!("mercury-intro-{}", self.index), None, [0.0; 4]);
        }
        self.index += 1;
        self.next = self.started
            + Duration::from_secs_f64(self.index as f64 / nus_render::mercury::INTRO_FPS as f64);
    }
}
fn bank() -> Vec<&'static [u8]> {
    let bytes = include_bytes!("../../../assets/icon/mercury/dock-motion.bin");
    assert_eq!(&bytes[..8], b"NUSM\x01\0\0\0");
    let read = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let count = read(8);
    assert_eq!(
        read(12),
        0,
        "Dock bank must not retain idle animation frames"
    );
    let mut at = 16;
    let frames = (0..count)
        .map(|_| {
            let len = read(at);
            at += 4;
            let frame = &bytes[at..at + len];
            at += len;
            frame as &'static [u8]
        })
        .collect();
    assert_eq!(at, bytes.len());
    frames
}
fn shell_frame(
    base: Retained<NSImage>,
    metal: Retained<NSImage>,
    index: usize,
    count: usize,
) -> Retained<NSImage> {
    let last = count.saturating_sub(1).max(1);
    if index == last {
        return metal;
    }
    let t = ((index as f64 / last as f64 - 0.72) / 0.28).clamp(0.0, 1.0);
    let opacity = 1.0 - t * t * (3.0 - 2.0 * t);
    let drawing = block2::RcBlock::new(move |bounds: NSRect| {
        base.drawInRect_fromRect_operation_fraction(
            bounds,
            NSRect::ZERO,
            NSCompositingOperation::SourceOver,
            opacity,
        );
        metal.drawInRect_fromRect_operation_fraction(
            bounds,
            NSRect::ZERO,
            NSCompositingOperation::SourceOver,
            1.0,
        );
        objc2::runtime::Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(256.0, 256.0), false, &drawing)
}

pub(super) struct Mercury {
    state: Box<RefCell<State>>,
    timer: *mut c_void,
}
impl Mercury {
    pub(super) fn new(reduced: bool, launch: bool) -> Self {
        let still = image(include_bytes!(
            "../../../assets/icon/mercury/dock-still.png"
        ));
        let intro = if launch && !reduced {
            bank()
        } else {
            Vec::new()
        };
        let base = if !intro.is_empty() {
            objc2::MainThreadMarker::new().and_then(|mtm| {
                let base = NSApplication::sharedApplication(mtm).applicationIconImage();
                if base.is_some() {
                    trace("mercury-shell-base", None, [0.0; 4]);
                }
                base
            })
        } else {
            None
        };
        let mut this = Self {
            state: Box::new(RefCell::new(State {
                intro,
                base,
                still,
                reduced,
                finished: false,
                index: 0,
                next: Instant::now(),
                started: Instant::now(),
            })),
            timer: ptr::null_mut(),
        };
        this.state.borrow_mut().tick();
        if this.state.borrow().finished {
            return this;
        }
        let mut context = TimerContext {
            version: 0,
            info: (&*this.state as *const RefCell<State>).cast_mut().cast(),
            retain: None,
            release: None,
            description: None,
        };
        unsafe {
            this.timer = CFRunLoopTimerCreate(
                ptr::null(),
                CFAbsoluteTimeGetCurrent() + 0.01,
                1.0 / 48.0,
                0,
                0,
                tick,
                &mut context,
            );
            if !this.timer.is_null() {
                CFRunLoopAddTimer(CFRunLoopGetMain(), this.timer, kCFRunLoopCommonModes);
            }
        }
        this
    }
    pub(super) fn update(&mut self, reduced: bool) {
        let mut state = self.state.borrow_mut();
        state.reduced = reduced;
        state.tick();
        if state.finished && !self.timer.is_null() {
            unsafe {
                CFRunLoopTimerInvalidate(self.timer);
            }
        }
    }
}
impl Drop for Mercury {
    fn drop(&mut self) {
        if !self.timer.is_null() {
            unsafe {
                CFRunLoopTimerInvalidate(self.timer);
                CFRelease(self.timer);
            }
        }
    }
}
unsafe extern "C" fn tick(timer: *mut c_void, info: *mut c_void) {
    if objc2::MainThreadMarker::new().is_none() {
        return;
    }
    let state = unsafe { &*info.cast::<RefCell<State>>() };
    if let Ok(mut state) = state.try_borrow_mut() {
        state.tick();
        if state.finished {
            unsafe {
                CFRunLoopTimerInvalidate(timer);
            }
        }
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
    fn CFAbsoluteTimeGetCurrent() -> f64;
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopTimerCreate(
        a: *const c_void,
        date: f64,
        interval: f64,
        flags: usize,
        order: isize,
        callback: unsafe extern "C" fn(*mut c_void, *mut c_void),
        context: *mut TimerContext,
    ) -> *mut c_void;
    fn CFRunLoopAddTimer(loop_: *mut c_void, timer: *mut c_void, mode: *const c_void);
    fn CFRunLoopTimerInvalidate(timer: *mut c_void);
    fn CFRelease(value: *const c_void);
}

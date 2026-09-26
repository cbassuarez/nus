//! CEF's contract with the application object on macOS.
//!
//! Chromium asks `NSApp` whether it is inside `-sendEvent:`
//! (`-isHandlingSendEvent`, CrAppProtocol) and tells it so
//! (`-setHandlingSendEvent:`, CrAppControlProtocol). CEF documents this as
//! `CefAppProtocol`: the host's NSApplication must implement it. winit's
//! NSApplication subclass doesn't, which went unnoticed while every browser
//! was windowless. The native DevTools window is a real CEF window: using
//! it (clicking about the console, copying, closing it) reaches
//! `[NSApp isHandlingSendEvent]`, AppKit raises "unrecognized selector",
//! and nus aborts.
//!
//! So at launch, once winit has made NSApp and before CEF starts, the two
//! methods are added to NSApp's class, and its `-sendEvent:` is wrapped to
//! keep the flag true while an event is being dispatched (CEF's own
//! `CefScopedSendingEvent` does exactly this).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{class, ffi, msg_send, sel};

static HANDLING: AtomicBool = AtomicBool::new(false);
/// winit's own `-sendEvent:`, which the wrapper calls.
static SEND_EVENT: OnceLock<Imp> = OnceLock::new();

unsafe extern "C-unwind" fn is_handling_send_event(_this: *mut AnyObject, _sel: Sel) -> Bool {
    Bool::new(HANDLING.load(Ordering::Relaxed))
}

unsafe extern "C-unwind" fn set_handling_send_event(_this: *mut AnyObject, _sel: Sel, on: Bool) {
    HANDLING.store(on.as_bool(), Ordering::Relaxed);
}

unsafe extern "C-unwind" fn send_event(this: *mut AnyObject, sel: Sel, event: *mut AnyObject) {
    let was = HANDLING.swap(true, Ordering::Relaxed);
    if let Some(original) = SEND_EVENT.get() {
        // SAFETY: the implementation `-sendEvent:` had before we wrapped it.
        let original: unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) = unsafe { std::mem::transmute(*original) };
        unsafe { original(this, sel, event) };
    }
    HANDLING.store(was, Ordering::Relaxed);
}

/// BOOL is `bool` on arm64 and `signed char` on x86_64.
const BOOL_TYPE: &str = if cfg!(target_arch = "aarch64") { "B" } else { "c" };

/// Give NSApp's class CefAppProtocol. Call once, after the event loop exists.
pub fn install() {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let cls = ffi::object_getClass(app) as *mut AnyClass;
        let getter = format!("{BOOL_TYPE}@:\0");
        let setter = format!("v@:{BOOL_TYPE}\0");
        let is_imp: Imp = std::mem::transmute(is_handling_send_event as unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool);
        let set_imp: Imp = std::mem::transmute(set_handling_send_event as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, Bool));
        let send_imp: Imp = std::mem::transmute(send_event as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject));
        // Adding fails harmlessly if the class already has them.
        let _ = ffi::class_addMethod(cls, sel!(isHandlingSendEvent), is_imp, getter.as_ptr().cast());
        let _ = ffi::class_addMethod(cls, sel!(setHandlingSendEvent:), set_imp, setter.as_ptr().cast());
        let method = ffi::class_getInstanceMethod(cls, sel!(sendEvent:));
        if let Some(original) = (!method.is_null()).then(|| ffi::method_getImplementation(method)).flatten() {
            let _ = SEND_EVENT.set(original);
            // An inherited -sendEvent: gets an override on this class; winit's
            // own is swapped in place.
            if !ffi::class_addMethod(cls, sel!(sendEvent:), send_imp, c"v@:@".as_ptr()).as_bool() {
                ffi::method_setImplementation(method, send_imp);
            }
        }
        // Declared conformance, for Chromium's debug checks, when the
        // protocols are registered by now.
        for name in [c"CrAppProtocol", c"CrAppControlProtocol", c"CefAppProtocol"] {
            let p = ffi::objc_getProtocol(name.as_ptr());
            if !p.is_null() {
                let _ = ffi::class_addProtocol(cls, p);
            }
        }
    }
}

/// Whether NSApp now answers what Chromium asks it.
pub fn installed() -> bool {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let yes: Bool = msg_send![app, respondsToSelector: sel!(isHandlingSendEvent)];
        let set: Bool = msg_send![app, respondsToSelector: sel!(setHandlingSendEvent:)];
        yes.as_bool() && set.as_bool()
    }
}

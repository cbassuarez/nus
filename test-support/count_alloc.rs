//! Scoped, per-thread allocation counts for tests; never installed in production.
//! Requested bytes count alloc/realloc traffic, not retained memory or footprint.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub calls: usize,
    pub bytes: usize,
}
thread_local! {
    static ACTIVE: Cell<Option<Counts>> = const { Cell::new(None) };
    static SAMPLE: Cell<usize> = const { Cell::new(0) };
    static STACK: std::cell::RefCell<Option<std::backtrace::Backtrace>> = const { std::cell::RefCell::new(None) };
}
struct Counting;
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn record(bytes: usize) {
    let _ = ACTIVE.try_with(|slot| {
        if let Some(mut n) = slot.get() {
            n.calls += 1;
            n.bytes += bytes;
            if SAMPLE.with(|v| v.get()) == n.calls {
                slot.set(None); // Stack capture allocates; exclude its own bookkeeping.
                STACK.with(|v| *v.borrow_mut() = Some(std::backtrace::Backtrace::force_capture()));
            }
            slot.set(Some(n));
        }
    });
}
// SAFETY: every operation forwards the original pointer/layout to System;
// bookkeeping uses non-allocating thread-local Cells and never dereferences it.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, size) };
        if !ptr.is_null() {
            record(size);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }
}
pub fn count<T>(f: impl FnOnce() -> T) -> (T, Counts) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|v| v.set(None));
        }
    }
    ACTIVE.with(|v| {
        assert!(v.get().is_none(), "nested allocation measurement");
        v.set(Some(Counts::default()));
    });
    let _reset = Reset;
    let value = f();
    (value, ACTIVE.with(|v| v.get().unwrap()))
}

#[allow(dead_code)]
pub fn sample<T>(
    nth: usize,
    f: impl FnOnce() -> T,
) -> (T, Counts, Option<std::backtrace::Backtrace>) {
    SAMPLE.with(|v| v.set(nth));
    let (value, n) = count(f);
    SAMPLE.with(|v| v.set(0));
    (value, n, STACK.with(|v| v.borrow_mut().take()))
}

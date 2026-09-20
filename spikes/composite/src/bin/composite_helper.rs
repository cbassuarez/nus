//! The macOS helper process.
//!
//! On Windows and Linux a CEF subprocess re-executes the main binary with a
//! `--type` switch, which is why `main.rs` calls `execute_process` first. macOS
//! will not do that: every subprocess must be its own executable inside
//! `Contents/Frameworks/<name> Helper.app`, and the bundler builds one from
//! this target for each helper role.
//!
//! It does the minimum: load the framework, then hand the process to CEF.

use cef::{args::Args, *};

fn main() {
    let args = Args::new();

    // The main process runs Chromium sandboxed, so every helper must enter the
    // sandbox before anything else. (cef-rs's example gates this on its own
    // crate's `sandbox` feature; this crate has none — `cef` carries it — and
    // the gate compiled the call away: every sandboxed helper then died on a
    // CHECK with SIGTRAP, "GPU process exited unexpectedly: exit_code=5".)
    #[cfg(target_os = "macos")]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };

    // The loader has to outlive every CEF call in the process.
    #[cfg(target_os = "macos")]
    let _loader = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), true);
        assert!(loader.load(), "helper could not load the CEF framework");
        loader
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
}

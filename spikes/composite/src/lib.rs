// CEF's Windows bootstrap owns sandbox services and loads nus.dll. Keep the
// same crate-root module paths as the executable; other platforms use main.rs.
#![cfg(target_os = "windows")]
include!("main.rs");

/// Called by the matching, bundled CEF bootstrap executable in every process.
#[no_mangle]
pub extern "C" fn RunWinMain(
    _instance: cef::sys::HINSTANCE,
    _command_line: *const u16,
    _command_show: i32,
    sandbox_info: *mut u8,
    _version_info: *const std::ffi::c_void,
) -> i32 {
    browser_runtime::set_sandbox_info(sandbox_info);
    run()
}

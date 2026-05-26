//! macOS NSWindow titleVisibility — hide the title text rendered inside the
//! traffic-light strip while keeping `window.title` populated for Dock
//! right-click / Mission Control / Cmd+Tab.
//!
//! Background: `titleBarStyle: Overlay` only floats the traffic lights over
//! content — it does NOT hide the title text. Without setting
//! `titleVisibility = .hidden`, the native title shows up next to the lights
//! (e.g. "AIjia") even though there is no visible title bar background.
//!
//! No-op on non-macOS platforms.

use tauri::{Runtime, WebviewWindow};

pub fn hide_window_title<R: Runtime>(_window: &WebviewWindow<R>) {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use objc2_app_kit::NSWindowTitleVisibility;

        // ns_window() returns a *mut c_void pointing at the NSWindow*.
        let Ok(ns_window_ptr) = _window.ns_window() else {
            return;
        };
        if ns_window_ptr.is_null() {
            return;
        }
        // SAFETY: Tauri hands back a live NSWindow pointer; we call two
        // standard AppKit selectors on it from the main thread (setup
        // closure already runs on the main thread).
        unsafe {
            let window: *mut AnyObject = ns_window_ptr as *mut AnyObject;
            let _: () = msg_send![window, setTitleVisibility: NSWindowTitleVisibility::Hidden];
        }
    }
}

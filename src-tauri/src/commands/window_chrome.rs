//! macOS NSWindow chrome adjustments for the overlay title bar.
//!
//! Background: `titleBarStyle: Overlay` only floats the traffic lights over
//! content — it does NOT hide the title text. Without setting
//! `titleVisibility = .hidden`, the native title shows up next to the lights
//! (e.g. "AIjia") even though there is no visible title bar background.
//! We also position the traffic-light buttons before showing the main window,
//! so the first visible frame already matches the 48px React header band.
//!
//! No-op on non-macOS platforms.

use tauri::{Runtime, WebviewWindow};

const MAC_TRAFFIC_LIGHT_X: f64 = 16.0;
const MAC_TRAFFIC_LIGHT_Y: f64 = 20.0;
// Give AppKit/WebView one layout pass before showing the hidden main window.
const MAC_TRAFFIC_LIGHT_REPOSITION_DELAY_MS: u64 = 400;

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

pub fn position_traffic_lights_then_show<R: Runtime + 'static>(_window: &WebviewWindow<R>) {
    apply_traffic_light_position(_window);

    #[cfg(target_os = "macos")]
    {
        let window = _window.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(
                MAC_TRAFFIC_LIGHT_REPOSITION_DELAY_MS,
            ))
            .await;

            let window_for_dispatch = window.clone();
            let window_for_positioning = window.clone();
            let _ = window_for_dispatch.run_on_main_thread(move || {
                apply_traffic_light_position(&window_for_positioning);
                if let Err(err) = window_for_positioning.show() {
                    log::warn!("Failed to show main window after macOS chrome positioning: {err}");
                }
                if let Err(err) = window_for_positioning.set_focus() {
                    log::warn!("Failed to focus main window after macOS chrome positioning: {err}");
                }
            });
        });
    }
}

fn apply_traffic_light_position<R: Runtime>(_window: &WebviewWindow<R>) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSView, NSWindow, NSWindowButton};

        let Ok(ns_window_ptr) = _window.ns_window() else {
            return;
        };
        if ns_window_ptr.is_null() {
            return;
        }

        // SAFETY: Tauri returns a live NSWindow pointer during setup on the
        // main thread. This mirrors WRY/Tao's traffic-light inset algorithm.
        unsafe {
            let window = &*(ns_window_ptr as *mut NSWindow);
            let Some(close) = window.standardWindowButton(NSWindowButton::CloseButton) else {
                return;
            };
            let Some(minimize) = window.standardWindowButton(NSWindowButton::MiniaturizeButton)
            else {
                return;
            };
            let zoom = window.standardWindowButton(NSWindowButton::ZoomButton);

            let Some(close_superview) = close.superview() else {
                return;
            };
            let Some(title_bar_container_view) = close_superview.superview() else {
                return;
            };

            let close_rect = NSView::frame(&close);
            let title_bar_frame_height = close_rect.size.height + MAC_TRAFFIC_LIGHT_Y;
            let mut title_bar_rect = NSView::frame(&title_bar_container_view);
            title_bar_rect.size.height = title_bar_frame_height;
            title_bar_rect.origin.y = window.frame().size.height - title_bar_frame_height;
            title_bar_container_view.setFrame(title_bar_rect);

            let space_between = NSView::frame(&minimize).origin.x - close_rect.origin.x;
            let buttons = [Some(close), Some(minimize), zoom];
            for (index, button) in buttons.into_iter().flatten().enumerate() {
                let mut rect = NSView::frame(&button);
                rect.origin.x = MAC_TRAFFIC_LIGHT_X + (index as f64 * space_between);
                button.setFrameOrigin(rect.origin);
            }
        }
    }
}

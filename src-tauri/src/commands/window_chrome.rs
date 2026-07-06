//! macOS NSWindow chrome adjustments for the overlay title bar.
//!
//! `trafficLightPosition` in `tauri.conf.json` is the primary path, but AppKit
//! can still do one titlebar layout pass while the hidden main window is being
//! created. Apply the same inset before showing the first visible frame.

use tauri::{Runtime, WebviewWindow};

const MAC_TRAFFIC_LIGHT_X: f64 = 16.0;
const MAC_TRAFFIC_LIGHT_Y: f64 = 20.0;
const MAC_TRAFFIC_LIGHT_REPOSITION_DELAY_MS: u64 = 400;
#[cfg(target_os = "macos")]
const MAC_TRAFFIC_LIGHT_EVENT_DEBOUNCE_MS: u64 = 120;
#[cfg(target_os = "macos")]
const MAC_TRAFFIC_LIGHT_POSITION_EPSILON: f64 = 0.5;

#[cfg(target_os = "macos")]
static TRAFFIC_LIGHT_EVENT_SEQUENCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub fn position_traffic_lights_then_show<R: Runtime + 'static>(_window: &WebviewWindow<R>) {
    apply_traffic_light_position_if_needed(_window);

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
                apply_traffic_light_position_if_needed(&window_for_positioning);
                if let Err(err) = window_for_positioning.show() {
                    log::warn!("Failed to show main window after macOS chrome positioning: {err}");
                }
                if let Err(err) = window_for_positioning.set_focus() {
                    log::warn!("Failed to focus main window after macOS chrome positioning: {err}");
                }
                apply_traffic_light_position_if_needed(&window_for_positioning);
                schedule_traffic_light_position_check(&window_for_positioning);
            });
        });
    }
}

pub fn schedule_traffic_light_position_check<R: Runtime + 'static>(_window: &WebviewWindow<R>) {
    #[cfg(target_os = "macos")]
    {
        let sequence =
            TRAFFIC_LIGHT_EVENT_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let window = _window.clone();

        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(
                MAC_TRAFFIC_LIGHT_EVENT_DEBOUNCE_MS,
            ))
            .await;

            if TRAFFIC_LIGHT_EVENT_SEQUENCE.load(std::sync::atomic::Ordering::Relaxed) != sequence {
                return;
            }

            let window_for_dispatch = window.clone();
            let window_for_positioning = window.clone();
            let _ = window_for_dispatch.run_on_main_thread(move || {
                apply_traffic_light_position_if_needed(&window_for_positioning);
            });
        });
    }
}

fn apply_traffic_light_position_if_needed<R: Runtime>(_window: &WebviewWindow<R>) {
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
        // main thread. This matches WRY's traffic-light inset algorithm.
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
            let expected_title_bar_origin_y = window.frame().size.height - title_bar_frame_height;
            if !is_near(title_bar_rect.size.height, title_bar_frame_height)
                || !is_near(title_bar_rect.origin.y, expected_title_bar_origin_y)
            {
                title_bar_rect.size.height = title_bar_frame_height;
                title_bar_rect.origin.y = expected_title_bar_origin_y;
                title_bar_container_view.setFrame(title_bar_rect);
            }

            let space_between = NSView::frame(&minimize).origin.x - close_rect.origin.x;
            let buttons = [Some(close), Some(minimize), zoom];
            for (index, button) in buttons.into_iter().flatten().enumerate() {
                let mut rect = NSView::frame(&button);
                let expected_x = MAC_TRAFFIC_LIGHT_X + (index as f64 * space_between);
                if !is_near(rect.origin.x, expected_x) {
                    rect.origin.x = expected_x;
                    button.setFrameOrigin(rect.origin);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn is_near(value: f64, expected: f64) -> bool {
    (value - expected).abs() <= MAC_TRAFFIC_LIGHT_POSITION_EPSILON
}

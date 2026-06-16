#[test]
fn macos_close_button_hides_main_window_instead_of_exiting_app() {
    let source = std::fs::read_to_string("src/lib.rs").expect("read src/lib.rs");

    assert!(
        source.contains(".on_window_event("),
        "macOS close behavior must be handled before the window is destroyed"
    );
    assert!(
        source.contains("tauri::WindowEvent::CloseRequested"),
        "macOS close behavior must intercept WindowEvent::CloseRequested"
    );
    assert!(
        source.contains("api.prevent_close()"),
        "macOS close behavior must prevent destroying the last main window"
    );
    assert!(
        source.contains("window.hide()"),
        "macOS close behavior must hide the main window so IM/background tasks keep running"
    );
    assert!(
        source.contains("#[cfg(target_os = \"macos\")]"),
        "macOS close-to-hide behavior must stay platform-scoped"
    );
}

#[test]
fn windows_close_button_minimizes_main_window_instead_of_exiting_app() {
    let source = std::fs::read_to_string("src/lib.rs").expect("read src/lib.rs");

    assert!(
        source.contains(".on_window_event("),
        "Windows close behavior must be handled before the window is destroyed"
    );
    assert!(
        source.contains("#[cfg(target_os = \"windows\")]"),
        "Windows close behavior must be explicit and platform-scoped"
    );
    assert!(
        source.contains("api.prevent_close()"),
        "Windows close behavior must prevent destroying the last main window"
    );
    assert!(
        source.contains("window.minimize()"),
        "Windows close button must minimize the window so IM/background tasks keep running and users can restore it from the taskbar"
    );
}

#[test]
fn macos_dock_reopen_restores_hidden_main_window() {
    let source = std::fs::read_to_string("src/lib.rs").expect("read src/lib.rs");

    assert!(
        source.contains("tauri::RunEvent::Reopen"),
        "macOS Dock activation must handle RunEvent::Reopen after the main window was hidden"
    );
    assert!(
        source.contains("has_visible_windows"),
        "macOS Reopen handling should only restore the window when no visible window exists"
    );
    assert!(
        source.contains("win.show()"),
        "macOS Dock activation must show the hidden main window"
    );
    assert!(
        source.contains("win.set_focus()"),
        "macOS Dock activation must focus the restored main window"
    );
}

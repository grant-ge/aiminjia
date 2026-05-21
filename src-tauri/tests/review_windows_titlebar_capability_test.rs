/// Regression test: default capability must grant the three window-control
/// permissions used by the Windows TitleBar component.
///
/// If these entries are missing, `getCurrentWindow().minimize()` /
/// `toggleMaximize()` / `close()` are silently rejected by the Tauri
/// security layer on Windows, making the custom titlebar buttons non-functional.
use std::fs;

fn read_default_capabilities() -> serde_json::Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities/default.json");
    let raw = fs::read_to_string(path).expect("capabilities/default.json must exist");
    serde_json::from_str(&raw).expect("capabilities/default.json must be valid JSON")
}

fn has_permission(permissions: &[serde_json::Value], name: &str) -> bool {
    permissions.iter().any(|p| p.as_str() == Some(name))
}

#[test]
fn windows_titlebar_window_control_permissions_are_present() {
    let config = read_default_capabilities();
    let perms: Vec<serde_json::Value> = config["permissions"]
        .as_array()
        .expect("permissions must be an array")
        .clone();

    assert!(
        has_permission(&perms, "core:window:allow-minimize"),
        "Missing core:window:allow-minimize — Minimize button will not work on Windows"
    );
    assert!(
        has_permission(&perms, "core:window:allow-toggle-maximize"),
        "Missing core:window:allow-toggle-maximize — Maximize button will not work on Windows"
    );
    assert!(
        has_permission(&perms, "core:window:allow-close"),
        "Missing core:window:allow-close — Close button will not work on Windows"
    );
}

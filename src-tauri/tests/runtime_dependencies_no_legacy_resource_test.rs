use std::fs;

#[test]
fn tauri_config_does_not_package_legacy_python_runtime_resource() {
    let config = fs::read_to_string("tauri.conf.json").expect("read tauri config");

    assert!(
        !config.contains("python-runtime"),
        "python-runtime is legacy and must not be packaged as an app resource"
    );
}

use std::fs;

const WINDOWS_ASSET_ORIGIN: &str = "http://asset.localhost";

fn read_json(path: &str) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("read {path}: {err}");
    }))
    .unwrap_or_else(|err| panic!("parse {path}: {err}"))
}

fn security_field<'a>(config: &'a serde_json::Value, field: &str) -> &'a str {
    config["app"]["security"][field]
        .as_str()
        .unwrap_or_else(|| panic!("missing app.security.{field}"))
}

fn assert_img_src_allows_windows_asset(csp: &str, label: &str) {
    let img_src = csp
        .split(';')
        .map(str::trim)
        .find(|directive| directive.starts_with("img-src "))
        .unwrap_or_else(|| panic!("{label} is missing img-src"));

    assert!(
        img_src.split_whitespace().any(|token| token == WINDOWS_ASSET_ORIGIN),
        "{label} img-src must allow {WINDOWS_ASSET_ORIGIN} because Tauri serves asset:// as http://asset.localhost on Windows; got: {img_src}"
    );
}

#[test]
fn tauri_csp_allows_windows_asset_protocol_for_local_profile_images() {
    let config = read_json("tauri.conf.json");
    assert_img_src_allows_windows_asset(security_field(&config, "csp"), "tauri.conf.json csp");
    assert_img_src_allows_windows_asset(
        security_field(&config, "devCsp"),
        "tauri.conf.json devCsp",
    );

    let e2e_config = read_json("tauri.conf.e2e.json");
    assert_img_src_allows_windows_asset(
        security_field(&e2e_config, "csp"),
        "tauri.conf.e2e.json csp",
    );

    let dev_script = fs::read_to_string("../scripts/tauri-dev.mjs")
        .expect("read scripts/tauri-dev.mjs");
    assert!(
        dev_script.contains(WINDOWS_ASSET_ORIGIN),
        "scripts/tauri-dev.mjs port override devCsp must allow {WINDOWS_ASSET_ORIGIN}"
    );
}

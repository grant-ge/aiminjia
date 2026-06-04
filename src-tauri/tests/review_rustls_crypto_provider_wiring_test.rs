#[test]
fn review_app_installs_rustls_crypto_provider_before_tauri_setup() {
    let source = include_str!("../src/lib.rs");

    let install = source
        .find("rustls::crypto::ring::default_provider().install_default()")
        .expect("app startup must install a process-level rustls CryptoProvider");
    let builder = source
        .find("tauri::Builder::default()")
        .expect("tauri builder should exist");
    let setup = source
        .find(".setup(|app|")
        .expect("tauri setup should exist");

    assert!(
        install < builder && install < setup,
        "rustls CryptoProvider must be installed before Tauri setup can auto-connect IM websocket clients"
    );
}

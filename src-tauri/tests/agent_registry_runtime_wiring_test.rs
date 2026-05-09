//! Smoke test: confirm runtime wiring code path used in lib.rs works in isolation.
//! We cannot bring up the full Tauri app in tests, so we replicate the load+manage
//! pattern using the same loader function and assert it returns a usable registry.

use std::fs;
use tempfile::TempDir;

use app_lib::runtime::agent::registry_loader::load_registry_with_user_dir;

#[test]
fn lib_rs_pattern_loads_builtins_when_no_user_dir() {
    // Mimic lib.rs path: user not logged in → user_agents_dir = None
    let reg = load_registry_with_user_dir(None, None);
    assert!(reg.get("explore").is_some());
    assert!(reg.get("daily_assistant_agent").is_some());
}

#[test]
fn lib_rs_pattern_loads_user_dir_when_present() {
    // Mimic lib.rs path: logged in → user_agents_dir = Some(/users/<scope>/agents)
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("custom.md"),
        "---\nname: custom\ndescription: User custom\n---\nbody",
    )
    .unwrap();

    let reg = load_registry_with_user_dir(Some(dir.path()), None);
    assert!(reg.get("custom").is_some());
    // builtins still loaded
    assert!(reg.get("explore").is_some());
}

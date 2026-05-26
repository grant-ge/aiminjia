//! 守护 CLAUDE.md 决策 #4：runtime/network/ 不得 use tauri::*。

use std::fs;
use std::path::PathBuf;

#[test]
fn network_module_does_not_use_tauri() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = crate_root.join("src/runtime/network");
    assert!(dir.exists(), "runtime/network module should exist");

    let mut bad = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap();
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains("use tauri::") || trimmed.contains("use tauri;") {
                bad.push(format!("{}:{}: {}", path.display(), i + 1, line));
            }
        }
    }

    assert!(
        bad.is_empty(),
        "runtime/network/ must not import tauri (CLAUDE.md #4):\n{}",
        bad.join("\n")
    );
}

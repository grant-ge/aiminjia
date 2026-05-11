//! Cross-validation: the Python-built `.aijia-skill` packages (produced by
//! `scripts/skills/build-bundle.py`) must be consumable by the Rust
//! unpacker — same OPS-standard zip layout (SKILL.md at root or one-level
//! subdirectory).
//!
//! Skipped (not failed) when `dist-skills/` doesn't exist — so `cargo test`
//! works on a fresh checkout without first running the build script.

use std::path::PathBuf;

use app_lib::storage::skill_package::unpack_skill_archive;
use tempfile::TempDir;

fn dist_dir() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("has parent")
        .join("dist-skills");
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

#[test]
fn python_built_bundles_match_rust_unpacker() {
    let Some(dist) = dist_dir() else {
        eprintln!("[skip] dist-skills/ not built — run scripts/skills/build-bundle.py first");
        return;
    };

    let tmp = TempDir::new().unwrap();
    let mut verified = 0;
    for entry in std::fs::read_dir(&dist).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("aijia-skill") {
            continue;
        }
        let unpack_root = tmp.path().join(
            path.file_stem().unwrap().to_string_lossy().to_string(),
        );
        let res = unpack_skill_archive(&path, &unpack_root).unwrap_or_else(|e| {
            panic!(
                "rust unpacker rejected python-built archive {:?}: {}",
                path.file_name().unwrap(),
                e
            )
        });
        assert!(!res.skill_id.is_empty());
        assert!(res.skill_dir.join("SKILL.md").is_file());
        verified += 1;
    }
    assert!(verified > 0, "no .aijia-skill archives found in dist-skills/");
    eprintln!("✅ cross-validated {} Python-built archives", verified);
}

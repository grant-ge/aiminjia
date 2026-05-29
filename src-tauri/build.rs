fn main() {
    sync_e2e_capability();
    tauri_build::build()
}

/// Copy `capabilities-e2e/pilot.json` into `capabilities/pilot.json` when
/// the `e2e` feature is on; remove it when off. The capabilities/ dir is
/// glob-scanned by tauri-build, but it can't condition entries on cargo
/// features — we manage the file presence ourselves so the default build
/// stays free of pilot:default (which would fail to resolve when the
/// optional tauri-plugin-pilot dep isn't compiled in).
///
/// The copied file is gitignored (see src-tauri/.gitignore).
fn sync_e2e_capability() {
    use std::fs;
    use std::path::PathBuf;

    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo"),
    );
    let src = manifest_dir.join("capabilities-e2e").join("pilot.json");
    let dst = manifest_dir.join("capabilities").join("pilot.json");

    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_E2E");

    let e2e_enabled = std::env::var("CARGO_FEATURE_E2E").is_ok();

    if e2e_enabled {
        // Content-equal skip: writing dst unconditionally bumps mtime, which
        // makes `tauri dev`'s watcher re-trigger a rebuild that re-runs this
        // build script — an infinite loop. Only write when content actually
        // differs (or dst is missing) so the steady state is mtime-stable.
        let src_bytes =
            fs::read(&src).unwrap_or_else(|e| panic!("failed to read {}: {e}", src.display()));
        let needs_write = match fs::read(&dst) {
            Ok(dst_bytes) => dst_bytes != src_bytes,
            Err(_) => true,
        };
        if needs_write {
            fs::write(&dst, &src_bytes)
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", dst.display()));
        }
    } else if dst.exists() {
        fs::remove_file(&dst)
            .unwrap_or_else(|e| panic!("failed to remove stale {}: {e}", dst.display()));
    }
}

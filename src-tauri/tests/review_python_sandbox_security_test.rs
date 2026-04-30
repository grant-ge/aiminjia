use app_lib::python::sandbox::SandboxConfig;
use std::path::PathBuf;

#[test]
fn review_preamble_enables_read_restriction_when_allowed_read_paths_non_empty() {
    let mut config = SandboxConfig::default();
    config.allowed_read_paths = vec![PathBuf::from("/workspace")];
    config.allowed_write_paths = vec![PathBuf::from("/workspace")];

    let preamble = config.preamble();

    assert!(
        preamble.contains(
            "if _ALLOWED_READ_PATHS and not _is_allowed_path(abs_path, _ALLOWED_READ_PATHS):"
        ),
        "preamble should enforce read path restrictions only when _ALLOWED_READ_PATHS is non-empty"
    );
}

#[test]
fn review_preamble_patches_shutil_write_apis_with_destination_guard() {
    let mut config = SandboxConfig::default();
    config.allowed_read_paths = vec![PathBuf::from("/workspace")];
    config.allowed_write_paths = vec![PathBuf::from("/workspace")];

    let preamble = config.preamble();

    assert!(
        preamble.contains("_patch_shutil_write_apis()"),
        "preamble should patch shutil write APIs"
    );
    assert!(
        preamble.contains("for _name in ('copy', 'copy2', 'copyfile', 'move')"),
        "shutil patch should cover copy/copy2/copyfile/move"
    );
    assert!(
        preamble.contains("_ensure_write_path(dst, op=name)"),
        "patched shutil APIs should validate destination write path"
    );
}

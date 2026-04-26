use std::path::PathBuf;

use app_lib::runtime::dependencies::{RuntimePathError, RuntimePaths};

#[test]
fn computes_bundle_scoped_runtime_directories() {
    let cache_root = std::env::temp_dir().join("renlijia-runtimes");
    let paths = RuntimePaths::new(cache_root.clone(), "renlijia-primary-runtime")
        .expect("valid runtime paths");

    let bundle_root = cache_root.join("renlijia-primary-runtime");
    assert_eq!(paths.bundle_root(), bundle_root);
    assert_eq!(paths.current_dir(), bundle_root.join("current"));
    assert_eq!(paths.versions_dir(), bundle_root.join("versions"));
    assert_eq!(paths.downloads_dir(), bundle_root.join("downloads"));
    assert_eq!(paths.staging_dir(), bundle_root.join("staging"));
}

#[test]
fn computes_version_directory_under_versions_layout() {
    let cache_root = std::env::temp_dir().join("renlijia-runtimes");
    let paths = RuntimePaths::new(cache_root.clone(), "renlijia-primary-runtime")
        .expect("valid runtime paths");

    assert_eq!(
        paths.version_dir("2026.04.25").expect("valid version"),
        cache_root
            .join("renlijia-primary-runtime")
            .join("versions")
            .join("2026.04.25")
    );
}

#[test]
fn rejects_relative_cache_root() {
    let error =
        RuntimePaths::new(PathBuf::from("relative-cache"), "renlijia-primary-runtime").unwrap_err();

    assert_eq!(
        error,
        RuntimePathError::NonAbsoluteCacheRoot {
            path: PathBuf::from("relative-cache"),
        }
    );
}

#[test]
fn rejects_bundle_id_that_is_not_a_safe_path_segment() {
    let cache_root = std::env::temp_dir().join("renlijia-runtimes");

    for bundle_id in ["", "../escape", "nested/bundle", ".", "/tmp/escape"] {
        let error = RuntimePaths::new(cache_root.clone(), bundle_id).unwrap_err();
        assert_eq!(
            error,
            RuntimePathError::UnsafePathSegment {
                field: "bundle_id",
                value: bundle_id.to_string(),
            }
        );
    }
}

#[test]
fn rejects_version_that_is_not_a_safe_path_segment() {
    let cache_root = std::env::temp_dir().join("renlijia-runtimes");
    let paths =
        RuntimePaths::new(cache_root, "renlijia-primary-runtime").expect("valid runtime paths");

    for version in ["", "../escape", "nested/version", ".", "/tmp/escape"] {
        let error = paths.version_dir(version).unwrap_err();
        assert_eq!(
            error,
            RuntimePathError::UnsafePathSegment {
                field: "version",
                value: version.to_string(),
            }
        );
    }
}

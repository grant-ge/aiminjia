use std::path::PathBuf;

use app_lib::runtime::dependencies::{validate_archive_entry_path, ArchiveError};

#[test]
fn rejects_parent_traversal_entry() {
    let dest = std::env::temp_dir().join("lotus-archive-boundary");

    let error = validate_archive_entry_path(&dest, "../escape").unwrap_err();

    assert_eq!(
        error,
        ArchiveError::UnsafeEntry {
            entry: "../escape".to_string(),
        }
    );
}

#[test]
fn rejects_special_or_windows_style_entries() {
    let dest = std::env::temp_dir().join("lotus-archive-boundary");

    for entry in [
        "",
        ".",
        "..",
        "node\\..\\escape",
        "C:\\escape",
        "\\\\server\\share",
    ] {
        assert_eq!(
            validate_archive_entry_path(&dest, entry),
            Err(ArchiveError::UnsafeEntry {
                entry: entry.to_string(),
            })
        );
    }
}

#[test]
fn rejects_absolute_entry() {
    let dest = std::env::temp_dir().join("lotus-archive-boundary");
    let absolute_entry = std::env::temp_dir().join("escape");

    let error =
        validate_archive_entry_path(&dest, absolute_entry.to_string_lossy().as_ref()).unwrap_err();

    assert_eq!(
        error,
        ArchiveError::UnsafeEntry {
            entry: absolute_entry.to_string_lossy().into_owned(),
        }
    );
}

#[test]
fn accepts_nested_relative_entry() {
    let dest = std::env::temp_dir().join("lotus-archive-boundary");
    let entry = PathBuf::from("node").join("bin").join("node");

    let validated = validate_archive_entry_path(&dest, entry.to_string_lossy().as_ref())
        .expect("nested relative entry should be accepted");

    assert_eq!(validated, dest.join("node").join("bin").join("node"));
}

#[test]
fn accepts_curdir_normal_path() {
    let dest = std::env::temp_dir().join("lotus-archive-boundary");
    let entry = PathBuf::from(".").join("safe").join("current");

    let validated = validate_archive_entry_path(&dest, entry.to_string_lossy().as_ref())
        .expect("relative entry should be accepted");

    assert_eq!(validated, dest.join("safe").join("current"));
}

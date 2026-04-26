use std::fs;

use app_lib::runtime::dependencies::{verify_sha256, ChecksumError};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    format!("{:x}", digest)
}

#[test]
fn verify_sha256_accepts_matching_checksum() {
    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), b"renlijia-runtime").expect("write test data");

    let expected = sha256_hex(b"renlijia-runtime");

    assert_eq!(verify_sha256(file.path(), &expected), Ok(()));
}

#[test]
fn verify_sha256_accepts_uppercase_expected_checksum() {
    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), b"renlijia-runtime").expect("write test data");

    let expected = sha256_hex(b"renlijia-runtime").to_uppercase();

    assert_eq!(verify_sha256(file.path(), &expected), Ok(()));
}

#[test]
fn verify_sha256_reports_mismatch_expected_and_actual() {
    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), b"renlijia-runtime").expect("write test data");

    let error = verify_sha256(
        file.path(),
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    )
    .expect_err("checksum should mismatch");

    assert_eq!(
        error,
        ChecksumError::Mismatch {
            expected: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                .to_string(),
            actual: sha256_hex(b"renlijia-runtime"),
        }
    );
}

#[test]
fn verify_sha256_rejects_invalid_expected_checksum_format() {
    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), b"renlijia-runtime").expect("write test data");

    let error = verify_sha256(file.path(), "not-a-valid-sha256")
        .expect_err("invalid expected checksum should fail before comparison");

    assert_eq!(
        error,
        ChecksumError::InvalidExpected {
            expected: "not-a-valid-sha256".to_string(),
        }
    );
}

#[test]
fn verify_sha256_reads_files_larger_than_single_buffer() {
    let file = NamedTempFile::new().expect("temp file");
    let data = vec![b'x'; 20_000];
    fs::write(file.path(), &data).expect("write test data");

    assert_eq!(verify_sha256(file.path(), &sha256_hex(&data)), Ok(()));
}

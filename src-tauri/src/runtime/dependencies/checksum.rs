use std::fs::File;
use std::io::{Read, Result as IoResult};
use std::path::Path;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksumError {
    Io(String),
    InvalidExpected { expected: String },
    Mismatch { expected: String, actual: String },
}

impl std::fmt::Display for ChecksumError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "checksum io error: {error}"),
            Self::InvalidExpected { expected } => {
                write!(f, "invalid expected sha256 checksum: {expected}")
            }
            Self::Mismatch { expected, actual } => {
                write!(f, "checksum mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for ChecksumError {}

pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), ChecksumError> {
    if !is_valid_sha256(expected) {
        return Err(ChecksumError::InvalidExpected {
            expected: expected.to_string(),
        });
    }

    let mut file = File::open(path).map_err(|error| ChecksumError::Io(error.to_string()))?;
    let actual = sha256_hex(&mut file).map_err(|error| ChecksumError::Io(error.to_string()))?;
    let expected_lower = expected.to_ascii_lowercase();

    if actual == expected_lower {
        Ok(())
    } else {
        Err(ChecksumError::Mismatch {
            expected: expected.to_string(),
            actual,
        })
    }
}

fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sha256_hex(file: &mut File) -> IoResult<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

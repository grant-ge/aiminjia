//! AES-256-GCM encryption for sensitive data (API keys).
//!
//! Master encryption key is stored as a file in the app data directory.
//! This is more reliable than OS Keychain in development mode, where
//! binary code signatures change between recompiles causing Keychain
//! access denials.
//!
//! All sensitive data is encrypted at rest using AES-256-GCM with random
//! 96-bit nonces. Encrypted output format: "nonce_hex:ciphertext_hex".
#![allow(dead_code)]

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use aes_gcm::aead::rand_core::RngCore;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Name of the master key file within the app data directory.
const KEY_FILE_NAME: &str = "master.key";

/// Encode a byte slice as a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>()
}

/// Decode a hex string into a byte vector.
/// Returns an error if the string has odd length or contains non-hex characters.
fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        anyhow::bail!("Hex string has odd length");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .with_context(|| format!("Invalid hex byte at position {}", i))
        })
        .collect()
}

pub struct SecureStorage {
    cipher: Aes256Gcm,
}

impl SecureStorage {
    /// Create a new SecureStorage instance.
    ///
    /// The master key is stored in a file at `data_dir/master.key`.
    /// If the file doesn't exist, a new random key is generated and saved.
    /// This approach is reliable across app restarts, unlike OS Keychain
    /// which can break when the binary code signature changes (dev mode).
    pub fn new(data_dir: &Path) -> Result<Self> {
        let key_bytes = Self::get_or_create_master_key(data_dir)?;
        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to initialize AES-256-GCM cipher: {}", e))?;
        Ok(Self { cipher })
    }

    /// Encrypt plaintext, returns hex-encoded "nonce:ciphertext".
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        let nonce_hex = hex_encode(&nonce_bytes);
        let ct_hex = hex_encode(&ciphertext);
        Ok(format!("{}:{}", nonce_hex, ct_hex))
    }

    /// Decrypt a previously encrypted string (format: "nonce_hex:ciphertext_hex").
    pub fn decrypt(&self, encrypted: &str) -> Result<String> {
        let parts: Vec<&str> = encrypted.splitn(2, ':').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid encrypted format: expected 'nonce_hex:ciphertext_hex'");
        }

        let nonce_bytes = hex_decode(parts[0]).context("Failed to decode nonce")?;
        if nonce_bytes.len() != 12 {
            anyhow::bail!(
                "Invalid nonce length: expected 12 bytes, got {}",
                nonce_bytes.len()
            );
        }
        let ciphertext = hex_decode(parts[1]).context("Failed to decode ciphertext")?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

        String::from_utf8(plaintext).context("Decrypted data is not valid UTF-8")
    }

    /// Get or create the master encryption key from a file.
    ///
    /// The key file stores the key as a hex string. If the file doesn't
    /// exist or is corrupted, a new key is generated.
    fn get_or_create_master_key(data_dir: &Path) -> Result<Vec<u8>> {
        let key_path = data_dir.join(KEY_FILE_NAME);

        // Try to read existing key
        if key_path.exists() {
            if let Ok(key_hex) = std::fs::read_to_string(&key_path) {
                let key_hex = key_hex.trim();
                if let Ok(key) = hex_decode(key_hex) {
                    if key.len() == 32 {
                        log::info!("Loaded master key from {}", key_path.display());
                        return Ok(key);
                    }
                }
            }
            log::warn!("Master key file corrupted, regenerating");
        }

        // Generate a new 256-bit (32-byte) key
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        let key_hex = hex_encode(&key);

        // Save to file with restrictive permissions
        std::fs::write(&key_path, &key_hex)
            .with_context(|| format!("Failed to write master key to {}", key_path.display()))?;

        // Set file permissions to owner-only (Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&key_path, perms).ok();
        }

        log::info!("Generated new master encryption key at {}", key_path.display());
        Ok(key.to_vec())
    }

    /// Get the path where the master key file would be stored.
    /// Useful for diagnostics.
    pub fn key_file_path(data_dir: &Path) -> PathBuf {
        data_dir.join(KEY_FILE_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_encode_decode_roundtrip() {
        let data = b"hello world";
        let encoded = hex_encode(data);
        let decoded = hex_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_hex_encode_known_values() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x0a, 0xab]), "00ff0aab");
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_decode_known_values() {
        assert_eq!(hex_decode("00ff0aab").unwrap(), vec![0x00, 0xff, 0x0a, 0xab]);
        assert_eq!(hex_decode("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn test_hex_decode_odd_length_fails() {
        assert!(hex_decode("abc").is_err());
    }

    #[test]
    fn test_hex_decode_invalid_chars_fails() {
        assert!(hex_decode("zzzz").is_err());
    }

    #[test]
    fn test_decrypt_invalid_format() {
        // Build a cipher with a fixed test key (32 bytes of zeros)
        let key = [0u8; 32];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let storage = SecureStorage { cipher };

        assert!(storage.decrypt("no_colon_here").is_err());
        assert!(storage.decrypt("").is_err());
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        // Build a cipher with a fixed test key
        let key = [0u8; 32];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let storage = SecureStorage { cipher };

        let plaintext = "sk-test-api-key-1234567890";
        let encrypted = storage.encrypt(plaintext).unwrap();

        // Verify format: should contain exactly one colon
        assert_eq!(encrypted.matches(':').count(), 1);

        // Nonce part should be 24 hex chars (12 bytes)
        let parts: Vec<&str> = encrypted.splitn(2, ':').collect();
        assert_eq!(parts[0].len(), 24);

        let decrypted = storage.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_produces_different_ciphertexts() {
        let key = [0u8; 32];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let storage = SecureStorage { cipher };

        let plaintext = "same plaintext";
        let enc1 = storage.encrypt(plaintext).unwrap();
        let enc2 = storage.encrypt(plaintext).unwrap();

        // Two encryptions of the same plaintext should produce different ciphertexts
        // because the nonce is random each time
        assert_ne!(enc1, enc2);

        // But both should decrypt to the same plaintext
        assert_eq!(storage.decrypt(&enc1).unwrap(), plaintext);
        assert_eq!(storage.decrypt(&enc2).unwrap(), plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_empty_string() {
        let key = [0u8; 32];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let storage = SecureStorage { cipher };

        let encrypted = storage.encrypt("").unwrap();
        let decrypted = storage.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn test_encrypt_decrypt_unicode() {
        let key = [0u8; 32];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let storage = SecureStorage { cipher };

        let plaintext = "Unicode test: Chinese characters, Japanese characters, emoji";
        let encrypted = storage.encrypt(plaintext).unwrap();
        let decrypted = storage.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let key = [0u8; 32];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let storage = SecureStorage { cipher };

        let encrypted = storage.encrypt("secret").unwrap();
        let parts: Vec<&str> = encrypted.splitn(2, ':').collect();

        // Tamper with the ciphertext by flipping a character
        let mut tampered_ct: Vec<u8> = hex_decode(parts[1]).unwrap();
        if !tampered_ct.is_empty() {
            tampered_ct[0] ^= 0xff;
        }
        let tampered = format!("{}:{}", parts[0], hex_encode(&tampered_ct));

        assert!(storage.decrypt(&tampered).is_err());
    }

    #[test]
    fn test_file_based_key_persistence() {
        let dir = std::env::temp_dir().join("aijia_crypto_test");
        std::fs::create_dir_all(&dir).unwrap();

        // First creation — generates key
        let ss1 = SecureStorage::new(&dir).unwrap();
        let encrypted = ss1.encrypt("test-key").unwrap();

        // Second creation — reads same key from file
        let ss2 = SecureStorage::new(&dir).unwrap();
        let decrypted = ss2.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "test-key");

        // Cleanup
        std::fs::remove_dir_all(&dir).ok();
    }
}

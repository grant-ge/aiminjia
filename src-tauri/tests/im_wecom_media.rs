//! 媒体解密：AES-256-CBC + PKCS#7 padding。
//!
//! 测试向量：用 openssl 命令构造已知 key + plaintext → ciphertext，
//! 然后 wecom::media::decrypt 应还原 plaintext。

use app_lib::connector::im::wecom::media::decrypt_aeskey_cbc;
use base64::{engine::general_purpose::STANDARD as B64, Engine};

#[test]
fn decrypt_known_vector_roundtrip() {
    // 用 aes crate 自构造一份 vector（保证算法实现一致性）。
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type Enc = cbc::Encryptor<aes::Aes256>;
    let key_bytes = [0x11u8; 32];
    let iv_bytes: [u8; 16] = key_bytes[..16].try_into().unwrap();
    let plaintext = b"hello world, this is a wecom file payload.";

    let mut buf = vec![0u8; plaintext.len() + 16];
    let cipher_len = {
        let enc = Enc::new_from_slices(&key_bytes, &iv_bytes).unwrap();
        enc.encrypt_padded_b2b_mut::<Pkcs7>(plaintext, &mut buf)
            .unwrap()
            .len()
    };
    let ciphertext = &buf[..cipher_len];

    let aeskey_b64 = B64.encode(key_bytes);
    let recovered = decrypt_aeskey_cbc(ciphertext, &aeskey_b64).expect("decrypt");
    assert_eq!(recovered, plaintext);
}

#[test]
fn decrypt_rejects_bad_padding() {
    let aeskey_b64 = base64::engine::general_purpose::STANDARD.encode([0u8; 32]);
    let bogus = vec![0u8; 32];
    assert!(decrypt_aeskey_cbc(&bogus, &aeskey_b64).is_err());
}

#[test]
fn decode_download_code_split_aeskey_and_url() {
    use app_lib::connector::im::wecom::media::decode_download_code;
    let dc = "wecom://AESKEY_VAL@https://example.com/file?id=1";
    let (key, url) = decode_download_code(dc).expect("parse");
    assert_eq!(key, "AESKEY_VAL");
    assert_eq!(url, "https://example.com/file?id=1");
}

#[test]
fn decode_download_code_rejects_non_wecom_prefix() {
    use app_lib::connector::im::wecom::media::decode_download_code;
    assert!(decode_download_code("dingtalk://...").is_err());
}

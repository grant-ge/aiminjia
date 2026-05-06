//! Decode bytes captured from external CLI / console output.
//!
//! Background: on Windows zh-CN systems the default OEM code page is CP936/GBK.
//! Tools we shell out to (`where.exe`, `tasklist`, occasionally Node CLIs that
//! call `console.log` instead of writing UTF-8 to a piped stdout) emit bytes in
//! the active code page, not UTF-8. Decoding those as UTF-8 yields mojibake or
//! `U+FFFD` replacements; the most visible victim is a Chinese username in
//! `C:\Users\张三\…` paths returned by `where.exe`.
//!
//! Strategy: try UTF-8 first (cheap, handles the common case where the child
//! process correctly writes UTF-8 to a piped stdout). If that produces
//! replacement characters, fall back to GBK on Windows. On non-Windows we
//! always trust UTF-8.

#[cfg(windows)]
use encoding_rs::GBK;

/// Decode console-captured bytes into a String.
///
/// On Windows, falls back to GBK if UTF-8 decoding produced replacement
/// characters. On other platforms, always UTF-8 (lossy).
pub fn decode_console_bytes(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }

    #[cfg(windows)]
    {
        let (cow, _, had_errors) = GBK.decode(bytes);
        if !had_errors {
            return cow.into_owned();
        }
    }

    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_ascii_as_utf8() {
        assert_eq!(decode_console_bytes(b"hello"), "hello");
    }

    #[test]
    fn decodes_utf8_chinese() {
        let bytes = "中文".as_bytes();
        assert_eq!(decode_console_bytes(bytes), "中文");
    }

    #[cfg(windows)]
    #[test]
    fn decodes_gbk_chinese_on_windows() {
        // "中" in GBK = D6 D0
        let bytes: &[u8] = &[0xD6, 0xD0];
        assert_eq!(decode_console_bytes(bytes), "中");
    }
}

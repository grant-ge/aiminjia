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
    #[cfg(windows)]
    {
        let (utf8, utf8_had_errors) = match std::str::from_utf8(bytes) {
            Ok(s) => (s.to_string(), false),
            Err(_) => (String::from_utf8_lossy(bytes).into_owned(), true),
        };
        let (gbk, _, gbk_had_errors) = GBK.decode(bytes);
        if !gbk_had_errors
            && should_prefer_gbk_decoding(&utf8, gbk.as_ref(), utf8_had_errors)
        {
            return gbk.into_owned();
        }
        return utf8;
    }

    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(any(windows, test))]
fn should_prefer_gbk_decoding(utf8: &str, gbk: &str, utf8_had_errors: bool) -> bool {
    if utf8_had_errors {
        return true;
    }

    let gbk_cjk = count_cjk(gbk);
    if gbk_cjk == 0 || gbk_cjk <= count_cjk(utf8) {
        return false;
    }

    let suspicious = count_suspicious_mojibake_chars(utf8);
    if suspicious == 0 {
        return false;
    }

    let ascii = utf8.chars().filter(|c| c.is_ascii_graphic()).count();
    ascii == 0 || suspicious >= 2
}

#[cfg(any(windows, test))]
fn count_cjk(s: &str) -> usize {
    s.chars()
        .filter(|&c| {
            ('\u{3400}'..='\u{4dbf}').contains(&c)
                || ('\u{4e00}'..='\u{9fff}').contains(&c)
                || ('\u{f900}'..='\u{faff}').contains(&c)
        })
        .count()
}

#[cfg(any(windows, test))]
fn count_suspicious_mojibake_chars(s: &str) -> usize {
    s.chars()
        .filter(|&c| {
            ('\u{00a0}'..='\u{00ff}').contains(&c)
                || ('\u{0400}'..='\u{052f}').contains(&c)
                || c == '\u{fffd}'
        })
        .count()
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

    #[cfg(windows)]
    #[test]
    fn decodes_gbk_bytes_that_are_also_valid_utf8_on_windows() {
        // GBK "猫" = C3 A8, which is valid UTF-8 for "è".
        assert_eq!(decode_console_bytes(&[0xC3, 0xA8]), "猫");
        // GBK "一" = D2 BB, which is valid UTF-8 for Cyrillic "һ".
        assert_eq!(decode_console_bytes(&[0xD2, 0xBB]), "一");
    }

    #[test]
    fn gbk_preference_heuristic_handles_valid_utf8_shaped_mojibake() {
        assert!(should_prefer_gbk_decoding("è", "猫", false));
        assert!(should_prefer_gbk_decoding("һ", "一", false));
        assert!(should_prefer_gbk_decoding("�", "中", true));
        assert!(!should_prefer_gbk_decoding("Café", "Caf茅", false));
        assert!(!should_prefer_gbk_decoding("中文", "涓枃", false));
    }
}

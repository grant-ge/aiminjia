//! Filename sanitization for cross-platform safety.
//!
//! Windows rejects more characters than POSIX and reserves certain DOS device
//! names (CON, PRN, AUX, NUL, COM1-9, LPT1-9). A user- or LLM-supplied filename
//! that's fine on macOS will silently fail to write on Windows. Validate at the
//! boundary instead.

use anyhow::{anyhow, Result};

const FORBIDDEN_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validate a filename for cross-platform filesystem safety. Returns Err with
/// a user-readable reason if the name would fail on Windows.
///
/// Checks: no path separators, no reserved DOS names (case-insensitive),
/// no control chars / Windows-forbidden chars, no trailing dot or space,
/// non-empty, ≤ 200 bytes (leaves headroom under Windows MAX_PATH 260).
pub fn ensure_safe_filename(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("filename is empty"));
    }
    if name.len() > 200 {
        return Err(anyhow!("filename too long ({} bytes, max 200)", name.len()));
    }
    if name == "." || name == ".." {
        return Err(anyhow!("filename '{}' is reserved", name));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(anyhow!(
            "filename '{}' has trailing dot or space (rejected by Windows)",
            name
        ));
    }
    for ch in name.chars() {
        if (ch as u32) < 0x20 {
            return Err(anyhow!("filename '{}' contains a control character", name));
        }
        if FORBIDDEN_CHARS.contains(&ch) {
            return Err(anyhow!(
                "filename '{}' contains forbidden character '{}'",
                name,
                ch
            ));
        }
    }
    let stem = name.split('.').next().unwrap_or(name);
    if RESERVED_NAMES.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return Err(anyhow!("filename '{}' uses a Windows reserved name", name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_names() {
        ensure_safe_filename("report.pdf").unwrap();
        ensure_safe_filename("分析-2026-05-06.xlsx").unwrap();
        ensure_safe_filename("a.b.c.json").unwrap();
    }

    #[test]
    fn rejects_path_separators() {
        assert!(ensure_safe_filename("a/b.txt").is_err());
        assert!(ensure_safe_filename("a\\b.txt").is_err());
    }

    #[test]
    fn rejects_reserved_dos_names() {
        assert!(ensure_safe_filename("CON").is_err());
        assert!(ensure_safe_filename("nul.txt").is_err());
        assert!(ensure_safe_filename("Com1.log").is_err());
    }

    #[test]
    fn rejects_trailing_dot_or_space() {
        assert!(ensure_safe_filename("a.").is_err());
        assert!(ensure_safe_filename("a ").is_err());
    }

    #[test]
    fn rejects_forbidden_chars() {
        for c in ['<', '>', ':', '"', '|', '?', '*'] {
            assert!(
                ensure_safe_filename(&format!("a{c}.txt")).is_err(),
                "should reject '{c}'"
            );
        }
    }

    #[test]
    fn rejects_empty_and_dots() {
        assert!(ensure_safe_filename("").is_err());
        assert!(ensure_safe_filename(".").is_err());
        assert!(ensure_safe_filename("..").is_err());
    }
}

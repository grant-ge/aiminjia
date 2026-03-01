//! Shared utility functions for tool handlers.

/// Escape a string for use inside a Python single-quoted string literal.
/// Handles backslashes, single quotes, and newlines.
pub(crate) fn py_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// Sanitize a string for use in a Python comment (strip newlines).
pub(super) fn py_comment_safe(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// Indent each non-empty line of Python code by `spaces` spaces.
/// Used to properly nest generated code blocks inside `try:` bodies.
pub(super) fn indent_python(code: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    code.lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", prefix, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Create a URL-safe slug from a title string.
pub(super) fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

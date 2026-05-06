use std::fs;
use std::io;
use std::path::Path;

/// Read a text file and strip a leading UTF-8 BOM (`\u{FEFF}`) if present.
///
/// Windows Notepad / many MS tooling write files with a BOM by default; serde_json
/// rejects the BOM as invalid JSON. Use this for any user-editable text/JSON file
/// loaded from disk.
pub fn read_to_string_strip_bom(path: impl AsRef<Path>) -> io::Result<String> {
    let mut content = fs::read_to_string(path)?;
    if content.starts_with('\u{FEFF}') {
        content.drain(..'\u{FEFF}'.len_utf8());
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn strips_leading_bom() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.json");
        fs::write(&path, "\u{FEFF}{\"k\":1}").unwrap();
        assert_eq!(read_to_string_strip_bom(&path).unwrap(), "{\"k\":1}");
    }

    #[test]
    fn passes_through_when_no_bom() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.json");
        fs::write(&path, "{\"k\":1}").unwrap();
        assert_eq!(read_to_string_strip_bom(&path).unwrap(), "{\"k\":1}");
    }
}

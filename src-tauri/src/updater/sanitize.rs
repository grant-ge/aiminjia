//! Strip macOS AppleDouble (`._*`) entries and `.DS_Store` from a tar.gz
//! before passing it to the Tauri updater. The bundled tarball produced by
//! `tauri build` on macOS embeds these files; the Rust `tar` crate used by
//! `tauri-plugin-updater::Update::install` doesn't understand AppleDouble
//! and trips when unpacking an entry like `AIjia.app/._AIjia.app`.

use anyhow::{Context, Result};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use std::io::{Cursor, Read, Write};
use std::path::Path;

fn is_macos_metadata(path: &Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        name == ".DS_Store" || name.starts_with("._")
    })
}

/// Re-pack `input` tar.gz, dropping macOS metadata entries. Preserves entry
/// types, permissions, and contents for everything else.
pub fn strip_macos_metadata(input: &[u8]) -> Result<Vec<u8>> {
    let cursor = Cursor::new(input);
    let decoder = GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(decoder);

    let out_buf: Vec<u8> = Vec::with_capacity(input.len());
    let encoder = GzEncoder::new(out_buf, Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder.follow_symlinks(false);

    let mut stripped = 0usize;
    let mut kept = 0usize;

    for entry in archive.entries().context("read tar entries")? {
        let mut entry = entry.context("read entry")?;
        let path = entry.path().context("read entry path")?.to_path_buf();
        if is_macos_metadata(&path) {
            stripped += 1;
            continue;
        }
        // Clone header so we can append a fresh entry. tar::Builder::append
        // wants Header + data; for directories/symlinks we copy header only.
        let mut header = entry.header().clone();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).context("read entry data")?;
        builder
            .append_data(&mut header, &path, Cursor::new(&data))
            .context("write entry to new tar")?;
        kept += 1;
    }

    let encoder = builder.into_inner().context("finalize tar")?;
    let buf = encoder.finish().context("finalize gz")?;
    log::info!(
        "[updater::sanitize] stripped {} macOS metadata entries, kept {}",
        stripped,
        kept
    );
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use std::io::Write;

    fn make_tar_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let buf: Vec<u8> = Vec::new();
        let enc = GzEncoder::new(buf, Compression::default());
        let mut b = tar::Builder::new(enc);
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).unwrap();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            b.append(&header, *data).unwrap();
        }
        let enc = b.into_inner().unwrap();
        enc.finish().unwrap()
    }

    fn list_entries(bytes: &[u8]) -> Vec<String> {
        let cursor = Cursor::new(bytes);
        let dec = GzDecoder::new(cursor);
        let mut arch = tar::Archive::new(dec);
        arch.entries()
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                e.path().unwrap().to_string_lossy().into_owned()
            })
            .collect()
    }

    #[test]
    fn strips_apple_double_files() {
        let input = make_tar_with(&[
            ("AIjia.app/Contents/Info.plist", b"<plist/>"),
            ("AIjia.app/Contents/._Info.plist", b"meta"),
            ("AIjia.app/._AIjia.app", b"top-level-meta"),
            ("AIjia.app/Contents/MacOS/aijia", b"binary"),
        ]);
        let output = strip_macos_metadata(&input).unwrap();
        let entries = list_entries(&output);
        assert!(entries.contains(&"AIjia.app/Contents/Info.plist".to_string()));
        assert!(entries.contains(&"AIjia.app/Contents/MacOS/aijia".to_string()));
        assert!(!entries.iter().any(|e| e.contains("._")));
    }

    #[test]
    fn strips_ds_store() {
        let input = make_tar_with(&[
            ("AIjia.app/Contents/.DS_Store", b"ds"),
            ("AIjia.app/Contents/Info.plist", b"<plist/>"),
        ]);
        let output = strip_macos_metadata(&input).unwrap();
        let entries = list_entries(&output);
        assert!(!entries.iter().any(|e| e.contains(".DS_Store")));
        assert!(entries.contains(&"AIjia.app/Contents/Info.plist".to_string()));
    }

    #[test]
    fn preserves_content_bytes() {
        let input = make_tar_with(&[("AIjia.app/Contents/Info.plist", b"original-content")]);
        let output = strip_macos_metadata(&input).unwrap();
        let cursor = Cursor::new(&output);
        let dec = GzDecoder::new(cursor);
        let mut arch = tar::Archive::new(dec);
        for entry in arch.entries().unwrap() {
            let mut e = entry.unwrap();
            let mut data = Vec::new();
            e.read_to_end(&mut data).unwrap();
            assert_eq!(data, b"original-content");
        }
    }
}

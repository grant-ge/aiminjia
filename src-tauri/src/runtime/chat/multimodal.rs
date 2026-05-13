use std::collections::HashSet;

use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::llm::streaming::{AnthropicContentBlock, AnthropicImageSource};
use crate::runtime::chat::chat_turn_driver::ChatAttachmentRef;

pub const MAX_ANTHROPIC_IMAGE_COUNT: usize = 4;
pub const MAX_ANTHROPIC_IMAGE_BYTES: u64 = 3 * 1024 * 1024;
pub const MAX_ANTHROPIC_IMAGE_BYTES_TOTAL: u64 = 6 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicImageBuildResult {
    pub image_blocks: Vec<AnthropicContentBlock>,
    pub converted_attachment_ids: HashSet<String>,
    pub degraded_attachment_ids: HashSet<String>,
    pub image_bytes_total: u64,
}

impl AnthropicImageBuildResult {
    pub fn empty() -> Self {
        Self {
            image_blocks: Vec::new(),
            converted_attachment_ids: HashSet::new(),
            degraded_attachment_ids: HashSet::new(),
            image_bytes_total: 0,
        }
    }
}

pub fn build_anthropic_image_blocks(
    attachments: &[ChatAttachmentRef],
) -> AnthropicImageBuildResult {
    let mut result = AnthropicImageBuildResult::empty();

    for attachment in attachments
        .iter()
        .filter(|attachment| is_image_attachment(attachment))
    {
        if result.image_blocks.len() >= MAX_ANTHROPIC_IMAGE_COUNT {
            result.degraded_attachment_ids.insert(attachment.id.clone());
            continue;
        }

        let Some(media_type) = normalized_image_mime(attachment) else {
            result.degraded_attachment_ids.insert(attachment.id.clone());
            continue;
        };

        let Ok(bytes) = std::fs::read(&attachment.file_path) else {
            result.degraded_attachment_ids.insert(attachment.id.clone());
            continue;
        };
        let byte_len = bytes.len() as u64;
        if byte_len == 0
            || byte_len > MAX_ANTHROPIC_IMAGE_BYTES
            || result.image_bytes_total + byte_len > MAX_ANTHROPIC_IMAGE_BYTES_TOTAL
        {
            result.degraded_attachment_ids.insert(attachment.id.clone());
            continue;
        }

        result.image_bytes_total += byte_len;
        result
            .converted_attachment_ids
            .insert(attachment.id.clone());
        result.image_blocks.push(AnthropicContentBlock::Image {
            source: AnthropicImageSource::Base64 {
                media_type,
                data: STANDARD.encode(bytes),
            },
        });
    }

    result
}

pub fn retain_text_fallback_attachments(
    attachments: &[ChatAttachmentRef],
    converted_attachment_ids: &HashSet<String>,
) -> Vec<ChatAttachmentRef> {
    attachments
        .iter()
        .filter(|attachment| !converted_attachment_ids.contains(&attachment.id))
        .cloned()
        .collect()
}

fn is_image_attachment(attachment: &ChatAttachmentRef) -> bool {
    attachment.file_type.eq_ignore_ascii_case("image")
        || attachment
            .mime_type
            .as_deref()
            .map(|mime| mime.to_ascii_lowercase().starts_with("image/"))
            .unwrap_or(false)
}

fn normalized_image_mime(attachment: &ChatAttachmentRef) -> Option<String> {
    if let Some(mime) = attachment.mime_type.as_deref() {
        if let Some(mapped) = map_mime(mime) {
            return Some(mapped);
        }
    }
    // Fallback: infer from file extension when frontend omitted mime_type.
    infer_mime_from_path(&attachment.file_path)
}

fn map_mime(mime: &str) -> Option<String> {
    let mime = mime.trim().to_ascii_lowercase();
    match mime.as_str() {
        "image/png" | "image/jpeg" | "image/webp" | "image/gif" => Some(mime),
        "image/jpg" => Some("image/jpeg".to_string()),
        _ => None,
    }
}

fn infer_mime_from_path(path: &str) -> Option<String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png".to_string()),
        "jpg" | "jpeg" => Some("image/jpeg".to_string()),
        "gif" => Some("image/gif".to_string()),
        "webp" => Some("image/webp".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn attachment(path: &std::path::Path, mime_type: Option<&str>) -> ChatAttachmentRef {
        ChatAttachmentRef {
            id: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("image")
                .to_string(),
            file_name: "image.png".to_string(),
            file_path: path.to_string_lossy().to_string(),
            kind: "file".to_string(),
            file_size: std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
            file_type: "image".to_string(),
            mime_type: mime_type.map(str::to_string),
        }
    }

    #[test]
    fn builds_anthropic_base64_image_blocks_without_data_url_prefix() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
        let attachment = attachment(file.path(), Some("image/png"));

        let result = build_anthropic_image_blocks(&[attachment.clone()]);

        assert_eq!(result.image_blocks.len(), 1);
        assert!(result.converted_attachment_ids.contains(&attachment.id));
        assert!(result.degraded_attachment_ids.is_empty());
        match &result.image_blocks[0] {
            AnthropicContentBlock::Image { source } => match source {
                AnthropicImageSource::Base64 { media_type, data } => {
                    assert_eq!(media_type, "image/png");
                    assert_eq!(data, "iVBORw0KGgo=");
                    assert!(!data.starts_with("data:"));
                }
            },
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn degrades_unsupported_mime_and_keeps_text_fallback() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello").unwrap();
        let attachment = attachment(file.path(), Some("text/plain"));

        let result = build_anthropic_image_blocks(&[attachment.clone()]);
        let fallback = retain_text_fallback_attachments(
            &[attachment.clone()],
            &result.converted_attachment_ids,
        );

        assert!(result.image_blocks.is_empty());
        assert!(result.degraded_attachment_ids.contains(&attachment.id));
        assert_eq!(fallback, vec![attachment]);
    }

    #[test]
    fn respects_image_count_limit() {
        let mut files = Vec::new();
        for _ in 0..(MAX_ANTHROPIC_IMAGE_COUNT + 1) {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
            files.push(file);
        }
        let attachments: Vec<_> = files
            .iter()
            .map(|file| attachment(file.path(), Some("image/png")))
            .collect();

        let result = build_anthropic_image_blocks(&attachments);

        assert_eq!(result.image_blocks.len(), MAX_ANTHROPIC_IMAGE_COUNT);
        assert_eq!(result.degraded_attachment_ids.len(), 1);
    }

    #[test]
    fn falls_back_to_path_extension_when_mime_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("screenshot.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
        let attachment = attachment(&path, None);

        let result = build_anthropic_image_blocks(&[attachment.clone()]);

        assert_eq!(result.image_blocks.len(), 1);
        assert!(result.converted_attachment_ids.contains(&attachment.id));
        assert!(result.degraded_attachment_ids.is_empty());
        match &result.image_blocks[0] {
            AnthropicContentBlock::Image { source } => match source {
                AnthropicImageSource::Base64 { media_type, .. } => {
                    assert_eq!(media_type, "image/png");
                }
            },
            other => panic!("expected image block, got {other:?}"),
        }
    }
}

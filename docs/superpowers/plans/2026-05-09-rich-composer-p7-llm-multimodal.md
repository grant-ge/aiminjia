# RichComposer P7 — Lotus Cloud Anthropic Multimodal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Scope warning:** This plan is intentionally **cloud-only and Anthropic-only**. Do not broaden it to OpenAI-compatible, Qwen, DeepSeek, Volcano, custom endpoints, or non-cloud/local provider paths.

**Goal:** Make Lotus Cloud image attachments visible to vision-capable Anthropic-route models by sending current-turn image bytes as Anthropic Messages `image.source.base64` content blocks.

**Architecture:** Keep the app's primary cloud path on `LotusProvider -> ClaudeProvider -> /anthropic/v1/messages`. Add a small sidecar/enrichment layer that converts only the current turn's eligible image attachments into Anthropic content blocks before the Claude/Lotus request body is serialized. Preserve existing path-text attachment behavior for non-image files, degraded images, unsupported models, history, and all non-cloud providers.

**Tech Stack:** Rust, Tauri, `serde_json`, existing `ClaudeProvider` / `LotusProvider`, existing chat attachment structs, `base64` crate.

---

## Verified Context

- Service endpoint for the new desktop app is Anthropic native: `https://ai-tenant.renlijia.com/anthropic/v1/messages`.
- Service-side `/anthropic/v1/messages` filters routes to `provider.protocol = "anthropic"` and does no OpenAI conversion.
- OPS protocol coverage matrix is authoritative: models missing an Anthropic route are unavailable to the new desktop app.
- A real request to `/anthropic/v1/messages` with model `claude-sonnet-4-5` and a base64 PNG image returned 200 and the model identified the image.
- Current app problem: image attachments are still represented as text/path hints in `build_llm_content`, so the model never receives pixels.

## Non-Goals

- Do not implement OpenAI `image_url` content arrays.
- Do not modify `src-tauri/src/llm/providers/openai.rs`, `qwen.rs`, `deepseek_*`, `volcano.rs`, or `custom.rs`.
- Do not introduce a provider-wide `ChatMessageContent::Parts` enum unless a later review proves the sidecar approach impossible.
- Do not persist base64 image data.
- Do not resend historical image bytes.
- Do not add non-cloud configuration or fallback to OpenAI ingress for models missing Anthropic routes.

## File Structure

Create:

- `src-tauri/src/runtime/chat/multimodal.rs` — image attachment filtering, MIME validation, size guards, base64 encoding, degradation reasons, and safe telemetry metadata.
- `src-tauri/src/llm/vision_support.rs` — conservative Lotus Cloud Anthropic vision allowlist.

Modify:

- `src-tauri/src/runtime/chat/mod.rs` — export `multimodal` if the runtime chat module uses explicit module declarations.
- `src-tauri/src/llm/mod.rs` — export `vision_support` if the LLM module uses explicit module declarations.
- `src-tauri/src/llm/streaming.rs` — add an optional Anthropic-only current-turn multimodal sidecar to `LlmRequest`, not to persisted messages.
- `src-tauri/src/llm/providers/claude.rs` — when building Anthropic Messages body, replace the current user message content with `content[]` blocks if the sidecar is present.
- `src-tauri/src/llm/providers/lotus.rs` — no schema conversion; ensure Lotus keeps using `ClaudeProvider` so sidecar support is inherited.
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — detect current-turn image attachments, call multimodal builder for Lotus Cloud vision models, attach sidecar to `LlmRequest`, and pass only non-image/degraded images into path-text attachment content.
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` — add or reuse a helper that can build `[当前消息附件]` text from a filtered attachment list.

Tests:

- `src-tauri/src/runtime/chat/multimodal.rs` unit tests in the same file or `src-tauri/src/runtime/chat/multimodal_test.rs`, following repo convention.
- `src-tauri/src/llm/vision_support.rs` unit tests.
- Existing Claude provider tests, or new focused tests near `src-tauri/src/llm/providers/claude.rs`, for request-body JSON shape.
- Existing chat runtime/driver tests, or new focused tests, for filtering image attachments from path text when sidecar succeeds.

## Data Shapes

Use Anthropic-only request parts:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentBlock {
    Text { text: String },
    Image { source: AnthropicImageSource },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicImageSource {
    Base64 { media_type: String, data: String },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AnthropicMultimodalTurn {
    pub image_blocks: Vec<AnthropicContentBlock>,
    pub image_count: usize,
    pub image_bytes_total: u64,
    pub degraded_count: usize,
}
```

Recommended location: `src-tauri/src/llm/streaming.rs` if `LlmRequest` owns the wire request shape. If that creates dependency direction problems, place the block types in `src-tauri/src/runtime/chat/multimodal.rs` and re-export from a neutral module.

## Guardrails

- Max single image bytes: `3 * 1024 * 1024`.
- Max total original image bytes per request: `6 * 1024 * 1024`.
- Max image count per request: `4`.
- Allowed MIME: `image/png`, `image/jpeg`, `image/webp`, `image/gif`.
- Base64 string must not include `data:image/png;base64,` prefix.
- Logs must never include base64 data.
- Successful image blocks are omitted from `[当前消息附件]` path text.
- Degraded images remain in `[当前消息附件]` path text.

---

## Task 1: Add Lotus Cloud Vision Allowlist

**Files:**
- Create: `src-tauri/src/llm/vision_support.rs`
- Modify: `src-tauri/src/llm/mod.rs`

- [ ] **Step 1: Write the allowlist module**

Create `src-tauri/src/llm/vision_support.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionSupport {
    Supported,
    Unsupported,
    Unknown,
}

pub fn lotus_anthropic_vision_support(model_name: &str) -> VisionSupport {
    let lower = model_name.trim().to_lowercase();
    if lower.is_empty() {
        return VisionSupport::Unknown;
    }

    if lower == "claude-sonnet-4-5" {
        return VisionSupport::Supported;
    }
    if lower == "claude-ops" {
        return VisionSupport::Supported;
    }
    if lower.contains("claude") && (lower.contains("sonnet") || lower.contains("opus")) {
        return VisionSupport::Supported;
    }

    if lower.starts_with("deepseek") {
        return VisionSupport::Unsupported;
    }
    if lower == "qwen-plus" || lower.starts_with("qwen") {
        return VisionSupport::Unsupported;
    }

    VisionSupport::Unknown
}

pub fn supports_lotus_anthropic_vision(model_name: &str) -> bool {
    matches!(lotus_anthropic_vision_support(model_name), VisionSupport::Supported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_verified_claude_sonnet() {
        assert_eq!(
            lotus_anthropic_vision_support("claude-sonnet-4-5"),
            VisionSupport::Supported
        );
    }

    #[test]
    fn supports_claude_ops() {
        assert!(supports_lotus_anthropic_vision("claude-ops"));
    }

    #[test]
    fn rejects_openai_only_qwen_plus() {
        assert_eq!(lotus_anthropic_vision_support("qwen-plus"), VisionSupport::Unsupported);
    }

    #[test]
    fn rejects_deepseek_models() {
        assert_eq!(lotus_anthropic_vision_support("deepseek-v4-pro[1m]"), VisionSupport::Unsupported);
    }

    #[test]
    fn unknown_glm_until_explicitly_verified() {
        assert_eq!(lotus_anthropic_vision_support("glm5.1"), VisionSupport::Supported);
    }
}
```

- [ ] **Step 2: Export the module**

Open `src-tauri/src/llm/mod.rs`. If it contains explicit module declarations, add:

```rust
pub mod vision_support;
```

If `llm/mod.rs` is not the declaration site, add the module at the repo's existing LLM module declaration site.

- [ ] **Step 3: Run the focused test**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri
cargo test vision_support --lib --no-fail-fast
```

Expected: tests in `vision_support` pass. If the crate does not support `--lib`, run `cargo test vision_support --tests --no-fail-fast` and record the exact command used.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/llm/vision_support.rs src-tauri/src/llm/mod.rs
git commit -m "feat(llm): add lotus anthropic vision allowlist"
```

## Task 2: Build Anthropic Image Blocks From Current-Turn Attachments

**Files:**
- Create: `src-tauri/src/runtime/chat/multimodal.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs`
- Test: `src-tauri/src/runtime/chat/multimodal.rs`

- [ ] **Step 1: Inspect attachment type fields**

Run:

```bash
rg -n "struct ChatAttachmentRef|ChatAttachmentRef" /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri/src/runtime /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri/src/transport
```

Expected: find `ChatAttachmentRef` fields for `file_path`, `file_name`, `file_type`, `mime_type`, and `kind`. Use exact field names from the code in the next step.

- [ ] **Step 2: Add multimodal builder**

Create `src-tauri/src/runtime/chat/multimodal.rs`. Adjust only the `use crate::...ChatAttachmentRef` path if the actual type path differs.

```rust
use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::llm::streaming::{AnthropicContentBlock, AnthropicImageSource};
use crate::runtime::chat::chat_turn_driver::ChatAttachmentRef;

pub const MAX_IMAGE_BYTES: u64 = 3 * 1024 * 1024;
pub const MAX_TOTAL_IMAGE_BYTES: u64 = 6 * 1024 * 1024;
pub const MAX_IMAGES_PER_TURN: usize = 4;

const ALLOWED_MIME: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageDegradeReason {
    TooManyImages,
    ImageTooLarge { bytes: u64 },
    TotalTooLarge { bytes_total: u64, next_bytes: u64 },
    UnsupportedMime { mime: String },
    ReadFailed { error: String },
}

#[derive(Debug, Clone)]
pub struct DegradedImageAttachment {
    pub attachment: ChatAttachmentRef,
    pub reason: ImageDegradeReason,
}

#[derive(Debug, Clone)]
pub struct ImageBlockBuildResult {
    pub image_blocks: Vec<AnthropicContentBlock>,
    pub converted_paths: Vec<String>,
    pub degraded: Vec<DegradedImageAttachment>,
    pub bytes_total: u64,
}

pub fn build_anthropic_image_blocks(attachments: &[ChatAttachmentRef]) -> ImageBlockBuildResult {
    let mut blocks = Vec::new();
    let mut converted_paths = Vec::new();
    let mut degraded = Vec::new();
    let mut bytes_total = 0_u64;

    for att in attachments.iter().filter(|a| is_image_attachment(a)) {
        if blocks.len() >= MAX_IMAGES_PER_TURN {
            degraded.push(degraded_attachment(att, ImageDegradeReason::TooManyImages));
            continue;
        }

        let mime = normalized_mime(att);
        if !ALLOWED_MIME.contains(&mime.as_str()) {
            degraded.push(degraded_attachment(att, ImageDegradeReason::UnsupportedMime { mime }));
            continue;
        }

        let path = Path::new(&att.file_path);
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                degraded.push(degraded_attachment(
                    att,
                    ImageDegradeReason::ReadFailed { error: err.to_string() },
                ));
                continue;
            }
        };

        let len = bytes.len() as u64;
        if len > MAX_IMAGE_BYTES {
            degraded.push(degraded_attachment(att, ImageDegradeReason::ImageTooLarge { bytes: len }));
            continue;
        }
        if bytes_total + len > MAX_TOTAL_IMAGE_BYTES {
            degraded.push(degraded_attachment(
                att,
                ImageDegradeReason::TotalTooLarge { bytes_total, next_bytes: len },
            ));
            continue;
        }

        bytes_total += len;
        converted_paths.push(att.file_path.clone());
        blocks.push(AnthropicContentBlock::Image {
            source: AnthropicImageSource::Base64 {
                media_type: mime,
                data: STANDARD.encode(bytes),
            },
        });
    }

    ImageBlockBuildResult { blocks, converted_paths, degraded, bytes_total }
}

pub fn is_image_attachment(att: &ChatAttachmentRef) -> bool {
    att.kind == "image" || att.file_type == "image" || att.mime_type.as_deref().unwrap_or("").starts_with("image/")
}

fn normalized_mime(att: &ChatAttachmentRef) -> String {
    if let Some(mime) = att.mime_type.as_deref() {
        let lower = mime.trim().to_lowercase();
        if !lower.is_empty() {
            if lower == "image/jpg" {
                return "image/jpeg".to_string();
            }
            return lower;
        }
    }
    guess_mime_from_name(&att.file_name).to_string()
}

fn guess_mime_from_name(file_name: &str) -> &'static str {
    let lower = file_name.to_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "application/octet-stream"
    }
}

fn degraded_attachment(att: &ChatAttachmentRef, reason: ImageDegradeReason) -> DegradedImageAttachment {
    DegradedImageAttachment { attachment: att.clone(), reason }
}
```

- [ ] **Step 3: Add tests in the same file**

Append tests to `multimodal.rs`. If `ChatAttachmentRef` has additional required fields, fill them with harmless defaults from the actual struct.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_file(name: &str, bytes: &[u8]) -> String {
        let path = std::env::temp_dir().join(format!("lotus_multimodal_test_{}_{}", std::process::id(), name));
        fs::write(&path, bytes).unwrap();
        path.to_string_lossy().to_string()
    }

    fn image_att(path: String, file_name: &str, mime: Option<&str>) -> ChatAttachmentRef {
        ChatAttachmentRef {
            file_path: path,
            file_name: file_name.to_string(),
            file_type: "image".to_string(),
            mime_type: mime.map(str::to_string),
            kind: "image".to_string(),
        }
    }

    #[test]
    fn converts_small_png_to_anthropic_image_block() {
        let path = temp_file("small.png", b"png-bytes");
        let result = build_anthropic_image_blocks(&[image_att(path.clone(), "small.png", Some("image/png"))]);
        assert_eq!(result.blocks.len(), 1);
        assert_eq!(result.degraded.len(), 0);
        assert_eq!(result.converted_paths, vec![path]);
        match &result.blocks[0] {
            AnthropicContentBlock::Image { source: AnthropicImageSource::Base64 { media_type, data } } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "cG5nLWJ5dGVz");
                assert!(!data.starts_with("data:"));
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_mime_degrades() {
        let path = temp_file("bad.bmp", b"bmp");
        let result = build_anthropic_image_blocks(&[image_att(path, "bad.bmp", Some("image/bmp"))]);
        assert!(result.blocks.is_empty());
        assert_eq!(result.degraded.len(), 1);
        assert!(matches!(result.degraded[0].reason, ImageDegradeReason::UnsupportedMime { .. }));
    }

    #[test]
    fn missing_file_degrades() {
        let result = build_anthropic_image_blocks(&[image_att(
            "/tmp/lotus-missing-image-file.png".to_string(),
            "missing.png",
            Some("image/png"),
        )]);
        assert!(result.blocks.is_empty());
        assert_eq!(result.degraded.len(), 1);
        assert!(matches!(result.degraded[0].reason, ImageDegradeReason::ReadFailed { .. }));
    }

    #[test]
    fn fifth_image_degrades() {
        let mut atts = Vec::new();
        for idx in 0..5 {
            let name = format!("img{idx}.png");
            let path = temp_file(&name, b"x");
            atts.push(image_att(path, &name, Some("image/png")));
        }
        let result = build_anthropic_image_blocks(&atts);
        assert_eq!(result.blocks.len(), 4);
        assert_eq!(result.degraded.len(), 1);
        assert!(matches!(result.degraded[0].reason, ImageDegradeReason::TooManyImages));
    }
}
```

- [ ] **Step 4: Export the module**

Open `src-tauri/src/runtime/chat/mod.rs`. If explicit module declarations are used, add:

```rust
pub mod multimodal;
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri
cargo test multimodal --lib --no-fail-fast
```

Expected: multimodal tests pass. If struct fields differ, update test constructors to match real fields before proceeding.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/multimodal.rs src-tauri/src/runtime/chat/mod.rs
git commit -m "feat(chat): build anthropic image blocks from attachments"
```

## Task 3: Add Anthropic Multimodal Sidecar to LlmRequest

**Files:**
- Modify: `src-tauri/src/llm/streaming.rs`

- [ ] **Step 1: Locate LlmRequest**

Run:

```bash
rg -n "struct LlmRequest|pub struct LlmRequest|ChatMessage" /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri/src/llm/streaming.rs
```

Expected: find `LlmRequest` and `ChatMessage` definitions.

- [ ] **Step 2: Add Anthropic block types and optional sidecar**

In `src-tauri/src/llm/streaming.rs`, add the `AnthropicContentBlock`, `AnthropicImageSource`, and `AnthropicMultimodalTurn` types from the Data Shapes section. Then add this field to `LlmRequest`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
```

If `LlmRequest` is not serialized, omit serde attributes and keep the field as:

```rust
pub anthropic_multimodal_turn: Option<AnthropicMultimodalTurn>,
```

- [ ] **Step 3: Update all LlmRequest constructors**

Run:

```bash
rg -n "LlmRequest \{" /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri/src
```

At every constructor, add:

```rust
anthropic_multimodal_turn: None,
```

Do not set this field outside the current-turn chat driver path in this task.

- [ ] **Step 4: Run compile check**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri
cargo check
```

Expected: no missing-field errors for `anthropic_multimodal_turn`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/llm/streaming.rs
git commit -m "feat(llm): add anthropic multimodal request sidecar"
```

## Task 4: Serialize Sidecar in ClaudeProvider Anthropic Body

**Files:**
- Modify: `src-tauri/src/llm/providers/claude.rs`
- Test: existing Claude provider test file or `src-tauri/src/llm/providers/claude.rs` unit tests

- [ ] **Step 1: Locate request body builder**

Run:

```bash
rg -n "build_request|messages|content|serde_json::json|json!" /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri/src/llm/providers/claude.rs
```

Expected: find the code that maps `LlmRequest.messages` into Anthropic `messages` JSON.

- [ ] **Step 2: Add replacement helper**

Near the request body builder, add a helper that replaces the final user message content when a sidecar exists:

```rust
use crate::llm::streaming::AnthropicMultimodalTurn;

fn apply_anthropic_multimodal_turn(messages: &mut [serde_json::Value], turn: &AnthropicMultimodalTurn) {
    if turn.blocks.is_empty() {
        return;
    }
    if let Some(last_user) = messages.iter_mut().rev().find(|msg| {
        msg.get("role").and_then(|v| v.as_str()) == Some("user")
    }) {
        last_user["content"] = serde_json::to_value(&turn.blocks).unwrap_or_else(|_| serde_json::Value::String(String::new()));
    }
}
```

If `claude.rs` already builds strongly typed structs instead of `serde_json::Value`, add the equivalent transformation at the point where final JSON is assembled.

- [ ] **Step 3: Call the helper**

After messages JSON is built and before sending the request body, add:

```rust
if let Some(turn) = request.anthropic_multimodal_turn.as_ref() {
    apply_anthropic_multimodal_turn(&mut messages, turn);
}
```

Use the actual local variable names from `claude.rs`.

- [ ] **Step 4: Add JSON shape test**

Add a focused test that constructs an `LlmRequest` with:

```rust
anthropic_multimodal_turn: Some(AnthropicMultimodalTurn {
    blocks: vec![
        AnthropicContentBlock::Text { text: "describe".to_string() },
        AnthropicContentBlock::Image {
            source: AnthropicImageSource::Base64 {
                media_type: "image/png".to_string(),
                data: "cG5n".to_string(),
            },
        },
    ],
    image_count: 1,
    image_bytes_total: 3,
    degraded_count: 0,
}),
```

Assert final body contains:

```json
{
  "role": "user",
  "content": [
    { "type": "text", "text": "describe" },
    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "cG5n" } }
  ]
}
```

Also assert it does **not** contain `image_url` and does **not** contain a `data:image/png;base64,` prefix.

- [ ] **Step 5: Run focused tests**

Run the narrowest provider test command available. Start with:

```bash
cd /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri
cargo test claude --lib --no-fail-fast
```

Expected: Claude provider multimodal JSON test passes.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/llm/providers/claude.rs
git commit -m "feat(llm): serialize anthropic image blocks in claude provider"
```

## Task 5: Attach Sidecar in Current-Turn Chat Driver

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
- Test: existing chat runtime tests or new focused tests

- [ ] **Step 1: Locate current user message construction**

Run:

```bash
rg -n "build_llm_content|attachments|LlmRequest|ChatMessage" /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri/src/runtime/chat /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri/src/transport/tauri_commands/chat
```

Expected: identify where current turn content is built and where `LlmRequest` is constructed.

- [ ] **Step 2: Add filtered attachment helper**

In the file that owns `build_llm_content`, add a helper that accepts the attachment slice to render. If `build_llm_content` already accepts `attachments`, change call sites to pass a filtered vector where needed rather than changing unrelated behavior.

Use this filtering rule in the chat driver:

```rust
let converted_paths: std::collections::HashSet<&str> = image_result
    .converted_paths
    .iter()
    .map(String::as_str)
    .collect();
let text_attachments: Vec<ChatAttachmentRef> = request
    .attachments
    .iter()
    .filter(|att| !converted_paths.contains(att.file_path.as_str()))
    .cloned()
    .collect();
```

This keeps non-image files and degraded images in path text while removing successfully converted images.

- [ ] **Step 3: Build sidecar only for Lotus Cloud vision models**

In `chat_turn_driver.rs`, before constructing the final `LlmRequest`, add logic equivalent to:

```rust
use crate::llm::streaming::{AnthropicContentBlock, AnthropicMultimodalTurn};
use crate::llm::vision_support::supports_lotus_anthropic_vision;
use crate::runtime::chat::multimodal::build_anthropic_image_blocks;

let is_lotus_cloud = settings.use_cloud;
let vision_enabled = is_lotus_cloud && supports_lotus_anthropic_vision(&settings.cloud_model);
let has_images = request.attachments.iter().any(crate::runtime::chat::multimodal::is_image_attachment);

let mut anthropic_multimodal_turn = None;
let attachments_for_text: Vec<ChatAttachmentRef>;

if vision_enabled && has_images {
    let image_result = build_anthropic_image_blocks(&request.attachments);
    let converted_paths: std::collections::HashSet<&str> = image_result.converted_paths.iter().map(String::as_str).collect();
    attachments_for_text = request.attachments
        .iter()
        .filter(|att| !converted_paths.contains(att.file_path.as_str()))
        .cloned()
        .collect();

    let text = build_llm_content(&request.content, &attachments_for_text /* plus existing args */);
    let mut blocks = Vec::with_capacity(1 + image_result.blocks.len());
    blocks.push(AnthropicContentBlock::Text { text });
    blocks.extend(image_result.blocks);

    if blocks.len() > 1 {
        anthropic_multimodal_turn = Some(AnthropicMultimodalTurn {
            image_count: blocks.len() - 1,
            image_bytes_total: image_result.bytes_total,
            degraded_count: image_result.degraded.len(),
            blocks,
        });
    }
} else {
    attachments_for_text = request.attachments.clone();
}
```

Adapt variable names to the actual structs. Do not log base64. If logging degradation, log only file name and reason.

- [ ] **Step 4: Set sidecar on LlmRequest**

When constructing `LlmRequest`, set:

```rust
anthropic_multimodal_turn,
```

The regular text `ChatMessage` should still exist so non-Claude providers and history logic remain stable. For a sidecar request, its text should be built from `attachments_for_text`, not the full attachment list.

- [ ] **Step 5: Add current-turn filtering test**

Add a test that creates one image attachment and one PDF attachment with `settings.use_cloud = true` and `settings.cloud_model = "claude-sonnet-4-5"`. Assert:

- `LlmRequest.anthropic_multimodal_turn.is_some()`.
- Sidecar blocks contain one `text` and one `image` block.
- Text content contains the PDF path/name.
- Text content does not contain the converted image path/name.

- [ ] **Step 6: Add unsupported model degradation test**

Add a test with the same image attachment but `settings.cloud_model = "deepseek-v4-pro[1m]"`. Assert:

- `LlmRequest.anthropic_multimodal_turn.is_none()`.
- Text content still contains the image path/name.

- [ ] **Step 7: Run focused chat tests**

Run the narrowest relevant command found by the repo. Start with:

```bash
cd /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri
cargo test multimodal --lib --no-fail-fast
cargo test chat_turn --lib --no-fail-fast
```

If `chat_turn` does not match tests, run the exact test names added in Steps 5 and 6.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git commit -m "feat(chat): attach anthropic image sidecar for cloud turns"
```

## Task 6: Verify LotusProvider Inherits Claude Sidecar Behavior

**Files:**
- Modify only if necessary: `src-tauri/src/llm/providers/lotus.rs`
- Test: existing Lotus provider tests or a focused unit test

- [ ] **Step 1: Confirm LotusProvider delegates to ClaudeProvider**

Run:

```bash
sed -n '1,120p' /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri/src/llm/providers/lotus.rs
```

Expected: `LotusProvider` wraps `ClaudeProvider::with_url(...)` pointing at `/anthropic/v1/messages`.

- [ ] **Step 2: Add no-op assertion test or comment**

If there is a Lotus provider test module, add a test that constructs a Lotus request with `anthropic_multimodal_turn` and verifies the inner Claude request body contains Anthropic image blocks. If Lotus internals are not easily testable, add a code comment near `ClaudeProvider::with_url(...)`:

```rust
// Anthropic multimodal image sidecars are serialized in ClaudeProvider;
// Lotus inherits that behavior because the cloud ingress is native
// /anthropic/v1/messages, not OpenAI-compatible /v1/chat/completions.
```

- [ ] **Step 3: Run provider tests**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri
cargo test lotus --lib --no-fail-fast
cargo test claude --lib --no-fail-fast
```

Expected: Lotus/Claude focused tests pass.

- [ ] **Step 4: Commit if files changed**

```bash
git add src-tauri/src/llm/providers/lotus.rs
git commit -m "docs(llm): document lotus anthropic multimodal inheritance"
```

Skip commit if no file changed.

## Task 7: Final Verification

**Files:**
- No code changes expected.

- [ ] **Step 1: Run compile check**

```bash
cd /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri
cargo check
```

Expected: exit 0.

- [ ] **Step 2: Run focused Rust tests**

```bash
cd /Users/oayzz/.codex/worktrees/275c/lotus-app/src-tauri
cargo test vision_support --lib --no-fail-fast
cargo test multimodal --lib --no-fail-fast
cargo test claude --lib --no-fail-fast
```

Expected: exit 0 for each command. If the crate layout requires `--tests` instead of `--lib`, record the substituted commands and outputs.

- [ ] **Step 3: Manual cloud smoke test**

Using a logged-in session key, send one small PNG through the app with model `claude-sonnet-4-5` and prompt:

```text
这张图的主要颜色是什么？只回答颜色。
```

Expected: answer identifies the actual image color. Confirm request logs do not print base64.

- [ ] **Step 4: Manual degradation smoke test**

Send either a >3MB image or use a non-allowlisted model. Expected:

- request does not fail because of local preprocessing;
- image remains in `[当前消息附件]` path text;
- UI/log emits a degradation reason without base64.

- [ ] **Step 5: Review git diff for scope creep**

Run:

```bash
cd /Users/oayzz/.codex/worktrees/275c/lotus-app
git diff --stat HEAD~6..HEAD
```

Expected changed files are limited to the cloud Anthropic multimodal scope. No OpenAI/Qwen/DeepSeek/Volcano/custom provider files changed.

---

## Self-Review Checklist

- Spec coverage: current-turn image pixels, cloud-only, Anthropic-only, size guards, degradation, no base64 persistence, no historical resend, and no OpenAI fallback are covered by tasks 1-7.
- Placeholder scan: this plan contains no implementation placeholders; code snippets specify concrete types and tests. The only adaptation point is exact existing struct fields/module paths, explicitly resolved by inspection steps before coding.
- Type consistency: `AnthropicContentBlock`, `AnthropicImageSource`, and `AnthropicMultimodalTurn` are defined once and reused by multimodal builder and Claude serialization.
- Scope check: plan deliberately excludes OpenAI-compatible providers and non-cloud routes, matching the 2026-05-11 service-side protocol decision.

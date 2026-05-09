# RichComposer P7 — LLM Gateway Multimodal 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.
>
> **⚠️ Risk note:** This plan touches the LLM provider serialization layer (~3k LOC across 5 providers). Subtle bugs silently break chat for every model. **Recommend manual code review + at least one real provider integration test before merge.** Do not skip the spec/quality reviewers.

**Goal:** Add a structured `Parts` content channel to `ChatMessage` so vision-capable models receive base64 image content for the current turn's image attachments. Models without vision support fall through to the existing path-text fallback (zero behavior change).

**Architecture:** `ChatMessage.content` becomes `enum ChatMessageContent { Text(String), Parts(Vec<ChatMessagePart>) }`. The `Text` variant remains the default form, preserving backward compatibility for all assistant / tool / system / history messages. Only the **current** user turn's `ChatMessage` may use `Parts` when (a) it has image attachments AND (b) the routed model `supports_vision()`. Each provider gains a `serialize_parts()` arm that emits the provider-native vision schema. A new `vision_support.rs` module ships a model→capability table.

**Tech Stack:** Rust (`serde`, `serde_json`), `base64` crate (already used elsewhere), existing provider modules.

---

## 文件结构

新增：
- `src-tauri/src/llm/vision_support.rs` — `supports_vision(model_name) -> Option<bool>` lookup table.
- `src-tauri/src/runtime/chat/multimodal.rs` — converts `ChatAttachmentRef` (image kind) to base64 part with size/format guards.

修改：
- `src-tauri/src/llm/streaming.rs` — extend `ChatMessage.content` to support `Parts` variant.
- `src-tauri/src/llm/providers/openai.rs` — emit OpenAI-compatible vision content array.
- `src-tauri/src/llm/providers/claude.rs` — emit Anthropic vision blocks.
- `src-tauri/src/llm/providers/qwen.rs` — emit OpenAI-compatible (qwen-vl uses OpenAI schema).
- `src-tauri/src/llm/providers/lotus.rs` — emit OpenAI-compatible (lotus is OpenAI proxy).
- `src-tauri/src/llm/providers/volcano.rs` — TBD: check whether it supports vision; if not, skip this provider.
- `src-tauri/src/llm/providers/deepseek_v3.rs` / `deepseek_r1.rs` — DeepSeek does not currently support vision; skip.
- `src-tauri/src/llm/providers/custom.rs` — emit OpenAI-compatible.
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — branch on `supports_vision` when constructing user message; build parts array including base64 images.
- `src-tauri/src/runtime/chat/history.rs` — historical messages always use `Text` (do not re-emit Parts; spec).
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs::build_llm_content` — when called for a vision-capable model, omit image entries from the `[当前消息附件]` text list.

不修改：
- `StoredMessage` schema — base64 data NOT persisted.
- 前端任何文件 — payload shape unchanged.

## 关键决策

### `ChatMessageContent` 设计

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatMessagePart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ChatMessagePart {
    Text { text: String },
    Image { source: ImageSource },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

impl Default for ChatMessageContent {
    fn default() -> Self {
        ChatMessageContent::Text(String::new())
    }
}
```

`#[serde(untagged)]` lets `Text("hello")` serialize as a plain string and deserialize from either string OR object array — keeps the wire format compatible with existing JSON.

`ChatMessage.content: ChatMessageContent`（之前是 `String`）。所有现有的 `m.content.clone()` / `m.content.is_empty()` 调用点要更新到 `match` 分支。

#### 兼容性 Helper

```rust
impl ChatMessage {
    pub fn text_content(&self) -> &str {
        match &self.content {
            ChatMessageContent::Text(s) => s,
            ChatMessageContent::Parts(parts) => parts.iter().find_map(|p| {
                if let ChatMessagePart::Text { text } = p { Some(text.as_str()) } else { None }
            }).unwrap_or(""),
        }
    }

    pub fn is_empty_content(&self) -> bool {
        match &self.content {
            ChatMessageContent::Text(s) => s.is_empty(),
            ChatMessageContent::Parts(p) => p.is_empty(),
        }
    }
}
```

调用点用这两个 helper 替换 `.content` 直接 string 操作。

### `vision_support.rs` 表

按 spec 三态：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionSupport {
    Supported,
    Unsupported,
    Unknown,
}

pub fn vision_support(model_name: &str) -> VisionSupport {
    let lower = model_name.to_lowercase();
    // Anthropic vision-capable models
    if lower.contains("claude-3") || lower.contains("claude-sonnet") || lower.contains("claude-opus") {
        return VisionSupport::Supported;
    }
    // OpenAI / GPT-5
    if lower.starts_with("gpt-4o") || lower.starts_with("gpt-5") || lower.contains("gpt-4-vision") {
        return VisionSupport::Supported;
    }
    // Gemini
    if lower.starts_with("gemini-1.5") || lower.starts_with("gemini-2") {
        return VisionSupport::Supported;
    }
    // Qwen-VL
    if lower.contains("qwen-vl") || lower.contains("qwen3-vl") {
        return VisionSupport::Supported;
    }
    // GLM-4V
    if lower.contains("glm-4v") || lower.contains("glm-4.5v") {
        return VisionSupport::Supported;
    }
    // DeepSeek (no vision support as of 2026-05)
    if lower.starts_with("deepseek") {
        return VisionSupport::Unsupported;
    }
    // Plain GPT-4 / GPT-3.5 — no vision
    if lower.starts_with("gpt-3") || lower == "gpt-4" || lower.starts_with("gpt-4-") {
        return VisionSupport::Unsupported;
    }
    // Plain Qwen / GLM (non-VL variants) — no vision
    if lower.starts_with("qwen") || lower.starts_with("glm-") {
        return VisionSupport::Unsupported;
    }
    log::warn!("vision_support: unknown model '{}', defaulting to Unsupported", model_name);
    VisionSupport::Unknown
}

pub fn supports_vision(model_name: &str) -> bool {
    matches!(vision_support(model_name), VisionSupport::Supported)
}
```

### 约束（spec 决策）

- 单图 ≤ 5MB。
- 单条消息 ≤ 10 张图。
- 格式白名单：`image/png`、`image/jpeg`、`image/webp`、`image/gif`。
- 超限/非白名单/读盘失败 → 该图静默降级回 path（保留 image 名出现在 `[当前消息附件]` 列表里），toast 单独通知。
- 历史消息永不重新生成 Parts。
- base64 不持久化。

### `multimodal.rs` 模块

```rust
use std::path::Path;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use crate::llm::streaming::{ChatMessagePart, ImageSource};
use crate::runtime::chat::chat_turn_driver::ChatAttachmentRef;

const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_IMAGES_PER_TURN: usize = 10;

const ALLOWED_MIME: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

pub struct ImagePartBuildResult {
    pub parts: Vec<ChatMessagePart>,
    pub degraded_attachments: Vec<ChatAttachmentRef>,
    pub bytes_total: u64,
}

pub fn build_image_parts(attachments: &[ChatAttachmentRef]) -> ImagePartBuildResult {
    let mut parts = Vec::new();
    let mut degraded = Vec::new();
    let mut bytes_total: u64 = 0;
    for att in attachments.iter().filter(|a| a.kind == "image") {
        if parts.len() >= MAX_IMAGES_PER_TURN {
            degraded.push(att.clone());
            continue;
        }
        let mime = att.mime_type.as_deref().unwrap_or_else(|| guess_mime(&att.file_name));
        if !ALLOWED_MIME.contains(&mime) {
            log::warn!("multimodal: skipping {} (unsupported mime '{}')", att.file_name, mime);
            degraded.push(att.clone());
            continue;
        }
        let path = Path::new(&att.file_path);
        match std::fs::read(path) {
            Ok(bytes) => {
                let len = bytes.len() as u64;
                if len > MAX_IMAGE_BYTES {
                    log::warn!("multimodal: skipping {} ({} bytes > 5MB)", att.file_name, len);
                    degraded.push(att.clone());
                    continue;
                }
                bytes_total += len;
                let data = STANDARD.encode(&bytes);
                parts.push(ChatMessagePart::Image {
                    source: ImageSource::Base64 {
                        media_type: mime.to_string(),
                        data,
                    },
                });
            }
            Err(e) => {
                log::warn!("multimodal: failed to read {}: {}", att.file_path, e);
                degraded.push(att.clone());
            }
        }
    }
    ImagePartBuildResult { parts, degraded_attachments: degraded, bytes_total }
}

fn guess_mime(file_name: &str) -> &'static str {
    let lower = file_name.to_lowercase();
    if lower.ends_with(".png") { "image/png" }
    else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") { "image/jpeg" }
    else if lower.ends_with(".webp") { "image/webp" }
    else if lower.ends_with(".gif") { "image/gif" }
    else { "application/octet-stream" }
}
```

### `chat_turn_driver` 接入点

构造 user message 时分支：

```rust
let routed_model = /* current routing */;
let has_images = attachments.iter().any(|a| a.kind == "image");

let user_msg = if has_images && supports_vision(&routed_model.model_name) {
    let result = build_image_parts(&attachments);
    // text content excludes image attachments from [当前消息附件] list
    let text_part = build_llm_content(content, &non_image_attachments_plus_degraded(...), ...);
    let mut parts = vec![ChatMessagePart::Text { text: text_part }];
    parts.extend(result.parts);
    if !result.degraded_attachments.is_empty() {
        // emit toast/log indicating which images were degraded
    }
    ChatMessage { content: ChatMessageContent::Parts(parts), ... }
} else {
    let text = build_llm_content(content, &attachments, ...);
    ChatMessage { content: ChatMessageContent::Text(text), ... }
};
```

### 各 provider 的 `serialize_parts()`

#### OpenAI / Qwen / Lotus / Custom (OpenAI-compatible)

```rust
fn parts_to_openai_content(parts: &[ChatMessagePart]) -> Vec<Value> {
    parts.iter().map(|p| match p {
        ChatMessagePart::Text { text } => json!({ "type": "text", "text": text }),
        ChatMessagePart::Image { source: ImageSource::Base64 { media_type, data } } => {
            json!({
                "type": "image_url",
                "image_url": { "url": format!("data:{};base64,{}", media_type, data) }
            })
        }
        ChatMessagePart::Image { source: ImageSource::Url { url } } => {
            json!({ "type": "image_url", "image_url": { "url": url } })
        }
    }).collect()
}
```

In `messages` build:

```rust
let content_value = match &m.content {
    ChatMessageContent::Text(s) => json!(s),
    ChatMessageContent::Parts(parts) => json!(parts_to_openai_content(parts)),
};
let mut msg = json!({ "role": m.role, "content": content_value });
```

#### Claude

```rust
fn parts_to_anthropic_content(parts: &[ChatMessagePart]) -> Vec<Value> {
    parts.iter().map(|p| match p {
        ChatMessagePart::Text { text } => json!({ "type": "text", "text": text }),
        ChatMessagePart::Image { source: ImageSource::Base64 { media_type, data } } => {
            json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": data,
                }
            })
        }
        ChatMessagePart::Image { source: ImageSource::Url { url } } => {
            json!({ "type": "image", "source": { "type": "url", "url": url } })
        }
    }).collect()
}
```

#### Gemini (lotus 内可能有 gemini route)

If `lotus.rs` proxies Gemini, structure is:

```json
{ "parts": [{ "text": "..." }, { "inline_data": { "mime_type": "image/png", "data": "<base64>" } }] }
```

Skip Gemini if not currently routed via lotus.

## 测试覆盖

### `vision_support_test.rs`

- claude-3-opus → Supported
- claude-sonnet-4 → Supported
- gpt-4o → Supported
- gpt-5 → Supported
- gemini-1.5-pro → Supported
- qwen-vl → Supported
- glm-4v → Supported
- deepseek-chat → Unsupported
- gpt-4 → Unsupported
- gpt-3.5-turbo → Unsupported
- qwen-max → Unsupported
- 未知模型 → Unsupported (log warn)

### `multimodal_test.rs`

- 普通图 (< 5MB) → 进 parts，bytes_total 累计
- 超大图 (> 5MB) → degraded
- 非白名单 mime → degraded
- 不存在的文件 → degraded
- 11 张图 → 前 10 进 parts，最后 1 张 degraded
- 0 张图 → empty parts
- mixed (image + non-image) → only image kind processed

### Provider serialize tests

每个 provider 加一个 round-trip：构造一个 `ChatMessage::Parts(text + image)`，调用 build_request_body，断言 JSON 输出符合 provider 自己的 schema。

### `chat_turn_driver` integration

- vision model + image attachment → ChatMessage with Parts
- non-vision model + image attachment → ChatMessage with Text (含 path)
- vision model + no image attachment → ChatMessage with Text
- vision model + image-only message → Parts 含 text 和 image

### `history_test.rs`

- 历史消息 (含 file 字段) → 永远生成 Text 形态，从不读盘 + base64

## 实施分期（10 个 task）

1. **streaming.rs ChatMessageContent enum** + helpers + 兼容反序列化 + 单测。
   - 这是 cross-cutting change；先做并跑全 build，确保所有现有 string-based access points 用 helper 替换。
2. **vision_support.rs** + 单测。
3. **multimodal.rs** + 单测。
4. **chat_turn_driver 接入** — 分支构造 user message + 单测。
5. **history.rs** — 确保历史不重构 Parts + 单测。
6. **build_llm_content** — vision 路径下 image attachment 不出现在 text 提示。
7. **OpenAI provider** — Parts → image_url + 单测。
8. **Claude provider** — Parts → image block + 单测。
9. **Qwen / Lotus / Custom providers** — OpenAI-compatible Parts + 单测。
10. **完整集成验证** — `cargo test review_` 全过；至少手动跑一次 vision-capable 模型的真请求确认能识图。

## 风险 / 应对

- **content 字段反序列化**：`#[serde(untagged)]` 让 `String` 和 `Vec<Part>` 都能反序列化进。但**老 stored data 里 `content` 是 string**，读回来不走 Parts 分支，OK。
- **Provider 漏改**：搜索所有 `m.content.clone()` / `&m.content` / `.content.is_empty()` 调用点，全部用 helper。  
- **base64 编码慢**：10 张 50MB 在主线程同步编码。可接受；后续可放到 tokio blocking 线程。
- **Image attachment 持久化路径稳定性**：`tmpImage/` 下的图，turn 完成前不能被清理。检查 `python/sandbox.rs` 的 cleanup 时机。
- **错误处理**：build_image_parts 里某个图 read 失败，整条消息仍发；degraded list 用于事后 toast。

## 验证

- [ ] `cd src-tauri && cargo test review_ --tests --no-fail-fast` — 0 failures
- [ ] `cd src-tauri && cargo test --tests` — 0 failures（注意不是仅 review_）
- [ ] 手动连一个支持视觉的模型（Claude Sonnet / GPT-4o），上传一张图 → 模型能描述图内容
- [ ] 手动连一个不支持视觉的模型（DeepSeek），上传一张图 → 模型走原 path 提示路径，不报错

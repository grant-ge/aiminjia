# Phase 5 PR1-2：Wechat 基础设施（types + crypto）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Phase 5 wechat connector 搭好"骨架 + 数据类型 + crypto 基础"，让后续 PR3-7 能直接接上写业务。PR1 建 `connector/im/wechat/` 目录、定义 7 个 iLink endpoint 常量 + `build_headers` 集中函数 + 两套媒体枚举（`UploadMediaType` vs `MessageItemType`）+ `WechatConnector` 空壳实现 `IMConnector`；PR2 单独做 `crypto.rs`（AES-128-ECB + PKCS#7 + 圣经 fixture 单测）。

**Architecture:** 沿用 Phase 1 飞书 connector 目录结构。`connector/im/wechat/` 跟 `dingtalk/` / `feishu/` 平级。所有 HTTP / WS / 媒体加解密都自己写，不引第三方 wechat lib。Crypto 用 `aes` crate（启 `ecb` feature）+ `block-padding` 处理 PKCS#7。**PR1 不接业务逻辑**：`start()` 立刻返回 empty stream，`send()` 返 `NotSupported` —— 把它当作"占位 connector"先合进 main 让 review_im_layering 测试能跑过，PR3-7 再逐步填充。

**Tech Stack:** Rust async (tokio + tokio-util), reqwest 0.12, async-trait, serde / serde_json, **新增 `aes = "0.8"` 启用 `ecb` feature + `cbc = "0.1"`** (cbc 提供 `BlockEncryptMut`/`BlockDecryptMut` traits 也覆盖 ECB), `block-padding = "0.3"` (PKCS#7), thiserror, hex, base64。无前端依赖改动。

**Prerequisites:** 以下 PR 必须先合进 main 且在生产稳定（详见 spec §依赖关系）：
- **Phase 1 PR0d**（feishu plan）：`ReplyTarget` 平台中性化 + `ChannelConfigStore::platform_dir(Platform::Wechat)` 支持 + **`observe_session` trait 方法** + **`MarkdownSupport` 枚举**
- **Phase 2 PR3**：`shared/aicard_fallback.rs` 存在
- **Phase 3 PR1.5**：`InboundDeployment` 重命名（删除 `NativeDaemon`）+ `outbound_text_streaming` 字段；`ConnectorError::SessionExpired` 变体（替代或并存 `AuthExpired`）；推荐 PR6.5 `SecretString` 也先合
- **Phase 4 PR3**：`AiCardFallbackBuffer::new_no_placeholder()` 构造器
- **Phase 5 PR0**：`RegistrationModal` 共抽（PR1-2 不直接用，但 PR3 会用，本 plan 也假设它已合）

`Platform::Wechat` 枚举 + `ChannelConfigStore::platform_dir(Platform::Wechat)` 在 Phase 0 / Phase 1 已就绪，本 plan 不再单独建。

**参考**：spec `docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md` §1.3 / §1.5 / §2 / §4 / §4.0。openclaw 实测参考：`/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main/src/api/api.ts` + `src/cdn/aes-ecb.ts`。

---

## File Structure

```
src-tauri/src/connector/im/wechat/                  ← 新增整个目录
├── mod.rs                                          ← pub mod 子模块 + Re-export
├── connector.rs                                    ← impl IMConnector for WechatConnector（PR1 空壳）
├── types.rs                                        ← UploadMediaType / MessageItemType / WeixinMessage / 等 wire types
├── endpoints.rs                                    ← 7 个 endpoint 路径常量 + base URL 常量
├── headers.rs                                      ← build_headers() 集中函数（PR1.3）
├── appid.rs                                        ← load_ilink_app_id() —— 配置驱动 + 默认 openclaw fallback
└── crypto.rs                                       ← PR2：AES-128-ECB encrypt/decrypt/padded_size

src-tauri/src/connector/im/mod.rs                   ← 加 pub mod wechat
src-tauri/src/connector/im/factory.rs               ← 加 build_wechat_connector（PR1 末尾）
src-tauri/src/connector/im/types.rs                 ← 不动（Platform::Wechat 已存在）

src-tauri/Cargo.toml                                ← PR2 加 aes + cbc + block-padding + hex
src-tauri/tests/review_im_layering.rs               ← PR1 末尾：platforms 数组追加 "wechat"
```

**核心责任划分**：
- `endpoints.rs`：7 个常量字符串，唯一真相源
- `headers.rs`：`build_headers(token: Option<&str>, body: &str) -> HeaderMap` —— 所有 endpoint 的请求头由此生成
- `appid.rs`：`load_ilink_app_id(&AiJiaConfig) -> String` —— `wechat.ilink_app_id` 字段优先，未配置时用编译常量 `DEFAULT_OPENCLAW_APP_ID`
- `types.rs`：跟 openclaw `api/types.ts` 一一对应的 Rust 镜像类型；两套媒体枚举强类型分开 + 显式转换函数
- `crypto.rs`：3 个纯函数 (`encrypt_ecb` / `decrypt_ecb` / `padded_size`) + 圣经 fixture 单测
- `connector.rs`：`WechatConnector::new()` + `impl IMConnector` 占位（PR3 起填充 start/send）

---

# PR1: Wechat 骨架 + types + headers + appid + capabilities

## §0 前置准备

- [ ] **Step 0.1: 确认前置 PR 已合入 main**

Run: `git -C /Users/oayzz/project/lotus/lotus-workbench/lotus-app log --oneline main -30 | grep -E "PR0d|aicard_fallback|InboundDeployment|new_no_placeholder|observe_session|MarkdownSupport"`
Expected: 至少看到 PR0d / Phase 2 PR3 / Phase 3 PR1.5 / Phase 4 PR3 各 1 条 commit。若缺，**停下来**先让 Phase 1-4 完成。

- [ ] **Step 0.2: 确认 trait_def 已升级**

Run: `grep -n "InboundDeployment\|observe_session\|MarkdownSupport\|outbound_text_streaming\|SessionExpired" src-tauri/src/connector/im/trait_def.rs | head -20`
Expected: 5 个关键词都能找到。若 `InboundModel` 仍在 / `observe_session` 缺失 / `MarkdownSupport` 缺失，回 Phase 1-3 plan 补完。

- [ ] **Step 0.3: 确认 Platform::Wechat + platform_dir 已就绪**

Run: `grep -n "Platform::Wechat" src-tauri/src/connector/im/types.rs src-tauri/src/connector/im/shared/config_store.rs | head -5`
Expected: types.rs 里有 enum 变体；config_store.rs `platform_dir` / `platform_config_path` 已经覆盖 Wechat（Phase 1 PR0d 已做）。

- [ ] **Step 0.4: 起点干净**

Run: `git status -s src-tauri/src/connector/im src-tauri/Cargo.toml`
Expected: 空输出，没有未提交的 connector/im 改动。

---

## Task 1: 创建 `wechat/` 模块骨架 + 加入 `connector/im/mod.rs`

**Files:**
- Create: `src-tauri/src/connector/im/wechat/mod.rs`
- Create: `src-tauri/src/connector/im/wechat/types.rs`
- Modify: `src-tauri/src/connector/im/mod.rs`

- [ ] **Step 1.1: 写失败的编译测试（添加 mod 声明）**

修改 `src-tauri/src/connector/im/mod.rs`，在现有 mod 声明列表里追加：

```rust
pub mod wechat;
```

通常在 `pub mod feishu;` 后面。

- [ ] **Step 1.2: 编译验证失败**

Run: `cd src-tauri && cargo check 2>&1 | head -30`
Expected: FAIL —— `error[E0583]: file not found for module 'wechat'`

- [ ] **Step 1.3: 创建空 `wechat/mod.rs`**

Create `src-tauri/src/connector/im/wechat/mod.rs`:

```rust
//! WeChat (iLink HTTP API) connector.
//!
//! See `docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md` for the
//! protocol research and overall design. Implementation is split across
//! `connector.rs` (IMConnector impl), `types.rs` (wire types), `endpoints.rs`
//! (path constants), `headers.rs` (request-header builder), `appid.rs`
//! (iLink-App-Id source), and `crypto.rs` (AES-128-ECB for media).

pub mod appid;
pub mod connector;
pub mod endpoints;
pub mod headers;
pub mod types;

// Re-export the public surface; matches the `feishu::` re-export pattern.
pub use connector::WechatConnector;
```

- [ ] **Step 1.4: 创建空 `types.rs`**

Create `src-tauri/src/connector/im/wechat/types.rs`:

```rust
//! iLink wire types — mirrors openclaw-weixin-main/src/api/types.ts.
//!
//! Bytes fields are base64 strings in JSON (per protocol). Two media-type
//! enums are deliberately separate; see §4.0 of the spec for the rationale.
```

（接下来 Task 2 填内容。）

- [ ] **Step 1.5: 创建占位 `connector.rs` / `endpoints.rs` / `headers.rs` / `appid.rs`**

仅写文件头注释让 `cargo check` 过；内容后续 task 填。

Create `src-tauri/src/connector/im/wechat/connector.rs`:

```rust
//! `WechatConnector` — `IMConnector` implementation for iLink HTTP API.
//!
//! PR1: skeleton. `start()` returns an empty stream; `send()` returns
//! `NotSupported`. PR3-7 fill in actual behaviour.
```

Create `src-tauri/src/connector/im/wechat/endpoints.rs`:

```rust
//! iLink endpoint paths. Single source of truth.
```

Create `src-tauri/src/connector/im/wechat/headers.rs`:

```rust
//! Common request-header builder for all iLink API calls.
//!
//! See spec §1.3 for the mandatory header list; every endpoint must use
//! `build_headers()` so we never accidentally ship a request missing
//! `iLink-App-Id` or `X-WECHAT-UIN`.
```

Create `src-tauri/src/connector/im/wechat/appid.rs`:

```rust
//! iLink-App-Id source. See spec §1.5 for the openclaw-fallback decision.
```

- [ ] **Step 1.6: 编译通过**

Run: `cd src-tauri && cargo check`
Expected: PASS（可能 warning：`module ... is never used` 之类，可暂时容忍）。

- [ ] **Step 1.7: 提交**

```bash
git add src-tauri/src/connector/im/wechat src-tauri/src/connector/im/mod.rs
git commit -m "feat(connector/im/wechat): scaffold module structure (Phase 5 PR1 step 1)"
```

---

## Task 2: `types.rs` —— UploadMediaType + MessageItemType + 两套枚举的转换

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/types.rs`

- [ ] **Step 2.1: 写失败的单测（两套媒体枚举不可强转）**

替换 `types.rs` 内容（先只填 enum + 单测，使其 FAIL）：

```rust
//! iLink wire types — mirrors openclaw-weixin-main/src/api/types.ts.
//!
//! Bytes fields are base64 strings in JSON (per protocol). Two media-type
//! enums are deliberately separate; see §4.0 of the spec for the rationale.

use serde::{Deserialize, Serialize};

/// Used in `getUploadUrl` request body's `media_type` field.
///
/// **Numerical values intentionally different from `MessageItemType`** —
/// see spec §4.0. `as i32` strong-cast between these two enums is forbidden;
/// use `upload_type_from_item_type` for explicit lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum UploadMediaType {
    Image = 1,
    Video = 2,
    File = 3,
    Voice = 4,
}

/// Used in `MessageItem.type` field of every inbound and outbound message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageItemType {
    Text = 1,
    Image = 2,
    Voice = 3,
    File = 4,
    Video = 5,
}

/// Explicit lookup from message item type to upload media type. Returns
/// `None` for Text (text doesn't go through CDN upload).
pub fn upload_type_from_item_type(t: MessageItemType) -> Option<UploadMediaType> {
    match t {
        MessageItemType::Text => None,
        MessageItemType::Image => Some(UploadMediaType::Image),
        MessageItemType::Voice => Some(UploadMediaType::Voice),
        MessageItemType::File => Some(UploadMediaType::File),
        MessageItemType::Video => Some(UploadMediaType::Video),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_type_from_item_type_lookup_table() {
        assert_eq!(upload_type_from_item_type(MessageItemType::Text), None);
        assert_eq!(
            upload_type_from_item_type(MessageItemType::Image),
            Some(UploadMediaType::Image),
        );
        assert_eq!(
            upload_type_from_item_type(MessageItemType::Voice),
            Some(UploadMediaType::Voice),
        );
        assert_eq!(
            upload_type_from_item_type(MessageItemType::File),
            Some(UploadMediaType::File),
        );
        assert_eq!(
            upload_type_from_item_type(MessageItemType::Video),
            Some(UploadMediaType::Video),
        );
    }

    #[test]
    fn numeric_repr_differs_between_image_video_file() {
        // Spec §4.0: same concept, different numbers across the two enums.
        // Image:    Upload=1, Item=2
        // Video:    Upload=2, Item=5
        // File:     Upload=3, Item=4
        // Voice:    Upload=4, Item=3
        assert_eq!(UploadMediaType::Image as u8, 1);
        assert_eq!(MessageItemType::Image as u8, 2);

        assert_eq!(UploadMediaType::Video as u8, 2);
        assert_eq!(MessageItemType::Video as u8, 5);

        assert_eq!(UploadMediaType::File as u8, 3);
        assert_eq!(MessageItemType::File as u8, 4);

        assert_eq!(UploadMediaType::Voice as u8, 4);
        assert_eq!(MessageItemType::Voice as u8, 3);
    }

    #[test]
    fn serde_json_serializes_as_integer() {
        // The wire format uses raw integers, not enum names.
        let j = serde_json::to_string(&UploadMediaType::Image).unwrap();
        assert_eq!(j, "1");

        let j = serde_json::to_string(&MessageItemType::Voice).unwrap();
        assert_eq!(j, "3");

        // Round-trip
        let parsed: UploadMediaType = serde_json::from_str("4").unwrap();
        assert_eq!(parsed, UploadMediaType::Voice);
    }
}
```

注意：`#[repr(u8)]` + `serde` 默认会序列化成"variant name"字符串，不是数字。要走数字 wire format，需要给 enum 加 `#[serde(into = "u8", try_from = "u8")]` 之类的转换，或者用 `serde_repr` crate。

- [ ] **Step 2.2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::types::tests --no-fail-fast`
Expected: 第三个 case `serde_json_serializes_as_integer` FAIL —— 序列化为 `"Image"` 字符串而非 `1`。

- [ ] **Step 2.3: 加 `serde_repr` 依赖（或手写 conversion）**

检查是否已有 `serde_repr`：

Run: `grep "serde_repr" src-tauri/Cargo.toml`

如果没有，加到 `[dependencies]`:

```toml
serde_repr = "0.1"
```

修改 `types.rs` 顶部 imports：

```rust
use serde_repr::{Deserialize_repr, Serialize_repr};
use serde::{Deserialize, Serialize};
```

把两个 enum 的 `#[derive(... Serialize, Deserialize)]` 改成 `#[derive(... Serialize_repr, Deserialize_repr)]`。

- [ ] **Step 2.4: 测试通过**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::types::tests`
Expected: 3/3 PASS。

- [ ] **Step 2.5: 提交**

```bash
git add src-tauri/src/connector/im/wechat/types.rs src-tauri/Cargo.toml
git commit -m "feat(connector/im/wechat): UploadMediaType + MessageItemType (two distinct enums, §4.0)"
```

---

## Task 3: `types.rs` —— WeixinMessage + MessageItem + CDNMedia + GetUpdatesResp 等 wire types

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/types.rs`

跟 openclaw `api/types.ts` 一一对应。所有字段都 `#[serde(default, skip_serializing_if = "Option::is_none")]` 因为 iLink 是 protobuf-JSON、字段可缺。

- [ ] **Step 3.1: 写失败的单测（反序列化一个真实 message fixture）**

在 `types.rs` `mod tests` 里追加：

```rust
    #[test]
    fn deserialize_inbound_text_message_fixture() {
        // 简化自 openclaw inbound.test.ts 的 fixture，最小私聊文本消息。
        let json = r#"{
            "ret": 0,
            "msgs": [{
                "message_id": 42,
                "from_user_id": "wxid_alice@im.wechat",
                "to_user_id": "wxid_bot@im.wechat",
                "create_time_ms": 1715990400000,
                "session_id": "sess-1",
                "message_type": 1,
                "message_state": 2,
                "item_list": [{
                    "type": 1,
                    "text_item": { "text": "Hello" }
                }],
                "context_token": "ctx-token-abc"
            }],
            "get_updates_buf": "next-buf-base64",
            "longpolling_timeout_ms": 35000
        }"#;
        let resp: GetUpdatesResp = serde_json::from_str(json).unwrap();
        assert_eq!(resp.ret, Some(0));
        assert_eq!(resp.longpolling_timeout_ms, Some(35000));
        assert_eq!(resp.get_updates_buf.as_deref(), Some("next-buf-base64"));
        let msgs = resp.msgs.unwrap();
        assert_eq!(msgs.len(), 1);
        let m = &msgs[0];
        assert_eq!(m.message_id, Some(42));
        assert_eq!(m.from_user_id.as_deref(), Some("wxid_alice@im.wechat"));
        assert_eq!(m.context_token.as_deref(), Some("ctx-token-abc"));
        let items = m.item_list.as_ref().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].r#type, Some(MessageItemType::Text));
        let text = items[0].text_item.as_ref().unwrap();
        assert_eq!(text.text.as_deref(), Some("Hello"));
    }

    #[test]
    fn deserialize_inbound_voice_message_with_text() {
        // VOICE message with server-side speech-to-text. parser will treat
        // these as text (spec §6 修正).
        let json = r#"{
            "ret": 0,
            "msgs": [{
                "from_user_id": "wxid_alice@im.wechat",
                "item_list": [{
                    "type": 3,
                    "voice_item": {
                        "encode_type": 6,
                        "playtime": 3500,
                        "text": "transcribed voice content"
                    }
                }]
            }],
            "get_updates_buf": ""
        }"#;
        let resp: GetUpdatesResp = serde_json::from_str(json).unwrap();
        let msgs = resp.msgs.unwrap();
        let voice = msgs[0].item_list.as_ref().unwrap()[0]
            .voice_item
            .as_ref()
            .unwrap();
        assert_eq!(voice.text.as_deref(), Some("transcribed voice content"));
    }
```

- [ ] **Step 3.2: 运行测试确认失败**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::types::tests`
Expected: 2 新 case FAIL —— `cannot find type GetUpdatesResp`。

- [ ] **Step 3.3: 实现 wire types**

在 `types.rs` enum 定义之后、`#[cfg(test)] mod tests` 之前追加：

```rust
//---------------------------------------------------------------------------
// MessageItem and its sub-types
//---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// CDN media reference. `aes_key` is base64-encoded.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CdnMedia {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypt_query_param: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aes_key: Option<String>,
    /// 0 = only fileid encrypted, 1 = thumbnail/mid info packed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypt_type: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<CdnMedia>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_media: Option<CdnMedia>,
    /// Hex string; preferred over `media.aes_key` for inbound decryption.
    /// See openclaw parser comments — this is the more stable inbound key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aeskey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_height: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_width: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hd_size: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<CdnMedia>,
    /// 1=pcm 2=adpcm 3=feature 4=speex 5=amr 6=silk 7=mp3 8=ogg-speex
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_type: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits_per_sample: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playtime: Option<i64>,
    /// Server-side speech-to-text result; when present, parser should treat
    /// the voice item as a text message instead of a "[unsupported]" placeholder.
    /// (spec §6 修正)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<CdnMedia>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<CdnMedia>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play_length: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_md5: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_media: Option<CdnMedia>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_height: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_width: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_item: Option<Box<MessageItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageItem {
    /// `type` is a Rust keyword; use raw identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<MessageItemType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_time_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_time_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_completed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_msg: Option<Box<RefMessage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_item: Option<TextItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_item: Option<ImageItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_item: Option<VoiceItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_item: Option<FileItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_item: Option<VideoItem>,
}

//---------------------------------------------------------------------------
// WeixinMessage envelope
//---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeixinMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_time_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_time_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_time_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_type: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_state: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_list: Option<Vec<MessageItem>>,
    /// Must be echoed back in `sendMessage`. See spec §3.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_token: Option<String>,
}

//---------------------------------------------------------------------------
// API request/response envelopes
//---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaseInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetUpdatesReq {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub get_updates_buf: Option<String>,
    pub base_info: BaseInfo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetUpdatesResp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ret: Option<i32>,
    /// Error code; -14 == SESSION_EXPIRED_ERRCODE (see §1.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errcode: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errmsg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msgs: Option<Vec<WeixinMessage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub get_updates_buf: Option<String>,
    /// Server-suggested timeout (ms) for the next long-poll request.
    /// Client MUST respect this. See spec §3.1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub longpolling_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SendMessageReq {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<WeixinMessage>,
    pub base_info: BaseInfo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetUploadUrlReq {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filekey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<UploadMediaType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rawsize: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rawfilemd5: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesize: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_rawsize: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_rawfilemd5: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_filesize: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_need_thumb: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aeskey: Option<String>,
    pub base_info: BaseInfo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetUploadUrlResp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_param: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_upload_param: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_full_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetConfigResp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ret: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errmsg: Option<String>,
    /// Base64-encoded typing ticket; pass to sendTyping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typing_ticket: Option<String>,
}

/// Typing indicator status; 1 = typing, 2 = cancel.
pub const TYPING_STATUS_TYPING: i32 = 1;
pub const TYPING_STATUS_CANCEL: i32 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SendTypingReq {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ilink_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typing_ticket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
    pub base_info: BaseInfo,
}

/// `errcode = -14` from getUpdates → session expired. See spec §1.2.
pub const SESSION_EXPIRED_ERRCODE: i32 = -14;
```

- [ ] **Step 3.4: 测试通过**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::types::tests`
Expected: 5/5 PASS（3 enum case + 2 fixture case）。

- [ ] **Step 3.5: 提交**

```bash
git add src-tauri/src/connector/im/wechat/types.rs
git commit -m "feat(connector/im/wechat): wire types — WeixinMessage / MessageItem / API envelopes"
```

---

## Task 4: `endpoints.rs` —— 7 个 iLink endpoint 常量

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/endpoints.rs`

- [ ] **Step 4.1: 写失败的常量测试**

替换 `endpoints.rs`：

```rust
//! iLink endpoint paths. Single source of truth.
//!
//! 7 endpoints total (5 POST + 2 GET). Verified against
//! openclaw-weixin-main/src/api/api.ts.

/// Default base URL. Switched to `baseurl` returned by `get_qrcode_status`
/// after login (IDC routing — see spec §1).
pub const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

// ----- POST endpoints (business) -----

pub const GET_UPDATES: &str = "ilink/bot/getupdates";
pub const SEND_MESSAGE: &str = "ilink/bot/sendmessage";
pub const GET_UPLOAD_URL: &str = "ilink/bot/getuploadurl";
pub const GET_CONFIG: &str = "ilink/bot/getconfig";
pub const SEND_TYPING: &str = "ilink/bot/sendtyping";

// ----- GET endpoints (login) -----

pub const GET_BOT_QRCODE: &str = "ilink/bot/get_bot_qrcode";
pub const GET_QRCODE_STATUS: &str = "ilink/bot/get_qrcode_status";

/// Default bot_type query parameter for `get_bot_qrcode`. Verified value from
/// openclaw plugin; do not change without re-validating the QR scan flow.
pub const DEFAULT_BOT_TYPE: &str = "3";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_seven_endpoints_have_the_ilink_bot_prefix() {
        for ep in [
            GET_UPDATES,
            SEND_MESSAGE,
            GET_UPLOAD_URL,
            GET_CONFIG,
            SEND_TYPING,
            GET_BOT_QRCODE,
            GET_QRCODE_STATUS,
        ] {
            assert!(ep.starts_with("ilink/bot/"), "endpoint {ep} missing prefix");
        }
    }

    #[test]
    fn base_url_is_https_and_not_trailing_slash() {
        assert!(DEFAULT_BASE_URL.starts_with("https://"));
        assert!(!DEFAULT_BASE_URL.ends_with('/'));
    }
}
```

- [ ] **Step 4.2: 测试通过**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::endpoints::tests`
Expected: 2/2 PASS。

- [ ] **Step 4.3: 提交**

```bash
git add src-tauri/src/connector/im/wechat/endpoints.rs
git commit -m "feat(connector/im/wechat): 7 iLink endpoint constants + default base URL"
```

---

## Task 5: `appid.rs` —— 配置驱动的 iLink-App-Id

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/appid.rs`

spec §1.5 决议：MVP 复用 openclaw 的 appid 写为常量；运行时读 config 覆盖。

- [ ] **Step 5.1: 查找 openclaw 的 ilink_appid 真实值**

Run: `grep '"ilink_appid"' "/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main/package.json"`
Expected: 输出形如 `"ilink_appid": "<值>"` —— **抄这个值**作为 `DEFAULT_OPENCLAW_APP_ID` 常量内容。如果该字段在 package.json 不存在，去 `src/api/api.ts` 找 `pkg.ilink_appid` 兜底逻辑。

如果完全找不到具体值（被运行时注入），**停下来**问 oayzz；继续要么去 openclaw 仓库的 issues 翻一下，要么用占位字符串 `"openclaw-placeholder"` + log warn 在 PR1 内显式标"未对接真实 appid"。

- [ ] **Step 5.2: 写失败的单测**

替换 `appid.rs`：

```rust
//! iLink-App-Id source. See spec §1.5 for the openclaw-fallback decision.
//!
//! Resolution order:
//! 1. `~/.renlijia/config.json` field `wechat.ilink_app_id` (string)
//! 2. compile-time constant `DEFAULT_OPENCLAW_APP_ID` (MVP fallback)
//!
//! When the user supplies their own appid via config, the connector logs the
//! switch at INFO level for traceability.

use std::path::Path;

use serde::Deserialize;

/// **MVP fallback**: openclaw plugin's appid. Replace with the AIjia-issued
/// value once Tencent allocates one (see spec §1.5). Storing as a const so
/// the binary still works offline (config file not required).
pub const DEFAULT_OPENCLAW_APP_ID: &str = "TODO_REPLACE_FROM_OPENCLAW_PACKAGE_JSON";

#[derive(Debug, Clone, Default, Deserialize)]
struct WechatConfigSection {
    #[serde(default)]
    ilink_app_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AijiaConfig {
    #[serde(default)]
    wechat: Option<WechatConfigSection>,
}

/// Read `~/.renlijia/config.json` and return the configured app id, if any.
/// Errors / missing file / missing field all collapse to `None` —— caller
/// uses the compile-time default.
pub fn read_configured_app_id(config_path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(config_path).ok()?;
    let cfg: AijiaConfig = serde_json::from_str(&raw).ok()?;
    let id = cfg.wechat.and_then(|w| w.ilink_app_id)?;
    let trimmed = id.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

/// Final resolved app id, with logging on which source won.
pub fn resolve_app_id(config_path: &Path) -> String {
    match read_configured_app_id(config_path) {
        Some(custom) => {
            log::info!(
                "[wechat] iLink-App-Id from config (custom, len={})",
                custom.len()
            );
            custom
        }
        None => {
            log::debug!("[wechat] iLink-App-Id from compile-time default (MVP)");
            DEFAULT_OPENCLAW_APP_ID.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn default_used_when_config_missing() {
        let nope = Path::new("/nonexistent/path/aijia-config.json");
        let id = resolve_app_id(nope);
        assert_eq!(id, DEFAULT_OPENCLAW_APP_ID);
    }

    #[test]
    fn config_value_overrides_default() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"wechat":{{"ilink_app_id":"AIJIA-CUSTOM-123"}}}}"#).unwrap();
        let id = resolve_app_id(f.path());
        assert_eq!(id, "AIJIA-CUSTOM-123");
    }

    #[test]
    fn empty_string_falls_back_to_default() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"wechat":{{"ilink_app_id":""}}}}"#).unwrap();
        let id = resolve_app_id(f.path());
        assert_eq!(id, DEFAULT_OPENCLAW_APP_ID);
    }

    #[test]
    fn malformed_config_falls_back_to_default() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "this is not json {{").unwrap();
        let id = resolve_app_id(f.path());
        assert_eq!(id, DEFAULT_OPENCLAW_APP_ID);
    }
}
```

- [ ] **Step 5.3: 测试通过**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::appid::tests`
Expected: 4/4 PASS。

- [ ] **Step 5.4: 替换 DEFAULT_OPENCLAW_APP_ID 占位**

从 Step 5.1 拿到的真实值（或确认替换的占位字符串）改进 `DEFAULT_OPENCLAW_APP_ID` 常量。**不要把真实值直接写到 commit message 里**——常量代码里有就够了。

- [ ] **Step 5.5: 提交**

```bash
git add src-tauri/src/connector/im/wechat/appid.rs
git commit -m "feat(connector/im/wechat): config-driven iLink-App-Id with openclaw MVP fallback (spec §1.5)"
```

---

## Task 6: `headers.rs` —— `build_headers` 集中函数

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/headers.rs`
- Modify: `src-tauri/Cargo.toml` (rand crate if not present)

- [ ] **Step 6.1: 确认 base64 / rand 依赖**

Run: `grep -E '^base64|^rand\b' src-tauri/Cargo.toml`
Expected: `base64 = "0.22"` 已存在。检查 `rand`；如果没有，加 `rand = "0.8"`。

- [ ] **Step 6.2: 写失败的单测**

替换 `headers.rs`：

```rust
//! Common request-header builder for all iLink API calls.
//!
//! See spec §1.3 for the mandatory header list; every endpoint must use
//! `build_headers()` so we never accidentally ship a request missing
//! `iLink-App-Id` or `X-WECHAT-UIN`.

use base64::Engine;
use rand::Rng;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

/// Header name for the iLink app identifier (custom).
pub const HEADER_ILINK_APP_ID: &str = "iLink-App-Id";
/// Header name for the client version (custom).
pub const HEADER_ILINK_APP_CLIENT_VERSION: &str = "iLink-App-ClientVersion";
/// Authorization-type marker (custom).
pub const HEADER_AUTHORIZATION_TYPE: &str = "AuthorizationType";
/// Per-request random uin (custom; required by server).
pub const HEADER_X_WECHAT_UIN: &str = "X-WECHAT-UIN";
/// Optional IDC routing tag (custom).
pub const HEADER_SK_ROUTE_TAG: &str = "SKRouteTag";

/// Constant; iLink server expects this exact value.
pub const AUTHORIZATION_TYPE_VALUE: &str = "ilink_bot_token";

/// Encode a SemVer-like "major.minor.patch" string into the uint32 wire format
/// expected by `iLink-App-ClientVersion`: `major<<16 | minor<<8 | patch`.
///
/// Non-numeric / missing components default to 0. Saturates at u8 per component
/// (matches openclaw's `& 0xff` behaviour).
pub fn encode_client_version(semver: &str) -> u32 {
    let mut parts = semver.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0) & 0xff;
    let minor = parts.next().unwrap_or(0) & 0xff;
    let patch = parts.next().unwrap_or(0) & 0xff;
    (major << 16) | (minor << 8) | patch
}

/// Generate the `X-WECHAT-UIN` header value: base64(decimal_string(random_u32)).
fn generate_wechat_uin() -> String {
    let n: u32 = rand::thread_rng().gen();
    base64::engine::general_purpose::STANDARD.encode(n.to_string().as_bytes())
}

/// Inputs needed to construct a request header set.
pub struct HeaderInputs<'a> {
    /// iLink-App-Id; resolved via `appid::resolve_app_id`.
    pub app_id: &'a str,
    /// Client version semver string, e.g. crate version.
    pub client_version: &'a str,
    /// Bearer token (`bot_token`). `None` for unauthenticated login endpoints.
    pub bot_token: Option<&'a str>,
    /// IDC routing tag from previous server response. `None` for first call.
    pub route_tag: Option<&'a str>,
    /// Serialized request body. Used to set Content-Length (reqwest computes
    /// it automatically, but openclaw sends it explicitly so we match).
    pub body: &'a str,
}

/// Build the full header map for an iLink POST request.
///
/// Caller is responsible for setting the method, URL, and body separately;
/// this only handles the headers.
pub fn build_headers(inputs: HeaderInputs) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h.insert(
        HeaderName::from_static("ilink-app-id"),
        HeaderValue::from_str(inputs.app_id).expect("app_id must be ASCII"),
    );
    let cv = encode_client_version(inputs.client_version).to_string();
    h.insert(
        HeaderName::from_static("ilink-app-clientversion"),
        HeaderValue::from_str(&cv).unwrap(),
    );
    h.insert(
        HeaderName::from_static("authorizationtype"),
        HeaderValue::from_static(AUTHORIZATION_TYPE_VALUE),
    );
    if let Some(t) = inputs.bot_token.map(str::trim).filter(|t| !t.is_empty()) {
        let v = format!("Bearer {t}");
        h.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&v).expect("token must be ASCII"),
        );
    }
    let uin = generate_wechat_uin();
    h.insert(
        HeaderName::from_static("x-wechat-uin"),
        HeaderValue::from_str(&uin).unwrap(),
    );
    if let Some(rt) = inputs.route_tag.filter(|s| !s.is_empty()) {
        h.insert(
            HeaderName::from_static("skroutetag"),
            HeaderValue::from_str(rt).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
    }
    // Content-Length: reqwest will set this from the body bytes automatically.
    // We don't override it — leaving it empty matches the openclaw behaviour
    // when the explicit "Content-Length" header is suppressed by fetch.
    let _ = inputs.body; // suppress unused — body is consumed by caller for the request.
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_client_version_known_values() {
        // openclaw example: "1.0.11" -> 0x0001000B = 65547
        assert_eq!(encode_client_version("1.0.11"), 0x0001_000b);
        assert_eq!(encode_client_version("0.0.0"), 0);
        assert_eq!(encode_client_version("2.1.7"), 0x0002_0107);
    }

    #[test]
    fn encode_client_version_saturates_per_byte() {
        // Each component clamped to 0..=255
        assert_eq!(encode_client_version("256.0.0"), 0);
        assert_eq!(encode_client_version("1.300.7"), 0x0001_2c07);
    }

    #[test]
    fn encode_client_version_malformed_components_default_zero() {
        assert_eq!(encode_client_version("abc.def.ghi"), 0);
        assert_eq!(encode_client_version("1..3"), 0x0001_0003);
        assert_eq!(encode_client_version(""), 0);
    }

    #[test]
    fn build_headers_includes_all_mandatory_keys() {
        let h = build_headers(HeaderInputs {
            app_id: "test-app-id",
            client_version: "1.2.3",
            bot_token: Some("the-token"),
            route_tag: Some("zone-a"),
            body: r#"{"foo":1}"#,
        });

        assert_eq!(h.get(CONTENT_TYPE).unwrap(), "application/json");
        assert_eq!(h.get("iLink-App-Id").unwrap(), "test-app-id");
        assert_eq!(
            h.get("iLink-App-ClientVersion").unwrap(),
            &encode_client_version("1.2.3").to_string()[..]
        );
        assert_eq!(h.get("AuthorizationType").unwrap(), "ilink_bot_token");
        assert_eq!(h.get(AUTHORIZATION).unwrap(), "Bearer the-token");
        assert_eq!(h.get("SKRouteTag").unwrap(), "zone-a");

        // X-WECHAT-UIN is randomized but always present and decodes to a base64-encoded ASCII number.
        let uin = h.get("X-WECHAT-UIN").unwrap().to_str().unwrap();
        let decoded = base64::engine::general_purpose::STANDARD.decode(uin).unwrap();
        let s = std::str::from_utf8(&decoded).unwrap();
        assert!(s.chars().all(|c| c.is_ascii_digit()), "got {s}");
    }

    #[test]
    fn build_headers_omits_authorization_when_token_none() {
        let h = build_headers(HeaderInputs {
            app_id: "x",
            client_version: "0.1.0",
            bot_token: None,
            route_tag: None,
            body: "{}",
        });
        assert!(h.get(AUTHORIZATION).is_none());
        assert!(h.get("SKRouteTag").is_none());
    }

    #[test]
    fn build_headers_omits_authorization_when_token_empty_or_whitespace() {
        for t in ["", "  ", "\n"] {
            let h = build_headers(HeaderInputs {
                app_id: "x",
                client_version: "0.1.0",
                bot_token: Some(t),
                route_tag: None,
                body: "{}",
            });
            assert!(h.get(AUTHORIZATION).is_none(), "token {t:?} should be skipped");
        }
    }
}
```

- [ ] **Step 6.3: 测试通过**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::headers::tests`
Expected: 5/5 PASS。如果 `HeaderName::from_static` 报小写要求（reqwest 要求 ASCII-lowercase），按提示调整 `from_static("ilink-app-id")` 等的大小写。

- [ ] **Step 6.4: 提交**

```bash
git add src-tauri/src/connector/im/wechat/headers.rs src-tauri/Cargo.toml
git commit -m "feat(connector/im/wechat): build_headers — central HTTP header builder (spec §1.3)"
```

---

## Task 7: `connector.rs` —— `WechatConnector` 空壳实现 IMConnector

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/connector.rs`
- Modify: `src-tauri/src/connector/im/factory.rs`

PR1 末尾的 connector 是占位：`start()` 立刻返回 empty stream，`send()` 返 NotSupported。这样可以让 `register_im_connectors` 列表加上 wechat 不挂；后续 PR3-7 逐步填充。

- [ ] **Step 7.1: 看 feishu connector PR1 时期的占位长什么样**

Run: `git log --oneline --all -- src-tauri/src/connector/im/feishu/connector.rs | tail -10`
Expected: 找到飞书最早的 PR1 commit。

Run: `git show <feishu-pr1-commit>:src-tauri/src/connector/im/feishu/connector.rs | head -100`
（用上一步找到的 commit hash）

照其骨架照搬。

- [ ] **Step 7.2: 写失败的 connector 单测（platform + capabilities）**

替换 `connector.rs`：

```rust
//! `WechatConnector` — `IMConnector` implementation for iLink HTTP API.
//!
//! PR1 (this commit): skeleton. `start()` returns an empty stream;
//! `send()` returns `NotSupported`. PR3-7 fill in actual behaviour.

use std::path::PathBuf;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};

use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector,
    InboundDeployment, MarkdownSupport, ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelMessage, Platform};

pub struct WechatConnector {
    /// Resolved iLink-App-Id (from config or compile-time default). See spec §1.5.
    app_id: String,
    /// Client version semver (e.g. crate version) used in
    /// `iLink-App-ClientVersion` header.
    client_version: String,
    /// AIjia config file path; used by future PRs to load secondary settings.
    #[allow(dead_code)]
    config_path: PathBuf,
}

impl WechatConnector {
    pub fn new(app_id: String, client_version: String, config_path: PathBuf) -> Self {
        Self {
            app_id,
            client_version,
            config_path,
        }
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    pub fn client_version(&self) -> &str {
        &self.client_version
    }
}

#[async_trait]
impl IMConnector for WechatConnector {
    fn platform(&self) -> Platform {
        Platform::Wechat
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundDeployment::SelfHosted,
            outbound_aicard: false,
            outbound_text_streaming: false,
            outbound_markdown: MarkdownSupport::Partial,
            supports_attachments: true,
            supports_group_chat: false,
            supports_private_chat: true,
            auth_flow: AuthFlow::QRCode,
        }
    }

    async fn start(
        &self,
        _ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        // PR1: not connected; return an empty stream. The manager treats
        // stream end as "connection lost"; that's fine here — we haven't
        // begun the registration flow yet.
        Ok(Box::pin(stream::empty()))
    }

    async fn send(
        &self,
        _target: ReplyTarget,
        _content: ReplyContent,
    ) -> Result<(), ConnectorError> {
        Err(ConnectorError::NotSupported("wechat send (PR1 skeleton)"))
    }

    // begin_registration / poll_registration: PR3 implements them.
    // Default trait impl returns NotSupported, which is correct for PR1.
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> WechatConnector {
        WechatConnector::new(
            "test-app-id".to_string(),
            "0.1.0".to_string(),
            PathBuf::from("/tmp/nope.json"),
        )
    }

    #[test]
    fn platform_is_wechat() {
        assert_eq!(make().platform(), Platform::Wechat);
    }

    #[test]
    fn capabilities_match_spec_section_2() {
        let c = make().capabilities();
        assert_eq!(c.inbound, InboundDeployment::SelfHosted);
        assert!(!c.outbound_aicard);
        assert!(!c.outbound_text_streaming);
        assert_eq!(c.outbound_markdown, MarkdownSupport::Partial);
        assert!(c.supports_attachments);
        assert!(!c.supports_group_chat);
        assert!(c.supports_private_chat);
        assert_eq!(c.auth_flow, AuthFlow::QRCode);
    }

    #[tokio::test]
    async fn send_returns_not_supported_in_pr1() {
        let connector = make();
        let target = ReplyTarget {
            session_id: "s1".to_string(),
            external_conversation_key: "wxid_alice@im.wechat".to_string(),
        };
        let result = connector
            .send(target, ReplyContent::Text("hi".to_string()))
            .await;
        assert!(matches!(result, Err(ConnectorError::NotSupported(_))));
    }
}
```

- [ ] **Step 7.3: 运行测试**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::connector::tests`
Expected: 编译错误或 FAIL —— `InboundDeployment` / `MarkdownSupport` 不在 trait_def（除非 Phase 1 PR0d + Phase 3 PR1.5 已合）。如果 trait_def 已经有，3/3 PASS。

如果 trait_def 尚未升级：
1. 确认 Step 0.2 列出的关键词全在 trait_def.rs
2. 否则停下来回 Phase 1 / Phase 3 plan 补完

- [ ] **Step 7.4: 提交**

```bash
git add src-tauri/src/connector/im/wechat/connector.rs
git commit -m "feat(connector/im/wechat): WechatConnector skeleton — platform + capabilities + PR1 stubs"
```

---

## Task 8: `factory.rs` —— `build_wechat_connector` 工厂方法 + 注册到 manager

**Files:**
- Modify: `src-tauri/src/connector/im/factory.rs`
- Modify: `src-tauri/tests/review_im_layering.rs`

- [ ] **Step 8.1: 看现有 factory 是怎么注册 dingtalk + feishu 的**

Run: `cat src-tauri/src/connector/im/factory.rs`
记下：① `build_dingtalk_connector` / `build_feishu_connector` 函数签名 ② 是否有"register all platforms"统一入口 ③ 何处读取 secure storage / config_store。

- [ ] **Step 8.2: 加 `build_wechat_connector`**

按 feishu 同款风格在 `factory.rs` 末尾追加：

```rust
// ... existing imports / fns ...

use crate::connector::im::wechat::{appid, WechatConnector};

/// Build a `WechatConnector` if `Platform::Wechat` is enabled in the channel
/// config store. PR1: this returns the skeleton connector; PR3+ adds real
/// runtime state.
pub fn build_wechat_connector(
    config_store: &ChannelConfigStore,
    aijia_config_path: &std::path::Path,
) -> Option<WechatConnector> {
    if !is_platform_enabled(config_store, Platform::Wechat) {
        return None;
    }
    let app_id = appid::resolve_app_id(aijia_config_path);
    let client_version = env!("CARGO_PKG_VERSION").to_string();
    Some(WechatConnector::new(
        app_id,
        client_version,
        aijia_config_path.to_path_buf(),
    ))
}
```

注意：`is_platform_enabled` / `aijia_config_path` 这些名字按 factory.rs 当前内容**对齐**——如果飞书走的是别的签名，照其样式。如果飞书没有"enabled gate"而是无条件构造，wechat 也无条件构造（PR1 占位无副作用）。

- [ ] **Step 8.3: 在统一注册入口加 wechat**

找到 factory.rs 里类似 `register_all_im_connectors` / `init_im_manager` 的函数（飞书的注册点），照样加 wechat 分支：

```rust
if let Some(conn) = build_wechat_connector(&config_store, &aijia_config_path) {
    manager.register(Platform::Wechat, std::sync::Arc::new(conn));
}
```

具体 API 形状以 factory.rs 当前的飞书注册路径为准。

- [ ] **Step 8.4: 更新 `review_im_layering.rs`**

Run: `grep -n 'platforms\|"wechat"' src-tauri/tests/review_im_layering.rs | head -10`

找到 `platforms = [...]` 数组定义。如果还没 `"wechat"`，加上：

```rust
const PLATFORMS: &[&str] = &["dingtalk", "feishu", "wecom", "telegram", "whatsapp", "wechat"];
```

具体常量名以现状为准。

- [ ] **Step 8.5: 跑全套 connector 测试**

Run: `cd src-tauri && cargo test --lib connector::im::`
Expected: 全 PASS。无新 fail。

Run: `cd src-tauri && cargo test --test review_im_layering`
Expected: PASS。

- [ ] **Step 8.6: 提交**

```bash
git add src-tauri/src/connector/im/factory.rs src-tauri/tests/review_im_layering.rs
git commit -m "feat(connector/im/wechat): wire WechatConnector into factory + review_im_layering"
```

---

## Task 9: PR1 全量验证

- [ ] **Step 9.1: 全量 unit 测试**

Run: `cd src-tauri && cargo test --lib connector::im::wechat`
Expected: 所有 case PASS。记录测试数。

- [ ] **Step 9.2: 全套架构回归**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast`
Expected: 全 PASS。

- [ ] **Step 9.3: Tauri full build smoke**

Run: `cd src-tauri && cargo build --release`
Expected: PASS（耗时较长，<5 分钟）。无新 warning 在 wechat 模块下。

- [ ] **Step 9.4: 整理 PR1 PR description 草稿**

Title: `feat(connector/im/wechat): scaffold + types + endpoints + headers + capabilities (Phase 5 PR1)`

Body：
```
Phase 5 PR1 — Wechat connector 骨架。

新增 src-tauri/src/connector/im/wechat/：
- types.rs：UploadMediaType / MessageItemType 两套媒体枚举严格分开（spec §4.0）
  + WeixinMessage / MessageItem / API envelopes 一一对应 openclaw types.ts
- endpoints.rs：7 个 iLink endpoint 常量（5 POST + 2 GET 扫码）
- headers.rs：build_headers() 集中函数，覆盖 iLink-App-Id / X-WECHAT-UIN /
  AuthorizationType / Authorization / SKRouteTag / Content-Type（spec §1.3）
- appid.rs：配置驱动的 iLink-App-Id 解析（spec §1.5）—— MVP 用 openclaw 的
  常量；config 文件 wechat.ilink_app_id 字段优先
- connector.rs：WechatConnector skeleton 实现 IMConnector；start() 返回 empty
  stream，send() 返回 NotSupported；platform/capabilities 已就位

Capabilities:
  inbound = SelfHosted (HTTP 长轮询)
  outbound_aicard = false / outbound_text_streaming = false
  outbound_markdown = Partial (StreamingMarkdownFilter)
  supports_group_chat = false (Phase 5 仅私聊)
  auth_flow = QRCode

PR1 没接业务（start/send/registration 都是占位）。PR3 接扫码登录，PR4 接长
轮询，PR5 接发收，PR6 接媒体，PR7 集成测试 + UI。

Spec: docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md
Plan: docs/superpowers/plans/2026-05-18-im-wechat-phase5-foundations.md

Tests:
- 5 unit tests / types.rs (enum repr / serde / fixture round-trip)
- 2 unit tests / endpoints.rs
- 4 unit tests / appid.rs (default / override / empty / malformed)
- 5 unit tests / headers.rs (encode_client_version + build_headers)
- 3 unit tests / connector.rs (platform / capabilities / send returns NotSupported)
- review_im_layering: platforms 数组加 "wechat"
```

- [ ] **Step 9.5: PR1 收尾**

Run: `git log --oneline -10`
Expected: 看到本 PR 的 7-8 个 commit 按顺序排列（scaffold → types enum → wire types → endpoints → appid → headers → connector → factory）。

---

# PR2: `crypto.rs` —— AES-128-ECB + PKCS#7 + 圣经 fixture

## §0 前置准备

- [ ] **Step P2.0.1: 确认 PR1 已合**

Run: `git log --oneline main -5 | head -10`
Expected: 顶部 commit 包含 PR1 全部内容（types / headers / etc.）。

- [ ] **Step P2.0.2: 准备圣经 fixture（关键前置）**

**fixture 必须来自真实 openclaw 加解密对**，spec §4.2 定为"merge gate"。两种获取方式：

1. **首选**：跑 openclaw NodeJS plugin 真实账号，截一段 iLink 媒体下载完整流程：
   - 在 plugin 里加 console.log 输出 `(aes_key_hex, ciphertext_first_64_bytes_hex, plaintext_first_64_bytes_hex)`
   - 接收一条图片消息，记录三元组
2. **次选**：跑 openclaw 单测里的 fixture（如果它自己有）：
   - 检查 `/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main/src/cdn/*.test.ts`
   - 看是否含 `(aes_key, ciphertext, plaintext)` 硬编码 fixture
3. **最次**：用 Node REPL 跑 openclaw 自己的 `encryptAesEcb` / `decryptAesEcb` 函数，喂任意明文得到密文对——这个**也可以**因为函数就是 openclaw 自己提供的，等于自洽。

Run（次选/最次）:
```bash
cd "/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main"
node -e '
const { encryptAesEcb, decryptAesEcb } = require("./src/cdn/aes-ecb.ts");
// 如果是 .ts 直接 require 不通，可以先 tsc 编译，或者自己手写一段调用 createCipheriv
const crypto = require("crypto");
const key = Buffer.from("00112233445566778899aabbccddeeff", "hex");  // 16 bytes
const plaintext = Buffer.from("hello, wechat ilink media!", "utf-8");
const cipher = crypto.createCipheriv("aes-128-ecb", key, null);
const ct = Buffer.concat([cipher.update(plaintext), cipher.final()]);
console.log("key   =", key.toString("hex"));
console.log("plain =", plaintext.toString("hex"));
console.log("ct    =", ct.toString("hex"));
'
```

**记下输出**——三元组（key / plaintext / ciphertext）将硬编码进 Rust 单测。

如果 fixture 完全拿不到，**停下来**回报 oayzz；PR2 不能 merge。

- [ ] **Step P2.0.3: 加 aes / cbc / block-padding / hex 依赖**

修改 `src-tauri/Cargo.toml`，在 `[dependencies]` 加：

```toml
aes = "0.8"
cbc = "0.1"           # for BlockEncryptMut/BlockDecryptMut trait re-exports
block-padding = "0.3" # Pkcs7
hex = "0.4"
```

注意：`aes 0.8` 默认就支持 ECB（通过 `aes::Aes128` 的 `BlockEncrypt`/`BlockDecrypt` trait），不需要 feature flag。`cbc` crate 是 `block-modes` 系列在 RustCrypto 拆分后的现代替代，**但它本身只做 CBC**——ECB 模式 RustCrypto 把它单独放在 `ecb` crate：

```toml
ecb = "0.1"           # provides EcbEnc / EcbDec wrappers
```

最终需要的 4 个 crate：

```toml
aes = "0.8"
ecb = "0.1"
block-padding = "0.3"
hex = "0.4"
```

去掉之前 plan 草稿里的 `cbc` 行（用 `ecb`）。

Run: `cd src-tauri && cargo check`
Expected: 编译通过（可能有 unused warning）。

- [ ] **Step P2.0.4: 提交依赖**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "build(deps): add aes / ecb / block-padding / hex for wechat crypto (Phase 5 PR2)"
```

---

## Task P2.1: `crypto.rs` —— encrypt / decrypt / padded_size 纯函数

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/crypto.rs`
- Modify: `src-tauri/src/connector/im/wechat/mod.rs`（加 `pub mod crypto`）

- [ ] **Step P2.1.1: 在 `mod.rs` 加 crypto 子模块**

```rust
pub mod crypto;
```

放在其他 `pub mod` 行之间。

- [ ] **Step P2.1.2: 写失败的单测（圣经 fixture）**

Create `src-tauri/src/connector/im/wechat/crypto.rs`:

```rust
//! AES-128-ECB encryption for iLink media transport.
//!
//! **Why ECB?** This is the algorithm the iLink server expects on the wire.
//! ECB is generally not recommended (no IV, plaintext patterns leak), but
//! we don't get to choose — the server-side protocol mandates it. If iLink
//! ever migrates to CBC/GCM we'll update; until then this matches the
//! reference NodeJS plugin (openclaw-weixin-main/src/cdn/aes-ecb.ts) byte-for-byte.

use aes::Aes128;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit};
use block_padding::Pkcs7;
use ecb::{Decryptor, Encryptor};
use thiserror::Error;

type Aes128EcbEnc = Encryptor<Aes128>;
type Aes128EcbDec = Decryptor<Aes128>;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("key must be exactly 16 bytes, got {0}")]
    InvalidKeyLength(usize),
    #[error("decryption failed (likely padding error)")]
    DecryptError,
}

/// Encrypt `plaintext` with AES-128-ECB + PKCS#7 padding.
/// `key` must be exactly 16 bytes.
pub fn encrypt_ecb(plaintext: &[u8], key: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if key.len() != 16 {
        return Err(CryptoError::InvalidKeyLength(key.len()));
    }
    let cipher = Aes128EcbEnc::new(key.into());
    Ok(cipher.encrypt_padded_vec_mut::<Pkcs7>(plaintext))
}

/// Decrypt `ciphertext` with AES-128-ECB + PKCS#7 padding.
/// `key` must be exactly 16 bytes.
pub fn decrypt_ecb(ciphertext: &[u8], key: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if key.len() != 16 {
        return Err(CryptoError::InvalidKeyLength(key.len()));
    }
    let cipher = Aes128EcbDec::new(key.into());
    cipher
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| CryptoError::DecryptError)
}

/// Compute AES-128-ECB ciphertext size (PKCS#7 padding to 16-byte boundary).
/// Matches openclaw `aesEcbPaddedSize`: `ceil((n + 1) / 16) * 16`.
pub fn padded_size(plaintext_size: usize) -> usize {
    ((plaintext_size + 1).div_ceil(16)) * 16
}

#[cfg(test)]
mod tests {
    use super::*;

    // ====================================================================
    // SCRIPTURE FIXTURE — must match openclaw's actual output byte-for-byte.
    //
    // Generated via:
    //   key       = "00112233445566778899aabbccddeeff" (hex)
    //   plaintext = "hello, wechat ilink media!" (utf-8)
    //
    // Captured ciphertext (hex) from openclaw's encryptAesEcb on Node v20:
    //   <REPLACE_WITH_REAL_HEX_FROM_NODE_REPL_BELOW>
    //
    // DO NOT regenerate by re-running encrypt_ecb — that would make this
    // test self-consistent but not validating against openclaw. The point
    // of the fixture is **cross-implementation byte-equality**.
    // ====================================================================

    const SCRIPTURE_KEY_HEX: &str = "00112233445566778899aabbccddeeff";
    const SCRIPTURE_PLAINTEXT: &[u8] = b"hello, wechat ilink media!";
    /// REPLACE with hex from Step P2.0.2 Node REPL output.
    const SCRIPTURE_CIPHERTEXT_HEX: &str =
        "REPLACE_WITH_HEX_FROM_OPENCLAW_NODE_REPL";

    #[test]
    fn scripture_encrypt_matches_openclaw_byte_for_byte() {
        let key = hex::decode(SCRIPTURE_KEY_HEX).unwrap();
        let expected = hex::decode(SCRIPTURE_CIPHERTEXT_HEX).unwrap();
        let actual = encrypt_ecb(SCRIPTURE_PLAINTEXT, &key).unwrap();
        assert_eq!(
            actual, expected,
            "Rust AES-128-ECB output diverged from openclaw NodeJS fixture"
        );
    }

    #[test]
    fn scripture_decrypt_round_trip() {
        let key = hex::decode(SCRIPTURE_KEY_HEX).unwrap();
        let ct = hex::decode(SCRIPTURE_CIPHERTEXT_HEX).unwrap();
        let decrypted = decrypt_ecb(&ct, &key).unwrap();
        assert_eq!(decrypted, SCRIPTURE_PLAINTEXT);
    }

    // ====================================================================
    // Self-consistent round-trip (always passes once encrypt+decrypt are
    // implemented correctly; backup safety net independent of scripture)
    // ====================================================================

    #[test]
    fn self_round_trip_various_lengths() {
        let key = vec![0x42u8; 16];
        // 0, 1, 15, 16, 17, 32, 100 bytes — covers PKCS#7 padding boundaries.
        for n in [0usize, 1, 15, 16, 17, 32, 100] {
            let plain: Vec<u8> = (0..n).map(|i| (i & 0xff) as u8).collect();
            let ct = encrypt_ecb(&plain, &key).unwrap();
            // Padded size invariant.
            assert_eq!(ct.len(), padded_size(plain.len()), "n={n}");
            let back = decrypt_ecb(&ct, &key).unwrap();
            assert_eq!(back, plain, "n={n}");
        }
    }

    #[test]
    fn padded_size_known_boundaries() {
        // openclaw aesEcbPaddedSize: Math.ceil((n+1)/16)*16
        assert_eq!(padded_size(0), 16);
        assert_eq!(padded_size(15), 16);
        assert_eq!(padded_size(16), 32);
        assert_eq!(padded_size(17), 32);
        assert_eq!(padded_size(31), 32);
        assert_eq!(padded_size(32), 48);
        assert_eq!(padded_size(100), 112);
    }

    #[test]
    fn rejects_wrong_key_length() {
        let plain = b"hi";
        assert!(matches!(
            encrypt_ecb(plain, &[0; 8]),
            Err(CryptoError::InvalidKeyLength(8))
        ));
        assert!(matches!(
            encrypt_ecb(plain, &[0; 32]),
            Err(CryptoError::InvalidKeyLength(32))
        ));
        assert!(matches!(
            decrypt_ecb(&[0; 16], &[0; 24]),
            Err(CryptoError::InvalidKeyLength(24))
        ));
    }

    #[test]
    fn decrypt_corrupted_padding_fails_cleanly() {
        let key = vec![0x42u8; 16];
        let plain = b"hello";
        let mut ct = encrypt_ecb(plain, &key).unwrap();
        // Flip a byte in the last block — destroys padding.
        let last = ct.len() - 1;
        ct[last] ^= 0xff;
        assert!(matches!(
            decrypt_ecb(&ct, &key),
            Err(CryptoError::DecryptError)
        ));
    }
}
```

- [ ] **Step P2.1.3: 替换圣经 fixture 占位为实际 hex**

把 Step P2.0.2 的 Node REPL 输出的 `ct` hex 字符串填进 `SCRIPTURE_CIPHERTEXT_HEX`。如果用了不同的 key/plaintext，三处常量都按真实值更新。

- [ ] **Step P2.1.4: 运行测试**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::crypto::tests`
Expected: 6/6 PASS。

如果 `scripture_encrypt_matches_openclaw_byte_for_byte` FAIL：
- 比对 Rust 输出 hex 和 Node 输出 hex 的字节差
- 如果差异在末尾 16 字节 → PKCS#7 padding 实现不同（不太可能；`block-padding::Pkcs7` 跟 OpenSSL 一致）
- 如果差异从某中间 16 字节块起 → 大概率 key 转换错（`key.into()` 是不是按字节序对应？检查 `GenericArray<u8, _>`）
- 如果完全不一样 → 调用了错误的 cipher（CBC/GCM 而非 ECB）

修复后再跑直到 PASS。

- [ ] **Step P2.1.5: 提交**

```bash
git add src-tauri/src/connector/im/wechat/crypto.rs src-tauri/src/connector/im/wechat/mod.rs
git commit -m "feat(connector/im/wechat): crypto.rs — AES-128-ECB + PKCS#7 with openclaw scripture fixture (Phase 5 PR2)"
```

---

## Task P2.2: PR2 验证

- [ ] **Step P2.2.1: 全套 wechat 测试**

Run: `cd src-tauri && cargo test --lib connector::im::wechat`
Expected: PR1 + PR2 所有 case PASS。

- [ ] **Step P2.2.2: 架构回归**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast`
Expected: 全 PASS。

- [ ] **Step P2.2.3: PR2 description 草稿**

Title: `feat(connector/im/wechat): crypto.rs — AES-128-ECB + PKCS#7 scripture fixture (Phase 5 PR2)`

Body：
```
Phase 5 PR2 — Wechat AES-128-ECB media crypto。

新增 src-tauri/src/connector/im/wechat/crypto.rs：
- encrypt_ecb / decrypt_ecb / padded_size 三个纯函数
- 圣经 fixture：(key, plaintext, ciphertext) 三元组来自 openclaw NodeJS plugin
  实测对（非凭空构造）—— 验证 Rust AES-128-ECB 实现与 openclaw byte-for-byte
  一致

ECB 模式技术上已知不安全（无 IV，重复明文产生重复密文），但这是 iLink 协议
规定的算法，不是选型 —— 这一点在文件 doc-comment 写明。

Cargo.toml 新增 aes/ecb/block-padding/hex 依赖。

Tests: 6 unit case
- scripture_encrypt_matches_openclaw_byte_for_byte（merge gate）
- scripture_decrypt_round_trip
- self_round_trip_various_lengths（0/1/15/16/17/32/100 字节边界）
- padded_size_known_boundaries
- rejects_wrong_key_length
- decrypt_corrupted_padding_fails_cleanly

Spec: docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md §4 / §4.2
Plan: docs/superpowers/plans/2026-05-18-im-wechat-phase5-foundations.md
```

---

## §End — PR1-2 完成自检

- [ ] **PR1 checklist:**
  - `src-tauri/src/connector/im/wechat/` 目录存在含 6 个 .rs 文件
  - types.rs 5 个单测全 PASS（含两套媒体枚举强类型分开）
  - endpoints.rs 7 个常量定义齐全
  - headers.rs 5 个单测全 PASS（encode_client_version + build_headers）
  - appid.rs 4 个单测全 PASS（config 覆盖 + fallback）
  - connector.rs 3 个单测全 PASS；platform/capabilities 完全对齐 spec §2
  - factory.rs 注册 wechat
  - review_im_layering platforms 数组含 wechat
  - `cargo build --release` 干净通过

- [ ] **PR2 checklist:**
  - crypto.rs 6 个单测全 PASS（含圣经 fixture 字节匹配）
  - 圣经 fixture 来自 openclaw 实测对（非自洽构造）
  - Cargo.toml 加 aes / ecb / block-padding / hex 依赖

- [ ] **跨 PR checklist:**
  - PR1 + PR2 各自独立可合，不依赖 PR3-7
  - `cargo test review_` 全 PASS
  - 两个 PR 的 commit 历史清晰：PR1 ~8 commit / PR2 ~3 commit

完成。Phase 5 PR3-7 plan（`2026-05-18-im-wechat-phase5-main.md`）可以开始执行。

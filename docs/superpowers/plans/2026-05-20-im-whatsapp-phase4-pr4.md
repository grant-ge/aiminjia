# Phase 4 WhatsApp PR4 — 入站消息（bot worker + parser + allow_from + dedup + quoted reply）

> Final destination: `docs/superpowers/plans/2026-05-20-im-whatsapp-phase4-pr4.md`
> (Plan is written to harness file because plan-mode only allows editing one file; first
> step of execution is to copy this file to the docs path.)

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`
> (recommended) or `superpowers:executing-plans` to implement this plan task-by-task.

---

## Context

PR1-PR3 已经把"扫码登录"跑通：用户可以从前端"添加 WhatsApp 账号"按钮 → 风险 banner →
QR → 扫码成功 → 写 `config.json` + `session.db`，重启 AIjia 自动复用既有 session。
但是 `IMConnector::start()` / `IMConnector::send()` 仍然返 `NotSupported(PR4/PR5)`，
也就是说 **AI 还看不见用户在 WhatsApp 上发的消息**。

PR4 的目标：让 wa-rs `Event::Message` 真正流到 manager → PendingQueueManager → AI
turn。**不**实现出站（PR5）和媒体下载（PR7）和 AI Card 编辑（PR6）。

Spec 来源：`docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md` §3.10
（allow_from 过滤）+ §3.12（quoted reply 解析）+ §4（入站事件 → ChannelMessage，整章）
+ §8（LoggedOut → NeedsReauth）。

### 4 个 brainstorm 决策（非显然且 spec 没钉死的）

**1. `bot.run()` ↔ `BoxStream<ChannelMessage>` 桥接 / `IMConnector::start()` 何时建 Bot**

Bot 是 PR3 `start_pairing_session` 在 manager 调 `begin_whatsapp_registration` /
`connect_whatsapp_from_store` 时就已经 build + run 了。Bot 是单例（一个 JoinHandle）；
wa-rs Client::run 内部自带断线重连，**所以一次启动即可服务整个 connector 生命周期**。
`IMConnector::start(ctx)` **不应该再 build 第二个 Bot**。

设计：连接器持有 `Arc<Mutex<Option<mpsc::Sender<ChannelMessage>>>>` 字段（默认 None）。
`runtime::handle_event` 闭包始终 capture 这个 Arc；处理 `Event::Message` 时 `lock` 看
是否有 sender，有就 push，没有就 drop（pair 前没 receiver 是预期）。
`IMConnector::start(ctx)` 创建 (tx, rx) mpsc，set Some(tx)，返 ReceiverStream(rx)。
`stop()` 取走 tx 让 stream 自然 end。

为什么不让 closure 持 mpsc::Sender 直接 capture？因为 `start()` 可以被 manager 多次
调用（rebuild stream），而 Bot 闭包是 build-once。`Arc<Mutex<Option<Sender>>>` 是
"运行时切换 sink" 的最简实现。

**2. allow_from 过滤时机**

spec §3.10 提的 "Arc<RwLock<Vec<String>>> + refresh_config()" 是一种实现。**简化方案**：
`handle_event::Message` 收到 message 时**直接从 `paths.config_path()` 读 config.json**
（`config::read(&path)`，已存在的 helper）。理由：① config.json 写得少（pair_success
+ PR8 UI 编辑）读得多但量级小（消息 ~5/s peak，每次 fs::read ~50µs）② 不需要显式
refresh，编辑后自动生效 ③ 没有锁竞争 / Arc 字段加。如果 PR8 之后 perf 实测有问题
再换 RwLock。

**3. wa-rs reconnect 与 `ReconnectBackoff`**

wa-rs Client::run() 已经有 `enable_auto_reconnect=true` + 指数退避（最多 30s）。
JoinHandle 只在 graceful shutdown 或 panic 时 resolve，**不会**在普通断网时返。
所以**外层 `ReconnectBackoff` 完全不需要**——把 spec §4.1 那段 retry loop 当成"备用
设计"忽略掉，让 wa-rs 内部重连负责。我们只要在 `Event::LoggedOut` / `Event::StreamReplaced`
时把 NeedsReauth 推给 manager + drop tx 让 BoxStream 结束。

**4. quoted reply 解析（§3.12）**

`wa::Message` 不同 variant 各自有 `context_info: Option<Box<ContextInfo>>`，
`ContextInfo.quoted_message: Option<Box<Message>>` 是被引用的消息。

实现两个内部 helper：
- `context_info_of(&wa::Message) -> Option<&ContextInfo>` —— 跨 6 个常见 variant 查 contextInfo
- `summarize_message(&wa::Message) -> String` —— 提取被引用消息的 plain-text 摘要
  （conversation 直接返；extended_text 取 `.text`；image 用 `[图片] caption`；
  document 用 `[文件 filename]`；其他 fallback `[消息]`）

parser 在拼 ChannelMessage.text 时，如果发现 quoted，prefix `[引用了消息："{summary}"]\n`。

---

## File Structure

新建：
- `src-tauri/src/connector/im/whatsapp/parser.rs` — `Event::Message` → `Option<ChannelMessage>`，
  含 conversation / extended_text / image-caption / document-caption + 不支持类型占位 +
  群 drop + allow_from 过滤 + quoted reply prefix。~280 行（含 ~10 单测）。

修改：
- `src-tauri/src/connector/im/whatsapp/mod.rs` — `pub mod parser;`
- `src-tauri/src/connector/im/whatsapp/connector.rs` — 加 `inbound_tx` 字段；
  `start()` 真实现（创 mpsc + 装 tx + 返 BoxStream）；trait `start()` `ConnectorContext`
  的 `cancel_token` 接进来用于 stop。**不**要触碰 `start_pairing_session` —— 它是
  build Bot 的入口，PR4 只是外挂一个 sink。
- `src-tauri/src/connector/im/whatsapp/runtime.rs` —
  ① `start_bot` 多接一个 `inbound_tx: Arc<Mutex<Option<mpsc::Sender<ChannelMessage>>>>`
     参数（也是 connector 字段的 clone），closure capture
  ② `handle_event` 加 `Event::Message` 分支（调 parser + dedup + push tx）
  ③ 加 `Event::LoggedOut` / `Event::StreamReplaced` 分支（NeedsReauth + drop tx）
  ④ `dedup: Arc<MessageDedupSet>` 也作为 closure capture（manager 不需要外部传，
     connector 内部建一个就行）
- `src-tauri/src/connector/im/manager.rs` — 把"start whatsapp connector + spawn 入站
  worker"提到一个 `spawn_whatsapp_inbound_worker(generation)` 内部 helper；从
  `begin_whatsapp_registration`（pair 成功后）+ `connect_whatsapp_from_store`（启动期）
  各调一次。worker 形态参考 telegram worker（manager.rs:599-887 那段），简化：
  whatsapp 私聊 only，不需要 group 分支；不需要附件下载（PR7）；不需要 telegram 的
  allowlist 路径（已经在 connector 内 filter 过）。
- `src-tauri/src/connector/im/whatsapp/connector.rs` 测试：start() 装 tx + 没消息时
  rx 不消费、stop() drop tx 让 rx 收到 None 等。

不动：
- `factory.rs`、`session.rs`、`config.rs`、`types.rs`、`Cargo.toml`、`commands/channel.rs`、
  其它平台、前端任何文件。
- spec §3.11 reaction（PR6）、§3.10 UI（PR8）、§4.5 observe_session（spec 已说不需要）

---

## Task 1: parser.rs

**Files:**
- Create: `src-tauri/src/connector/im/whatsapp/parser.rs`
- Modify: `src-tauri/src/connector/im/whatsapp/mod.rs`（加 `pub mod parser;`）

- [ ] **Step 1: 写 parser.rs（核心 normalize 函数 + helper + 10 单测）**

骨架：
```rust
//! `Event::Message` → `Option<ChannelMessage>`. spec v3 §4.3 + §3.10 + §3.12.
//!
//! 不支持类型映射占位（让 AI 知道用户发了东西但内容不可用）；群事件 drop；
//! allow_from 列表过滤；quoted reply 前缀。
//!
//! 媒体下载（IMAGE / DOCUMENT 真实下载到 tmp）留 PR7。本 PR 把 IMAGE / DOCUMENT
//! 的 caption 提进 text，attachments 留空。

use std::sync::Arc;

use wa_rs::types::events::Event;
use wa_rs::types::message::MessageInfo;
use wa_rs_proto::whatsapp as wa;

use super::config::WhatsAppChannelConfig;
use crate::connector::im::types::{ChannelMessage, ConversationType};

/// 私聊判定：MessageSource.is_group=false。spec MVP 私聊 only。
pub fn is_private_chat(info: &MessageInfo) -> bool {
    !info.source.is_group
}

/// 转 ChannelMessage 的入口。失败/drop 返 None。caller（runtime.rs）拿到 Some
/// 才往 mpsc tx push。
///
/// allow_from：如 cfg.allow_from 是 Some(non-empty) 且发送方手机号不在列表 → drop（None）。
/// allow_from = None 或 Some(空 vec) → 不过滤。
/// is_from_me=true → 永远 drop（不让 AI 回自己）。
pub fn normalize(
    msg: &wa::Message,
    info: &MessageInfo,
    cfg: Option<&WhatsAppChannelConfig>,
) -> Option<ChannelMessage> {
    if info.source.is_from_me {
        return None;
    }
    if !is_private_chat(info) {
        return None;
    }
    if !is_allowed_sender(&info.source.sender, cfg) {
        log::debug!(
            "[whatsapp] sender {} not in allow_from, dropping",
            info.source.sender
        );
        return None;
    }

    // body：正常文本 / image caption / document caption / 不支持类型占位
    let body = extract_body_text(msg);
    let text = match maybe_quoted_prefix(msg) {
        Some(prefix) => format!("{prefix}{body}"),
        None => body,
    };

    let conv_key = format!("{}@{}", info.source.chat.user, info.source.chat.server);
    let sender_id = format!("{}@{}", info.source.sender.user, info.source.sender.server);

    Some(ChannelMessage {
        msg_id: info.id.to_string(),
        conversation_type: ConversationType::Private,
        conversation_key: conv_key,
        sender_id,
        sender_nick: info.push_name.clone(),
        text,
        robot_code: String::new(),       // whatsapp 单账号无 robot_code 概念
        reply_group_id: String::new(),
        attachments: vec![],             // PR7 才填
        session_webhook: None,
        created_at_ms: Some(info.timestamp.timestamp_millis()),
    })
}

fn extract_body_text(msg: &wa::Message) -> String {
    // 1. 普通文本
    if let Some(s) = msg.conversation.as_ref() {
        if !s.is_empty() {
            return s.clone();
        }
    }
    if let Some(ext) = msg.extended_text_message.as_ref() {
        if let Some(t) = ext.text.as_ref() {
            return t.clone();
        }
    }
    // 2. caption-bearing 类型
    if let Some(img) = msg.image_message.as_ref() {
        return img.caption.clone().unwrap_or_else(|| "[图片]".into());
    }
    if let Some(doc) = msg.document_message.as_ref() {
        let name = doc.file_name.clone().unwrap_or_else(|| "文件".into());
        let cap = doc.caption.clone().unwrap_or_default();
        if cap.is_empty() {
            return format!("[文件：{name}]");
        }
        return format!("[文件：{name}] {cap}");
    }
    if let Some(vid) = msg.video_message.as_ref() {
        let cap = vid.caption.clone().unwrap_or_default();
        if cap.is_empty() {
            return "[不支持的消息类型：视频]".into();
        }
        return format!("[不支持的消息类型：视频] {cap}");
    }
    // 3. 占位类型
    if msg.audio_message.is_some() {
        return "[不支持的消息类型：语音]".into();
    }
    if msg.sticker_message.is_some() {
        return "[不支持的消息类型：表情贴纸]".into();
    }
    if msg.location_message.is_some() || msg.live_location_message.is_some() {
        return "[不支持的消息类型：位置]".into();
    }
    if msg.contact_message.is_some() || msg.contacts_array_message.is_some() {
        return "[不支持的消息类型：联系人]".into();
    }
    // 4. 完全不认识
    "[不支持的消息类型]".into()
}

fn context_info_of(msg: &wa::Message) -> Option<&wa::ContextInfo> {
    if let Some(e) = msg.extended_text_message.as_ref() {
        if e.context_info.is_some() { return e.context_info.as_deref(); }
    }
    if let Some(i) = msg.image_message.as_ref() {
        if i.context_info.is_some() { return i.context_info.as_deref(); }
    }
    if let Some(d) = msg.document_message.as_ref() {
        if d.context_info.is_some() { return d.context_info.as_deref(); }
    }
    if let Some(v) = msg.video_message.as_ref() {
        if v.context_info.is_some() { return v.context_info.as_deref(); }
    }
    if let Some(a) = msg.audio_message.as_ref() {
        if a.context_info.is_some() { return a.context_info.as_deref(); }
    }
    if let Some(s) = msg.sticker_message.as_ref() {
        if s.context_info.is_some() { return s.context_info.as_deref(); }
    }
    None
}

fn maybe_quoted_prefix(msg: &wa::Message) -> Option<String> {
    let ctx = context_info_of(msg)?;
    let quoted = ctx.quoted_message.as_deref()?;
    let summary = summarize_quoted(quoted);
    if summary.is_empty() { return None; }
    Some(format!("[引用了消息：\"{summary}\"]\n"))
}

fn summarize_quoted(msg: &wa::Message) -> String {
    if let Some(s) = msg.conversation.as_ref() {
        if !s.is_empty() { return truncate_for_quote(s); }
    }
    if let Some(e) = msg.extended_text_message.as_ref() {
        if let Some(t) = e.text.as_ref() {
            if !t.is_empty() { return truncate_for_quote(t); }
        }
    }
    if msg.image_message.is_some() {
        let cap = msg.image_message.as_ref()
            .and_then(|i| i.caption.clone()).unwrap_or_default();
        if cap.is_empty() { return "[图片]".into(); }
        return format!("[图片] {}", truncate_for_quote(&cap));
    }
    if msg.document_message.is_some() {
        let name = msg.document_message.as_ref()
            .and_then(|d| d.file_name.clone()).unwrap_or_else(|| "文件".into());
        return format!("[文件：{name}]");
    }
    if msg.audio_message.is_some() { return "[语音]".into(); }
    if msg.video_message.is_some() { return "[视频]".into(); }
    if msg.sticker_message.is_some() { return "[贴纸]".into(); }
    "[消息]".into()
}

fn truncate_for_quote(s: &str) -> String {
    const MAX_CHARS: usize = 60;
    let trimmed = s.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= MAX_CHARS { return trimmed.to_string(); }
    let head: String = chars.into_iter().take(MAX_CHARS).collect();
    format!("{head}...")
}

fn is_allowed_sender(sender: &wa_rs::Jid, cfg: Option<&WhatsAppChannelConfig>) -> bool {
    let allow = match cfg.and_then(|c| c.allow_from.as_ref()) {
        Some(a) if !a.is_empty() => a,
        _ => return true,  // None / 空 = 接收所有
    };
    let phone = format!("+{}", sender.user);
    allow.iter().any(|s| normalize_phone(s) == phone)
}

fn normalize_phone(raw: &str) -> String {
    let cleaned: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    format!("+{cleaned}")
}
```

测试块：覆盖 10 个 case
1. `normalize_drops_self_message` — is_from_me=true 返 None
2. `normalize_drops_group_message` — is_group=true 返 None
3. `normalize_extracts_text_from_conversation`
4. `normalize_extracts_text_from_extended_text`
5. `normalize_image_caption_or_placeholder`
6. `normalize_document_filename_and_caption`
7. `normalize_voice_video_sticker_placeholders`
8. `normalize_quoted_reply_prefix`
9. `allow_from_filters_unlisted_sender`
10. `allow_from_none_or_empty_passes_all`

测试时构造 `wa::Message::default()` 然后挑字段填，info 用 `MessageInfo::default()`
+ override `source.{chat,sender,is_from_me,is_group}` + `id` + `push_name`。
`wa_rs::Jid::default()` 然后填 `user/server`（PR3 测试已经这样做）。

⚠️ wa-rs `Jid` Display 要看下源码。从我们 grep 出来的 jid.rs 看 Jid 有
`pub user / server / agent / device / integrator`，Display 通常是
`user@server`。如果是 `user.device@server`（带 device suffix）或者其他形态，
`format!("{}@{}", info.source.chat.user, info.source.chat.server)` 是显式构造，
绕过 Display 偏差，**这正是稳妥写法**——保持 user@server 简洁形态，PR4 不依赖
device 后缀。

- [ ] **Step 2: 加 `pub mod parser;` 到 mod.rs（alphabetical：放 mod runtime 之前）**

```rust
pub mod config;
pub mod connector;
pub mod parser;        // ← new
pub mod runtime;
pub mod session;
pub mod types;
```

- [ ] **Step 3: 编译 + 跑 parser tests**

```bash
cd src-tauri && cargo test --lib connector::im::whatsapp::parser:: 2>&1 | tail -10
```
Expected: 10 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connector/im/whatsapp/{parser.rs,mod.rs}
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR4 加 parser.rs（normalize Event::Message）

spec v3 §4.3 + §3.10 allow_from + §3.12 quoted reply。

- 私聊 only（is_group=true 返 None）
- is_from_me=true 返 None（不让 AI 回自己）
- allow_from filter：cfg.allow_from Some(non-empty) 且 sender 不在列表 → None
- text 提取覆盖 conversation / extended_text / image-caption /
  document-name+caption / video-caption；audio/sticker/location/contact
  按 spec §4.3 走具体占位文案
- quoted reply prefix `[引用了消息："..."]\n` 截断 60 字符
- attachments 留 PR7；本 PR IMAGE / DOCUMENT 把 caption 直接进 text

10 个 unit test 覆盖各分支 + allow_from 边界。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: connector.rs 加 inbound_tx 字段 + start() 实现

**Files:**
- Modify: `src-tauri/src/connector/im/whatsapp/connector.rs`

- [ ] **Step 1: 加字段**

在 struct 里 `pairing_state` 后加：
```rust
    /// PR4 入站消息 sink。`runtime::handle_event` 的 closure capture 这个 Arc；
    /// `start()` 装 mpsc::Sender 进去；`stop()` 取走 Sender 让 BoxStream 结束。
    /// closure build 一次，sink 可以多次切换——典型"运行时切换 sink"模式。
    pub(crate) inbound_tx:
        Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>>,

    /// PR4 入站去重。connector 内部 owns；不需要外部传。
    pub(crate) dedup: Arc<crate::connector::im::shared::dedup::MessageDedupSet>,
```

`with_status_callback` / `new` 初始化：
```rust
    inbound_tx: Arc::new(tokio::sync::Mutex::new(None)),
    dedup: Arc::new(
        crate::connector::im::shared::dedup::MessageDedupSet::with_default_cap(),
    ),
```

- [ ] **Step 2: 改 `start_pairing_session` 把 inbound_tx + dedup 传进 `start_bot`**

```rust
        let handle = super::runtime::start_bot(
            paths,
            Arc::clone(&self.pairing_state),
            Arc::clone(&self.on_status),
            Arc::clone(&self.inbound_tx),
            Arc::clone(&self.dedup),
        ).await?;
```

- [ ] **Step 3: 实现 `start()` trait 方法**

替换现有 NotSupported 桩：
```rust
    async fn start(
        &self,
        _ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        use futures::stream::StreamExt;
        use tokio_stream::wrappers::ReceiverStream;
        let (tx, rx) = tokio::sync::mpsc::channel::<ChannelMessage>(64);
        // 装入 sink；如果已有旧 sink 则替换（旧 stream 自然 end）
        *self.inbound_tx.lock().await = Some(tx);
        log::info!("[whatsapp] inbound stream attached");
        Ok(ReceiverStream::new(rx).boxed())
    }
```

`ConnectorContext.cancel_token` 不直接订阅——cancel 走 `stop()` 链路（manager 已在
shutdown 时调 stop）。这跟 telegram pattern 一致：cancel 通过 connector.stop() 而非
ctx.cancel_token 直接监听。

- [ ] **Step 4: 改 `stop()` 同时 drop inbound_tx**

```rust
    async fn stop(&self) -> Result<(), ConnectorError> {
        // 1. drop inbound sink → consumer 收到 None
        *self.inbound_tx.lock().await = None;
        // 2. abort bot task
        if let Some(handle) = self.bot_handle.lock().await.take() {
            handle.abort();
            log::info!("[whatsapp] bot task aborted");
        }
        Ok(())
    }
```

- [ ] **Step 5: 跑 connector 测试 + 加新单测**

新加：
```rust
    #[tokio::test]
    async fn start_attaches_inbound_sink_and_returns_box_stream() {
        use futures::StreamExt;
        let c = WhatsAppConnector::new();
        let ctx = test_ctx();
        let mut stream = c.start(ctx).await.expect("start ok");
        // 没人 push tx，stream 应该 pending（用 timeout 验证）
        assert!(c.inbound_tx.lock().await.is_some());
        let res = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            stream.next(),
        ).await;
        assert!(res.is_err(), "stream should pend with no senders posting");
    }

    #[tokio::test]
    async fn stop_drops_inbound_tx_so_stream_ends() {
        use futures::StreamExt;
        let c = WhatsAppConnector::new();
        let ctx = test_ctx();
        let mut stream = c.start(ctx).await.expect("start ok");
        c.stop().await.expect("stop ok");
        assert!(c.inbound_tx.lock().await.is_none());
        // tx 已 drop → next 应该返 None
        let res = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            stream.next(),
        ).await;
        assert_eq!(res.expect("stream not pending"), None);
    }

    #[tokio::test]
    async fn pushing_message_through_inbound_tx_arrives_at_stream() {
        use futures::StreamExt;
        let c = WhatsAppConnector::new();
        let ctx = test_ctx();
        let mut stream = c.start(ctx).await.expect("start ok");
        // 模拟 runtime closure 直接 push
        let tx = c.inbound_tx.lock().await.clone().expect("tx installed");
        tokio::spawn(async move {
            let cm = ChannelMessage {
                msg_id: "M1".into(),
                conversation_type: crate::connector::im::types::ConversationType::Private,
                conversation_key: "k".into(),
                sender_id: "s".into(),
                sender_nick: "Alice".into(),
                text: "hi".into(),
                robot_code: String::new(),
                reply_group_id: String::new(),
                attachments: vec![],
                session_webhook: None,
                created_at_ms: Some(0),
            };
            let _ = tx.send(cm).await;
        });
        let got = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            stream.next(),
        ).await.expect("not pending").expect("not closed");
        assert_eq!(got.msg_id, "M1");
    }
```

删掉旧的 `start_still_returns_not_supported_in_pr2` 测试（它已不再适用）。

- [ ] **Step 6: 编译 + 测试**

```bash
cd src-tauri && cargo test --lib connector::im::whatsapp::connector:: 2>&1 | tail -15
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/connector/im/whatsapp/connector.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR4 connector.start() 装 inbound sink

spec v3 §4.1。

新增字段：
- inbound_tx: Arc<Mutex<Option<mpsc::Sender<ChannelMessage>>>>
- dedup: Arc<MessageDedupSet>（shared，5000 cap）

start(ctx)：
- 创 (tx, rx) mpsc(64)
- *inbound_tx = Some(tx)
- 返 ReceiverStream(rx).boxed()

stop()：drop tx → consumer 自然收到 None；保留原有 bot_handle.abort 行为。

start_pairing_session 把 inbound_tx + dedup 透传给 runtime::start_bot。
runtime closure capture 这个 Arc，PR4 Task 3 在 Event::Message 分支用。

删掉过时的 PR2 NotSupported 测试，加 3 个新测覆盖装 sink / drop sink /
端到端推送。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: runtime.rs 加 Event::Message + LoggedOut 分支

**Files:**
- Modify: `src-tauri/src/connector/im/whatsapp/runtime.rs`

- [ ] **Step 1: 改 `start_bot` 签名 + closure capture**

```rust
pub async fn start_bot(
    paths: WhatsAppPaths,
    pairing_state: Arc<Mutex<PairingState>>,
    on_status: Arc<dyn Fn(...) + ...>,
    inbound_tx: Arc<Mutex<Option<mpsc::Sender<ChannelMessage>>>>,
    dedup: Arc<MessageDedupSet>,
) -> anyhow::Result<JoinHandle<()>> {
    // ...
    let inbound_for_closure = Arc::clone(&inbound_tx);
    let dedup_for_closure = Arc::clone(&dedup);
    let mut bot = Bot::builder()
        .with_backend(backend)
        // ...
        .on_event(move |event, _client| {
            let paths = paths_for_closure.clone();
            let pairing_state = Arc::clone(&state_for_closure);
            let on_status = Arc::clone(&on_status_for_closure);
            let inbound_tx = Arc::clone(&inbound_for_closure);
            let dedup = Arc::clone(&dedup_for_closure);
            async move {
                handle_event(event, &paths, pairing_state, on_status, inbound_tx, dedup).await;
            }
        })
        .build().await?;
    // ...
}
```

- [ ] **Step 2: 改 `handle_event` 加 Message / LoggedOut / StreamReplaced**

```rust
pub(crate) async fn handle_event(
    event: Event,
    paths: &WhatsAppPaths,
    pairing_state: Arc<Mutex<PairingState>>,
    on_status: Arc<dyn Fn(...) + ...>,
    inbound_tx: Arc<Mutex<Option<mpsc::Sender<ChannelMessage>>>>,
    dedup: Arc<MessageDedupSet>,
) {
    use crate::connector::im::types::ChannelConnectionState;
    match event {
        // ... PR3 已有的 PairingQrCode / PairSuccess / PairError / Connected ...

        Event::Message(msg, info) => {
            // 1. dedup
            let msg_id = info.id.to_string();
            if !dedup.observe(&msg_id).await {
                log::debug!("[whatsapp] duplicate msg_id {}, dropping", msg_id);
                return;
            }
            // 2. 读 config.json 拿 allow_from（每次都读，简单&自动 fresh）
            let cfg = match super::config::read(&paths.config_path()) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("[whatsapp] failed to read config.json: {e}, allowing all");
                    None
                }
            };
            // 3. parser
            let cm = match super::parser::normalize(&msg, &info, cfg.as_ref()) {
                Some(c) => c,
                None => return,        // dropped by parser (group / is_from_me / allow_from)
            };
            // 4. push 到 sink（如果有 receiver）
            if let Some(tx) = inbound_tx.lock().await.as_ref() {
                if let Err(e) = tx.try_send(cm) {
                    log::warn!("[whatsapp] inbound channel send failed: {e}");
                }
            } else {
                log::trace!("[whatsapp] no inbound receiver, dropping msg {}", msg_id);
            }
        }

        Event::LoggedOut(lo) => {
            log::warn!(
                "[whatsapp] LoggedOut on_connect={} reason={:?}",
                lo.on_connect, lo.reason
            );
            *inbound_tx.lock().await = None;
            on_status(
                ChannelConnectionState::NeedsReauth,
                Some(format!("WhatsApp 已登出: {:?}", lo.reason)),
            );
        }

        Event::StreamReplaced(_) => {
            log::warn!("[whatsapp] StreamReplaced — another device took over");
            *inbound_tx.lock().await = None;
            on_status(
                ChannelConnectionState::NeedsReauth,
                Some("已在其他设备登录".into()),
            );
        }

        // 显式 drop 这些 noisy events
        Event::Receipt(_) | Event::Presence(_) | Event::ChatPresence(_) => {}

        _ => {}
    }
}
```

⚠️ `tx.try_send` 而不是 `tx.send` —— send 会 await（buffer 满时阻塞），可能堵住整个
event loop。用 try_send，buffer 满（>64 in-flight）时 drop 单条消息只 log warn。
WhatsApp 一对一私聊不会突发到 64+，绝大多数情况 try_send 立即成功。

- [ ] **Step 3: 加 4 个新单测**

```rust
    #[tokio::test]
    async fn handle_event_message_pushes_to_inbound_when_attached() {
        // 装 tx → push 一条 conversation message → rx 收到对应 ChannelMessage
    }

    #[tokio::test]
    async fn handle_event_message_dropped_when_no_inbound_tx() {
        // 不装 tx → handler 不 panic，silent drop
    }

    #[tokio::test]
    async fn handle_event_dedup_drops_repeat() {
        // 同 msg_id 推 2 次，只到达 1 次
    }

    #[tokio::test]
    async fn handle_event_logged_out_drops_tx_and_emits_needs_reauth() {
        // 装 tx + on_status spy → emit Event::LoggedOut → tx None + on_status 收到 NeedsReauth
    }
```

构造 `Event::Message(Arc<wa::Message>, Arc<MessageInfo>)`：
```rust
let msg = Arc::new({
    let mut m = wa::Message::default();
    m.conversation = Some("hello".into());
    m
});
let info = Arc::new({
    let mut i = MessageInfo::default();
    i.id = "M_TEST_1".into();
    i.source.is_group = false;
    i.source.is_from_me = false;
    i.source.chat = wa_rs::Jid {
        user: "8613912345678".into(),
        server: "s.whatsapp.net".into(),
        ..Default::default()
    };
    i.source.sender = i.source.chat.clone();
    i.push_name = "Alice".into();
    i
});
```

`MessageId` 是不是 String 直接传？grep wacore_binary。如果 `MessageId` 是
newtype（String wrapper），用 `.into()` 或显式构造。如果实测出错，看 wacore_binary
的源码调整。

- [ ] **Step 4: 编译 + 跑 runtime tests**

```bash
cd src-tauri && cargo test --lib connector::im::whatsapp::runtime:: 2>&1 | tail -15
```
Expected: PR3 4 个 + PR4 4 个 = 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/connector/im/whatsapp/runtime.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR4 runtime 加 Event::Message + LoggedOut 处理

spec v3 §4 + §8。

start_bot 签名加 inbound_tx + dedup 参数；closure 各 clone Arc capture。

handle_event 新分支：
- Event::Message：dedup → 读 config.json 拿 allow_from → parser::normalize
  → tx.try_send（满了只 log warn）
- Event::LoggedOut / StreamReplaced：drop inbound tx 让 BoxStream 结束 +
  on_status 推 NeedsReauth + last_error 文案
- Event::Receipt / Presence / ChatPresence 显式 drop（避免 noisy log）

依赖：wa-rs 内部已有 enable_auto_reconnect=true，外层 ReconnectBackoff
不接（spec §4.4 说"避免双层退避相互踩"）。普通断线由 wa-rs 自己处理；
我们只在 LoggedOut/StreamReplaced 时切 NeedsReauth。

4 个新单测覆盖 push / no-receiver-drop / dedup / LoggedOut。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: manager.rs 加 spawn_whatsapp_inbound_worker

**Files:**
- Modify: `src-tauri/src/connector/im/manager.rs`

- [ ] **Step 1: 看 telegram worker 抄一份**

Run:
```bash
sed -n '545,890p' src-tauri/src/connector/im/manager.rs
```

学习骨架：worker 拿 connector → start(ctx) → 循环 recv → router get_or_create_session
→ enqueue 进 PendingQueueManager → emit channel:message 给前端。

- [ ] **Step 2: 加 `spawn_whatsapp_inbound_worker` helper**

放在 `connect_whatsapp_from_store` 附近（manager.rs ~2647 行附近）。

```rust
    /// Phase 4 PR4：起入站 worker。manager 持 dyn IMConnector handle 调
    /// `start()` 拿 BoxStream<ChannelMessage>，spawn task 把每条消息走
    /// router → PendingQueueManager 链路。
    ///
    /// 调用时机：
    /// - `connect_whatsapp_from_store`（启动期、config.json 已有）
    /// - `begin_whatsapp_registration` 扫码成功后（PR4 暂在 start_pairing_session
    ///   后立即调；扫码完成前 inbound 流量为空，没问题）
    async fn spawn_whatsapp_inbound_worker(&self) -> Result<()> {
        let new_token = CancellationToken::new();
        let ctx = ConnectorContext {
            config_store: Arc::clone(&self.config_store),
            secure_storage: None,
            ask_coordinator: self.ask_coordinator.as_ref().map(Arc::clone),
            pending_manager: Arc::clone(&self.pending_manager),
            cancel_token: new_token.clone(),
        };
        let connector = {
            let map = self.connectors.read().await;
            map.get(&Platform::Whatsapp).cloned()
                .ok_or_else(|| anyhow::anyhow!("whatsapp connector not registered"))?
        };
        let mut message_stream = connector
            .start(ctx)
            .await
            .map_err(|e| anyhow::anyhow!("whatsapp connector start failed: {e}"))?;

        // 注册 cancel token（让 set_whatsapp_connection_state 等能 cancel 旧 stream）
        self.platform_state_mutate(Platform::Whatsapp, |s| {
            s.stream_cancel = Some(new_token.clone());
        }).await;

        let adapter = Arc::clone(&self.chat_adapter);
        let conv_store = Arc::clone(&self.conversation_store);
        let sessions_path = self.sessions_paths[&Platform::Whatsapp].clone();
        let convs = Arc::clone(&self.conversations);
        let app_handle = self.app_handle.clone();
        let channel_session_ids_ref = Arc::clone(&self.channel_session_ids);
        let pending_manager_ref = Arc::clone(&self.pending_manager);
        let message_cancel = new_token.clone();
        // whatsapp 单账号 router_key 用常量
        let router_key = "whatsapp".to_string();

        tokio::spawn(async move {
            use futures::StreamExt;
            let mut router = match ChannelSessionRouter::migrate_or_load(
                &sessions_path,
                conv_store.as_ref(),
            ) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("[channel/whatsapp] failed to load router: {:#}", e);
                    return;
                }
            };

            loop {
                let msg = tokio::select! {
                    _ = message_cancel.cancelled() => {
                        log::info!("[channel/whatsapp] worker cancelled");
                        break;
                    }
                    next = message_stream.next() => match next {
                        Some(m) => m,
                        None => {
                            log::info!("[channel/whatsapp] worker stream ended");
                            break;
                        }
                    }
                };
                log::info!(
                    "[channel/whatsapp] worker received msg msg_id={} text_len={}",
                    msg.msg_id, msg.text.len()
                );

                let conv_type = msg.conversation_type.clone();
                let conv_key = msg.conversation_key.clone();
                let sender_nick = msg.sender_nick.clone();
                let text = msg.text.clone();
                let store_ref = Arc::clone(&conv_store);
                let sender_nick_for_create = sender_nick.clone();
                let conv_key_for_create = conv_key.clone();
                let session_id = match router.get_or_create_session(
                    &conv_type,
                    &router_key,
                    &conv_key,
                    || {
                        let title = format!("WhatsApp 私聊 {sender_nick_for_create}");
                        let id = uuid::Uuid::new_v4().to_string();
                        store_ref.create_conversation_with_im_source(
                            &id, &title, Platform::Whatsapp.as_str(),
                        ).map_err(|e| anyhow::anyhow!(e))?;
                        Ok(id)
                    },
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("[channel/whatsapp] get_or_create_session failed: {:#}", e);
                        continue;
                    }
                };

                {
                    let mut ids = channel_session_ids_ref.write()
                        .expect("channel_session_ids poisoned");
                    ids.insert(session_id.clone());
                }

                {
                    let mut convs_lock = convs.write().await;
                    if !convs_lock.iter().any(|c| c.session_id == session_id) {
                        convs_lock.push(ChannelConversation {
                            session_id: session_id.clone(),
                            platform: Platform::Whatsapp,
                            conversation_type: conv_type.clone(),
                            external_id: conv_key.clone(),
                            display_name: sender_nick.clone(),
                            unread_count: 0,
                            robot_code: router_key.clone(),
                            is_active_robot: true,
                        });
                    }
                }

                let preview = if text.chars().count() > 30 {
                    format!("{}...", text.chars().take(30).collect::<String>())
                } else { text.clone() };
                let _ = app_handle.emit("channel:message", &ChannelMessagePayload {
                    platform: "whatsapp".into(),
                    session_id: session_id.clone(),
                    sender_nick: sender_nick.clone(),
                    text_preview: preview,
                });

                let request = build_channel_chat_request(
                    session_id.clone(), &conv_type, &sender_nick, &text,
                    vec![],     // PR4 没附件，PR7 才有
                    &Vec::<String>::new(),
                );
                let pending_item =
                    super::shared::pending_adapter::build_pending_item_from_telegram(
                        &msg.msg_id, &conv_type, &sender_nick, &text,
                        vec![], &Vec::<String>::new(),
                    );

                let adapter_for_turn = Arc::clone(&adapter);
                let pending_manager_for_send = Arc::clone(&pending_manager_ref);
                let session_for_enqueue =
                    crate::runtime::ids::SessionId::new(session_id.clone());
                tokio::spawn(async move {
                    if let Err(e) = pending_manager_for_send
                        .enqueue_or_send(session_for_enqueue, pending_item)
                        .await
                    {
                        log::warn!("[channel/whatsapp] pending enqueue failed: {:#}", e);
                        return;
                    }
                    let _ = adapter_for_turn.run_chat_request(request).await;
                });
            }
        });

        Ok(())
    }
```

⚠️ 几个**真实存在但需要 implementer 验证**的依赖：
1. `self.sessions_paths[&Platform::Whatsapp]` —— PR1/PR2 应已经在 sessions_paths 加
   Whatsapp 对应路径。如果没加：grep `sessions_paths.*Telegram\|sessions_paths.*Whatsapp`
   看看，必要时仿 Telegram 加一行。
2. `build_pending_item_from_telegram` 命名是 telegram-specific 但 shape 平台中性
   (`msg_id, conv_type, nick, text, attachments, failures`)。复用即可，**不要**新加
   `_from_whatsapp`（YAGNI）。
3. `build_channel_chat_request` —— grep 看签名，按现有形态调。

- [ ] **Step 3: 在两个入口调用**

修改 `connect_whatsapp_from_store`：在 `concrete.start_pairing_session(paths).await?;`
之后加：
```rust
        if let Err(e) = self.spawn_whatsapp_inbound_worker().await {
            log::error!("[channel/whatsapp] failed to spawn inbound worker: {:#}", e);
        }
```

修改 `begin_whatsapp_registration`：在 `concrete.start_pairing_session(paths).await?;`
之后加同样调用。这样扫码后立即 attach inbound sink，不需要等用户做任何事。

- [ ] **Step 4: 编译**

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
```
Expected: `Finished` clean。**任何 unimplemented! / FIXME / TODO 不允许 ship**。

- [ ] **Step 5: 全 IM 回归**

```bash
cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -5
```
Expected: 0 new failures vs. PR3 baseline.

- [ ] **Step 6: review_im_layering**

```bash
cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -5
```
Expected: 3 passed.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/connector/im/manager.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR4 manager 起入站 worker

spec v3 §4。

新增 `spawn_whatsapp_inbound_worker(&self)`：
- ConnectorContext + new CancellationToken
- connector.start(ctx) → BoxStream<ChannelMessage>
- spawn worker：循环 next() → router.get_or_create_session →
  PendingQueueManager.enqueue_or_send → ChatAdapter.run_chat_request
- emit channel:message 给前端
- stream 结束（None）即 worker 退出（LoggedOut / StreamReplaced /
  stop 都让 connector 把 inbound tx drop）

入口接入：
- connect_whatsapp_from_store（启动期复用 session.db）调一次
- begin_whatsapp_registration（扫码成功）调一次

复用：
- shared::pending_adapter::build_pending_item_from_telegram（命名 telegram
  但参数平台中性，YAGNI 不新加 _from_whatsapp）
- ChannelSessionRouter / ChannelConversation / ChannelMessagePayload

跟 telegram worker 结构相同，简化为：私聊 only / 无附件下载 / 无 allowlist
（filter 已在 connector 内做）。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: 收尾校验

**Files:**（无修改）

- [ ] **Step 1: 全 PR4 测试**

```bash
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -15
```
Expected: PR3 baseline 31 + PR4 (10 parser + 3 connector + 4 runtime) = ~48 pass.

- [ ] **Step 2: 全 IM 回归**

```bash
cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -5
```

- [ ] **Step 3: review_im_layering**

```bash
cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -5
```

- [ ] **Step 4: Clippy on PR4 files**

```bash
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'src/connector/im/whatsapp/' | head -20
```
Expected: 0 warnings on PR4-touched files.

- [ ] **Step 5: Cargo fmt**

```bash
cd src-tauri && cargo fmt -- --check 2>&1 | head -5
```

- [ ] **Step 6: 前端 tsc + lint**

```bash
pnpm exec tsc --noEmit 2>&1 | tail -3
pnpm lint 2>&1 | tail -3
```
Expected: 0 new errors.（PR4 没动前端，应该全 clean。）

- [ ] **Step 7: 实测（可选，时间紧只验证 dev server 不崩）**

```bash
pnpm tauri:dev
```
- 已配对的账号下，从手机给桌面 AIjia 发一条文字 → 控制台看到
  `[channel/whatsapp] worker received msg` + `channel:message` 事件 →
  前端"WhatsApp 私聊 …"会话里出现这条消息 → AI 起 turn 回复（PR5 出站还没做，
  PR4 阶段 AI 回复会以 `NotSupported` 失败——这是预期，会在 PR5 修）

---

## Self-Review

### 1. Spec 覆盖（v3 §3.10 + §3.12 + §4 + §8）

| spec 子段 | task | 状态 |
|---|---|---|
| §4.1 worker | Task 4 spawn_whatsapp_inbound_worker | ✅ |
| §4.2 dispatch | Task 3 handle_event Event::Message + LoggedOut | ✅ |
| §4.3 parser 表 | Task 1 parser.rs（私聊 / 群 drop / 7 不支持类型占位） | ✅ |
| §4.4 dedup + reconnect | Task 3 shared::MessageDedupSet + 不接外层 backoff（trust wa-rs） | ✅ |
| §4.5 observe_session | spec 已说 whatsapp 不需要 | N/A |
| §3.10 allow_from | Task 1 parser is_allowed_sender；Task 3 每事件读 config.json | ✅ |
| §3.11 reaction | PR6 范围 | N/A |
| §3.12 quoted reply | Task 1 parser maybe_quoted_prefix | ✅ |
| §8.1 LoggedOut | Task 3 Event::LoggedOut + StreamReplaced → NeedsReauth | ✅ |

### 2. Placeholder scan

- Task 4 manager 几处依赖（`sessions_paths`, `build_pending_item_from_telegram`, `build_channel_chat_request`）—— 实施时 grep 确认存在；如有 mismatch 按现有形态调整，不写 unimplemented!()。
- 对 wa-rs `MessageId` / `Jid` Display / 默认构造细节有不确定 —— Task 1 + Task 3 测试构造时**先 grep wacore_binary** 验证字段，按真实 API 调整。同 PR3 同款实测套路。

### 3. 类型一致性

- `ChannelMessage.created_at_ms: Option<i64>` ← `info.timestamp.timestamp_millis()`
- `ChannelMessage.conversation_key` 用 `chat.user@chat.server`（绕过 Display 偏差）
- `ChannelMessage.msg_id` 用 `info.id.to_string()`
- `Platform::Whatsapp.as_str() == "whatsapp"`（PR1 已锁）
- 单账号 `router_key = "whatsapp"` 常量；与 begin/poll 的 `device_code` 复用同字符串

### 4. 不在 PR4 范围（明确避免）

- 出站 send（PR5）/ AI Card（PR6）/ 媒体下载（PR7）/ allow_from UI（PR8）/ reaction（PR6）
- 添加新的 shared 抽象（不抽 markdown_simple、不抽 fallback aicard）
- 修改 wa-rs 依赖版本

---

## Execution Handoff

**估时**：5 个 task / ~700 行新代码（含测试）/ 实际 1.5-2 小时（subagent-driven）。

执行步骤：
1. 把本 plan 内容拷到 `docs/superpowers/plans/2026-05-20-im-whatsapp-phase4-pr4.md`
2. 跑 `superpowers:subagent-driven-development` skill，让它逐 task 执行
3. 期间任一 task BLOCKED（wa-rs 实测 API 差异） → 当面看 caveat 段落调整后续 task
4. 全部 commit + 测试通过后，更新 memory `project_phase4_whatsapp_progress.md` PR4 状态行

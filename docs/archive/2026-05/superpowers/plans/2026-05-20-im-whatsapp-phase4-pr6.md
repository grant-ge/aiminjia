# Phase 4 WhatsApp PR6 — 出站 AI Card 占位 + 增量编辑

> Subagent-driven execution. Pre-brainstorm done with user; 4 decisions confirmed.

---

## Context

PR5 让 AI 能给 WhatsApp 用户回**最终**文本，但 `AiCardChunk { final_chunk: false }`
当前是静默丢中间 chunk。本 PR 真接增量编辑路径：

- 1st chunk → 发 ⏳ reaction 到用户那条原消息 + 发 placeholder 文本消息
- 后续 chunk → 累积，达到 2s + edit_count<6 → edit_message 编辑 placeholder
- final chunk → 最后一次 edit + 把 ⏳ reaction 换成 ✅
- AiCardFail → reaction → ❌ + edit placeholder 到"生成失败"

Spec：`docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md` §6 + §3.11。

## 4 decisions (user-confirmed)

1. **原 msg_id 反查**：manager worker push 入站后调
   `concrete_whatsapp.remember_inbound(session_id, msg_id, jid)`，connector
   内存 `session_inbound: Arc<RwLock<HashMap<SessionId, LastInbound>>>`。send 走
   reaction/edit 路径时读它。仿 telegram `remember_session` 模式。
2. **AiCard 双路径**：reaction ⏳ + placeholder 文本消息（spec §3.11 + §6.1）。
3. **状态机存哪**：connector 内部 `fallback_buffers: Mutex<HashMap<SessionId, WhatsAppAiCardSession>>`。
   不复用 shared::aicard_fallback（它是"占位+最终"两态模型，跟 PR6 的"占位+增量
   edit+final"不匹配；spec §2 明确"aicard.rs 不抽 shared"）。
4. **edit 失败**：静默丢 + 下次 chunk 触发条件重试。保 placeholder_msg_id 不动。

---

## File structure

新建：
- `src-tauri/src/connector/im/whatsapp/aicard.rs` — `WhatsAppAiCardSession` 结构体
  + `aicard_handle` + `aicard_fail` 状态机。~250 行 + ~10 单测（纯逻辑测试，不真发；
  状态机用 mock IO trait 让测试可断言"是否会发送/编辑哪条/哪种"）。

修改：
- `mod.rs` — `pub mod aicard;`
- `types.rs` — 加 `pub struct WhatsAppLastInbound { pub jid: String, pub msg_id: String, pub sender_jid: String, pub is_group: bool }`（reaction key 需要 from_me / participant 推断；spec 是私聊 only 所以 is_group 默认 false，但留字段方便 future）
- `connector.rs` —
  ① 加 `session_inbound: Arc<RwLock<HashMap<String, WhatsAppLastInbound>>>` 字段
  ② 加 `fallback_buffers: Arc<Mutex<HashMap<String, aicard::WhatsAppAiCardSession>>>` 字段
  ③ 加 `pub async fn remember_inbound(&self, session_id, last)` inherent method（manager 调）
  ④ `send()` 的 `AiCardChunk` / `AiCardFail` 分支真接 aicard 路径
  ⑤ `stop()` 清空 session_inbound + fallback_buffers
- `sender.rs` — 加 3 个 helper：
  ① `pub async fn send_reaction(client, target_jid, target_msg_id, sender_jid, is_group, emoji) -> Result<(), ConnectorError>`
  ② `pub async fn edit_text(client, jid_str, original_id, new_body) -> Result<(), ConnectorError>`
  ③ `pub async fn send_text_returning_id(client, jid_str, body) -> Result<String, ConnectorError>`（PR5 的 send_text 已返 String，但语义上 send_text 出参 sent_id 是 placeholder_msg_id 的源——保留命名即可，不重命名）
- `manager.rs` — `spawn_whatsapp_inbound_worker` 里在 push pending 之前调
  `concrete_whatsapp.remember_inbound(session_id.clone(), WhatsAppLastInbound { jid, msg_id, sender_jid, is_group })`
  - **拿 concrete handle**：worker spawn 之前 `self.whatsapp_concrete.read().await.clone()`，
    把 `Arc<WhatsAppConnector>` 也 capture 进 task

不动：parser / markdown / runtime / config / session / factory / 其它平台 / 前端。

---

## Task 1 — aicard.rs 状态机（纯逻辑 + 10 单测）

新建 `src-tauri/src/connector/im/whatsapp/aicard.rs`：

```rust
//! AI Card 占位 + 增量编辑 状态机。spec v3 §6.1 + §3.11。

use std::time::{Duration, Instant};

const EDIT_THROTTLE: Duration = Duration::from_secs(2);
const EDIT_COUNT_LIMIT: u32 = 6;

#[derive(Debug, Default)]
pub struct WhatsAppAiCardSession {
    pub placeholder_msg_id: Option<String>,
    pub accumulated_text: String,
    pub last_edit_at: Option<Instant>,
    pub edit_count: u32,
    pub finalized: bool,
    pub reaction_sent: bool,
}

#[derive(Debug, PartialEq)]
pub enum AiCardAction {
    /// 不发任何消息（中间 chunk，未达节流阈值）
    Buffer,
    /// 1st chunk：发 reaction 到原消息 + 发 placeholder 文本（拿到 placeholder_msg_id 写回 session）
    StartPlaceholder { text: String },
    /// 后续 chunk 触发节流：edit placeholder（已有 placeholder_msg_id）
    EditPlaceholder { msg_id: String, text: String },
    /// final 到达且没有 placeholder（1st 就 final）：直接发完整文本，不走 placeholder
    SendFinal { text: String },
    /// final 到达且已有 placeholder：最后一次 edit 把完整结果落到 placeholder
    EditFinal { msg_id: String, text: String },
    /// AiCardFail：edit placeholder 到 "生成失败" 文案；如果没 placeholder 也无所谓（连占位都没发就 fail，跳过）
    EditFailMessage { msg_id: String },
    /// finalized 之后又收到 chunk：log warn，不动
    DropAfterFinalized,
    /// 已 finalized 又收到 fail：no-op
    Noop,
}

impl WhatsAppAiCardSession {
    pub fn observe_chunk(&mut self, delta: &str, final_chunk: bool, now: Instant) -> AiCardAction {
        if self.finalized {
            return AiCardAction::DropAfterFinalized;
        }
        self.accumulated_text.push_str(delta);

        match (self.placeholder_msg_id.as_ref(), final_chunk) {
            // 1st chunk + 非 final：发 placeholder
            (None, false) => AiCardAction::StartPlaceholder {
                text: "_正在生成回复..._".into(),
            },
            // 1st chunk + final：直接发完整文本
            (None, true) => {
                self.finalized = true;
                AiCardAction::SendFinal {
                    text: std::mem::take(&mut self.accumulated_text),
                }
            }
            // 后续 chunk
            (Some(msg_id), is_final) => {
                let msg_id_owned = msg_id.clone();
                let elapsed = self.last_edit_at.map(|t| now.duration_since(t)).unwrap_or(Duration::ZERO);
                let should_edit = is_final
                    || (elapsed >= EDIT_THROTTLE && self.edit_count < EDIT_COUNT_LIMIT);
                if !should_edit {
                    return AiCardAction::Buffer;
                }
                if is_final {
                    self.finalized = true;
                    AiCardAction::EditFinal {
                        msg_id: msg_id_owned,
                        text: self.accumulated_text.clone(),
                    }
                } else {
                    AiCardAction::EditPlaceholder {
                        msg_id: msg_id_owned,
                        text: self.accumulated_text.clone(),
                    }
                }
            }
        }
    }

    /// caller 调 send 拿到 placeholder_msg_id 后写回 session。
    pub fn record_placeholder(&mut self, msg_id: String, now: Instant) {
        self.placeholder_msg_id = Some(msg_id);
        self.last_edit_at = Some(now);
        self.edit_count = 1;
    }

    /// caller edit_message 成功后调，更新 last_edit_at + edit_count。
    /// edit 失败时**不**调（spec PR6 决策 #4：静默丢，下次 chunk 重试）。
    pub fn record_edit_success(&mut self, now: Instant) {
        self.last_edit_at = Some(now);
        self.edit_count = self.edit_count.saturating_add(1);
    }

    pub fn observe_fail(&mut self) -> AiCardAction {
        if self.finalized {
            return AiCardAction::Noop;
        }
        self.finalized = true;
        match self.placeholder_msg_id.as_ref() {
            Some(msg_id) => AiCardAction::EditFailMessage { msg_id: msg_id.clone() },
            None => AiCardAction::Noop,  // 占位还没发就 fail，没法 edit；直接 noop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t0() -> Instant { Instant::now() }

    #[test]
    fn first_chunk_non_final_emits_placeholder() {
        let mut s = WhatsAppAiCardSession::default();
        let a = s.observe_chunk("hello", false, t0());
        assert!(matches!(a, AiCardAction::StartPlaceholder { .. }));
    }

    #[test]
    fn first_chunk_final_sends_complete_and_skips_placeholder() {
        let mut s = WhatsAppAiCardSession::default();
        let a = s.observe_chunk("done", true, t0());
        match a {
            AiCardAction::SendFinal { text } => assert_eq!(text, "done"),
            other => panic!("expected SendFinal, got {other:?}"),
        }
        assert!(s.finalized);
    }

    #[test]
    fn chunk_within_throttle_returns_buffer() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        let a = s.observe_chunk("more", false, t0() + Duration::from_millis(500));
        assert_eq!(a, AiCardAction::Buffer);
    }

    #[test]
    fn chunk_after_throttle_emits_edit() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        let a = s.observe_chunk("more", false, t0() + Duration::from_secs(3));
        match a {
            AiCardAction::EditPlaceholder { msg_id, .. } => assert_eq!(msg_id, "P1"),
            other => panic!("expected EditPlaceholder, got {other:?}"),
        }
    }

    #[test]
    fn edit_count_caps_at_limit_returns_buffer() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        s.edit_count = EDIT_COUNT_LIMIT;
        let a = s.observe_chunk("more", false, t0() + Duration::from_secs(5));
        // 即使节流时间过了，count 满也不 edit
        assert_eq!(a, AiCardAction::Buffer);
    }

    #[test]
    fn final_after_throttle_emits_edit_final() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        let a = s.observe_chunk("end", true, t0() + Duration::from_secs(3));
        match a {
            AiCardAction::EditFinal { msg_id, .. } => assert_eq!(msg_id, "P1"),
            other => panic!("expected EditFinal, got {other:?}"),
        }
        assert!(s.finalized);
    }

    #[test]
    fn final_within_throttle_still_emits_edit_final() {
        // spec §6.3：final 强制突破 throttle/count 上限 1 次
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        s.edit_count = EDIT_COUNT_LIMIT;  // count 满也突破
        let a = s.observe_chunk("end", true, t0() + Duration::from_millis(100));
        assert!(matches!(a, AiCardAction::EditFinal { .. }));
    }

    #[test]
    fn chunk_after_finalized_returns_drop() {
        let mut s = WhatsAppAiCardSession::default();
        s.finalized = true;
        let a = s.observe_chunk("late", false, t0());
        assert_eq!(a, AiCardAction::DropAfterFinalized);
    }

    #[test]
    fn fail_with_placeholder_emits_edit_fail_msg() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        let a = s.observe_fail();
        match a {
            AiCardAction::EditFailMessage { msg_id } => assert_eq!(msg_id, "P1"),
            other => panic!("expected EditFailMessage, got {other:?}"),
        }
    }

    #[test]
    fn fail_without_placeholder_is_noop() {
        let mut s = WhatsAppAiCardSession::default();
        let a = s.observe_fail();
        assert_eq!(a, AiCardAction::Noop);
    }
}
```

### Verification

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp::aicard:: 2>&1 | tail -15  # 10 pass
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep 'whatsapp/aicard.rs' | head -5  # 0
cd src-tauri && cargo fmt -- --check 2>&1 | grep whatsapp/aicard || echo OK
```

### Commit

```
git add src-tauri/src/connector/im/whatsapp/aicard.rs src-tauri/src/connector/im/whatsapp/mod.rs
git commit -m "feat(connector/im/whatsapp): PR6 加 aicard.rs 状态机

spec v3 §6.1 + §3.11。

WhatsAppAiCardSession + observe_chunk(delta, final, now) → AiCardAction：
- 1st non-final → StartPlaceholder（caller 发文本拿 msg_id 后调 record_placeholder）
- 1st final → SendFinal（直接发完整，不走 placeholder）
- 后续 chunk 节流 throttle 2s + edit_count<6 → EditPlaceholder
- final 强制突破 throttle/count 1 次 → EditFinal
- finalized 后再来 chunk → DropAfterFinalized
- fail 有 placeholder → EditFailMessage；无 placeholder → Noop

record_placeholder / record_edit_success 给 caller 提供成功路径回写；
edit 失败时不调，下次 chunk 触发条件时自然重试（决策 4）。

10 个单测覆盖所有分支 + 边界（throttle / count cap / final 突破 / finalized
后 chunk / fail 双路径）。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2 — sender.rs 加 reaction + edit 包装

修改 `src-tauri/src/connector/im/whatsapp/sender.rs`，**追加** 2 个函数（不动 PR5 的 send_text / map_send_error）：

```rust
/// 发 reaction（emoji）到指定消息上。spec §3.11。
///
/// `target_msg_id` 是用户那条原消息的 ID（PR4 parser 写入 ChannelMessage.msg_id）。
/// `target_jid` 是用户的 jid（私聊场景=chat jid=sender jid）。
/// `is_group` PR4 是私聊 only，always false，但留参数以备 future。
///
/// 空字符串 emoji = "撤回 reaction"，不调用方场景。
pub async fn send_reaction(
    client: &Arc<Client>,
    chat_jid_str: &str,
    target_msg_id: &str,
    sender_jid_str: &str,
    is_group: bool,
    emoji: &str,
) -> Result<(), ConnectorError> {
    let chat_jid = Jid::from_str(chat_jid_str)
        .map_err(|e| ConnectorError::Fatal(format!("invalid chat jid '{chat_jid_str}': {e}")))?;
    let key = wa::MessageKey {
        remote_jid: Some(chat_jid_str.to_string()),
        from_me: Some(false),  // 我们要 react 用户发的消息
        id: Some(target_msg_id.to_string()),
        participant: if is_group {
            Some(sender_jid_str.to_string())
        } else {
            None
        },
    };
    let reaction = wa::message::ReactionMessage {
        key: Some(key),
        text: Some(emoji.to_string()),
        sender_timestamp_ms: Some(chrono::Utc::now().timestamp_millis()),
        ..Default::default()
    };
    let msg = wa::Message {
        reaction_message: Some(reaction),
        ..Default::default()
    };
    client
        .send_message(chat_jid, msg)
        .await
        .map_err(map_send_error)?;
    Ok(())
}

/// edit 现有消息的文本内容。spec §6.1。
pub async fn edit_text(
    client: &Arc<Client>,
    chat_jid_str: &str,
    original_msg_id: &str,
    new_body: &str,
) -> Result<(), ConnectorError> {
    let to = Jid::from_str(chat_jid_str)
        .map_err(|e| ConnectorError::Fatal(format!("invalid jid '{chat_jid_str}': {e}")))?;
    let new_content = wa::Message {
        conversation: Some(new_body.to_string()),
        ..Default::default()
    };
    client
        .edit_message(to, original_msg_id.to_string(), new_content)
        .await
        .map_err(map_send_error)?;
    Ok(())
}
```

**测试**：reaction / edit 都需要真 Client 才能跑，纯单元测试 mock 不动。新加 2 个测试只是
**编译时签名锁**（确认函数存在 + 参数类型正确）—— 用 `#[cfg(test)] fn _compile_test()`
风格，不实际调用。或者**完全不加测试**，让集成测试 PR8 时再覆盖。**决策：不加测试**，
PR5 sender 也是同样原则（send_text 无单测，只测 map_send_error）。

### Verification

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp::sender:: 2>&1 | tail -10  # 仍 8（PR5 baseline 不动）
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep 'whatsapp/sender.rs' | head -5
cd src-tauri && cargo fmt -- --check 2>&1 | grep whatsapp/sender || echo OK
```

### Commit

```
git add src-tauri/src/connector/im/whatsapp/sender.rs
git commit -m "feat(connector/im/whatsapp): PR6 sender 加 send_reaction + edit_text

spec v3 §3.11 + §6.1。

send_reaction(client, chat_jid, target_msg_id, sender_jid, is_group, emoji)：
- 构造 wa::message::ReactionMessage { key, text, sender_timestamp_ms }
- key.from_me = false（我们 react 的是用户那条入站消息）
- key.participant 群聊时填，私聊 None
- 包成 wa::Message { reaction_message: Some(...) } 走 send_message

edit_text(client, chat_jid, original_msg_id, new_body)：
- wa::Message { conversation: Some(new_body), .. } 作为 new_content
- 调 client.edit_message(to, original_id, new_content)

错误走 map_send_error 同 send_text 一套规则。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3 — types.rs 加 WhatsAppLastInbound + connector.rs 接 reaction/edit 路径

修改 `src-tauri/src/connector/im/whatsapp/types.rs`：追加

```rust
/// PR6：manager worker 把入站消息的元信息写到这里，send() 走 reaction/edit
/// 路径时读回来。spec §6.1 + §3.11。
#[derive(Debug, Clone)]
pub struct WhatsAppLastInbound {
    /// 对话方 jid（私聊场景 = chat jid = sender jid 同一个）
    pub chat_jid: String,
    /// 发送者 jid（群聊时跟 chat_jid 不同；私聊时同 chat_jid）
    pub sender_jid: String,
    /// 用户那条入站消息的 msg_id（react 时填 key.id）
    pub msg_id: String,
    /// 是否群聊（PR4 是 private-only，固定 false；留字段以备 future）
    pub is_group: bool,
}
```

修改 `src-tauri/src/connector/im/whatsapp/connector.rs`：

**A. 加字段** 在 `bot_client` 后：
```rust
    /// PR6 入站消息上下文表，manager worker 写、send() 读。spec §6.1。
    pub(crate) session_inbound: Arc<tokio::sync::RwLock<std::collections::HashMap<String, super::types::WhatsAppLastInbound>>>,
    /// PR6 AI Card 状态机 per session。
    pub(crate) fallback_buffers: Arc<tokio::sync::Mutex<std::collections::HashMap<String, super::aicard::WhatsAppAiCardSession>>>,
```

**B. init in `with_status_callback`**：
```rust
    session_inbound: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
    fallback_buffers: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
```

**C. 加 inherent method**：
```rust
    /// manager worker 在 push pending 之前调，把入站消息上下文存下来，给 send() 反查用。
    pub async fn remember_inbound(&self, session_id: String, last: super::types::WhatsAppLastInbound) {
        self.session_inbound.write().await.insert(session_id, last);
    }
```

**D. 改 `send()` 的 AiCardChunk / AiCardFail 分支**（替换 PR5 的 final-only 降级）。

新版 send() 的 AiCardChunk 路径（伪码）：

```rust
ReplyContent::AiCardChunk { delta, final_chunk } => {
    let session_id = target.session_id.clone();
    let chat_jid_str = target.external_conversation_key.clone();
    let now = Instant::now();
    
    // 1. 拿状态 + observe
    let action = {
        let mut buffers = self.fallback_buffers.lock().await;
        let session = buffers.entry(session_id.clone()).or_default();
        session.observe_chunk(&delta, final_chunk, now)
    };
    
    // 2. 执行 action
    use super::aicard::AiCardAction;
    match action {
        AiCardAction::Buffer | AiCardAction::DropAfterFinalized | AiCardAction::Noop => Ok(()),
        AiCardAction::SendFinal { text } => {
            // 1st chunk 就 final：直接发完整文本（跟 PR5 一致行为）
            let _ = super::sender::send_text(&client, &chat_jid_str, &text).await?;
            self.fallback_buffers.lock().await.remove(&session_id);
            Ok(())
        }
        AiCardAction::StartPlaceholder { text } => {
            // 先发 reaction ⏳ 到用户那条原消息（best-effort，失败不阻塞）
            if !already_sent_reaction(&session_id) {
                if let Some(last) = self.session_inbound.read().await.get(&session_id).cloned() {
                    let _ = super::sender::send_reaction(
                        &client, &last.chat_jid, &last.msg_id, &last.sender_jid, last.is_group, "⏳",
                    ).await
                        .inspect_err(|e| log::debug!("[whatsapp] reaction send failed (best-effort): {e}"));
                }
            }
            // 然后发 placeholder 文本
            let msg_id = super::sender::send_text(&client, &chat_jid_str, &text).await?;
            // record_placeholder + mark reaction_sent
            let mut buffers = self.fallback_buffers.lock().await;
            if let Some(session) = buffers.get_mut(&session_id) {
                session.record_placeholder(msg_id, now);
                session.reaction_sent = true;
            }
            Ok(())
        }
        AiCardAction::EditPlaceholder { msg_id, text } => {
            // edit 失败静默丢 + 下次 chunk 重试（不调 record_edit_success）
            match super::sender::edit_text(&client, &chat_jid_str, &msg_id, &text).await {
                Ok(()) => {
                    let mut buffers = self.fallback_buffers.lock().await;
                    if let Some(session) = buffers.get_mut(&session_id) {
                        session.record_edit_success(now);
                    }
                }
                Err(e) => log::debug!("[whatsapp] edit_placeholder failed (silent retry): {e}"),
            }
            Ok(())
        }
        AiCardAction::EditFinal { msg_id, text } => {
            // final edit 也允许失败：失败时 fallback 发新的 send_text 完整内容（不让用户丢内容）
            if let Err(e) = super::sender::edit_text(&client, &chat_jid_str, &msg_id, &text).await {
                log::warn!("[whatsapp] edit_final failed, falling back to send_text: {e}");
                let _ = super::sender::send_text(&client, &chat_jid_str, &text).await?;
            }
            // 换 reaction ⏳ → ✅（best-effort）
            if let Some(last) = self.session_inbound.read().await.get(&session_id).cloned() {
                let _ = super::sender::send_reaction(
                    &client, &last.chat_jid, &last.msg_id, &last.sender_jid, last.is_group, "✅",
                ).await
                    .inspect_err(|e| log::debug!("[whatsapp] final reaction failed: {e}"));
            }
            self.fallback_buffers.lock().await.remove(&session_id);
            Ok(())
        }
        AiCardAction::EditFailMessage { .. } => unreachable!("EditFailMessage 只能从 observe_fail 出来"),
    }
}
ReplyContent::AiCardFail => {
    let session_id = target.session_id.clone();
    let chat_jid_str = target.external_conversation_key.clone();
    let action = {
        let mut buffers = self.fallback_buffers.lock().await;
        let session = buffers.entry(session_id.clone()).or_default();
        session.observe_fail()
    };
    use super::aicard::AiCardAction;
    match action {
        AiCardAction::EditFailMessage { msg_id } => {
            let _ = super::sender::edit_text(&client, &chat_jid_str, &msg_id, "_[生成失败]_")
                .await
                .inspect_err(|e| log::debug!("[whatsapp] edit_fail_message failed: {e}"));
            // reaction ⏳ → ❌
            if let Some(last) = self.session_inbound.read().await.get(&session_id).cloned() {
                let _ = super::sender::send_reaction(
                    &client, &last.chat_jid, &last.msg_id, &last.sender_jid, last.is_group, "❌",
                ).await
                    .inspect_err(|e| log::debug!("[whatsapp] fail reaction failed: {e}"));
            }
        }
        AiCardAction::Noop => {
            // 没 placeholder：还没发任何消息就 fail，发一条简单失败提示
            let _ = super::sender::send_text(&client, &chat_jid_str, "❌ 处理失败，请重试").await?;
        }
        other => log::warn!("[whatsapp] unexpected aicard action on fail: {other:?}"),
    }
    self.fallback_buffers.lock().await.remove(&session_id);
    Ok(())
}
```

⚠️ caveat：实施时 `client` 变量从 `self.bot_client.lock().await.clone()` None → Transient 拿，跟 PR5 同款；上面伪码省略了。implementer 把 PR5 那段"取 client + None → Transient"的代码块**保留在函数顶部**，所有 ReplyContent 都先过这关。

**E. 改 `stop()`**：清空 session_inbound + fallback_buffers
```rust
    self.session_inbound.write().await.clear();
    self.fallback_buffers.lock().await.clear();
    // ... 已有 bot_client / inbound_tx clear + bot_handle abort
```

**F. 测试**：

- 删 PR5 加的 `send_aicard_chunk_non_final_returns_transient_lockfirst`（行为变了）
- 加：
  - `aicard_chunk_routes_through_state_machine_when_bot_running`（用 ChannelMessage push 进 inbound_tx 后 mock？ — 难做。**改成**：测 connector 的 fallback_buffers / session_inbound HashMap 字段在 send 后的状态，不真发 IO，验证状态机被命中——实施时如果设计不允许 unit-test，那就 _ignored 标真 IO 测试 + 由 PR8 集成测试覆盖。）

  实际上 — send() 内部需要真 Client，没法纯 unit-test 全链路。**决策**：删测试，PR8 集成测试覆盖。但要加 1 个**新单测**：
  - `send_aicard_returns_transient_when_bot_not_running` — 没装 bot_client → 走 lock-first 返 Transient，跟 PR5 行为一致。这个能纯 unit-test 因为不到 send_text。
  
  以及把 PR5 的 `send_returns_transient_when_bot_not_running`（Text 路径）保留不动。

### Verification

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp::connector:: 2>&1 | tail -15  # 9 - 1 + 1 = 9
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -10
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'whatsapp/(connector|types)\.rs' | head -10
cd src-tauri && cargo fmt -- --check 2>&1 | grep whatsapp/ || echo OK
```

### Commit

```
git add src-tauri/src/connector/im/whatsapp/types.rs src-tauri/src/connector/im/whatsapp/connector.rs
git commit -m "feat(connector/im/whatsapp): PR6 connector 接 reaction + 增量编辑路径

spec v3 §6 + §3.11。

types.rs 加 WhatsAppLastInbound { chat_jid, sender_jid, msg_id, is_group } —
PR6 reaction 需要原 msg_id + chat jid + (群聊时) sender_jid。

connector 新增 2 字段：
- session_inbound: RwLock<HashMap<SessionId, WhatsAppLastInbound>>
  manager worker 写、send() 读
- fallback_buffers: Mutex<HashMap<SessionId, WhatsAppAiCardSession>>
  aicard 状态机 per session

inherent method remember_inbound(session_id, last) 给 manager 调。

send() AiCardChunk 真接增量路径：
- 走 aicard::observe_chunk 拿 AiCardAction
- StartPlaceholder：先发 reaction ⏳ 到用户原消息（best-effort，失败不阻塞）
  + 发 placeholder 文本，拿 placeholder_msg_id 回写 session
- EditPlaceholder：edit 失败静默丢，下次 chunk 重试（不调 record_edit_success）
- EditFinal：edit 失败 fallback send_text 完整内容；reaction ⏳ → ✅
- SendFinal：1st chunk 就 final → 直接 send_text 跳过 placeholder

send() AiCardFail：observe_fail → edit placeholder 到 \"_[生成失败]_\" +
  reaction ⏳ → ❌；如果没 placeholder 直接 send_text \"❌ 处理失败，请重试\"。

stop() 清空 session_inbound + fallback_buffers。

测试调整：删 PR5 加的 non-final-returns-transient 测试（行为变了，PR6 改
为状态机驱动）；加 send_aicard_returns_transient_when_bot_not_running
锁住 lock-first ordering。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4 — manager.rs worker 调 remember_inbound

修改 `src-tauri/src/connector/im/manager.rs::spawn_whatsapp_inbound_worker`：

1. **worker spawn 之前**额外拿一个 concrete handle：
   ```rust
   let concrete_for_worker = self.whatsapp_concrete.read().await.clone();
   ```
2. **worker tokio::spawn 内**：在 push pending 之前调 `remember_inbound`：
   ```rust
   if let Some(concrete) = concrete_for_worker.clone() {
       concrete.remember_inbound(session_id.clone(), super::whatsapp::types::WhatsAppLastInbound {
           chat_jid: conv_key.clone(),
           sender_jid: msg.sender_id.clone(),
           msg_id: msg.msg_id.clone(),
           is_group: matches!(conv_type, ConversationType::Group),
       }).await;
   }
   ```
3. concrete_for_worker `Arc<WhatsAppConnector>` clone into worker — Arc 可 .clone() 廉价。

### Verification

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -3
cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -3  # 3 passed
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep manager.rs | head -10
cd src-tauri && cargo fmt -- --check 2>&1 | grep manager.rs || echo OK
```

### Commit

```
git add src-tauri/src/connector/im/manager.rs
git commit -m "feat(connector/im/whatsapp): PR6 manager worker 调 remember_inbound

spec v3 §6.1。

spawn_whatsapp_inbound_worker 在 push pending 之前拿 concrete connector handle
调 remember_inbound(session_id, WhatsAppLastInbound { chat_jid, sender_jid,
msg_id, is_group })。send() 的 reaction / edit 路径靠这个表反查原消息上下文。

concrete_for_worker = self.whatsapp_concrete.read().await.clone() 在 spawn
之前拿；worker 内 .clone() Arc 廉价。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5 — 收尾校验

```bash
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -3
cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -3
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'whatsapp/' | head -10
cd src-tauri && cargo fmt -- --check 2>&1 | head -5
cd .. && pnpm exec tsc --noEmit 2>&1 | tail -3
```

Expected: 71 + 10 = 81 whatsapp tests pass; 0 new clippy/tsc warnings; fmt clean.

更新 memory PR6 行。

---

## Self-Review

| spec | task |
|---|---|
| §6.1 状态机 | Task 1 aicard.rs |
| §6.2 触发条件 | Task 1 (throttle / count) |
| §6.3 final 突破上限 | Task 1 (`final_within_throttle_still_emits_edit_final` 测试) |
| §6.4 占位文案 | Task 3 (`_正在生成回复..._`) |
| §6.5 cleanup | Task 3 (`remove(&session_id)` after EditFinal/AiCardFail) |
| §3.11 reaction | Task 2 send_reaction + Task 3 ⏳/✅/❌ |
| §3.11 降级 | Task 3 (reaction 失败 best-effort 不阻塞) |

无 unimplemented!/TODO。

执行：task 1-4 sequential（Task 4 依赖 Task 3 加的 remember_inbound 方法），Task 5 collat。

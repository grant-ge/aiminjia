# Phase 4 WhatsApp PR5 — 出站文本 + markdown strip + 错误映射

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`
> (recommended) or `superpowers:executing-plans` to implement this plan task-by-task.

---

## Context

PR4 已经把入站消息打通，但 `IMConnector::send()` 仍然返 `NotSupported(PR5 ...)`。
所以入站到达 → AI 起 turn → adapter.send_chat_request → 走 reply_forwarder →
**connector.send 失败** → 用户看不到 AI 回复。

PR5 的目标：让 AI 真能给 WhatsApp 用户回文本/Markdown 消息。**不**实现 AI Card
增量编辑（PR6）和媒体回复（不在 Phase 4 范围）。

Spec 来源：`docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md` §5。

### 4 个 brainstorm 决策（已 user-confirmed）

**1. 怎么拿 `Arc<Client>` 给 send()**

PR3/PR4 没把 Bot 的 client 存进 connector。PR5 加 `bot_client: Arc<Mutex<Option<Arc<Client>>>>`
字段，跟 inbound_tx 同款"运行时切换"模式：runtime.rs::start_bot 在 spawn handle 之前
调 `bot.client()` 拿 Arc<Client> 写进字段；stop() 取走。send() 读字段 None 时返
`Transient("bot not running")`。

**2. wa-rs anyhow::Error → ConnectorError 4 类**

wa-rs `Client::send_message` 返 `Result<String, anyhow::Error>`，**没有结构化错误枚举**。
靠错误文本关键字粗分（spec §5.3 的"4 类表"是 wa-rs 假想 enum，实际只能近似）：

| 关键字（lowercase contains） | 映射 |
|---|---|
| `not logged in` / `unauthorized` / `401` / `403` / `auth` / `revoked` | `AuthExpired` |
| `timeout` / `connection` / `refused` / `reset` / `closed` / `network` / `transport` | `Transient` |
| 其余 | `Fatal` |

匹配做在 `sender.rs::map_send_error` helper，`#[cfg(test)]` 锁 6 行规则（每类 2 个样本）。

**3. Markdown strip**

先 grep 确认仓库**没有**现成 markdown-to-text helper（telegram/feishu 是 markdown
直发，不 strip；dingtalk 是 AI Card 不需要 strip）。所以 PR5 自己写 `markdown.rs`，
覆盖 spec §5.2 的 8 行规则：

| 输入 markdown | 输出 |
|---|---|
| `**粗体**` | `*粗体*`（双星 → 单星，WhatsApp 的粗体是 `*x*`） |
| `*斜体*` 或 `_斜体_` | `_斜体_`（统一 underscore） |
| `# 标题` / `## 二级` / ... | 标题文字（前缀 `#+ ` 去掉） |
| `` `code` `` | `code`（反引号去掉） |
| ` ```block``` ` | block 内容（三连反引号去掉，换行原样保留） |
| `[link](url)` | `link (url)` |
| `> 引用` | 引用文字 |
| `- 列表` / `* 列表` / `1. 列表` | `• 列表` |

实现思路：行级处理 + inline 处理。先按 `\n` split，每行：① 检测 ` ``` ` 块边界（多行 fence）；
② 应用前缀替换（标题 / 引用 / 列表）；③ 应用 inline 替换（粗体/斜体/code/link）。
~150 行代码 + ~15 单测覆盖各规则的边界。

不抽 `shared::markdown_simple` —— Phase 5 spec 已经有"将来再抽"伏笔，但 PR5 单
平台需求，YAGNI。

**4. AiCardChunk / AiCardFail 在 PR5 怎么处理**

跟 telegram/connector.rs:217 同款 final-only 降级：

```rust
ReplyContent::AiCardChunk { delta, final_chunk } => {
    if final_chunk { send_text(jid, &delta).await }
    else { Ok(()) }   // 静默丢中间 chunk，等 final 一次性
}
ReplyContent::AiCardFail => send_text(jid, "❌ 处理失败，请重试").await,
```

PR6 时改成真编辑路径（占位 + 增量 edit_message）。

---

## File Structure

新建：
- `src-tauri/src/connector/im/whatsapp/sender.rs` — 出站调用 wa-rs `Client::send_message`
  的薄包装 + 错误映射 + jid 解析。~120 行 + ~10 单测。
- `src-tauri/src/connector/im/whatsapp/markdown.rs` — `pub fn strip_to_wa(s: &str) -> String`
  —— spec §5.2 8 行规则。~150 行 + ~15 单测。

修改：
- `src-tauri/src/connector/im/whatsapp/mod.rs` — `pub mod sender;` + `pub mod markdown;`
- `src-tauri/src/connector/im/whatsapp/connector.rs` —
  ① 加 `bot_client: Arc<Mutex<Option<Arc<Client>>>>` 字段
  ② `start_pairing_session` 把 `Arc::clone(&self.bot_client)` 透传给 `runtime::start_bot`
  ③ `stop()` drop bot_client（在 abort 之前）
  ④ `send()` 真实现：解析 jid，按 ReplyContent 分支，调 sender::send_text，错误映射
- `src-tauri/src/connector/im/whatsapp/runtime.rs` —
  ① `start_bot` 加 `bot_client_slot` 参数
  ② 在 `bot.run().await` 之前 `*bot_client_slot.lock().await = Some(bot.client())`
- `src-tauri/src/connector/im/whatsapp/types.rs` — **可能需要给 `WhatsAppSessionTarget`
  加 `Default` derive 或字段，验证 PR4 是否已有 session_id → jid 反查路径**（如果
  没建，PR5 就用 `target.external_conversation_key` 当 jid 用——manager worker 在 PR4
  push ChannelMessage 时已经写了 conversation_key 用 `user@server` 形态，trait 那边
  ReplyTarget 透传，所以 send() 直接 parse `target.external_conversation_key` 就行）

不动：
- factory.rs / session.rs / config.rs / parser.rs / Cargo.toml / commands/channel.rs
- manager.rs（PR4 worker 已 wire 好；PR5 send 是 trait method，manager 不需要新增方法）
- 前端
- 其它平台

---

## Task 1: markdown.rs（spec §5.2 strip 规则）

**Files:**
- Create: `src-tauri/src/connector/im/whatsapp/markdown.rs`
- Modify: `src-tauri/src/connector/im/whatsapp/mod.rs`（加 `pub mod markdown;`）

- [ ] **Step 1: grep 确认无现成 helper**

```bash
grep -rn 'markdown_to_text\|markdown_strip\|strip_markdown\|to_plain_text' src-tauri/src/ | head -10
```
Expected: 0 hits（如果有 hits，先看是不是公共 helper 可以复用；多半不是）。

- [ ] **Step 2: 写 markdown.rs**

```rust
//! Markdown → WhatsApp 受限格式。spec v3 §5.2。
//!
//! WhatsApp 文本格式仅支持：
//! - `*粗体*`（单星，不是双星）
//! - `_斜体_`（下划线，不是单星）
//! - `~删除~`（波浪号）—— 本 strip 不主动转换 markdown 删除线（不在 spec 表里）
//!
//! 8 行规则（spec §5.2 钉死，禁改）：
//! | `**粗体**`            → `*粗体*`     |
//! | `*斜体*` / `_斜体_`   → `_斜体_`     |
//! | `# 标题` / `## ...`   → 去前缀 `#+ ` |
//! | `` `code` ``          → `code`       |
//! | ` ```block``` `       → `block` 内容（保留换行）|
//! | `[link](url)`         → `link (url)` |
//! | `> 引用`              → 引用文字     |
//! | `- list` / `1. list`  → `• list`     |

/// 把 markdown 文本规整为 WhatsApp 可识别的最小子集。
pub fn strip_to_wa(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_fence = false;

    for line in input.split_inclusive('\n') {
        // 行尾的 \n 单独处理，主体走 trimmed
        let (body, has_nl) = match line.strip_suffix('\n') {
            Some(b) => (b, true),
            None => (line, false),
        };

        // ``` fence 边界
        let trimmed = body.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            // fence 那行整行去掉（不输出 ``` 标记 / 不输出 lang 标识）
            continue;
        }
        if in_fence {
            // fence 内：原样输出（带行尾换行如有）
            out.push_str(body);
            if has_nl { out.push('\n'); }
            continue;
        }

        // 行级前缀：标题 / 引用 / 列表
        let stripped = strip_line_prefix(body);

        // inline 替换
        let inlined = strip_inline(&stripped);
        out.push_str(&inlined);
        if has_nl { out.push('\n'); }
    }
    out
}

fn strip_line_prefix(line: &str) -> String {
    let trimmed = line.trim_start();
    let leading_ws = &line[..line.len() - trimmed.len()];

    // 标题 # / ## / ### …
    if let Some(after) = trimmed.strip_prefix('#') {
        let mut rest = after;
        while let Some(r) = rest.strip_prefix('#') { rest = r; }
        if let Some(after_space) = rest.strip_prefix(' ') {
            return format!("{leading_ws}{after_space}");
        }
        // `#word` 没空格不当标题（保持原样防误吃 hashtag）
        return line.to_string();
    }
    // 引用 `> text`
    if let Some(after) = trimmed.strip_prefix("> ") {
        return format!("{leading_ws}{after}");
    }
    if trimmed == ">" {
        return leading_ws.to_string();
    }
    // 无序列表：`- ` / `* ` / `+ `
    for marker in ["- ", "* ", "+ "] {
        if let Some(after) = trimmed.strip_prefix(marker) {
            return format!("{leading_ws}• {after}");
        }
    }
    // 有序列表：`1. ` / `12. ` …
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
    if i > 0 && i + 2 <= bytes.len() && bytes[i] == b'.' && bytes[i + 1] == b' ' {
        let after = &trimmed[i + 2..];
        return format!("{leading_ws}• {after}");
    }
    line.to_string()
}

fn strip_inline(line: &str) -> String {
    let mut s = line.to_string();
    // 1. 反引号代码 `code`：去反引号（不嵌套，简单 first-match）
    s = replace_pairs(&s, '`', '`', |inner| inner.to_string());
    // 2. 链接 [text](url) → "text (url)"
    s = strip_links(&s);
    // 3. 双星粗体 **x** → *x*  （**先于** *x*，否则会被吃成空斜体）
    s = strip_bold(&s);
    // 4. 单星斜体 *x* → _x_
    //    用 underscore 形态统一 —— spec 表第 2 行
    s = replace_pairs(&s, '*', '*', |inner| format!("_{inner}_"));
    // 5. _x_ 已经是目标形态，保留不动
    s
}

fn strip_bold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 3 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            // find 接下来的 **
            if let Some(close) = find_double_star(&chars, i + 2) {
                out.push('*');
                for &c in &chars[i + 2..close] { out.push(c); }
                out.push('*');
                i = close + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn find_double_star(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == '*' && chars[i + 1] == '*' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn replace_pairs<F>(s: &str, open: char, close: char, transform: F) -> String
where F: Fn(&str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut buf = String::new();
    let mut in_pair = false;
    while let Some(c) = chars.next() {
        if !in_pair && c == open {
            in_pair = true;
            buf.clear();
        } else if in_pair && c == close {
            out.push_str(&transform(&buf));
            in_pair = false;
        } else if in_pair {
            buf.push(c);
        } else {
            out.push(c);
        }
    }
    if in_pair {
        // 不闭合：原样吐 open + buf
        out.push(open);
        out.push_str(&buf);
    }
    out
}

fn strip_links(s: &str) -> String {
    // 简单 [text](url) 扫描；不处理 nested。
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(rb) = find_byte(bytes, i + 1, b']') {
                if rb + 1 < bytes.len() && bytes[rb + 1] == b'(' {
                    if let Some(rp) = find_byte(bytes, rb + 2, b')') {
                        let text = &s[i + 1..rb];
                        let url = &s[rb + 2..rp];
                        out.push_str(&format!("{text} ({url})"));
                        i = rp + 1;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_byte(bytes: &[u8], start: usize, b: u8) -> Option<usize> {
    bytes[start..].iter().position(|&x| x == b).map(|p| p + start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn passthrough_plain_text() { assert_eq!(strip_to_wa("hello"), "hello"); }
    #[test] fn bold_double_star_to_single() { assert_eq!(strip_to_wa("**hi**"), "*hi*"); }
    #[test] fn italic_star_to_underscore() { assert_eq!(strip_to_wa("*hi*"), "_hi_"); }
    #[test] fn italic_underscore_passthrough() { assert_eq!(strip_to_wa("_hi_"), "_hi_"); }
    #[test] fn code_inline_strip() { assert_eq!(strip_to_wa("a `code` b"), "a code b"); }
    #[test] fn link_inline_to_paren() { assert_eq!(strip_to_wa("see [docs](https://x.y)!"), "see docs (https://x.y)!"); }
    #[test] fn heading_one_strip() { assert_eq!(strip_to_wa("# Title"), "Title"); }
    #[test] fn heading_three_strip() { assert_eq!(strip_to_wa("### Sub Sub"), "Sub Sub"); }
    #[test] fn quote_strip() { assert_eq!(strip_to_wa("> quoted"), "quoted"); }
    #[test] fn dash_list_to_bullet() { assert_eq!(strip_to_wa("- item"), "• item"); }
    #[test] fn ordered_list_to_bullet() { assert_eq!(strip_to_wa("1. first\n2. second"), "• first\n• second"); }
    #[test] fn fenced_code_block_strips_fences_keeps_body() {
        assert_eq!(strip_to_wa("```\nlet x = 1;\n```"), "let x = 1;\n");
    }
    #[test] fn unclosed_bold_keeps_original() { assert_eq!(strip_to_wa("**hi"), "**hi"); }
    #[test] fn nested_bold_italic() {
        // **bold *italic* end** —— 我们简单实现：bold 优先匹配双星，剩余 *italic* 走单星
        assert_eq!(strip_to_wa("**bold *italic* end**"), "*bold _italic_ end*");
    }
    #[test] fn multi_line_mixed() {
        let input = "# Title\n\n- a\n- b\n\n**bold** _italic_";
        assert_eq!(strip_to_wa(input), "Title\n\n• a\n• b\n\n*bold* _italic_");
    }
}
```

⚠️ caveat：手写 inline 替换边界 case 多。**测试驱动 implement** —— 先写测试，run，
按 fail 调代码。如果 `nested_bold_italic` 顺序不对，可以接受 deviation——本测试钉
"我们简单实现"。spec §5.2 没有 nested rule，简单贪婪实现即可。

- [ ] **Step 3: mod.rs 加 `pub mod markdown;`（alphabetical）**

```rust
pub mod config;
pub mod connector;
pub mod markdown;     // ← new
pub mod parser;
pub mod runtime;
pub mod session;
pub mod types;
```

- [ ] **Step 4: 编译 + 测试**

```bash
cd src-tauri && cargo test --lib connector::im::whatsapp::markdown:: 2>&1 | tail -20
```
Expected: 15 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/connector/im/whatsapp/markdown.rs src-tauri/src/connector/im/whatsapp/mod.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR5 加 markdown.rs（strip_to_wa）

spec v3 §5.2。把 markdown 规整为 WhatsApp 受限格式：
- **bold** → *bold*
- *italic* / _italic_ → _italic_
- # 标题 → 标题（前缀去掉）
- `code` → code（反引号去掉）
- ```block``` → block 内容（fence 去掉，body 保留换行）
- [link](url) → link (url)
- > quote → quote
- - / * / + / 1. 列表 → • 列表

15 个 unit test 覆盖各规则 + 边界（未闭合双星 / 嵌套 / 多行）。

不抽 shared::markdown_simple —— PR5 单平台需求，YAGNI。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: sender.rs（wa-rs Client::send_message 包装 + 错误映射）

**Files:**
- Create: `src-tauri/src/connector/im/whatsapp/sender.rs`
- Modify: `src-tauri/src/connector/im/whatsapp/mod.rs`（加 `pub mod sender;`）

- [ ] **Step 1: 写 sender.rs**

```rust
//! 出站文本发送 + 错误映射。spec v3 §5.1 + §5.3。
//!
//! wa-rs `Client::send_message(to: Jid, msg: wa::Message) -> Result<String, anyhow::Error>`
//! 没有结构化错误枚举，所以错误分类靠文本关键字粗分（spec §5.3 4 类近似）。

use std::str::FromStr;
use std::sync::Arc;

use wa_rs::client::Client;
use wa_rs::Jid;
use wa_rs::wa_rs_proto::whatsapp as wa;

use crate::connector::im::trait_def::ConnectorError;

/// 把 plain text 包成 wa::Message::conversation 发出。
/// `external_key` 形如 `8613912345678@s.whatsapp.net`（PR4 parser 写入 ChannelMessage.conversation_key
/// 的形态）。返回 Ok(sent_msg_id) / Err。
pub async fn send_text(
    client: &Arc<Client>,
    external_key: &str,
    body: &str,
) -> Result<String, ConnectorError> {
    let to = Jid::from_str(external_key).map_err(|e| {
        ConnectorError::Fatal(format!("invalid jid '{external_key}': {e}"))
    })?;
    let msg = wa::Message {
        conversation: Some(body.to_string()),
        ..Default::default()
    };
    client.send_message(to, msg).await.map_err(map_send_error)
}

/// 把 wa-rs 裸 anyhow::Error 按文本关键字归到 ConnectorError 4 类。
/// **关键字大小写无关**。spec §5.3。
pub fn map_send_error(e: anyhow::Error) -> ConnectorError {
    let msg = format!("{e:#}");
    let low = msg.to_lowercase();

    // AuthExpired：用户在手机端登出 / 设备解链 / token 失效
    if contains_any(&low, &["not logged in", "unauthorized", "401", "403", "auth", "revoked", "logged out"]) {
        return ConnectorError::AuthExpired(msg);
    }

    // Transient：网络抖动 / 连接断开 / 服务端重置
    if contains_any(&low, &["timeout", "connection", "refused", "reset", "closed", "network", "transport", "rate limit", "ratelimit", "429"]) {
        return ConnectorError::Transient(msg);
    }

    // 其它：归 Fatal（无效 jid / 消息过长 / 协议错）
    ConnectorError::Fatal(msg)
}

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(s: &str) -> anyhow::Error { anyhow::anyhow!("{s}") }

    #[test]
    fn auth_expired_when_not_logged_in() {
        let r = map_send_error(err("Not logged in"));
        assert!(matches!(r, ConnectorError::AuthExpired(_)), "got {r:?}");
    }

    #[test]
    fn auth_expired_when_unauthorized() {
        assert!(matches!(map_send_error(err("HTTP 401 Unauthorized")), ConnectorError::AuthExpired(_)));
    }

    #[test]
    fn auth_expired_on_revoked() {
        assert!(matches!(map_send_error(err("device session revoked")), ConnectorError::AuthExpired(_)));
    }

    #[test]
    fn transient_on_timeout() {
        assert!(matches!(map_send_error(err("operation timed out after 30s")), ConnectorError::Transient(_)));
    }

    #[test]
    fn transient_on_connection_reset() {
        assert!(matches!(map_send_error(err("connection reset by peer")), ConnectorError::Transient(_)));
    }

    #[test]
    fn transient_on_rate_limit() {
        assert!(matches!(map_send_error(err("HTTP 429 rate limit exceeded")), ConnectorError::Transient(_)));
    }

    #[test]
    fn fatal_on_unknown_error() {
        assert!(matches!(map_send_error(err("internal protocol error: unknown stanza")), ConnectorError::Fatal(_)));
    }

    #[test]
    fn fatal_on_invalid_message() {
        assert!(matches!(map_send_error(err("message exceeds maximum length")), ConnectorError::Fatal(_)));
    }
}
```

⚠️ caveat：`use wa_rs::client::Client` —— grep 确认；PR4 Task 1 已确认 `wa_rs::wa_rs_proto::whatsapp as wa`
是正确路径（顶层 re-export）。如果 `Client` 不在 `wa_rs::client`，看 wa-rs 的 lib.rs：
PR1 grep 已经发现 `pub use client::Client;` 在 lib.rs，所以 `wa_rs::Client` 也能用。
两者均可，按 PR3 runtime.rs 现在的导入风格选一致写法。

- [ ] **Step 2: mod.rs 加 `pub mod sender;`**

```rust
pub mod config;
pub mod connector;
pub mod markdown;
pub mod parser;
pub mod runtime;
pub mod sender;       // ← new
pub mod session;
pub mod types;
```

- [ ] **Step 3: 编译 + 测试**

```bash
cd src-tauri && cargo test --lib connector::im::whatsapp::sender:: 2>&1 | tail -20
```
Expected: 8 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connector/im/whatsapp/sender.rs src-tauri/src/connector/im/whatsapp/mod.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR5 加 sender.rs（send_text + 错误映射）

spec v3 §5.1 + §5.3。

send_text(client, jid_str, body)：
- Jid::from_str 解析 conversation_key
- wa::Message { conversation: Some(body), .. }
- client.send_message(to, msg).await 返 sent_id / 错误

map_send_error：anyhow::Error → ConnectorError 4 类
- "not logged in" / "unauthorized" / "401" / "403" / "auth" / "revoked" / "logged out"
  → AuthExpired
- "timeout" / "connection" / "refused" / "reset" / "closed" / "network"
  / "transport" / "rate limit" / "429"
  → Transient
- 其余 → Fatal

wa-rs 没有结构化错误枚举，所以是文本关键字近似匹配；spec §5.3 4 类表
做不到精确（不存在的 enum），8 个 unit test 锁住每类的代表性关键字。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: connector.rs 加 bot_client 字段 + send() 真实现

**Files:**
- Modify: `src-tauri/src/connector/im/whatsapp/connector.rs`

- [ ] **Step 1: 加字段**

在 `inbound_tx` 后加：
```rust
    /// PR5 出站 client 句柄。runtime::start_bot 在 spawn 前 set；
    /// stop() take。send() 读这个 Arc 拿 client 调用 wa-rs。None
    /// 时返 Transient（"bot not running"）。
    pub(crate) bot_client: Arc<tokio::sync::Mutex<Option<Arc<wa_rs::client::Client>>>>,
```

`with_status_callback` 初始化加：
```rust
    bot_client: Arc::new(tokio::sync::Mutex::new(None)),
```

- [ ] **Step 2: 改 `start_pairing_session` 透传**

```rust
    let handle = super::runtime::start_bot(
        paths,
        Arc::clone(&self.pairing_state),
        Arc::clone(&self.on_status),
        Arc::clone(&self.inbound_tx),
        Arc::clone(&self.dedup),
        Arc::clone(&self.bot_client),    // ← new
    ).await?;
```

- [ ] **Step 3: 改 `stop()` drop bot_client（在 abort 前）**

```rust
    async fn stop(&self) -> Result<(), ConnectorError> {
        *self.inbound_tx.lock().await = None;
        *self.bot_client.lock().await = None;       // ← new
        if let Some(handle) = self.bot_handle.lock().await.take() {
            handle.abort();
            log::info!("[whatsapp] bot task aborted");
        }
        Ok(())
    }
```

- [ ] **Step 4: 替换 `send()` trait 方法实现**

```rust
    async fn send(
        &self,
        target: ReplyTarget,
        content: ReplyContent,
    ) -> Result<(), ConnectorError> {
        let client = match self.bot_client.lock().await.clone() {
            Some(c) => c,
            None => return Err(ConnectorError::Transient(
                "whatsapp: bot not running, cannot send".into()
            )),
        };
        let body = match content {
            ReplyContent::Text(t) => t,
            ReplyContent::Markdown(m) => super::markdown::strip_to_wa(&m),
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                if final_chunk {
                    delta
                } else {
                    // 中间 chunk 静默丢；PR6 才接增量编辑路径。
                    return Ok(());
                }
            }
            ReplyContent::AiCardFail => "❌ 处理失败，请重试".to_string(),
        };
        let _sent_id = super::sender::send_text(
            &client,
            &target.external_conversation_key,
            &body,
        ).await?;
        log::info!(
            "[whatsapp] sent reply to={} text_len={}",
            target.external_conversation_key,
            body.chars().count()
        );
        Ok(())
    }
```

- [ ] **Step 5: 加新单测**

旧测试 `send_still_returns_not_supported_in_pr2` 必须删（已不再适用）。

新加 3 个：

```rust
    #[tokio::test]
    async fn send_returns_transient_when_bot_not_running() {
        let c = WhatsAppConnector::new();
        // bot_client 默认 None
        let err = c.send(
            ReplyTarget {
                session_id: "s".into(),
                external_conversation_key: "8613800138000@s.whatsapp.net".into(),
            },
            ReplyContent::Text("hi".into()),
        ).await.unwrap_err();
        match err {
            ConnectorError::Transient(msg) => assert!(msg.contains("bot not running")),
            other => panic!("expected Transient, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_aicard_chunk_non_final_silently_succeeds() {
        let c = WhatsAppConnector::new();
        // 没装 client 也 OK——non-final chunk 在装 client 之前就 return Ok
        let r = c.send(
            ReplyTarget {
                session_id: "s".into(),
                external_conversation_key: "anything".into(),
            },
            ReplyContent::AiCardChunk { delta: "partial".into(), final_chunk: false },
        ).await;
        // **Note**: 这里其实会先打 Transient 因为我们目前先 lock client；
        // 但预期 final-chunk 检查应在 lock 前？看实施时 ordering 调整测试断言。
        // 如果 lock-first → 预期 Transient；如果 chunk-check-first → 预期 Ok。
        // 选 lock-first（跟 send_returns_transient_when_bot_not_running 一致），
        // 把这个测试改成"装 None client 时 non-final 也返 Transient"
        match r {
            Err(ConnectorError::Transient(_)) => (),
            Ok(()) => panic!("expected Transient (lock-first), got Ok"),
            other => panic!("unexpected {other:?}"),
        }
    }
```

⚠️ 第二个测试有歧义。**实际 ordering**：plan code 是 lock-first（先拿 client，None
直接 Transient）。这样所有 ReplyContent 在 bot 没起时一律 Transient，行为一致。如果
implementer 觉得 non-final chunk 静默丢更合理（即使 bot 没起也 OK），可以反过来先
检查 chunk——但那样需要 spec 二次确认，**默认按 plan lock-first 实现**。

- [ ] **Step 6: 编译 + 测试**

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp::connector:: 2>&1 | tail -15
```
Expected: PR4 8 个 - 1（删的旧测试）+ 2 = 9 tests pass.

⚠️ Task 3 commit 后 build 仍然会 fail —— start_pairing_session 调 runtime::start_bot
的签名会变（多 1 个参数），但 runtime.rs 还没加这个参数。**预期 Task 3 commit 编译
fail**，Task 4 一起补。或者：Task 3 + Task 4 合并 commit。

**决策**：Task 3 + Task 4 各自完成代码，但 commit 在 Task 4 step 5 一起 do
（参考 PR3 plan task 1+2 的"合并 commit"模式）。

- [ ] **Step 7: 不 commit，进 Task 4**

---

## Task 4: runtime.rs::start_bot 新增 bot_client_slot 参数

**Files:**
- Modify: `src-tauri/src/connector/im/whatsapp/runtime.rs`

- [ ] **Step 1: 改 start_bot 签名 + 写 client slot**

```rust
pub async fn start_bot(
    paths: WhatsAppPaths,
    pairing_state: Arc<Mutex<PairingState>>,
    on_status: Arc<dyn Fn(...) + ...>,
    inbound_tx: Arc<Mutex<Option<mpsc::Sender<ChannelMessage>>>>,
    dedup: Arc<MessageDedupSet>,
    bot_client_slot: Arc<Mutex<Option<Arc<Client>>>>,    // ← new
) -> anyhow::Result<JoinHandle<()>> {
    // ...（前面构造 backend / closure 不变）

    let mut bot = Bot::builder()
        .with_backend(backend)
        // ...
        .build()
        .await?;

    // 起 PairingState → AwaitingQr（不变）
    {
        let mut state = pairing_state.lock().await;
        *state = PairingState::AwaitingQr { started_at: Instant::now() };
    }

    // PR5：在 spawn 之前把 client 句柄存到 connector 的 bot_client_slot
    *bot_client_slot.lock().await = Some(bot.client());

    bot.run().await
}
```

`use wa_rs::client::Client;`（如果 PR4 还没引入）。

- [ ] **Step 2: 编译 + 测试**

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -15
```
Expected: 全 PR4 测试 + Task 1/2 新加测试 + Task 3 新加测试都过。

⚠️ runtime.rs 现有的 4 个 PR3/PR4 测试不直接测 start_bot（没有真起 bot），所以
不需要改 runtime tests 来覆盖 client slot——Task 3 的 connector 测试已经覆盖
"None client → Transient"路径。

- [ ] **Step 3: Commit Task 3 + Task 4**

```bash
git add src-tauri/src/connector/im/whatsapp/connector.rs \
        src-tauri/src/connector/im/whatsapp/runtime.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR5 connector.send() 真发 + bot_client 句柄

spec v3 §5.1。

connector 新增 bot_client: Arc<Mutex<Option<Arc<Client>>>> 字段（同 PR4
inbound_tx 同款"运行时切换"模式）。runtime::start_bot 在 spawn handle
之前把 bot.client() 存进去，stop() 取走；send() 读它。

send() 真实现：
- bot_client None → Transient("bot not running")
- ReplyContent::Text(t) → sender::send_text(client, jid, t)
- ReplyContent::Markdown(m) → strip_to_wa(m) → send_text
- ReplyContent::AiCardChunk { final_chunk: true } → send_text(delta)
- ReplyContent::AiCardChunk { final_chunk: false } → 静默 Ok（PR6 接增量编辑）
- ReplyContent::AiCardFail → send_text("❌ 处理失败，请重试")

stop() 同时 drop bot_client + inbound_tx + abort bot_handle。

删掉过时的 PR2 send_still_returns_not_supported_in_pr2 测试，加 2 个新测覆盖
None client → Transient / non-final chunk lock-first → Transient。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: 收尾校验

**Files:**（无修改）

- [ ] **Step 1: 全 PR5 测试**

```bash
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -15
```
Expected: PR4 baseline 47 + PR5 (15 markdown + 8 sender + 2 connector - 1 deleted) = ~71 pass.

- [ ] **Step 2: 全 IM 回归**

```bash
cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -5
```
Expected: 0 new failures vs PR4 baseline.

- [ ] **Step 3: review_im_layering**

```bash
cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -5
```
Expected: 3 passed.

- [ ] **Step 4: Clippy on PR5 files**

```bash
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'whatsapp/(sender|markdown|connector|runtime)\.rs' | head -10
```
Expected: 0 new warnings on PR5-touched files.

- [ ] **Step 5: Cargo fmt**

```bash
cd src-tauri && cargo fmt -- --check 2>&1 | head -5
```

- [ ] **Step 6: 前端**

```bash
pnpm exec tsc --noEmit 2>&1 | tail -3
```
PR5 不动前端，应 clean。

- [ ] **Step 7: 实测（可选）**

```bash
pnpm tauri:dev
```
- 已配对账号下，从手机给桌面 AIjia 发"hello" → AI 回复**真到达手机**（PR4 时还
  收不到回复，PR5 应该收到）
- 测试 markdown 回复（让 AI 输出 `**bold**` / `[link](url)`）→ 在手机看到
  WhatsApp 渲染为 `*bold*` / `link (url)`

---

## Self-Review

### 1. Spec 覆盖（v3 §5）

| spec 子段 | task | 状态 |
|---|---|---|
| §5.1 send 入口 | Task 3 connector.send() + Task 2 sender::send_text | ✅ |
| §5.2 markdown 8 行规则 | Task 1 markdown.rs strip_to_wa + 15 测试 | ✅ |
| §5.3 错误映射 4 类 | Task 2 sender::map_send_error + 8 测试 | ✅（文本关键字近似） |
| §5.4 送达成功 = Ok | Task 3 send() 不等 ACK | ✅ |
| §5.5 markdown / aicard 路径分离 | Task 3 send() match 各 ReplyContent variant | ✅ |

### 2. Placeholder scan

- 无 unimplemented! / TODO。Task 3 的 ordering 决策（lock-first）在 caveat 里说清，
  实施时坚持。
- 实施时几个**已知**实测点：
  1. `wa_rs::client::Client` vs `wa_rs::Client` —— 都对，照 runtime.rs 风格
  2. `Jid::from_str` 是否 bridged at lib.rs（grep wa-rs/src/lib.rs `pub use jid` 已
     确认，但 implementer 跑 `cargo check` 验一遍）
  3. wa-rs 的 send_message 错误真实文本格式 —— 关键字匹配是 best-effort，PR8 真账号
     测试时再调整关键字表

### 3. 类型一致性

- `ReplyTarget.external_conversation_key` 用 `user@server`（PR4 parser 写入的格式）
- `Jid::from_str("user@server")` 解析出 user + server，agent/device/integrator 默认
- send_text 返 `Result<String, ConnectorError>`，`String` 是 sent msg_id（PR6 编辑路径
  会用，PR5 只 log）

### 4. 不在 PR5 范围

- AI Card 增量编辑（PR6）—— `AiCardChunk { final_chunk: false }` 静默 Ok
- 媒体回复（不在 Phase 4 范围 / Phase 10 后续）
- send_image / send_document（同上）
- reaction（PR6）

---

## Execution Handoff

**估时**：5 个 task / ~700 行新代码（含测试）/ 实际 1.5-2 小时（subagent-driven）。

执行步骤：
1. 跑 `superpowers:subagent-driven-development` skill，逐 task 执行
2. Task 3 + Task 4 合并 commit（中间 build fail 是预期）
3. 全部测试 + clippy + fmt 通过后，更新 memory `project_phase4_whatsapp_progress.md`
   PR5 状态行

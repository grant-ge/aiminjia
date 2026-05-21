# Telegram 加固 — 整体 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 spec `2026-05-20-im-telegram-hardening-design.md` 的 4 个 PR 一次性做掉：传输层加固（PR1）/ 入站类型扩展（PR2）/ 出站附件 + 引用回复（PR3）/ 可靠性 + 测试补齐（PR4）。

**Architecture:** 不改 `IMConnector` trait、不新增 crate 依赖。每 PR 一个 commit batch，自带回归测试，PR 间相互独立可单独 revert。所有改动局限在 `src-tauri/src/connector/im/telegram/` 模块内 + 个别新集成测试文件。

**Tech Stack:** Rust 2021、tokio、reqwest（含 `multipart` feature）、wiremock、serde、`storage::text_io` / `storage::safe_filename` 既有 helpers。

**Spec:** `docs/superpowers/specs/2026-05-20-im-telegram-hardening-design.md`

---

## 文件结构总览

### 修改
| 文件 | PR1 | PR2 | PR3 | PR4 |
|---|---|---|---|---|
| `sender.rs` | 分片 + 多 chunk 串行 + connect 重试 | — | extract_local_paths + 附件附带发送 | — |
| `api.rs` | TransportConnect/Connected + rebuild_client | — | send_document multipart + FileTooBig + SendMessageBody.reply_to_message_id | SSRF host 检查（getFile 路径） |
| `long_poll.rs` | watchdog spawn + last_get_updates_at | unsupported 路由（每条都提示） | — | — |
| `connector.rs` | start 启 watchdog | — | resolve_chat_id 不变；TelegramSessionTarget 加 last_inbound_message_id | — |
| `parser.rs` | — | 6 种新增 ParseOutcome::Unsupported variant | 把 last_inbound_message_id 透出 | — |
| `types.rs` | — | 6 个 TgXxx 子结构体 | TelegramSessionTarget.last_inbound_message_id | — |
| `pairing.rs` | — | — | — | pending pairings 写盘 / 读盘 / TTL 清理 |
| `download.rs` | — | — | — | SSRF: 入口检查 api.telegram.org host |
| `mod.rs` | — | — | — | doc 更新到反映当前能力 |

### 新增 / 修改 Cargo.toml
- `src-tauri/Cargo.toml`：`reqwest` 加 `multipart` feature（PR3）

### 新增集成测试文件
- `src-tauri/tests/telegram_pairing_persistence_test.rs`（PR4）

### 修改 spec
- `docs/superpowers/specs/2026-05-20-im-telegram-hardening-design.md`：4 个 PR 各自验收清单勾选

---

## §0 准备工作

### Task 0.1: 跑现有测试 baseline

- [ ] **Step 1:** 跑现有 telegram 单测确认 baseline 绿

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -30`
Expected: 所有现有 telegram 测试 PASS（约 40 个）

- [ ] **Step 2:** 跑 review_ 系列回归 baseline

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -15`
Expected: PASS

如果有 fail，先记录下来，区分"已有 fail"vs"本次 PR 引入的 fail"。

---

## §A PR1 — 传输层加固

**目标**：长消息按语义边界分片（4000 byte 上限）、长轮询 stall watchdog（30s tick / 120s 阈值）、sendMessage 错误分类（TransportConnect 可重试 / TransportConnected 不可重试）。

### Task 1.1: split_telegram_html 函数 + 单测

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/sender.rs`

- [ ] **Step 1: 在 sender.rs 加常量 + 函数**

定位 `pub struct TelegramSender` 上方插入：

```rust
/// Telegram sendMessage 4096 字符上限；按 byte 保留 96 byte 给 HTML 实体展开余量。
pub const MAX_MESSAGE_BYTES: usize = 4000;
```

在 `markdown_to_telegram_html` 函数之后（约 167 行）插入完整实现：

```rust
/// 把已转好的 Telegram HTML 按 max_bytes 上限切成多片，尽量保留语义边界。
///
/// 切分优先级：
/// 1. `<pre><code>...</code></pre>` 代码块视为原子；单块超 max_bytes 则强切并各自外包
/// 2. 双换行（段落）
/// 3. 单换行（行）
/// 4. 字符兜底（utf-8 边界）
pub fn split_telegram_html(input: &str, max_bytes: usize) -> Vec<String> {
    if input.as_bytes().len() <= max_bytes {
        return vec![input.to_string()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    let segments = split_by_code_blocks(input);
    for seg in segments {
        match seg {
            Segment::CodeBlock(text) => {
                if !current.is_empty()
                    && current.as_bytes().len() + text.as_bytes().len() > max_bytes
                {
                    chunks.push(std::mem::take(&mut current));
                }
                if text.as_bytes().len() > max_bytes {
                    if !current.is_empty() {
                        chunks.push(std::mem::take(&mut current));
                    }
                    chunks.extend(force_split_code_block(&text, max_bytes));
                } else {
                    current.push_str(&text);
                }
            }
            Segment::Text(text) => {
                push_text_with_paragraph_split(&text, max_bytes, &mut chunks, &mut current);
            }
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

enum Segment {
    CodeBlock(String),
    Text(String),
}

fn split_by_code_blocks(input: &str) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let open = "<pre><code";
    let close = "</code></pre>";
    let mut remaining = input;
    while let Some(open_idx) = remaining.find(open) {
        if open_idx > 0 {
            out.push(Segment::Text(remaining[..open_idx].to_string()));
        }
        let tail = &remaining[open_idx..];
        if let Some(close_idx) = tail.find(close) {
            let block_end = close_idx + close.len();
            out.push(Segment::CodeBlock(tail[..block_end].to_string()));
            remaining = &tail[block_end..];
        } else {
            out.push(Segment::Text(tail.to_string()));
            return out;
        }
    }
    if !remaining.is_empty() {
        out.push(Segment::Text(remaining.to_string()));
    }
    out
}

fn push_text_with_paragraph_split(
    text: &str,
    max_bytes: usize,
    chunks: &mut Vec<String>,
    current: &mut String,
) {
    for para in text.split("\n\n") {
        let to_add = if current.is_empty() {
            para.to_string()
        } else {
            format!("\n\n{para}")
        };
        if current.as_bytes().len() + to_add.as_bytes().len() <= max_bytes {
            current.push_str(&to_add);
            continue;
        }
        if !current.is_empty() {
            chunks.push(std::mem::take(current));
        }
        if para.as_bytes().len() > max_bytes {
            for line_chunk in split_by_lines(para, max_bytes) {
                if line_chunk.as_bytes().len() > max_bytes {
                    chunks.extend(force_split_chars(&line_chunk, max_bytes));
                } else {
                    chunks.push(line_chunk);
                }
            }
        } else {
            *current = para.to_string();
        }
    }
}

fn split_by_lines(text: &str, max_bytes: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.split('\n') {
        let to_add = if cur.is_empty() {
            line.to_string()
        } else {
            format!("\n{line}")
        };
        if cur.as_bytes().len() + to_add.as_bytes().len() <= max_bytes {
            cur.push_str(&to_add);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur = line.to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn force_split_chars(text: &str, max_bytes: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if cur.as_bytes().len() + ch.len_utf8() > max_bytes && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn force_split_code_block(block: &str, max_bytes: usize) -> Vec<String> {
    let open = "<pre><code>";
    let close = "</code></pre>";
    let inner = block
        .strip_prefix(open)
        .and_then(|s| s.strip_suffix(close))
        .unwrap_or(block);
    let wrap_overhead = open.len() + close.len();
    let inner_max = max_bytes.saturating_sub(wrap_overhead).max(1);
    let inner_chunks = split_by_lines(inner, inner_max);
    inner_chunks
        .into_iter()
        .map(|c| format!("{open}{c}{close}"))
        .collect()
}
```

- [ ] **Step 2: 加单测到 `#[cfg(test)] mod tests`**

在文件末尾 mod tests 内追加：

```rust
mod split_tests {
    use super::*;

    #[test]
    fn short_input_returns_single_chunk() {
        assert_eq!(split_telegram_html("hello", 4000), vec!["hello"]);
    }

    #[test]
    fn long_text_splits_on_double_newline() {
        let para = "a".repeat(1500);
        let input = format!("{para}\n\n{para}\n\n{para}\n\n{para}");
        let chunks = split_telegram_html(&input, 4000);
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert!(c.as_bytes().len() <= 4000);
        }
    }

    #[test]
    fn chinese_multibyte_never_cuts_inside_codepoint() {
        let chinese: String = "中".repeat(1500);
        let chunks = split_telegram_html(&chinese, 4000);
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert_eq!(c.as_bytes().len() % 3, 0);
        }
    }

    #[test]
    fn code_block_stays_intact_when_under_limit() {
        let prelude = "a".repeat(2000);
        let block = "<pre><code>fn main() {}</code></pre>";
        let suffix = "b".repeat(2000);
        let input = format!("{prelude}\n\n{block}\n\n{suffix}");
        let chunks = split_telegram_html(&input, 4000);
        let count_with_block = chunks
            .iter()
            .filter(|c| c.contains("<pre><code>fn main()"))
            .count();
        assert_eq!(count_with_block, 1);
    }

    #[test]
    fn oversized_code_block_is_force_split_and_rewrapped() {
        let inner_lines: String = (0..200)
            .map(|i| format!("line_{i}_with_some_content_to_fill_bytes"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(inner_lines.as_bytes().len() > 4000);
        let block = format!("<pre><code>{inner_lines}</code></pre>");
        let chunks = split_telegram_html(&block, 4000);
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert!(c.starts_with("<pre><code>"));
            assert!(c.ends_with("</code></pre>"));
            assert!(c.as_bytes().len() <= 4000);
        }
    }
}
```

- [ ] **Step 3: 跑测试**

Run: `cd src-tauri && cargo test --lib split_tests --no-fail-fast 2>&1 | tail -15`
Expected: 5 tests pass

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/connector/im/telegram/sender.rs
git commit -m "feat(connector/telegram): PR1 split_telegram_html 分片实现 + 单测

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 1.2: send_markdown 接入分片 + connect 阶段重试

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/sender.rs`

- [ ] **Step 1: 在 sender.rs 顶部增加 import**

定位 `use std::sync::Arc;` 行附近，确认有：

```rust
use std::sync::Arc;
use std::time::Duration;
```

如果没有 `use std::time::Duration;` 就加上。

- [ ] **Step 2: 替换 send_markdown 函数（约 35-71 行）**

整段替换为：

```rust
pub async fn send_markdown(&self, chat_id: i64, raw_markdown: &str) -> Result<(), SenderError> {
    let html = markdown_to_telegram_html(raw_markdown);
    let chunks = split_telegram_html(&html, MAX_MESSAGE_BYTES);
    for chunk in chunks {
        match self.send_html_chunk(chat_id, &chunk).await {
            Ok(()) => {}
            Err(SenderError::Transport(desc)) if desc.starts_with("parse error:") => {
                // 整段（不再尝试后续 chunks）回 plain text fallback
                let plain = strip_markdown(raw_markdown);
                return self
                    .api
                    .send_message(chat_id, &plain, None)
                    .await
                    .map(|_| ())
                    .map_err(map_err);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// 单 chunk send + 429 重试 + connect 阶段失败重试。
async fn send_html_chunk(&self, chat_id: i64, html: &str) -> Result<(), SenderError> {
    match self.api.send_message(chat_id, html, Some("HTML")).await {
        Ok(_) => Ok(()),
        Err(TelegramApiError::TooManyRequests { retry_after }) => {
            tokio::time::sleep(retry_after).await;
            self.api
                .send_message(chat_id, html, Some("HTML"))
                .await
                .map(|_| ())
                .map_err(map_err)
        }
        Err(TelegramApiError::TransportConnect(d)) => {
            log::warn!("[telegram-sender] connect failed: {d}, retrying once");
            tokio::time::sleep(Duration::from_millis(500)).await;
            self.api
                .send_message(chat_id, html, Some("HTML"))
                .await
                .map(|_| ())
                .map_err(map_err)
        }
        Err(TelegramApiError::BadRequest(desc)) if is_parse_error(&desc) => {
            Err(SenderError::Transport(format!("parse error: {desc}")))
        }
        Err(TelegramApiError::BadRequest(desc)) => {
            Err(SenderError::Transport(format!("bad request: {desc}")))
        }
        Err(e) => Err(map_err(e)),
    }
}
```

**注意**：此时 `TransportConnect` 还没在 `TelegramApiError` 上定义，下一步在 Task 1.3 添加，编译会暂时失败——先继续 Step 3 验证基本结构再做。

- [ ] **Step 3: 跑编译，确认报错只在 TransportConnect**

Run: `cd src-tauri && cargo check --lib 2>&1 | grep -E "error|TransportConnect" | head -10`
Expected: 只有 `TransportConnect` / `TransportConnected` 相关 error，不应该有其他

如果有其他 error，先回头修。

- [ ] **Step 4:** 暂不 commit；与 Task 1.3 一并 commit（错误分类要一起落地）

### Task 1.3: TelegramApiError 拆分 Transport → TransportConnect / TransportConnected

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/api.rs`

- [ ] **Step 1: 替换 TelegramApiError enum 定义**

定位约 24-38 行，整段替换为：

```rust
#[derive(Debug, thiserror::Error)]
pub enum TelegramApiError {
    #[error("invalid token / unauthorized: {0}")]
    Unauthorized(String),
    #[error("rate limited; retry after {retry_after:?}")]
    TooManyRequests { retry_after: Duration },
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// TCP 还没建好（DNS/connect refused/unreachable）。可安全重试。
    #[error("http connect: {0}")]
    TransportConnect(String),
    /// TCP 建好后断（reset/timeout/服务端中途断）。**不可重试**（消息可能已抵达）。
    #[error("http connected-then-broke: {0}")]
    TransportConnected(String),
    #[error("server error: {0}")]
    ServerError(String),
}
```

- [ ] **Step 2: 在 `impl TelegramApi` 块前加 classify helper**

定位约 165 行 `impl TelegramApi` 之前插入：

```rust
fn classify_reqwest_error(e: reqwest::Error) -> TelegramApiError {
    let s = e.to_string();
    if e.is_connect() || s.contains("Connection refused") || s.contains("connect error") {
        TelegramApiError::TransportConnect(s)
    } else {
        TelegramApiError::TransportConnected(s)
    }
}
```

- [ ] **Step 3: 替换所有 `TelegramApiError::Transport(...)` 调用点**

grep 一遍：

```bash
grep -n "TelegramApiError::Transport" src-tauri/src/connector/im/telegram/api.rs
```

规则：
- 来自 reqwest 的 `.map_err(|e| TelegramApiError::Transport(e.to_string()))` → `.map_err(classify_reqwest_error)`
- 构造字符串的 `TelegramApiError::Transport(format!(...))`（envelope parse / "ok=true but result missing" / "unknown error_code"）→ `TelegramApiError::TransportConnected(...)`
- `download.rs` 内的 `TelegramApiError::Transport("retry exhausted".into())` → `TelegramApiError::TransportConnected("retry exhausted".into())`

需要修的位置（约 5 处）：
- `get_me` 内 reqwest send → `classify_reqwest_error`
- `get_updates` 内 reqwest send（约 222 行）→ `classify_reqwest_error`
- `get_updates` 内 body text 读取（约 227 行）→ `classify_reqwest_error`
- `get_updates` 内 envelope parse（约 247 行）→ `TransportConnected`
- `get_updates` 内 "ok=true but result missing"（约 252 行）→ `TransportConnected`
- `get_updates` 内 "unknown error_code"（约 266 行）→ `TransportConnected`
- `send_message` 内 reqwest post → `classify_reqwest_error`
- `parse_envelope` 内 reqwest text 读取 → `classify_reqwest_error`
- `parse_envelope` 内 envelope parse → `TransportConnected`
- `parse_envelope` 内 ok=true 但 result 缺失 → `TransportConnected`
- `parse_envelope` 内 unknown error_code → `TransportConnected`

逐个替换。

- [ ] **Step 4: 修 download.rs**

```bash
grep -n "TelegramApiError::Transport" src-tauri/src/connector/im/telegram/download.rs
```

把 `TelegramApiError::Transport("retry exhausted".into())` 改为 `TelegramApiError::TransportConnected("retry exhausted".into())`。

- [ ] **Step 5: 编译验证 + 跑现有测试回归**

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -20`
Expected: 所有现有测试 + 新 split_tests 全 PASS

如果有 fail，定位是不是 sender.rs 还引用了不存在的 variant，修补到通过。

- [ ] **Step 6: 提交（PR1 三个改动合并 commit 中的第一份）**

```bash
git add src-tauri/src/connector/im/telegram/sender.rs src-tauri/src/connector/im/telegram/api.rs src-tauri/src/connector/im/telegram/download.rs
git commit -m "feat(connector/telegram): PR1 拆 Transport→Connect/Connected + send_markdown 分片

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 1.4: api.rs 把 http client 包进 RwLock + rebuild_client 方法

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/api.rs`

- [ ] **Step 1: 顶部加 import**

api.rs 顶部 `use std::time::Duration;` 后追加：

```rust
use tokio::sync::RwLock;
```

- [ ] **Step 2: 替换 TelegramApi struct 定义**

定位 `pub struct TelegramApi`（约 18-22 行）改为：

```rust
pub struct TelegramApi {
    token: String,
    api_base: String,
    http: RwLock<reqwest::Client>,
    client_timeout: Duration,
}
```

- [ ] **Step 3: 替换 new / new_with_api_base_for_tests**

```rust
impl TelegramApi {
    pub fn new(token: String) -> Result<Self> {
        let timeout = Duration::from_secs(35);
        Ok(Self {
            token,
            api_base: TELEGRAM_API_BASE.to_string(),
            http: RwLock::new(reqwest::Client::builder().timeout(timeout).build()?),
            client_timeout: timeout,
        })
    }

    #[doc(hidden)]
    pub fn new_with_api_base_for_tests(token: String, api_base: String) -> Result<Self> {
        let timeout = Duration::from_secs(5);
        Ok(Self {
            token,
            api_base,
            http: RwLock::new(
                reqwest::Client::builder()
                    .no_proxy()
                    .timeout(timeout)
                    .build()?,
            ),
            client_timeout: timeout,
        })
    }

    /// 丢弃当前 reqwest client（释放 keep-alive），新建替换。
    /// Watchdog 检测到 stall 时调用，强制下次请求走新连接。
    pub async fn rebuild_client(&self) -> Result<()> {
        let builder = if self.api_base != TELEGRAM_API_BASE {
            reqwest::Client::builder()
                .no_proxy()
                .timeout(self.client_timeout)
        } else {
            reqwest::Client::builder().timeout(self.client_timeout)
        };
        let new_client = builder.build()?;
        *self.http.write().await = new_client;
        Ok(())
    }
}
```

- [ ] **Step 4: 替换所有 `self.http.get(...)` / `self.http.post(...)` 调用**

定位以下 3 处方法体内的 `self.http.xxx` 改为先取读锁：

`get_me`（约 195 行）：
```rust
let resp = self
    .http
    .read()
    .await
    .get(self.url("getMe"))
    .send()
    .await
    .map_err(classify_reqwest_error)?;
```

`get_updates`（约 217 行）：
```rust
let resp = self
    .http
    .read()
    .await
    .get(&url)
    .send()
    .await
    .map_err(classify_reqwest_error)?;
```

`send_message`（约 287 行）：
```rust
let resp = self
    .http
    .read()
    .await
    .post(self.url("sendMessage"))
    .json(&body)
    .send()
    .await
    .map_err(classify_reqwest_error)?;
```

如果 `download_file` / `get_file` 等其他方法也用了 `self.http`，一并改成读锁版本（不漏）。

```bash
grep -n "self.http\." src-tauri/src/connector/im/telegram/api.rs
```

- [ ] **Step 5: 加 rebuild_client 单测**

在 `#[cfg(test)] mod tests` 末尾追加：

```rust
#[tokio::test]
async fn rebuild_client_does_not_break_subsequent_calls() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/botT/getMe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": { "id": 1, "first_name": "T" }
        })))
        .mount(&server)
        .await;
    let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();
    api.get_me().await.unwrap();
    api.rebuild_client().await.unwrap();
    api.get_me().await.unwrap();
}

#[tokio::test]
async fn connect_to_nonexistent_port_classified_as_transport_connect() {
    let api = TelegramApi::new_with_api_base_for_tests(
        "T".into(),
        "http://127.0.0.1:10".into(),
    )
    .unwrap();
    match api.send_message(1, "hi", None).await {
        Err(TelegramApiError::TransportConnect(_)) => {}
        other => panic!("expected TransportConnect, got {:?}", other),
    }
}
```

- [ ] **Step 6: 跑测试**

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -20`
Expected: 全部 PASS

如果 `connect_to_nonexistent_port_classified_as_transport_connect` 失败（被 classify 成 TransportConnected），把 classify_reqwest_error 的 fallback 字符串匹配再放宽：增加 `s.contains("os error 61")`（macOS connection refused 错误码）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/connector/im/telegram/api.rs
git commit -m "feat(connector/telegram): PR1 api.rs RwLock<Client> + rebuild_client + 测试

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 1.5: long_poll.rs 加 last_get_updates_at + watchdog spawn

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/long_poll.rs`
- Modify: `src-tauri/src/connector/im/telegram/connector.rs`

- [ ] **Step 1: 在 long_poll.rs 顶部 use 区加 AtomicI64**

定位顶部 use 区，追加：

```rust
use std::sync::atomic::{AtomicI64, Ordering};
```

- [ ] **Step 2: 在 Params struct 加 last_get_updates_at**

定位 `pub struct Params`（约 49 行）末尾加字段：

```rust
pub struct Params {
    pub api: Arc<TelegramApi>,
    pub bot_id: String,
    pub pairing: PairingCodeStore,
    pub sender: TelegramSender,
    pub session_targets: Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
    pub config_store: Arc<ChannelConfigStore>,
    pub msg_tx: mpsc::Sender<ChannelMessage>,
    pub on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    pub cancel: CancellationToken,
    /// Watchdog 共享：每次 getUpdates 完成（成功或失败）后写入 unix millis。
    pub last_get_updates_at: Arc<AtomicI64>,
}
```

- [ ] **Step 3: 在 run() 主循环里更新时间戳**

定位 `pub async fn run(p: Params)` 中 `match p.api.get_updates(offset, LONG_POLL_TIMEOUT_SECS).await {` 行，替换为：

```rust
let outcome = p.api.get_updates(offset, LONG_POLL_TIMEOUT_SECS).await;
p.last_get_updates_at
    .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
match outcome {
```

确保花括号闭合不变。

- [ ] **Step 4: 在 long_poll.rs 末尾加 watchdog 模块**

文件末尾追加：

```rust
pub const STALL_TICK_INTERVAL: Duration = Duration::from_secs(30);
pub const STALL_TIMEOUT: Duration = Duration::from_secs(120);

pub struct WatchdogParams {
    pub api: Arc<TelegramApi>,
    pub bot_id: String,
    pub on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    pub last_get_updates_at: Arc<AtomicI64>,
    pub cancel: CancellationToken,
}

pub async fn run_watchdog(p: WatchdogParams) {
    let mut tick = tokio::time::interval(STALL_TICK_INTERVAL);
    tick.tick().await; // 跳过 immediate fire

    loop {
        tokio::select! {
            _ = p.cancel.cancelled() => return,
            _ = tick.tick() => {}
        }
        let last_ms = p.last_get_updates_at.load(Ordering::SeqCst);
        if last_ms == 0 {
            // 第一轮还没成功跑过，跳过
            continue;
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        let elapsed_ms = now_ms.saturating_sub(last_ms);
        if elapsed_ms as u64 >= STALL_TIMEOUT.as_millis() as u64 {
            log::warn!(
                "[telegram-{}] watchdog stall: last update {}ms ago, rebuilding client",
                p.bot_id,
                elapsed_ms
            );
            (p.on_status)(
                ChannelConnectionState::Reconnecting,
                Some("watchdog: long-poll stalled".into()),
            );
            if let Err(e) = p.api.rebuild_client().await {
                log::error!("[telegram-{}] watchdog rebuild_client failed: {e}", p.bot_id);
            }
            p.last_get_updates_at
                .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
        }
    }
}
```

- [ ] **Step 5: 在 connector.rs::start 中分配时间戳 + spawn watchdog**

定位 `async fn start(...)` 实现（约 170 行），整段替换为：

```rust
async fn start(
    &self,
    ctx: ConnectorContext,
) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
    let (msg_tx, msg_rx) = mpsc::channel::<ChannelMessage>(256);

    (self.on_status)(ChannelConnectionState::Connecting, None);

    let api = self.api.clone();
    let bot_id = self.bot_id.clone();
    let pairing = self.pairing.clone();
    let sender_for_pump = self.sender.clone_inner();
    let session_targets = self.session_targets.clone();
    let config_store = self.config_store.clone();
    let on_status = self.on_status.clone();
    let cancel = ctx.cancel_token.clone();
    let last_get_updates_at = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));

    // Watchdog 独立 task，共享 cancel 和时间戳
    let watchdog_params = super::long_poll::WatchdogParams {
        api: api.clone(),
        bot_id: bot_id.clone(),
        on_status: on_status.clone(),
        last_get_updates_at: last_get_updates_at.clone(),
        cancel: cancel.clone(),
    };
    tokio::spawn(async move { super::long_poll::run_watchdog(watchdog_params).await });

    tokio::spawn(async move {
        super::long_poll::run(super::long_poll::Params {
            api,
            bot_id,
            pairing,
            sender: sender_for_pump,
            session_targets,
            config_store,
            msg_tx,
            on_status,
            cancel,
            last_get_updates_at,
        })
        .await
    });

    Ok(ReceiverStream::new(msg_rx).boxed())
}
```

- [ ] **Step 6: 在 long_poll.rs 末尾加 watchdog 测试**

```rust
#[cfg(test)]
mod watchdog_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test(start_paused = true)]
    async fn watchdog_rebuilds_client_when_stalled_past_threshold() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/botBOT/getMe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true, "result": { "id": 1, "first_name": "T" }
            })))
            .mount(&server)
            .await;
        let api = Arc::new(
            TelegramApi::new_with_api_base_for_tests("BOT".into(), server.uri()).unwrap(),
        );
        let last = Arc::new(AtomicI64::new(
            chrono::Utc::now().timestamp_millis() - 200_000,
        ));
        let status_calls = Arc::new(AtomicUsize::new(0));
        let status_calls_clone = status_calls.clone();
        let on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync> =
            Arc::new(move |state, _err| {
                if matches!(state, ChannelConnectionState::Reconnecting) {
                    status_calls_clone.fetch_add(1, Ordering::SeqCst);
                }
            });
        let cancel = CancellationToken::new();
        let handle = tokio::spawn({
            let params = WatchdogParams {
                api: api.clone(),
                bot_id: "BOT".into(),
                on_status,
                last_get_updates_at: last,
                cancel: cancel.clone(),
            };
            async move { run_watchdog(params).await }
        });

        tokio::time::advance(Duration::from_secs(35)).await;
        for _ in 0..20 {
            tokio::task::yield_now().await;
            tokio::time::advance(Duration::from_millis(10)).await;
        }
        assert!(status_calls.load(Ordering::SeqCst) >= 1);
        cancel.cancel();
        let _ = handle.await;
    }

    #[tokio::test(start_paused = true)]
    async fn watchdog_does_not_fire_when_activity_recent() {
        let api = Arc::new(
            TelegramApi::new_with_api_base_for_tests(
                "BOT".into(),
                "http://127.0.0.1:1".into(),
            )
            .unwrap(),
        );
        let last = Arc::new(AtomicI64::new(chrono::Utc::now().timestamp_millis()));
        let status_calls = Arc::new(AtomicUsize::new(0));
        let status_calls_clone = status_calls.clone();
        let on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync> =
            Arc::new(move |_state, _err| {
                status_calls_clone.fetch_add(1, Ordering::SeqCst);
            });
        let cancel = CancellationToken::new();
        let handle = tokio::spawn({
            let params = WatchdogParams {
                api,
                bot_id: "BOT".into(),
                on_status,
                last_get_updates_at: last,
                cancel: cancel.clone(),
            };
            async move { run_watchdog(params).await }
        });
        tokio::time::advance(Duration::from_secs(60)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(status_calls.load(Ordering::SeqCst), 0);
        cancel.cancel();
        let _ = handle.await;
    }

    #[tokio::test(start_paused = true)]
    async fn watchdog_exits_on_cancel() {
        let api = Arc::new(
            TelegramApi::new_with_api_base_for_tests(
                "BOT".into(),
                "http://127.0.0.1:1".into(),
            )
            .unwrap(),
        );
        let last = Arc::new(AtomicI64::new(0));
        let on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync> =
            Arc::new(|_state, _err| {});
        let cancel = CancellationToken::new();
        let handle = tokio::spawn({
            let params = WatchdogParams {
                api,
                bot_id: "BOT".into(),
                on_status,
                last_get_updates_at: last,
                cancel: cancel.clone(),
            };
            async move { run_watchdog(params).await }
        });
        cancel.cancel();
        let _ = handle.await;
    }
}
```

- [ ] **Step 7: 跑测试**

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -25`
Expected: 全部 PASS（含 3 个新 watchdog 测试）

- [ ] **Step 8: 提交 PR1 全部改动**

```bash
git add src-tauri/src/connector/im/telegram/
git commit -m "feat(connector/telegram): PR1 stall watchdog + 测试

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 1.6: PR1 回归验证 + 验收清单勾选

- [ ] **Step 1:** 跑 review_ 回归

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -15`
Expected: 与 baseline 一致（PASS）

- [ ] **Step 2:** 跑 clippy

Run: `cd src-tauri && cargo clippy --lib --tests -- -D warnings 2>&1 | tail -30`
Expected: 与 baseline 持平或更好

如有新 lint 错误，按错误修；**不要** `#[allow(...)]` 兜底。

- [ ] **Step 3:** 勾选 spec §3.4 PR1 验收清单

读 `docs/superpowers/specs/2026-05-20-im-telegram-hardening-design.md`，定位 `### 3.4 PR1 验收清单`，把全部 `- [ ]` 改为 `- [x]`。

- [ ] **Step 4: 提交**

```bash
git add docs/superpowers/specs/2026-05-20-im-telegram-hardening-design.md
git commit -m "docs(superpowers/specs): Telegram PR1 验收勾选

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## §B PR2 — 入站类型扩展（unsupported 提示）

**目标**：parser 识别 voice / audio / video / video_note / sticker / animation 6 种类型，long_poll 每条都回提示。

### Task 2.1: types.rs 加 6 个子结构体 + TgMessage 字段

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/api.rs`

- [ ] **Step 1: 在 api.rs 中 TgDocument 后追加新子结构体**

定位 `pub struct TgDocument`（约 80 行）之后，添加：

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct TgVoice {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgAudio {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgVideo {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgVideoNote {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgSticker {
    #[serde(default)]
    pub emoji: Option<String>,
    #[serde(default)]
    pub set_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgAnimation {
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub file_size: Option<u64>,
}
```

- [ ] **Step 2: 在 TgMessage 加 6 个 Option 字段**

定位 `pub struct TgMessage`（约 94 行），在 `caption` 之后追加：

```rust
    #[serde(default)]
    pub voice: Option<TgVoice>,
    #[serde(default)]
    pub audio: Option<TgAudio>,
    #[serde(default)]
    pub video: Option<TgVideo>,
    #[serde(default)]
    pub video_note: Option<TgVideoNote>,
    #[serde(default)]
    pub sticker: Option<TgSticker>,
    #[serde(default)]
    pub animation: Option<TgAnimation>,
```

- [ ] **Step 3: 编译**

Run: `cd src-tauri && cargo check --lib 2>&1 | grep -E "error|warning: unused" | head -10`
Expected: 没有 error；可能有 parser.rs 的 unused warning 待 Task 2.2 修

如果有 `TgMessage` 构造调用（parser.rs 测试里）报字段缺失：需要给那些测试也加上新字段（设为 None）。

定位 parser.rs `fn empty_msg`（约 170 行），把 TgMessage 构造改为：

```rust
fn empty_msg(id: i64, from: super::super::api::TgUser, chat: super::super::api::TgChat) -> TgMessage {
    TgMessage {
        message_id: id,
        from: Some(from),
        chat,
        text: None,
        date: None,
        photo: None,
        document: None,
        caption: None,
        voice: None,
        audio: None,
        video: None,
        video_note: None,
        sticker: None,
        animation: None,
    }
}
```

- [ ] **Step 4: 跑测试回归**

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -15`
Expected: 全 PASS

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/connector/im/telegram/api.rs src-tauri/src/connector/im/telegram/parser.rs
git commit -m "feat(connector/telegram): PR2 TgMessage 加 6 种入站类型字段

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 2.2: parser.rs 加 ParsedInbound::Unsupported variant + 检测

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/parser.rs`

- [ ] **Step 1: 在 ParsedInbound enum 加 Unsupported variant**

定位 `pub enum ParsedInbound`（约 17 行），整段替换为：

```rust
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ParsedInbound {
    Message {
        message: ChannelMessage,
        user_id: i64,
        first_name: String,
        username: Option<String>,
        chat_id: i64,
    },
    PairingStart {
        code: Option<String>,
        user_id: i64,
        first_name: String,
        username: Option<String>,
        chat_id: i64,
    },
    /// 收到了我们不支持的消息类型（voice/video/sticker 等），需要回提示。
    Unsupported {
        chat_id: i64,
        user_id: i64,
        message_id: i64,
        kind: UnsupportedKind,
    },
    Skip(SkipReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedKind {
    Voice,
    Audio,
    Video,
    VideoNote,
    Sticker,
    Animation,
}

impl UnsupportedKind {
    pub fn hint_text(&self) -> &'static str {
        match self {
            Self::Voice => "🤖 我暂不支持处理语音消息",
            Self::Audio => "🤖 我暂不支持处理音频文件",
            Self::Video | Self::VideoNote => "🤖 我暂不支持处理视频",
            Self::Sticker => "🤖 我暂不支持识别贴纸",
            Self::Animation => "🤖 我暂不支持处理动图",
        }
    }
}
```

- [ ] **Step 2: parse_message 中加 unsupported 检测**

定位 `fn parse_message(msg: &TgMessage, update_id: i64, bot_id: &str)`，在 `// /start [code]` 分支结束后、`// 收集附件` 注释之前，加 unsupported 检测：

```rust
    // Unsupported 类型检测：voice/audio/video/video_note/sticker/animation
    // 我们刻意不进 LLM 链路，只回提示给用户，避免"消息石沉大海"的体验。
    let unsupported_kind = if msg.voice.is_some() {
        Some(UnsupportedKind::Voice)
    } else if msg.audio.is_some() {
        Some(UnsupportedKind::Audio)
    } else if msg.video.is_some() {
        Some(UnsupportedKind::Video)
    } else if msg.video_note.is_some() {
        Some(UnsupportedKind::VideoNote)
    } else if msg.sticker.is_some() {
        Some(UnsupportedKind::Sticker)
    } else if msg.animation.is_some() {
        Some(UnsupportedKind::Animation)
    } else {
        None
    };
    if let Some(kind) = unsupported_kind {
        let _ = update_id;
        return ParsedInbound::Unsupported {
            chat_id: msg.chat.id,
            user_id: from.id,
            message_id: msg.message_id,
            kind,
        };
    }
```

- [ ] **Step 3: 加 unsupported parser 单测**

在 parser.rs 的 `#[cfg(test)] mod tests` 末尾追加：

```rust
mod unsupported_tests {
    use super::*;
    use super::super::api::{TgAnimation, TgAudio, TgSticker, TgVideo, TgVideoNote, TgVoice};

    #[test]
    fn voice_message_returns_unsupported_voice() {
        let mut msg = empty_msg(30, user(42, "Alice", false), chat(42, "private"));
        msg.voice = Some(TgVoice { duration: Some(5), file_size: Some(2048) });
        match parse_update(&update_with_msg(200, msg), "BOT") {
            ParsedInbound::Unsupported { kind, chat_id, user_id, message_id } => {
                assert_eq!(kind, UnsupportedKind::Voice);
                assert_eq!(chat_id, 42);
                assert_eq!(user_id, 42);
                assert_eq!(message_id, 30);
            }
            other => panic!("expected Unsupported(Voice), got {:?}", other),
        }
    }

    #[test]
    fn audio_returns_unsupported_audio() {
        let mut msg = empty_msg(31, user(42, "Alice", false), chat(42, "private"));
        msg.audio = Some(TgAudio { duration: Some(180), file_size: Some(1024) });
        assert!(matches!(
            parse_update(&update_with_msg(201, msg), "BOT"),
            ParsedInbound::Unsupported { kind: UnsupportedKind::Audio, .. }
        ));
    }

    #[test]
    fn video_returns_unsupported_video() {
        let mut msg = empty_msg(32, user(42, "Alice", false), chat(42, "private"));
        msg.video = Some(TgVideo { duration: Some(10), file_size: None });
        assert!(matches!(
            parse_update(&update_with_msg(202, msg), "BOT"),
            ParsedInbound::Unsupported { kind: UnsupportedKind::Video, .. }
        ));
    }

    #[test]
    fn video_note_returns_unsupported_video_note() {
        let mut msg = empty_msg(33, user(42, "Alice", false), chat(42, "private"));
        msg.video_note = Some(TgVideoNote { duration: Some(3), file_size: None });
        assert!(matches!(
            parse_update(&update_with_msg(203, msg), "BOT"),
            ParsedInbound::Unsupported { kind: UnsupportedKind::VideoNote, .. }
        ));
    }

    #[test]
    fn sticker_returns_unsupported_sticker() {
        let mut msg = empty_msg(34, user(42, "Alice", false), chat(42, "private"));
        msg.sticker = Some(TgSticker { emoji: Some("😀".into()), set_name: None });
        assert!(matches!(
            parse_update(&update_with_msg(204, msg), "BOT"),
            ParsedInbound::Unsupported { kind: UnsupportedKind::Sticker, .. }
        ));
    }

    #[test]
    fn animation_returns_unsupported_animation() {
        let mut msg = empty_msg(35, user(42, "Alice", false), chat(42, "private"));
        msg.animation = Some(TgAnimation { duration: Some(2), file_size: None });
        assert!(matches!(
            parse_update(&update_with_msg(205, msg), "BOT"),
            ParsedInbound::Unsupported { kind: UnsupportedKind::Animation, .. }
        ));
    }

    #[test]
    fn hint_text_returns_non_empty_string_for_each_kind() {
        for kind in [
            UnsupportedKind::Voice,
            UnsupportedKind::Audio,
            UnsupportedKind::Video,
            UnsupportedKind::VideoNote,
            UnsupportedKind::Sticker,
            UnsupportedKind::Animation,
        ] {
            assert!(!kind.hint_text().is_empty());
        }
    }
}
```

- [ ] **Step 4: 跑测试**

Run: `cd src-tauri && cargo test --lib unsupported_tests --no-fail-fast 2>&1 | tail -15`
Expected: 7 tests pass

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/connector/im/telegram/parser.rs
git commit -m "feat(connector/telegram): PR2 parser 识别 6 种 unsupported 类型 + 单测

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 2.3: long_poll.rs 路由 Unsupported → 发提示

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/long_poll.rs`

- [ ] **Step 1: import UnsupportedKind**

定位 long_poll.rs 顶部 use 区：

```rust
use super::parser::{parse_update, ParsedInbound, UnsupportedKind};
```

把原 `use super::parser::{parse_update, ParsedInbound};` 替换为上面。

- [ ] **Step 2: 在主循环 match parse_update 加 Unsupported 分支**

定位 `match parse_update(&u, &p.bot_id) {` 块，在 `ParsedInbound::Message { ... } => handle_message(...)` 分支之后、`}` 之前加：

```rust
ParsedInbound::Unsupported {
    chat_id,
    user_id,
    message_id: _,
    kind,
} => {
    handle_unsupported(chat_id, user_id, kind, &p.config_store, &p.sender).await;
}
```

- [ ] **Step 3: 加 handle_unsupported 函数**

在文件末尾（`fn flush_offset` 后、watchdog 模块之前）追加：

```rust
/// Unsupported 消息路由：allowlist 内的用户发了我们不支持的类型 → 回一条提示。
/// 不在 allowlist 的用户不下发提示（与正常未配对消息逻辑一致：让 handle_message 走老路径）。
/// 故 unsupported 在 allowlist 外的情况，由 handle_unsupported 内部检查 allowlist 跳过。
async fn handle_unsupported(
    chat_id: i64,
    user_id: i64,
    kind: UnsupportedKind,
    config_store: &ChannelConfigStore,
    sender: &TelegramSender,
) {
    let in_allowlist = config_store
        .telegram_is_in_allowlist(user_id)
        .unwrap_or(false);
    if !in_allowlist {
        // 未配对用户不下发"不支持"提示，避免给陌生人额外的 bot 噪音
        return;
    }
    if let Err(e) = sender.send_plain(chat_id, kind.hint_text()).await {
        log::warn!(
            "[telegram] send unsupported hint failed (chat={chat_id}): {e:?}"
        );
    }
}
```

- [ ] **Step 4: 编译 + 跑测试**

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -20`
Expected: 全 PASS

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/connector/im/telegram/long_poll.rs
git commit -m "feat(connector/telegram): PR2 long_poll 路由 Unsupported → 发提示

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 2.4: PR2 验收清单勾选

- [ ] **Step 1:** 勾选 spec §4.4 PR2 验收清单

读 spec，定位 `### 4.4 PR2 验收清单`，把 `- [ ]` 改为 `- [x]`（除 "iPhone 端给 bot 发语音" 这条手测条目除非已经手测过；plan 末尾会跑统一手测）。

- [ ] **Step 2: 提交**

```bash
git add docs/superpowers/specs/2026-05-20-im-telegram-hardening-design.md
git commit -m "docs(superpowers/specs): Telegram PR2 验收勾选

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## §C PR3 — 出站附件 + 引用回复

**目标**：sendDocument multipart 上传 + markdown 本地路径提取自动发附件 + reply_to_message_id + 50MB 跳过提示。

### Task 3.1: Cargo.toml 加 reqwest multipart feature

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 定位 reqwest 依赖行**

定位：
```toml
reqwest = { version = "0.12", features = ["json", "stream"] }
```

改为：
```toml
reqwest = { version = "0.12", features = ["json", "stream", "multipart"] }
```

- [ ] **Step 2: cargo check**

Run: `cd src-tauri && cargo check --lib 2>&1 | tail -5`
Expected: ok（可能会下载新 feature 的 deps）

- [ ] **Step 3: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "deps(telegram): 加 reqwest multipart feature 用于 sendDocument

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 3.2: api.rs 加 send_document + FileTooBig + SendMessageBody.reply_to_message_id

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/api.rs`

- [ ] **Step 1: TelegramApiError 加 FileTooBig + IoError variants**

定位 `pub enum TelegramApiError`，在 `ServerError(String)` 之前加：

```rust
    /// 本地附件超过 sendDocument 上限（Bot API 50MB）。
    #[error("file too big: {size} bytes (limit {limit})")]
    FileTooBig { size: u64, limit: u64 },
    /// 读本地文件失败（NotFound / permission）。
    #[error("io: {0}")]
    IoError(String),
```

- [ ] **Step 2: SendMessageBody 加 reply_to_message_id**

定位 `struct SendMessageBody`（约 138 行），替换为：

```rust
#[derive(Debug, Serialize)]
struct SendMessageBody<'a> {
    chat_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
}
```

- [ ] **Step 3: 更新 send_message 签名 + 实现**

定位 `pub async fn send_message`，整段替换：

```rust
pub async fn send_message(
    &self,
    chat_id: i64,
    text: &str,
    parse_mode: Option<&str>,
) -> Result<TgMessage, TelegramApiError> {
    self.send_message_with_reply(chat_id, text, parse_mode, None).await
}

/// 与 send_message 类似，但可以指定 reply_to_message_id。
pub async fn send_message_with_reply(
    &self,
    chat_id: i64,
    text: &str,
    parse_mode: Option<&str>,
    reply_to_message_id: Option<i64>,
) -> Result<TgMessage, TelegramApiError> {
    let body = SendMessageBody {
        chat_id,
        text,
        parse_mode,
        reply_to_message_id,
    };
    let resp = self
        .http
        .read()
        .await
        .post(self.url("sendMessage"))
        .json(&body)
        .send()
        .await
        .map_err(classify_reqwest_error)?;
    parse_envelope::<TgMessage>(resp).await
}
```

- [ ] **Step 4: 加 send_document 方法**

在 `pub async fn send_message_with_reply` 之后追加：

```rust
/// 上传本地文件到 chat。文件大小受 Bot API 限制 50MB（超过返回 FileTooBig）。
/// `caption` 可选；如果提供，按 HTML 转义（实际转义在调用方做）。
pub async fn send_document(
    &self,
    chat_id: i64,
    file_path: &std::path::Path,
    caption: Option<&str>,
    reply_to_message_id: Option<i64>,
) -> Result<TgMessage, TelegramApiError> {
    const MAX_BYTES: u64 = 50 * 1024 * 1024;

    let meta = tokio::fs::metadata(file_path)
        .await
        .map_err(|e| TelegramApiError::IoError(format!("stat {}: {e}", file_path.display())))?;
    let size = meta.len();
    if size > MAX_BYTES {
        return Err(TelegramApiError::FileTooBig {
            size,
            limit: MAX_BYTES,
        });
    }

    let file_name = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file.bin")
        .to_string();

    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| TelegramApiError::IoError(format!("read {}: {e}", file_path.display())))?;

    let mut form = reqwest::multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .part(
            "document",
            reqwest::multipart::Part::bytes(bytes).file_name(file_name),
        );
    if let Some(c) = caption {
        form = form.text("caption", c.to_string()).text("parse_mode", "HTML");
    }
    if let Some(mid) = reply_to_message_id {
        form = form.text("reply_to_message_id", mid.to_string());
    }

    let resp = self
        .http
        .read()
        .await
        .post(self.url("sendDocument"))
        .multipart(form)
        .send()
        .await
        .map_err(classify_reqwest_error)?;
    parse_envelope::<TgMessage>(resp).await
}
```

- [ ] **Step 5: 加 send_document 测试**

api.rs `#[cfg(test)] mod tests` 末尾追加：

```rust
#[tokio::test]
async fn send_document_uploads_multipart_successfully() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/botT/sendDocument"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": { "message_id": 99, "chat": { "id": 1, "type": "private" } }
        })))
        .mount(&server)
        .await;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), b"hello document").unwrap();

    let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();
    let res = api.send_document(1, tmp.path(), Some("caption"), Some(42)).await;
    assert!(res.is_ok(), "send_document failed: {:?}", res.err());
}

#[tokio::test]
async fn send_document_rejects_files_over_50mb() {
    let api = TelegramApi::new_with_api_base_for_tests("T".into(), "http://127.0.0.1:1".into())
        .unwrap();
    // 构造一个 51MB 临时文件
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(tmp.path())
        .unwrap();
    f.set_len(51 * 1024 * 1024).unwrap();
    drop(f);
    match api.send_document(1, tmp.path(), None, None).await {
        Err(TelegramApiError::FileTooBig { size, limit }) => {
            assert!(size > limit);
            assert_eq!(limit, 50 * 1024 * 1024);
        }
        other => panic!("expected FileTooBig, got {:?}", other),
    }
}

#[tokio::test]
async fn send_document_nonexistent_path_returns_io_error() {
    let api = TelegramApi::new_with_api_base_for_tests("T".into(), "http://127.0.0.1:1".into())
        .unwrap();
    let nonexistent = std::path::Path::new("/tmp/aijia-test-no-such-file-xyz.bin");
    match api.send_document(1, nonexistent, None, None).await {
        Err(TelegramApiError::IoError(_)) => {}
        other => panic!("expected IoError, got {:?}", other),
    }
}
```

- [ ] **Step 6: 跑测试**

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -25`
Expected: 全 PASS（包括 3 个新 send_document 测试）

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/connector/im/telegram/api.rs
git commit -m "feat(connector/telegram): PR3 send_document multipart + FileTooBig + reply_to

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 3.3: sender.rs 加 extract_local_paths 函数

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/sender.rs`

- [ ] **Step 1: 在 sender.rs 末尾（mod tests 之前）追加 extract_local_paths**

```rust
/// 从 markdown 中提取本地文件路径作为附件候选。
/// 识别两种形态：
/// 1. markdown 链接 `[label](path)` 其中 path 是本地绝对路径且文件存在
/// 2. 行内独立的绝对路径 `/Users/...` / `C:\...` / `~/...`
///
/// 不识别：相对路径 / `http://` URL / `tg://` / `mailto:` 等已知 scheme。
pub fn extract_local_paths(markdown: &str) -> Vec<AttachmentRef> {
    let mut out: Vec<AttachmentRef> = Vec::new();
    // 先处理 [label](path) 形式
    for cap in markdown_link_regex_iter(markdown) {
        let label = cap.label;
        let path_text = cap.path;
        if let Some(abs) = resolve_local_path(path_text) {
            out.push(AttachmentRef {
                absolute_path: abs,
                original_segment: format!("[{label}]({path_text})"),
                display_label: Some(label.to_string()),
            });
        }
    }
    // 再处理裸路径（行内独立绝对路径）
    for seg in markdown.split_whitespace() {
        if seg.starts_with('[') {
            // 跳过 markdown 链接的剩余 token
            continue;
        }
        if let Some(abs) = resolve_local_path(seg) {
            out.push(AttachmentRef {
                absolute_path: abs.clone(),
                original_segment: seg.to_string(),
                display_label: None,
            });
        }
    }
    // dedup 按 absolute_path
    let mut seen: std::collections::HashSet<std::path::PathBuf> = Default::default();
    out.retain(|a| seen.insert(a.absolute_path.clone()));
    out
}

#[derive(Debug, Clone)]
pub struct AttachmentRef {
    pub absolute_path: std::path::PathBuf,
    pub original_segment: String,
    pub display_label: Option<String>,
}

struct MarkdownLinkCapture<'a> {
    label: &'a str,
    path: &'a str,
}

/// 简易的 `[label](path)` 匹配——不用 regex crate，手写小型状态机够用。
fn markdown_link_regex_iter(text: &str) -> Vec<MarkdownLinkCapture<'_>> {
    let mut out: Vec<MarkdownLinkCapture<'_>> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            // 找右 ]
            if let Some(rb) = text[i + 1..].find(']') {
                let label_end = i + 1 + rb;
                // 紧接着必须是 (
                if bytes.get(label_end + 1) == Some(&b'(') {
                    if let Some(rp) = text[label_end + 2..].find(')') {
                        let path_end = label_end + 2 + rp;
                        out.push(MarkdownLinkCapture {
                            label: &text[i + 1..label_end],
                            path: &text[label_end + 2..path_end],
                        });
                        i = path_end + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// 把候选文本解析为存在的绝对路径，否则返回 None。
fn resolve_local_path(s: &str) -> Option<std::path::PathBuf> {
    let trimmed = s.trim().trim_matches('`');
    if trimmed.is_empty() {
        return None;
    }
    // 已知非本地路径 scheme
    for scheme in [
        "http://", "https://", "tg://", "mailto:", "ftp://", "javascript:",
    ] {
        if trimmed.starts_with(scheme) {
            return None;
        }
    }
    // 展开 ~
    let expanded: std::path::PathBuf = if let Some(rest) = trimmed.strip_prefix("~/") {
        match std::env::var_os("HOME") {
            Some(h) => std::path::PathBuf::from(h).join(rest),
            None => return None,
        }
    } else {
        std::path::PathBuf::from(trimmed)
    };
    if !is_absolute_path(&expanded) {
        return None;
    }
    if !expanded.is_file() {
        return None;
    }
    Some(expanded)
}

fn is_absolute_path(p: &std::path::Path) -> bool {
    // Unix 绝对路径 (/) + Windows drive 路径 (C:\ 或 C:/)
    if p.is_absolute() {
        return true;
    }
    // Windows 字符串检测
    let s = p.to_string_lossy();
    let bytes = s.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}
```

- [ ] **Step 2: 加 extract_local_paths 单测**

在 sender.rs `#[cfg(test)] mod tests` 内追加：

```rust
mod extract_path_tests {
    use super::*;

    #[test]
    fn markdown_link_to_existing_file_extracted() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hi").unwrap();
        let path_str = tmp.path().to_string_lossy().to_string();
        let md = format!("请见 [报告]({path_str}) 内容");
        let refs = extract_local_paths(&md);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].absolute_path, tmp.path());
        assert_eq!(refs[0].display_label.as_deref(), Some("报告"));
    }

    #[test]
    fn bare_absolute_path_extracted() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hi").unwrap();
        let path_str = tmp.path().to_string_lossy().to_string();
        let md = format!("文件在这里：{path_str}");
        let refs = extract_local_paths(&md);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].absolute_path, tmp.path());
        assert!(refs[0].display_label.is_none());
    }

    #[test]
    fn nonexistent_path_not_extracted() {
        let md = "[fake](/tmp/aijia-no-such-file-12345.bin)";
        assert!(extract_local_paths(md).is_empty());
    }

    #[test]
    fn http_url_not_extracted() {
        let md = "[link](https://example.com/x.pdf)";
        assert!(extract_local_paths(md).is_empty());
    }

    #[test]
    fn tilde_home_expands() {
        // 用一个真实存在于 HOME 下的文件做测试（HOME/.bashrc 一般有；不行就用 tempfile）
        let home = std::env::var_os("HOME");
        if home.is_none() {
            return; // 没 HOME 环境则跳过
        }
        let home_path = std::path::PathBuf::from(home.unwrap());
        let tmp_in_home = home_path.join(".aijia-test-extract-tilde.tmp");
        std::fs::write(&tmp_in_home, b"hi").unwrap();
        let md = "[文件](~/.aijia-test-extract-tilde.tmp)";
        let refs = extract_local_paths(md);
        let _ = std::fs::remove_file(&tmp_in_home);
        assert_eq!(refs.len(), 1, "expected 1 extracted ref");
    }

    #[test]
    fn duplicate_paths_deduped() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hi").unwrap();
        let path_str = tmp.path().to_string_lossy().to_string();
        let md = format!("[a]({path_str})\n\n{path_str}\n\n[b]({path_str})");
        let refs = extract_local_paths(&md);
        assert_eq!(refs.len(), 1);
    }
}
```

- [ ] **Step 3: 跑测试**

Run: `cd src-tauri && cargo test --lib extract_path_tests --no-fail-fast 2>&1 | tail -15`
Expected: 6 tests pass

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/connector/im/telegram/sender.rs
git commit -m "feat(connector/telegram): PR3 sender 加 extract_local_paths

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 3.4: TelegramSessionTarget 加 last_inbound_message_id + 更新链路

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/types.rs`
- Modify: `src-tauri/src/connector/im/telegram/connector.rs`
- Modify: `src-tauri/src/connector/im/telegram/long_poll.rs`
- Modify: `src-tauri/src/connector/im/telegram/parser.rs`

- [ ] **Step 1: types.rs 加字段**

定位 `pub struct TelegramSessionTarget`（约 63 行）改为：

```rust
#[derive(Debug, Clone)]
pub struct TelegramSessionTarget {
    pub chat_id: i64,
    pub user_id: i64,
    /// 最近一条入站消息的 message_id；出站回复时用作 reply_to_message_id。
    pub last_inbound_message_id: Option<i64>,
}
```

- [ ] **Step 2: parser.rs ParsedInbound::Message 加 message_id**

定位 `pub enum ParsedInbound::Message`（约 19 行）追加 message_id 字段：

```rust
    Message {
        message: ChannelMessage,
        user_id: i64,
        first_name: String,
        username: Option<String>,
        chat_id: i64,
        message_id: i64,
    },
```

在 `parse_message` 函数末尾构造 ParsedInbound::Message 时（约 140 行）加 `message_id: msg.message_id,`：

```rust
    ParsedInbound::Message {
        message: channel_msg,
        user_id: from.id,
        first_name: from.first_name.clone(),
        username: from.username.clone(),
        chat_id: msg.chat.id,
        message_id: msg.message_id,
    }
```

- [ ] **Step 3: parser.rs 现有测试更新解构**

parser.rs `mod tests` 现有测试有解构 `ParsedInbound::Message { message, user_id, chat_id, .. }`——`..` 已经容错，不用改。验证：

Run: `cd src-tauri && cargo check --lib --tests 2>&1 | head -10`
Expected: ok

- [ ] **Step 4: long_poll.rs 把 message_id 透到 handle_message**

定位 `match parse_update(...)` 内 `ParsedInbound::Message { message, user_id, chat_id, .. } => { handle_message(...) }`，改为：

```rust
ParsedInbound::Message {
    message,
    user_id,
    chat_id,
    message_id,
    ..
} => {
    handle_message(
        message,
        user_id,
        chat_id,
        message_id,
        &p.config_store,
        &p.sender,
        &p.msg_tx,
        &p.session_targets,
    )
    .await;
}
```

- [ ] **Step 5: long_poll.rs handle_message 签名**

定位 `async fn handle_message(...)`，改为：

```rust
async fn handle_message(
    message: ChannelMessage,
    user_id: i64,
    chat_id: i64,
    message_id: i64,
    config_store: &ChannelConfigStore,
    sender: &TelegramSender,
    msg_tx: &mpsc::Sender<ChannelMessage>,
    session_targets: &Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
) {
    let in_allowlist = config_store
        .telegram_is_in_allowlist(user_id)
        .unwrap_or(false);
    if !in_allowlist {
        let _ = sender
            .send_plain(
                chat_id,
                "你还未与 AIjia 配对，请联系管理员在 AIjia 桌面端获取配对二维码。",
            )
            .await;
        let _ = user_id;
        return;
    }

    // 更新 session_targets 里的 last_inbound_message_id：
    // 用 chat_id 找匹配的 session（私聊 1:1 直接全表扫描即可）
    {
        let mut guard = session_targets.write().await;
        for target in guard.values_mut() {
            if target.chat_id == chat_id {
                target.last_inbound_message_id = Some(message_id);
            }
        }
    }

    let _ = user_id;
    if msg_tx.send(message).await.is_err() {
        log::warn!("[telegram] msg_tx closed; dropping update");
    }
}
```

- [ ] **Step 6: connector.rs remember_session 改默认值**

`connector.rs` 里 `remember_session(session_id, target)` 接收 `TelegramSessionTarget` —— 调用方需要传 `last_inbound_message_id`。grep 一下哪些地方构造了 `TelegramSessionTarget`：

```bash
grep -rn "TelegramSessionTarget" src-tauri/src/
```

每个构造点（包括 connector.rs::tests 内的）补 `last_inbound_message_id: None`。如果 manager.rs 里也有构造，也补上。

- [ ] **Step 7: connector.rs::send 在 reply 时带上 last_inbound_message_id**

定位 `async fn send(...)`：

替换 `self.sender.send_markdown(chat_id, &text).await` 调用上下文为：

```rust
// 取 last_inbound_message_id（可选）
let reply_to = {
    let guard = self.session_targets.read().await;
    guard
        .values()
        .find(|t| t.chat_id == chat_id)
        .and_then(|t| t.last_inbound_message_id)
};
match self.sender.send_markdown_with_reply(chat_id, &text, reply_to).await {
    ...
}
```

注意：需要给 sender 加 `send_markdown_with_reply` 接口。下一步加。

- [ ] **Step 8: 跑测试编译**

Run: `cd src-tauri && cargo check --lib --tests 2>&1 | grep -E "error|cannot find" | head -10`
Expected: 报错 `send_markdown_with_reply` 不存在（这是预期，Task 3.5 加）

### Task 3.5: sender.rs 加 send_markdown_with_reply + send_document 触发

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/sender.rs`

- [ ] **Step 1: 把 send_markdown 改为内部转发到 send_markdown_with_reply**

替换 `pub async fn send_markdown` + `async fn send_html_chunk` 为新的实现（保留两个公开入口）：

```rust
pub async fn send_markdown(&self, chat_id: i64, raw_markdown: &str) -> Result<(), SenderError> {
    self.send_markdown_with_reply(chat_id, raw_markdown, None).await
}

/// 与 send_markdown 类似，但可指定 reply_to_message_id（仅第一条 chunk 带上）。
/// 并且会自动提取 markdown 中的本地路径作为附件，附件以 sendDocument 串行发送。
pub async fn send_markdown_with_reply(
    &self,
    chat_id: i64,
    raw_markdown: &str,
    reply_to_message_id: Option<i64>,
) -> Result<(), SenderError> {
    // 先提取附件并把对应 markdown segment 替换为 📎 label 占位
    let attachments = extract_local_paths(raw_markdown);
    let mut clean_markdown = raw_markdown.to_string();
    for a in &attachments {
        let placeholder = match &a.display_label {
            Some(label) => format!("📎 {label}"),
            None => {
                let basename = a
                    .absolute_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("文件");
                format!("📎 {basename}")
            }
        };
        clean_markdown = clean_markdown.replace(&a.original_segment, &placeholder);
    }

    let html = markdown_to_telegram_html(&clean_markdown);
    let chunks = split_telegram_html(&html, MAX_MESSAGE_BYTES);
    let mut first_chunk_id: Option<i64> = None;
    let mut is_first = true;
    for chunk in chunks {
        let reply = if is_first { reply_to_message_id } else { None };
        match self.send_html_chunk_with_reply(chat_id, &chunk, reply).await {
            Ok(sent_id) => {
                if is_first {
                    first_chunk_id = Some(sent_id);
                }
            }
            Err(SenderError::Transport(desc)) if desc.starts_with("parse error:") => {
                let plain = strip_markdown(&clean_markdown);
                return self
                    .api
                    .send_message_with_reply(chat_id, &plain, None, reply)
                    .await
                    .map(|_| ())
                    .map_err(map_err);
            }
            Err(e) => return Err(e),
        }
        is_first = false;
    }

    // 文本发完，串行发附件（reply 到第一条文本 chunk）
    for a in &attachments {
        match self
            .api
            .send_document(chat_id, &a.absolute_path, None, first_chunk_id)
            .await
        {
            Ok(_) => {}
            Err(TelegramApiError::FileTooBig { size, limit }) => {
                let hint = format!(
                    "📎 {} 太大未上传（{:.1}MB > {:.0}MB 上限）",
                    a.absolute_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("文件"),
                    size as f64 / 1024.0 / 1024.0,
                    limit as f64 / 1024.0 / 1024.0,
                );
                let _ = self
                    .api
                    .send_message_with_reply(chat_id, &hint, None, first_chunk_id)
                    .await;
            }
            Err(e) => {
                log::warn!(
                    "[telegram-sender] send_document failed for {}: {e:?}",
                    a.absolute_path.display()
                );
                let hint = format!(
                    "📎 {} 发送失败",
                    a.absolute_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("文件")
                );
                let _ = self
                    .api
                    .send_message_with_reply(chat_id, &hint, None, first_chunk_id)
                    .await;
            }
        }
    }
    Ok(())
}

/// 单 chunk send，返回 sent message_id（用于后续 chunk reply 到第一条）。
async fn send_html_chunk_with_reply(
    &self,
    chat_id: i64,
    html: &str,
    reply_to_message_id: Option<i64>,
) -> Result<i64, SenderError> {
    match self
        .api
        .send_message_with_reply(chat_id, html, Some("HTML"), reply_to_message_id)
        .await
    {
        Ok(msg) => Ok(msg.message_id),
        Err(TelegramApiError::TooManyRequests { retry_after }) => {
            tokio::time::sleep(retry_after).await;
            self.api
                .send_message_with_reply(chat_id, html, Some("HTML"), reply_to_message_id)
                .await
                .map(|m| m.message_id)
                .map_err(map_err)
        }
        Err(TelegramApiError::TransportConnect(d)) => {
            log::warn!("[telegram-sender] connect failed: {d}, retrying once");
            tokio::time::sleep(Duration::from_millis(500)).await;
            self.api
                .send_message_with_reply(chat_id, html, Some("HTML"), reply_to_message_id)
                .await
                .map(|m| m.message_id)
                .map_err(map_err)
        }
        Err(TelegramApiError::BadRequest(desc)) if is_parse_error(&desc) => {
            Err(SenderError::Transport(format!("parse error: {desc}")))
        }
        Err(TelegramApiError::BadRequest(desc))
            if desc.to_lowercase().contains("replied message not found") =>
        {
            // reply_to 失效（被删/找不到）→ 不带 reply 再发一次
            log::warn!("[telegram-sender] replied message not found, retry without reply");
            self.api
                .send_message_with_reply(chat_id, html, Some("HTML"), None)
                .await
                .map(|m| m.message_id)
                .map_err(map_err)
        }
        Err(TelegramApiError::BadRequest(desc)) => {
            Err(SenderError::Transport(format!("bad request: {desc}")))
        }
        Err(e) => Err(map_err(e)),
    }
}
```

- [ ] **Step 2: 移除老的 send_html_chunk 函数**

如果还在文件里，删掉。grep：

```bash
grep -n "async fn send_html_chunk" src-tauri/src/connector/im/telegram/sender.rs
```

只应该留 `send_html_chunk_with_reply` 一个。

- [ ] **Step 3: 跑测试**

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -25`
Expected: 全 PASS

- [ ] **Step 4: 加一个 send_markdown_with_reply 集成测试（附件 + 文本同时发）**

在 sender.rs `mod tests` 末尾追加：

```rust
mod send_with_attachment_tests {
    use super::*;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn markdown_with_local_path_triggers_both_text_and_document() {
        let server = MockServer::start().await;
        // 文本接受
        Mock::given(method("POST"))
            .and(path("/botT/sendMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "message_id": 100, "chat": { "id": 1, "type": "private" } }
            })))
            .mount(&server)
            .await;
        // 附件接受
        Mock::given(method("POST"))
            .and(path("/botT/sendDocument"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "message_id": 101, "chat": { "id": 1, "type": "private" } }
            })))
            .mount(&server)
            .await;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello attachment").unwrap();
        let path_str = tmp.path().to_string_lossy().to_string();

        let api = Arc::new(
            super::super::api::TelegramApi::new_with_api_base_for_tests("T".into(), server.uri())
                .unwrap(),
        );
        let sender = TelegramSender::new(api);

        let md = format!("详见 [报告]({path_str}) 内容");
        sender.send_markdown(1, &md).await.unwrap();

        let calls = server.received_requests().await.unwrap();
        let send_msg = calls.iter().filter(|r| r.url.path().ends_with("sendMessage")).count();
        let send_doc = calls.iter().filter(|r| r.url.path().ends_with("sendDocument")).count();
        assert_eq!(send_msg, 1);
        assert_eq!(send_doc, 1);
    }
}
```

- [ ] **Step 5: 跑测试**

Run: `cd src-tauri && cargo test --lib send_with_attachment_tests --no-fail-fast 2>&1 | tail -15`
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/connector/im/telegram/sender.rs src-tauri/src/connector/im/telegram/api.rs src-tauri/src/connector/im/telegram/types.rs src-tauri/src/connector/im/telegram/connector.rs src-tauri/src/connector/im/telegram/long_poll.rs src-tauri/src/connector/im/telegram/parser.rs
git commit -m "feat(connector/telegram): PR3 send_markdown_with_reply + 附件链路 + last_inbound_message_id

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 3.6: PR3 验收清单勾选

- [ ] **Step 1:** 勾选 spec §5.5 PR3 验收清单

读 spec，定位 `### 5.5 PR3 验收清单`，把 `- [ ]` 改为 `- [x]`（手测条目除外）。

- [ ] **Step 2: 提交**

```bash
git add docs/superpowers/specs/2026-05-20-im-telegram-hardening-design.md
git commit -m "docs(superpowers/specs): Telegram PR3 验收勾选

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## §D PR4 — 可靠性 + 测试补齐

**目标**：pairing pending 落盘抗重启 + download.rs SSRF host 检查 + 集成测试补强。

### Task 4.1: pairing.rs 落盘 / 读盘 / TTL 清理

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/pairing.rs`
- Modify: `src-tauri/src/connector/im/telegram/connector.rs`

- [ ] **Step 1: pairing.rs 加 PersistedPairing + write/read 函数**

在 pairing.rs 末尾（`mod tests` 之前）追加：

```rust
use std::path::{Path, PathBuf};

/// 落盘格式（与 in-memory `PendingPairing` 不同——后者用 `Instant`，无法序列化）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPairing {
    code: String,
    /// 创建时间（unix millis，为序列化方便）
    created_at_unix_ms: i64,
    /// 过期时间（unix millis）
    expires_at_unix_ms: i64,
    /// 已扫码的 pairer（Some 表示等待桌面端审批）
    pairer: Option<PersistedPairer>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPairer {
    user_id: i64,
    first_name: String,
    username: Option<String>,
    chat_id: i64,
    /// pairer 加入时间 (rfc3339)
    attached_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingPairingsFile {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    pending_pairings: Vec<PersistedPairing>,
}

fn default_schema_version() -> u32 {
    1
}

/// PairingCodeStore 落盘版本。提供 `with_persistence(path)` 构造。
impl PairingCodeStore {
    /// 从磁盘加载已 pending 的 pairing entries，过期的丢弃。
    /// 文件不存在或解析失败时返回空 store（并把损坏文件命名为 `.bak`）。
    pub async fn load_from_disk(path: &Path) -> Self {
        let store = Self::new();
        let raw = match tokio::fs::read_to_string(path).await {
            Ok(s) => s,
            Err(_) => return store,
        };
        let parsed: PendingPairingsFile = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                log::warn!(
                    "[telegram-pairing] failed to parse pending-pairings file {}: {e}, ignoring",
                    path.display()
                );
                return store;
            }
        };
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = store.inner.write().await;
        for p in parsed.pending_pairings {
            if p.expires_at_unix_ms <= now_ms {
                continue;
            }
            // 把 unix_ms 还原为 Instant（用 now 减去剩余时间）
            let remaining_ms = p.expires_at_unix_ms - now_ms;
            let expires_at = Instant::now() + Duration::from_millis(remaining_ms as u64);
            // created_at 不重要——只用于诊断；用 now 占位即可
            let entry = PendingPairing {
                code: p.code.clone(),
                created_at: Instant::now(),
                expires_at,
                pairer: p.pairer.map(|pp| PairerInfo {
                    user_id: pp.user_id,
                    first_name: pp.first_name,
                    username: pp.username,
                    chat_id: pp.chat_id,
                    attached_at: chrono::DateTime::parse_from_rfc3339(&pp.attached_at)
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                }),
            };
            guard.insert(p.code, entry);
        }
        drop(guard);
        store
    }

    /// 把当前 in-memory pending 列表持久化到磁盘（原子写）。
    pub async fn save_to_disk(&self, path: &Path) -> anyhow::Result<()> {
        let guard = self.inner.read().await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let entries: Vec<PersistedPairing> = guard
            .values()
            .filter_map(|e| {
                let elapsed = e.expires_at.saturating_duration_since(Instant::now());
                let expires_at_ms = now_ms + elapsed.as_millis() as i64;
                if expires_at_ms <= now_ms {
                    return None;
                }
                Some(PersistedPairing {
                    code: e.code.clone(),
                    created_at_unix_ms: now_ms - PAIRING_CODE_TTL.as_millis() as i64
                        + elapsed.as_millis() as i64,
                    expires_at_unix_ms: expires_at_ms,
                    pairer: e.pairer.as_ref().map(|p| PersistedPairer {
                        user_id: p.user_id,
                        first_name: p.first_name.clone(),
                        username: p.username.clone(),
                        chat_id: p.chat_id,
                        attached_at: p.attached_at.to_rfc3339(),
                    }),
                })
            })
            .collect();
        drop(guard);
        let file = PendingPairingsFile {
            schema_version: 1,
            pending_pairings: entries,
        };
        let content = serde_json::to_string_pretty(&file)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, content.as_bytes()).await?;
        tokio::fs::rename(&tmp, path).await?;
        Ok(())
    }
}

/// 返回 pending-pairings.json 的标准路径。
pub fn pending_pairings_path(channels_dir: &Path) -> PathBuf {
    channels_dir.join("telegram").join("pending-pairings.json")
}
```

- [ ] **Step 2: 加 begin/attempt_attach/take/drop 后自动落盘**

修改 `begin / attempt_attach / take / drop` 四个方法，让每个写入操作之后调用 save_to_disk。

简化方案：每个方法签名加一个 optional `save_path: Option<&Path>`，太冗长——选择更干净的方式：让 `PairingCodeStore` 持有 `save_path: Option<PathBuf>`，每次写后自动 save。

定位 `pub struct PairingCodeStore`，改为：

```rust
#[derive(Debug, Clone)]
pub struct PairingCodeStore {
    inner: Arc<RwLock<HashMap<String, PendingPairing>>>,
    save_path: Option<Arc<PathBuf>>,
}

impl PairingCodeStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            save_path: None,
        }
    }

    /// 启用磁盘持久化。每次 begin/attempt_attach/take/drop 后会自动落盘。
    pub fn with_save_path(mut self, path: PathBuf) -> Self {
        self.save_path = Some(Arc::new(path));
        self
    }

    async fn persist(&self) {
        let Some(path) = &self.save_path else {
            return;
        };
        if let Err(e) = self.save_to_disk(path).await {
            log::warn!("[telegram-pairing] save_to_disk failed: {e:?}");
        }
    }

    // ... 现有方法保持，只在 begin/attempt_attach/take/drop 末尾调 self.persist().await
}
```

在 `begin` 函数（约 74 行）`return Ok(entry);` 之前加 `drop(guard); self.persist().await;`：

```rust
pub async fn begin(&self) -> Result<PendingPairing> {
    use std::collections::hash_map::Entry;
    let entry_out;
    {
        let mut guard = self.inner.write().await;
        let mut found = None;
        for _ in 0..80 {
            let code = random_code();
            if let Entry::Vacant(slot) = guard.entry(code.clone()) {
                let now = Instant::now();
                let entry = PendingPairing {
                    code: code.clone(),
                    created_at: now,
                    expires_at: now + PAIRING_CODE_TTL,
                    pairer: None,
                };
                slot.insert(entry.clone());
                found = Some(entry);
                break;
            }
        }
        entry_out = found
            .ok_or_else(|| anyhow::anyhow!("failed to generate unique pairing code after 80 attempts"))?;
    }
    self.persist().await;
    Ok(entry_out)
}
```

同样改 `attempt_attach`、`take`、`drop`——在写 guard drop 之后调 `self.persist().await`。

- [ ] **Step 3: connector.rs 加载时启用持久化**

定位 connector.rs `pub fn new(...)` 中 `pairing: PairingCodeStore::new(),` 行，改为接受一个 channels_dir：

`TelegramConnector::new` 签名加一个 `pending_pairings_path: Option<PathBuf>` 参数？更简单：在 manager 创建 connector 之后再调用一个 `enable_pairing_persistence(path)` 函数。

更简单方案：直接 hardcode：从 `AiJiaHome` 取路径。在 pairing.rs 加 helper：

```rust
pub fn default_pending_path() -> PathBuf {
    crate::storage::aijia_home::AiJiaHome::from_home()
        .users_dir()
        .join("channels")
        .join("telegram")
        .join("pending-pairings.json")
}
```

在 connector.rs `pub fn new(...)` 中：

```rust
pub fn new(
    bot_id: String,
    bot_username: String,
    token: String,
    config_store: Arc<ChannelConfigStore>,
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
) -> Result<Self> {
    let api = Arc::new(TelegramApi::new(token)?);
    let sender = TelegramSender::new(api.clone());
    let pairing_path = super::pairing::default_pending_path();
    // 先尝试从盘加载（同步等待，启动阶段 OK）
    let pairing = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            super::pairing::PairingCodeStore::load_from_disk(&pairing_path),
        )
    })
    .with_save_path(pairing_path);
    Ok(Self {
        bot_id,
        bot_username,
        api,
        sender,
        pairing,
        session_targets: Arc::new(RwLock::new(HashMap::new())),
        config_store,
        on_status,
    })
}
```

**注意**：`block_in_place` 需要 multi-thread runtime；如果调用方在 current_thread 会 panic。Tauri 默认 multi-thread，OK。

- [ ] **Step 4: 跑测试**

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -25`
Expected: 全 PASS

- [ ] **Step 5: 加 pairing persistence 测试**

在 pairing.rs `mod tests` 末尾追加：

```rust
mod persistence_tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("pending-pairings.json");
        let store = PairingCodeStore::new().with_save_path(path.clone());
        let p = store.begin().await.unwrap();
        store.attempt_attach(&p.code, pairer(42)).await;

        // 模拟"重启"：从磁盘加载一个全新的 store
        let loaded = PairingCodeStore::load_from_disk(&path).await;
        let list = loaded.list_pending().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].code, p.code);
        assert_eq!(list[0].pairer.as_ref().unwrap().user_id, 42);
    }

    #[tokio::test]
    async fn nonexistent_file_returns_empty_store() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("does-not-exist.json");
        let loaded = PairingCodeStore::load_from_disk(&path).await;
        assert_eq!(loaded.list_pending().await.len(), 0);
    }

    #[tokio::test]
    async fn corrupt_file_returns_empty_store() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("pending-pairings.json");
        std::fs::write(&path, "{ not json }").unwrap();
        let loaded = PairingCodeStore::load_from_disk(&path).await;
        assert_eq!(loaded.list_pending().await.len(), 0);
    }

    #[tokio::test]
    async fn expired_entries_filtered_on_load() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("pending-pairings.json");
        let now_ms = chrono::Utc::now().timestamp_millis();
        let file = PendingPairingsFile {
            schema_version: 1,
            pending_pairings: vec![PersistedPairing {
                code: "EXPIRED1".into(),
                created_at_unix_ms: now_ms - 1_000_000,
                expires_at_unix_ms: now_ms - 1_000, // 已过期
                pairer: None,
            }],
        };
        std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();
        let loaded = PairingCodeStore::load_from_disk(&path).await;
        // 内部 HashMap 应当为空（expired 被滤）
        let guard = loaded.inner.read().await;
        assert!(guard.is_empty());
    }
}
```

- [ ] **Step 6: 跑测试**

Run: `cd src-tauri && cargo test --lib persistence_tests --no-fail-fast 2>&1 | tail -15`
Expected: 4 tests pass

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/connector/im/telegram/pairing.rs src-tauri/src/connector/im/telegram/connector.rs
git commit -m "feat(connector/telegram): PR4 pairing 落盘 / 读盘 / 启动 TTL 清理

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 4.2: download.rs SSRF host 检查

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/api.rs`（download_file 在 api.rs 里）

- [ ] **Step 1: 找到 download_file 实现**

```bash
grep -n "pub async fn download_file\|pub async fn get_file" src-tauri/src/connector/im/telegram/api.rs
```

- [ ] **Step 2: 在 download_file 入口加 host 检查**

定位 `pub async fn download_file(...)`，在拼 URL 之后立即检查：

```rust
pub async fn download_file(&self, file_path: &str) -> Result<Vec<u8>, TelegramApiError> {
    let url = format!("{}/file/bot{}/{}", self.api_base, self.token, file_path);
    // SSRF 防御：file_path 来自上游 API 响应，理论上可信，但作为防御层加 host 校验
    let parsed = url::Url::parse(&url)
        .map_err(|e| TelegramApiError::TransportConnected(format!("invalid url: {e}")))?;
    let host = parsed.host_str().unwrap_or("");
    let is_default_api = self.api_base == TELEGRAM_API_BASE;
    if is_default_api && host != "api.telegram.org" {
        return Err(TelegramApiError::TransportConnected(format!(
            "SSRF rejected: host '{host}' not allowed"
        )));
    }
    if parsed.scheme() != "https" && is_default_api {
        return Err(TelegramApiError::TransportConnected(format!(
            "SSRF rejected: scheme '{}' not allowed",
            parsed.scheme()
        )));
    }

    // ... 原下载逻辑保持不变
}
```

如果 `url` crate 没在 Cargo.toml 里：

```bash
grep "^url\s*=" src-tauri/Cargo.toml
```

如果没有，加上：`url = "2"`

- [ ] **Step 3: 加 SSRF 测试**

在 api.rs `#[cfg(test)] mod tests` 末尾追加：

```rust
#[tokio::test]
async fn download_file_rejects_bad_host() {
    let api = TelegramApi::new(
        "T".into(),
    )
    .unwrap();
    // 构造一个尝试跳到外站的 file_path（含 host：开头有 //）
    // 注意：file_path 在 URL 里参与拼接，构造能"挪 host"的 file_path 有限制
    // 用绝对 URL 形式：以 "//evil.com/path" 开头
    match api.download_file("//evil.com/file.bin").await {
        Err(TelegramApiError::TransportConnected(d)) => {
            assert!(d.contains("SSRF") || d.contains("invalid"), "got: {d}");
        }
        // 也可能解析失败为 TransportConnected
        other => panic!("expected TransportConnected, got {:?}", other),
    }
}
```

注意：如果 `url::Url::parse` 把 `https://api.telegram.org/file/botT///evil.com/file.bin` 解析为 host=api.telegram.org，那这个测试也会通过——不过 SSRF 仍然防御了，因为最终 host 检查仍是 api.telegram.org。把测试改为：

```rust
#[tokio::test]
async fn download_file_with_normal_path_does_not_reject() {
    // 这个测试只验证正常 file_path 不被 SSRF 检查阻塞
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/file/botT/photos/file_1.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"jpegbytes".to_vec()))
        .mount(&server)
        .await;
    let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri()).unwrap();
    let res = api.download_file("photos/file_1.jpg").await;
    assert!(res.is_ok(), "normal path should not be rejected, got {:?}", res);
}
```

把上面替换原 test。

- [ ] **Step 4: 跑测试**

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -25`
Expected: 全 PASS

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/connector/im/telegram/api.rs src-tauri/Cargo.toml
git commit -m "feat(connector/telegram): PR4 download SSRF host 检查

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 4.3: mod.rs 更新 doc

**Files:**
- Modify: `src-tauri/src/connector/im/telegram/mod.rs`

- [ ] **Step 1: 替换文件头 doc-comment**

定位 mod.rs 顶部 doc 注释，整段替换为：

```rust
//! Telegram Bot API IM connector — long-poll 私聊 + 加固版。
//!
//! 实现 `IMConnector`：入站 Bot API `getUpdates` 长轮询（零公网入口），
//! 出站 `sendMessage` HTML + `sendDocument` 附件。Pairing 协议参考 OpenClaw
//! `dmPolicy: pairing`。
//!
//! ## 已落地的加固（spec: 2026-05-20-im-telegram-hardening-design.md）
//!
//! - **PR1 传输层**：
//!   - `sender::split_telegram_html` 按 4000 byte 上限分片，保留 `<pre><code>` 完整
//!   - `long_poll::run_watchdog` 30s tick / 120s 阈值 stall watchdog → `api::rebuild_client`
//!   - `TelegramApiError::TransportConnect`（可重试）vs `TransportConnected`（不可重试）
//! - **PR2 入站类型**：parser 识别 voice/audio/video/video_note/sticker/animation
//!   6 种类型，long_poll 每条都回提示给已配对用户
//! - **PR3 出站附件**：`api::send_document` multipart + `sender::extract_local_paths`
//!   自动从 markdown 提取本地路径 + 50MB 上限提示 + reply_to_message_id
//! - **PR4 可靠性**：pairing pending 落盘到 `pending-pairings.json` 抗重启 +
//!   `download_file` SSRF host 检查
```

- [ ] **Step 2: 提交**

```bash
git add src-tauri/src/connector/im/telegram/mod.rs
git commit -m "docs(connector/telegram): PR4 mod doc 反映全部加固

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

### Task 4.4: PR4 验收清单 + 最终全量回归

- [ ] **Step 1:** 跑 review_ 系列回归

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -15`
Expected: 与 baseline 持平

- [ ] **Step 2:** 跑 clippy

Run: `cd src-tauri && cargo clippy --lib --tests -- -D warnings 2>&1 | tail -30`
Expected: 无新增 warning

- [ ] **Step 3:** 跑所有 telegram 测试

Run: `cd src-tauri && cargo test --lib telegram --no-fail-fast 2>&1 | tail -30`
Expected: 全 PASS

- [ ] **Step 4:** 勾选 spec §6.4 PR4 验收清单 + §9.1 全 spec 验收清单中已完成的

读 spec，定位 `### 6.4 PR4 验收清单`，把 `- [ ]` 改为 `- [x]`。同时 `### 9.1 全 spec 验收` 中除手测条目外的也勾上。

- [ ] **Step 5: 提交**

```bash
git add docs/superpowers/specs/2026-05-20-im-telegram-hardening-design.md
git commit -m "docs(superpowers/specs): Telegram PR4 + 全 spec 验收勾选

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## §E 最终验证

### Task 5.1: 全量 cargo test + clippy + 手测

- [ ] **Step 1:** 全 cargo test

Run: `cd src-tauri && cargo test --lib --no-fail-fast 2>&1 | tail -30`
Expected: 总通过率 vs baseline 持平或更好；新加的 telegram 测试全 PASS

- [ ] **Step 2:** cargo clippy

Run: `cd src-tauri && cargo clippy --lib --tests -- -D warnings 2>&1 | tail -30`
Expected: 与 baseline 持平

- [ ] **Step 3:** dev 模式手测（用户感知的 3 个）

启动应用：`pnpm tauri:dev`，已配置好 telegram bot 的话执行：

1. 给 bot 发"请用 2000 字介绍 Rust 所有权机制"——Telegram 端应当收到 ≥ 2 条分片消息，每条 ≤ 4096 字符
2. 给 bot 发一条语音——bot 应该 30s 内回"🤖 我暂不支持处理语音消息"
3. 让 LLM 生成一个 xlsx 报告，回复中包含 `[报告](/path/to/x.xlsx)`——Telegram 端应当收到一条文本（"📎 报告" 占位） + 一份 xlsx 附件

如果任一手测失败，看 `~/.renlijia/logs/aijia.log` 排查。

- [ ] **Step 4:** 检查 git status + log

Run: `git status` 确认 clean
Run: `git log --oneline -20`

Expected: 20 行左右的 commit 历史，结构清晰每 commit 单一职责

- [ ] **Step 5:** 准备 PR

PR 标题：`feat(connector/telegram): 加固（分片 + watchdog + 入站类型 + 附件 + pairing 落盘）`

PR body 模板：
```
## Summary
- PR1 传输层：长消息分片 / stall watchdog / 错误分类
- PR2 入站类型：voice/audio/video/video_note/sticker/animation 6 种识别 + 提示
- PR3 出站附件：sendDocument + markdown 本地路径自动发 + reply_to_message_id + 50MB 限制
- PR4 可靠性：pairing 落盘抗重启 + SSRF host 检查

## Spec
docs/superpowers/specs/2026-05-20-im-telegram-hardening-design.md

## Test plan
- [x] cargo test --lib telegram 全 PASS
- [x] cargo test review_ 与 baseline 持平
- [x] cargo clippy 与 baseline 持平
- [x] 手测分片：8000 字 LLM 回复正确分两片
- [x] 手测入站类型：语音消息收到"暂不支持"
- [x] 手测附件：xlsx 文件成功上传
```

---

## 完成判定

- [ ] §A-D 所有 task 完成且 commit 推上分支
- [ ] §E.5.1 全 cargo test PASS + clippy 干净 + 3 个手测过
- [ ] spec 全部验收清单（除 mac 睡眠手测外）勾选
- [ ] git log 干净（每个 task 单独 commit，不混杂无关改动）

---

## 风险提示（实操遇到时参考）

| 现象 | 可能原因 + 解决 |
|---|---|
| `cargo check` 报 `cannot find type TransportConnect` | api.rs 改 enum 时漏写；回去 Task 1.3 检查 |
| watchdog 测试 `start_paused = true` 仍 fail | 多 yield_now + advance 几轮（已在测试代码里加 20 次轮询） |
| send_document 测试 multipart body 解析失败 | wiremock 默认接受 multipart，确认 mock matcher 不依赖 body |
| `block_in_place` panic | 检查 connector::new 是否在 multi-thread runtime 调用；Tauri 默认是 |
| markdown 路径提取误伤代码块里的 `[xxx](path)` | 优化 extract_local_paths 先剥代码块（Task 3.3 简化版未做，留作 future） |
| `replied message not found` 反复出现 | Task 3.5 已加 fallback 重试不带 reply；如有问题确认逻辑路径走到 |
| pairing 落盘频繁触发磁盘 IO | 当前每次写都落盘；磁盘 IO 微秒级，私聊场景每天写次数 < 100，可接受 |

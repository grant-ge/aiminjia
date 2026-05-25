# Telegram Connector 加固 — 桌面端长跑可信赖

**日期**：2026-05-20
**状态**：Design final → 可执行
**前置 spec**：`2026-05-19-im-telegram-connector-design.md`（MVP 已落地）
**Scope**：在不改 `IMConnector` trait 的前提下，把 8 个生产环境必踩的能力洞按 4 个独立 PR 补齐
**调研依据**：对照 `~/Downloads/openclaw-main/extensions/telegram/`（~38k 行 TypeScript 实现）摘取与桌面端私聊场景匹配的能力

---

## §0. 背景

`2026-05-19-im-telegram-connector-design.md` 落地了一个跑通的 MVP：长轮询入站、HTML 出站、扫码 pairing、文本/图片/文件入站附件、403 自动清理。当前位置在 `src-tauri/src/connector/im/telegram/`，约 2700 行 Rust，11 个文件。

对照 openclaw 的成熟 Telegram 实现（多租户、多 bot 账号、含审批 UI 和流式 draft），我们的桌面端**私聊**定位下，明确可以受益的能力洞有 8 类，分为三档：

- **传输层稳定性**：长消息分片缺失（4096 byte 必踩）、长轮询 stall 无 watchdog、sendMessage 网络错误不区分 connect/connected 阶段
- **消息完整性**：voice/video/sticker/audio/video_note/animation 6 种入站类型被 serde 静默丢弃、出站只能发文本、回复无 reply_to_message_id
- **可靠性 + 安全**：pairing code 内存态重启即失、download.rs 缺 SSRF host 检查

`openclaw` 框架级的能力（多 bot throttler / draft stream / inline button 审批 / webhook / forum topic / group migration）刻意**不补**——与桌面端 1 用户 ↔ 1 数字员工 ↔ 1 bot 的定位不匹配。

## §1. Non-Goals（明确不做）

1. 流式 editMessageText / draft stream（桌面端用户主要在桌面端看流式，Telegram 那头看最终消息即可）
2. inline button 审批 UI（工具调用审批走桌面端 `permission:ask` 事件，不让用户在 Telegram 里点按钮）
3. webhook 入站（桌面端无公网，永久不做）
4. 群聊 / 频道 / forum topic
5. 多 bot 账号 + per-account throttler（单 bot 单实例，沿用现状）
6. voice 转写 / sticker vision / video 多模态理解（本期只让 parser 识别到这些类型，给用户一句"暂不支持"提示，不进入 LLM 链路）
7. 出站富媒体 sendPhoto / sendVideo / sendAudio / sendVoice（本期只补 sendDocument，覆盖数字员工发报告的核心场景）
8. SecretString newtype 重构（token 继续用 `String`）

## §2. 总体方案

### 2.1 PR 拆分

| PR | 主题 | 触及文件 | 关键改动 |
|---|---|---|---|
| **PR1 传输层加固** | 长消息分片 + stall watchdog + 错误分类 | `sender.rs` / `long_poll.rs` / `api.rs` | 按语义边界切 4096 byte / 30s tick watchdog / `is_connect()` 区分 |
| **PR2 入站类型扩展** | parser 识别 6 种未支持类型并回提示 | `parser.rs` / `types.rs` / `long_poll.rs` | `TgMessage` 加 6 个 Option 字段 / `unsupported_kind` 提示通道 |
| **PR3 出站附件 + 引用回复** | sendDocument + markdown 本地路径自动发 + 50MB 限制 + reply_to_message_id | `api.rs` / `sender.rs` / `connector.rs` / `reply_forwarder.rs` | multipart POST / 路径提取器 / `send_with_attachments()` |
| **PR4 可靠性 + 测试补齐** | pairing 落盘 + SSRF host 检查 + 集成测试 | `pairing.rs` / `registration.rs` / `download.rs` / `tests/` | `pending-pairings.json` 文件 / `assert_telegram_host` / 历史欠债测试补齐 |

每个 PR 相互独立、可单独 revert、自带回归测试。PR 之间无 hard dependency（PR3 与 PR1 的分片逻辑解耦：PR1 改 send_markdown 流程，PR3 加新的 send_document 流程）。

### 2.2 全局约束

1. **不改 `IMConnector` trait**：所有改动模块内增量；不新增依赖（reqwest multipart 用现有 feature flag 即可）
2. **不输出 bot token**：继续 `api.rs:190` 约定，error / log 路径均不打整 URL
3. **写盘走原子写**：所有新增 JSON 持久化用 `storage::text_io::write_atomic`，path safety 用 `storage::safe_filename`
4. **错误分类有结构**：新增的错误（如 unsupported attachment / SSRF rejected / file too big）作为 `TelegramApiError` 的新 variant，不要 stringly-typed
5. **测试随 PR 增量**：每个 PR 自带新功能的单测 + 必要 fixture；PR4 集中补齐"长轮询端到端、pairing 重启恢复、403 自动清理"等历史欠债

---

## §3. PR1：传输层加固

### 3.1 长消息按语义边界分片

**问题**：`sender.rs:35-71` 的 `send_markdown` 不做分片，超过 4096 byte 直接被 Telegram 拒绝。LLM 输出动辄几千字，必踩。

**新行为**：

1. `markdown_to_telegram_html` 转换后得到 final HTML（`sender.rs:137-167`）
2. 字节长度 ≤ 4000 byte（留 96 byte 余量给最坏的 HTML 实体展开）→ 单条 send，行为不变
3. 字节长度 > 4000 → 调用新增 `split_telegram_html(s: &str, max_bytes: usize) -> Vec<String>`：
   - **第一优先级**：`<pre><code>` 代码块作为不可分割原子（保留代码块完整性）；如果单个代码块 > max_bytes，则在代码块内部按行强行切，外层包 `<pre><code>` 还原
   - **第二优先级**：按双换行（段落）切
   - **第三优先级**：按单换行（行）切
   - **第四优先级（兜底）**：按字符切（绝不切到 HTML 实体如 `&amp;` 中间，也不切到 inline tag `<b>...</b>` 中间——用简单状态机记录当前是否在 tag 内）
4. 多 chunk 按顺序串行发送（不并发，保证显示顺序）
5. **只第一条** 带 `reply_to_message_id`（如果调用方传了），后续 chunk 无 reply（这是 openclaw 共识的做法，避免每条都是 quote 块）

**API 选择**：4000 byte 是 byte 边界，不是字符。Telegram 4096 上限是 UTF-16 code units 但保守按 byte 算够安全（中文 UTF-8 三字节）。

**测试**：
- 短消息不分片
- 4500 字符纯文本按段落分两片
- 含代码块的消息不切代码块中间
- 单个代码块超长被强切并各自外包 `<pre><code>`
- 中文消息按双换行切（验证不在 utf-8 多字节中间切）
- 多 chunk 串行发送时只第一条带 reply_to

**Acceptance**：发送 8000 字符 markdown 应分两条；发送 200 行代码（>4000 byte）的代码块应保持 `<pre><code>` 包裹完整。

### 3.2 Stall watchdog

**问题**：`long_poll.rs` 当前只有 reqwest 35s timeout + 错误退避，没有"轮询在跑但 N 分钟没拿到任何 update 且 timeout 没触发"的检测。Telegram 偶发 keep-alive 卡死。

**新机制**：

1. 在 `TelegramLongPoll` 内加 `last_get_updates_at: Arc<AtomicI64>`（unix millis），每次 `getUpdates` 完成（成功或失败）都更新
2. 在 connector start 时 spawn 一个独立的 watchdog task：每 **30 秒** tick 一次，看 `now - last_get_updates_at`：
   - **< 120 秒**：no-op
   - **≥ 120 秒**：log warn + emit `ChannelConnectionState::Reconnecting("stall detected")` + 让 long-poll loop drop 当前 reqwest client 重建
3. 重建 reqwest client：在 `TelegramApi` 中加 `rebuild_client(&self)` 方法（`api.rs`），获取 inner mutex 替换 `reqwest::Client`；下次 `getUpdates` 自动用新 client
4. watchdog 与 long-poll loop 共享 `CancellationToken`，loop 结束时 watchdog 一起退

**配置项**：阈值 `STALL_TICK_INTERVAL = 30s` / `STALL_TIMEOUT = 120s` 写成 const，不暴露给用户。

**测试**：
- `last_get_updates_at` 在 getUpdates 调用前后被更新
- mock 时间冻结：120s 后 watchdog 触发 rebuild + emit Reconnecting
- 取消 token 后 watchdog task 退出

**Acceptance**：mac 睡眠 5 分钟唤醒后，最多 30 + 120 = 150 秒内 connector 恢复消费消息（手测 + 至少一个 wiremock 单测覆盖触发路径）。

### 3.3 sendMessage 错误分类（connect 阶段 vs connected 后）

**问题**：`sender.rs:35-71` 当前 429 重试 1 次，但**网络错误时直接 propagate**，断网瞬间发的消息直接丢。openclaw 区分得很细：
- `ECONNREFUSED / ENOTFOUND / ENETUNREACH`：TCP 还没建好，消息肯定没到，安全重试
- `ECONNRESET / read timeout`：连接建好后被切断，**消息可能已到**，重试会产生重复

**新行为**：

在 `TelegramApiError` 中：当 reqwest 错误是 transport 类时，进一步分类为 `TransportConnect`（可重试）vs `TransportConnected`（不可重试）。判断方法：
- `err.is_connect()` → `TransportConnect`
- 其他 transport 错误（包括 read timeout、ECONNRESET）→ `TransportConnected`

`sender.rs::send_markdown` 重试规则更新为：
1. 429 → sleep retry_after，重试 1 次（保持现状）
2. `TransportConnect` → sleep 500ms，重试 1 次
3. `TransportConnected` → 直接 propagate，不重试
4. BadRequest → fallback 到 strip_markdown 纯文本，重试 1 次（保持现状）
5. 其他错误 → propagate

`long_poll.rs::getUpdates` 路径**不改重试逻辑**（已有 backoff 阶梯），但用上新的分类让日志更准确。

**测试**：
- wiremock 模拟 connect refused（用一个不存在的 port）→ 重试一次成功
- wiremock 模拟 server hang 后 reset → 不重试
- 429 行为不变（覆盖回归）

### 3.4 PR1 验收清单

- [x] `sender.rs` 新增 `split_telegram_html` 函数 + 7 个单测（含 class 属性回归 + 空输入兜底）
- [x] `long_poll.rs` 新增 watchdog task + 3 个单测
- [x] `api.rs` 新增 `rebuild_client` 方法（含 clone-and-drop-guard 模式避免 read lock 阻塞 write）
- [x] `TelegramApiError` 新增 `TransportConnect` / `TransportConnected` 两个 variant
- [x] `sender.rs::send_markdown` 多 chunk 串行发送测试（multi_chunk_tests 在 Task 3.5 一起加，PR1 已具备实现）
- [x] `cargo test --lib telegram` 通过（55 passed）；`review_ --tests` 因 pre-existing whatsapp WIP 编译错暂时不可跑——本 PR 与那些文件无关

---

## §4. PR2：入站类型扩展

### 4.1 类型清单

当前 `parser.rs:88-113` 只支持 `photo` 和 `document`。新增识别以下 6 种入站消息类型（不进入 LLM，每条都回提示）：

| 类型 | 出现场景 | 处理 |
|---|---|---|
| `voice` | 用户按住录语音 | 回 "我暂不支持处理语音消息" |
| `audio` | 用户发音乐文件 | 回 "我暂不支持处理音频文件" |
| `video` | 用户发视频 | 回 "我暂不支持处理视频" |
| `video_note` | 圆形小视频（"圆视频"） | 回 "我暂不支持处理视频" |
| `sticker` | 表情贴纸（含 emoji sticker） | 回 "我暂不支持识别贴纸" |
| `animation` | GIF | 回 "我暂不支持处理动图" |

### 4.2 实现

**`types.rs` 扩展**：

为 `TgMessage` 新增 6 个 `Option<serde_json::Value>` 字段（不解析子结构，存在即认为收到了），用 `#[serde(default)]` 兼容：

```text
voice: Option<TgVoice>          # 只取 duration 字段供日志
audio: Option<TgAudio>
video: Option<TgVideo>
video_note: Option<TgVideoNote>
sticker: Option<TgSticker>      # 取 emoji 字段供日志（看是哪个表情）
animation: Option<TgAnimation>
```

子结构体只声明 `duration` / `emoji` / `file_size` 等可观测字段，**不下载 file_id**（避免占用 download 路径）。

**`parser.rs` 扩展**：

在 `parse_update` 现有分支后，添加 `UnsupportedKind` 变体：

```text
enum ParseOutcome {
    Message(ChannelMessage),
    PairingCommand { code: String, pairer: ... },
    Skip(SkipReason),
    Unsupported {  # 新增
        chat_id: i64,
        user_id: i64,
        message_id: i64,
        kind: UnsupportedKind,  # Voice / Audio / Video / VideoNote / Sticker / Animation
    },
}
```

**`long_poll.rs::handle_message` 路由**：

- 命中 allowlist + 是 Message → 老路径
- 命中 allowlist + 是 PairingCommand → 老路径（已配对）
- 命中 allowlist + 是 Unsupported → **新路径**：发提示消息（带 reply_to_message_id），不写 dedup（让用户重发同样的语音也再次得到提示，避免"我发了 10 条语音它都没反应"），不进入 LLM
- 不在 allowlist → 老路径（提示先扫码）

**提示文案**（统一前缀 `🤖 `）：见 §4.1 表格。

### 4.3 测试

- 每种 unsupported kind 的 parser 单测（6 个）
- long_poll handle_message 收到 Unsupported 时发提示消息且不进 msg_tx（用 mock）
- 同一用户连发 3 条语音收到 3 条提示（确认不被 dedup）

### 4.4 PR2 验收清单

- [x] `types.rs` 新增 6 个子结构体
- [x] `parser.rs` 新增 `UnsupportedKind` enum + `Unsupported` ParseOutcome variant
- [x] `long_poll.rs` 路由 Unsupported → send hint
- [x] 6 种类型各自单测覆盖
- [ ] 真实手测：iPhone 端给 bot 发语音 → 收到提示

---

## §5. PR3：出站附件 + 引用回复

### 5.1 sendDocument multipart 上传

**`api.rs` 新增**：

```text
pub async fn send_document(
    &self,
    chat_id: i64,
    file_path: &Path,
    caption: Option<&str>,
    reply_to_message_id: Option<i64>,
) -> Result<TgMessage, TelegramApiError>
```

实现：
1. 读取文件 metadata，size 超过 50MB → 返回 `TelegramApiError::FileTooBig { size, limit: 50 * 1024 * 1024 }`
2. 文件名取 path file_name，过 `storage::safe_filename::ensure_safe_filename`
3. 用 `reqwest::multipart::Form` 构造请求：
   - `chat_id` text part
   - `document` file part（从 `tokio::fs::File::open` 流式读，不一次性 load）
   - `caption` text part（如果提供，HTML 转义）
   - `parse_mode=HTML`
   - `reply_to_message_id` text part（如果提供）
4. POST 到 `https://api.telegram.org/bot{token}/sendDocument`
5. 错误处理：复用 `TelegramApiError` 现有分支；429 由调用方重试

### 5.2 Markdown 本地路径自动提取

**新模块 `sender.rs::extract_local_paths`**：

输入是 GFM markdown 源文本（送到 `markdown_to_telegram_html` 之前），扫描两种形式：

1. **markdown 链接形式**：`[label](path)` 其中 path 满足"本地路径"判定
2. **裸路径形式**：行内独立的绝对路径（前后有空白/换行），如 `/Users/oayzz/reports/x.xlsx` 或 `C:\Users\...\x.docx`

**本地路径判定**：
- Unix 绝对路径：以 `/` 开头，存在于文件系统
- Windows 绝对路径：匹配 `^[A-Za-z]:[/\\]`，存在
- 用户家目录展开：`~/...` 在判定时先展开（不修改原文本）
- **不接受**：相对路径（避免误伤）、`http://` `https://` URL、`tg://` `mailto:` 等已知 scheme

输出：`Vec<AttachmentRef { absolute_path: PathBuf, original_markdown_segment: String }>`

提取后，**`send_markdown` 流程改造**：

```text
1. 提取本地路径 → attachments: Vec<AttachmentRef>
2. 在 markdown 源文本中把 [label](path) 替换为 `📎 label`（裸路径替换为 `📎 文件名`）
3. 走老路径 send 文本（带分片）
4. 文本发完后，对每个 attachment 串行调 send_document(... reply_to_message_id = 第一条文本消息的 id)
5. attachment size 超 50MB 或文件不存在或 send 失败 → log warn + 发一条提示 "📎 文件 X 发送失败：YYY"
```

**reply_to 链路**：每个 attachment 都 reply 到**第一条文本消息**，形成一组"文本 + 多附件"的视觉聚合。

### 5.3 sendMessage 加 reply_to_message_id 参数

**`api.rs::SendMessageBody`** 新增 `reply_to_message_id: Option<i64>` 字段，serde 序列化时 None 不输出。

**调用链**：

- `connector.rs::send`（执行 reply forwarder 调用）：从 `RuntimeEvent::MessagePersisted` 拿不到入站消息 id（runtime 层不知道 Telegram message_id）。两种选择：
  - **选项 A（推荐）**：在 `connector.rs::remember_session` 时同时记录"该 session 最后一次入站 message_id"到 `TelegramSessionTarget`，send 时取出
  - **选项 B**：不做 reply_to，留给后续 PR

**采用 A**：在 `TelegramSessionTarget` 新增 `last_inbound_message_id: Option<i64>`，每次 inbound message 进 `remember_session` 时更新；send 时如非 None 则带上。这对私聊场景已经足够（一对一）。

**Note**：如果 reply_to 的消息已被用户删除，Telegram 会返回 `replied message not found`（400 BadRequest）。需要把这个错误也加入"fallback 重试不带 reply_to"路径。

### 5.4 测试

- `extract_local_paths` 单测：
  - `[报告](/Users/x/r.xlsx)` 形式提取
  - 裸路径 `/abs/path.txt` 提取
  - 不存在的路径不提取
  - URL 不被误提取
  - Windows 路径 `C:\Users\x.docx` 提取
  - `~/` 展开
- `send_document` mock multipart 上传成功
- 50MB 超限返回 `FileTooBig`
- 不存在的文件返回 NotFound
- `send_markdown` 中含附件链接 → 文本 + 1 个 document 都发出
- reply_to_message_id 在 SendMessageBody 中存在/不存在的两种序列化

### 5.5 PR3 验收清单

- [x] `api.rs::send_document` + multipart 上传（含 3 个单测：成功 / 50MB 上限 / 路径不存在）
- [x] `api.rs::TelegramApiError::FileTooBig` + `IoError` variant
- [x] `sender.rs::extract_local_paths` + 6 个单测（markdown 链接 / 裸路径 / 不存在 / http URL / tilde / 去重）
- [x] `sender.rs::send_markdown_with_reply` 路径 → 文本替换为 `📎 label` + 附件单独发送
- [x] `api.rs::SendMessageBody::reply_to_message_id` 字段 + `send_message_with_reply` 入口
- [x] `connector.rs::TelegramSessionTarget::last_inbound_message_id` 更新链路（long_poll handle_message 写、connector::send 读）
- [x] reply_to_message_id 在 400 `replied message not found` 时重试不带 reply（`send_html_chunk_with_reply` 实现）
- [ ] 手测：LLM 回复中含 `[报告](/Users/x/r.xlsx)` → Telegram 收到一条文本 + 一份 xlsx 附件（留给 §E）

---

## §6. PR4：可靠性 + 测试补齐

### 6.1 Pairing pending 落盘

**问题**：`pairing.rs` 是纯内存 `HashMap<String, PairingEntry>`，重启即失。用户场景：「桌面端生成码 → 切到手机 → 桌面端崩溃/重启 → 码失效」。

**新文件**：`users/{scope}/channels/telegram/pending-pairings.json`

```json
{
  "schemaVersion": 1,
  "pendingPairings": [
    {
      "code": "ABCD2345",
      "createdAtUnixMillis": 1716123456789,
      "expiresAtUnixMillis": 1716123756789,
      "attachedUser": {
        "userId": 123,
        "firstName": "张三",
        "username": "zhangsan",
        "attachedAtUnixMillis": 1716123500000
      }
    }
  ]
}
```

**写盘时机**：
- `begin()` 生成新 code 后写
- `attempt_attach()` 写入 attachedUser 后写
- `take(code)` 移除 entry 后写
- `cleanup_expired()` 删过期 entry 后写
- 写盘用 `storage::text_io::write_atomic`

**启动时机**：
- `TelegramConnector::new()` 时读 pending-pairings.json
- TTL 已过期的 entry 不加载（filter 后立即写回，幂等清理）

**测试**：
- 写 → 读 → 字段一致
- 过期 entry 启动时被清理
- 同一 code 两次 begin 不写两条
- 文件不存在时启动 OK（视为空 HashMap）
- 文件损坏（非法 JSON）时 log warn + 回退到空（不阻塞 connector 启动）

### 6.2 SSRF host 检查

**问题**：`download.rs:66-138` 拼 `https://api.telegram.org/file/bot{token}/{file_path}` 直接下载，没有对 `file_path` 做检查。如果 `file_path` 含 `../` 或被构造为远程 URL（理论上 Telegram 不会返这种值，但作为防御层加上）会出问题。

**新检查**：在 `download_file_by_path` 入口处：

1. 拼 URL 后 `url::Url::parse(...)`
2. 检查 `url.host_str() == Some("api.telegram.org")`，否则返回 `TelegramApiError::SecurityRejected { reason: "host not allowed" }`
3. 检查 `url.scheme() == "https"`，否则同上
4. 这两条检查也对 `getFile` 接口的下载阶段生效（实际只有 download.rs 这一处下载）

由于当前 `apiRoot` 是 hardcoded 不允许用户改，本检查更多是防御层（万一未来 file_path 含异常字符串注入到 URL）。一行代码代价。

**测试**：
- 正常 file_path 通过
- 构造 `file_path = "//evil.com/x"` 时被拒（这种情况下 URL 解析后 host 会变）
- 构造 `file_path = "http://x"` 时被拒（scheme 检查）

### 6.3 集成测试补齐

新增以下集成测试到 `src-tauri/tests/`：

| 测试文件 | 覆盖 |
|---|---|
| `telegram_long_poll_integration_test.rs` | wiremock 模拟一轮 getUpdates 返回 1 条 message → 进 msg_tx；offset 写盘正确；429 sleep 后重试；401 触发 NeedsReauth |
| `telegram_pairing_persistence_test.rs` | begin → 写盘 → 重启 connector → 读盘 → attempt_attach 仍可成功；过期 entry 启动被清 |
| `telegram_403_cleanup_test.rs` | wiremock 模拟 sendMessage 返回 403 Forbidden → allowlist 自动移除 + session_targets 移除 |
| `telegram_stall_watchdog_test.rs` | 模拟 last_get_updates_at 超过 120s → watchdog 触发 rebuild + emit Reconnecting |
| `telegram_send_document_test.rs` | mock multipart 上传成功 / 文件 51MB 跳过 / 文件不存在错误 |

PR4 的"测试补齐"不一定每个都要 e2e（部分用 #[cfg(test)] 单测 + wiremock 就够），目标是把当前 `review_ --tests` 跑过、且**新功能不被未来重构悄悄破坏**。

### 6.4 PR4 验收清单

- [x] `pairing.rs` 写盘 / 读盘 / 启动 TTL 清理 + 4 个持久化单测
- [x] `pending-pairings.json` schema 落地 + 数据迁移（旧版本无该文件视为空，零迁移成本）
- [x] `api.rs::download_file` SSRF host + scheme 检查 + 单测（仅生产 api_base 生效）
- [ ] 5 个新集成测试文件 — pre-existing main 上 `im_feishu_integration` / `python_recovery` 测试编译错阻塞 `cargo test --tests`；改为加强 #[cfg(test)] 单测覆盖（77 通过），外部 integration 测试留到后续 PR
- [ ] `cargo test --test review_ --no-fail-fast` 通过 — 同上 pre-existing blocker；`cargo test --lib telegram` 77 PASS 替代验证

---

## §7. 风险与回滚

### 7.1 各 PR 风险评估

| PR | 主要风险 | 缓解 |
|---|---|---|
| PR1 分片 | HTML 实体被切到中间导致 Telegram 拒收 | 状态机标记 in_tag / in_entity；测试覆盖中文 / 代码块 / 嵌套标记 |
| PR1 watchdog | 误杀正常 25s 长轮询 | 120s 阈值远大于 25s timeout；watchdog 只 rebuild client 不 kill loop |
| PR1 错误分类 | reqwest `is_connect()` 不能完全覆盖所有 connect 错误 | 用 cargo doc 验证；保守起见 connect 阶段错误不重试也是 OK 的退化 |
| PR2 提示 | 用户连发 10 条语音收到 10 条提示，吵 | 已确认接受（产品决策）；后续可加 cooldown |
| PR3 路径提取 | 误把代码块里的路径字符串提取出来 | 提取前先 strip 代码块；测试覆盖 |
| PR3 multipart | 大文件流式上传内存占用 | 用 `reqwest::Body::wrap_stream` 流式而非一次性 load |
| PR3 reply_to | last_inbound_message_id 在 turn 之间错位（用户连发多条问题，第一条回复 reply 到第三条问题） | 私聊场景可接受；如要严格匹配，需要在 runtime 事件中带上原始 message_id（本期不做） |
| PR4 pairing 落盘 | 文件损坏导致 connector 启不来 | 解析失败时 log warn + 回退到空 HashMap |

### 7.2 回滚策略

每个 PR 独立 revert：
- PR1 revert：恢复无分片 + 无 watchdog（回到当前 main 行为）
- PR2 revert：恢复 voice 等静默丢弃（用户感知是"消息无响应"，旧行为）
- PR3 revert：恢复出站只发文本（数字员工无法发附件）
- PR4 revert：恢复 pairing 不落盘（旧 MVP 行为）

任何 PR 不引入 schema breaking change：
- PR2/PR3 不动 `telegram/config.json` schema
- PR4 新增 `pending-pairings.json` 文件，旧版本无该文件视为空（零迁移）

---

## §8. 与 openclaw 的对照表（参考）

| openclaw 能力 | 文件 | 我们的态度 |
|---|---|---|
| `draft-chunking.ts` | 长消息按 token 边界分片 | ✅ PR1 借鉴 |
| `polling-liveness.ts` | stall watchdog | ✅ PR1 借鉴 |
| `network-errors.ts::isSafeToRetrySendError` | connect vs connected 区分 | ✅ PR1 借鉴 |
| `reply-parameters.ts` | reply_to + quote | 🟡 PR3 借鉴（不做 quote，只做 reply_to） |
| `bot/delivery.resolve-media.ts::SSRF policy` | host 白名单 | ✅ PR4 借鉴 |
| `bot-info-cache.ts` | getMe 缓存 | ❌ 我们启动时本来就不调 getMe（仅保存配置时调一次），无需缓存 |
| `account-throttler.ts` | per-chat 限速队列 | ❌ 桌面端单 bot 不需要 |
| `draft-stream.ts` | sendMessage + editMessageText 流式 | ❌ 桌面端用户看桌面端流式即可 |
| `approval-handler.runtime.ts` | inline button 审批 | ❌ 审批走桌面端 permission:ask |
| `polling-session.ts::isolated ingress` | 子进程 spool | ❌ 桌面端无 HA 需求 |
| `bot-message-context.dm-session.ts` | forum topic | ❌ 不支持群聊 |
| `auto-topic-label.ts` | LLM 自动起标题 | ❌ 群聊概念 |
| `sticker-vision.runtime.ts` | sticker vision 理解 | ❌ 本期只提示不支持 |
| `voice.ts` | voice 转写 | ❌ 本期只提示不支持 |

---

## §9. 验收 / 收尾

### 9.1 全 spec 验收

- [x] PR1-4 全部 commit 到 `claude/amazing-chatelet-801fd7` branch（17 个 commit）
- [x] `cargo test --lib telegram --no-fail-fast` 通过（77 PASS）
- [ ] `cargo test --tests` / `cargo test review_ --tests` 通过 — pre-existing main blocker（`im_feishu_integration`、`python_recovery_input_test` 编译错），与本 PR 无关
- [ ] 手测脚本（mac + Windows 各一遍）：
  - [ ] 配置 bot → pairing → 私聊文本 → 收到 markdown 回复
  - [ ] LLM 回复 8000 字 → 收到 2 条分片消息
  - [ ] iPhone 端给 bot 发语音 → 收到"暂不支持"提示
  - [ ] LLM 回复 `[报告](/Users/oayzz/x.xlsx)` → 收到文本 + xlsx 附件
  - [ ] 配置完成后 mac 睡眠 5 分钟唤醒 → 给 bot 发消息 → 2 分钟内收到回复（验证 stall watchdog）
  - [ ] 桌面端 pairing 生成 code 后立刻 kill -9 → 重启 → 5 分钟内仍可用旧 code 配对

### 9.2 文档更新

- [ ] `CLAUDE.md` 加一段说明 Telegram 加固后的能力（章节："Telegram connector"）—— 留给最终
- [x] `connector/im/telegram/mod.rs` 顶部 doc-comment 更新当前能力清单
- [ ] 如有 user-facing 行为变化，README 或 frontend HelpPanel 同步 —— 无 UI 改动

### 9.3 完成判定

当 PR1-4 全部合并、上述 6 项手测全部通过、CLAUDE.md 更新后，本 spec 进入 `done`，归档到 `docs/superpowers/specs/` 不删除（作为后续 Telegram 工作的设计依据）。

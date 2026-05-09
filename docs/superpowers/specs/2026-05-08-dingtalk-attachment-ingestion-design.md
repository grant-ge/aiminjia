# 钉钉机器人附件接入（图片 / 文件 / richText / 语音）

- 状态：Draft
- 起草日期：2026-05-08
- 关联：`docs/superpowers/specs/2026-05-06-im-channel-dingtalk-design.md`、`src-tauri/src/connector/channel/`

## 1. 背景

钉钉机器人 Stream 模式当前只处理 `msgtype=text` 的回调，其他消息类型（picture / file / audio / richText / interactiveCard）会被静默丢弃。已有的 interactiveCard 通过 `sessionWebhook` 给出"暂不支持"提示。本设计要补齐图片 / 文件 / richText / audio 四种类型的接收处理，让钉钉用户在私聊或群里直接发送附件就能进入正常的 LLM 对话流程。

经过抓样验证，钉钉的 `richText` msgtype 已经把"用户单次发送的多张图 + 文字"按出现顺序聚合在一条 callback 里，因此**不需要我们做攒批 / 时间窗口等推断**，每条 callback 即一次用户意图，对应一次 `ChatTurnRequest`。

## 2. 范围与非目标

### 2.1 本次范围

- 接收 `msgtype` 为 `picture` / `file` / `richText` / `audio` 的钉钉 IM callback，下载附件到 workspace，构造 `ChatTurnRequest` 触发现有 LLM 链路。
- 文件落盘到 `~/.renlijia/<workspace>/dingtalk_downloads/`，按 sha256 重命名去重。
- 部分附件下载失败时仍起 turn，把失败信息追加到 LLM 输入。
- 全部附件失败且无文字时通过 `sessionWebhook` 提示用户重发。
- audio 直接使用钉钉自带的 `content.recognition`（ASR 文字结果）当做 text，不下载原始音频。
- 已有的 interactiveCard "暂不支持" 提示保持现状。

### 2.2 非目标

- **不做钉钉文档 / 钉盘文件的对话内 OAuth**（已经讨论过：需要公网 callback、钉盘 API 审批、企业 admin 配合，工程量过大），interactiveCard 仍走"暂不支持"。
- **不引入 LLM 多模态调用**：钉钉图片走与前端上传相同的 attachment 链路，让 LLM 通过 `read_workspace_file` / `execute_python` 等工具自行读取。
- **不限文件类型 / 大小**：照单全收，由 LLM / 工具按扩展名自行决定能不能处理。
- **不做 metrics**：先靠日志，后续若发现下载失败率高再补 `metrics.jsonl`。
- **不修改前端附件 UI**：复用现有展示，不区分附件来源是钉钉还是本地上传。
- **不纳入 `upload_gc` 自动清理**：钉钉历史会话里仍能看到那张图却本地没了，体验差，让用户在 app 设置里手动清理。

## 3. 用户与 LLM 视角

### 3.1 用户在钉钉的体验

- 单独发 1 张图 / 1 个文件：app 收到、起 1 轮对话，LLM 看到附件后回复（默认会主动问"需要做什么"或开始分析）。
- 一次发送多张图 + 一段文字（钉钉 UI 支持）：app 收到 1 条 `richText`，下载所有图，文字作为正文，1 轮对话搞定。
- 单独发文字 / 链接 / 语音：行为与现状一致（语音首次纳入，靠钉钉 ASR）。
- 发钉钉文档 / 钉盘超链卡片：收到"暂不支持，请导出后再发"提示（已实现）。
- 发的图 / 文件下载失败：起 turn 时正文里多一行"以下附件下载失败"，LLM 自行决定怎么回；全部失败且没文字 → 收到"附件下载失败请重发"提示。

### 3.2 LLM 视角

LLM 收到的 user message 与现有"前端上传附件"形态一致：

```
<用户原文 / 群聊带 [发送者]: 前缀>

[当前消息附件]
- image_msg5dva_0.jpg (path: ".../dingtalk_downloads/<sha256>.jpg", 类型: jpg)
- image_msg5dva_3.jpg (path: ".../dingtalk_downloads/<sha256>.jpg", 类型: jpg)

本轮附件已自动加入授权目录...
```

部分失败追加：

```
[注意：以下附件下载失败，未能加载：image_xxx.jpg]
```

## 4. 架构

```
钉钉服务端
    │ WSS callback
    ▼
┌─────────────────────────────────────┐
│ dingtalk_stream.rs                  │  ← 只做 schema 解析，无 IO
│   parse_im_message → ParseResult    │
│     ::Forward(ChannelMessage)       │
│     ::AutoReply{webhook,text}       │  ← 已有，interactiveCard
│     ::Drop                          │
└──────┬──────────────────────────────┘
       │ mpsc::Sender<ChannelMessage>
       ▼
┌─────────────────────────────────────┐
│ channel/manager.rs message worker   │  ← 串行消费，做 IO
│   1. seen_msg_ids 去重（已有）      │
│   2. 路由 session（已有）            │
│   3. ★ 下载 attachments（新增）     │
│   4. 拼正文 + ChatAttachmentRef     │
│   5. send_chat_request（已有）      │
└──────┬──────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────┐
│ chat_adapter（已有）                 │
│   ChatTurnRequest                   │
│     content + attachments[]         │
└─────────────────────────────────────┘
```

### 4.1 核心约束

1. `dingtalk_stream.rs` 永不做 IO，`parse_im_message` 仍是同步纯函数。所有 ws 帧 ACK + 解析路径不阻塞。
2. 下载发生在 message worker 内 `await`，单条 IM 串行处理；ws read loop 不被任何下载阻塞。
3. 复用 `ChatAttachmentRef` / `chat_runtime_impl::build_llm_content`，不动 LLM provider 与前端附件 UI。

### 4.2 文件改动一览

| 路径 | 改动类型 | 说明 |
|---|---|---|
| `src-tauri/src/connector/channel/dingtalk_download.rs` | **新增** | 钉钉文件两步下载器 + sha256 去重落盘 |
| `src-tauri/src/connector/channel/types.rs` | 扩展 | `ChannelMessage` 增加 `attachments` / `session_webhook`；新增 `ChannelAttachmentSpec` / `AttachmentKind` |
| `src-tauri/src/connector/channel/dingtalk_stream.rs` | 修改 | `DingtalkImData` 扩字段；`parse_im_message` 处理 picture / file / richText / audio |
| `src-tauri/src/connector/channel/manager.rs` | 修改 | message worker 在路由 session 之后、`send_chat_request` 之前加下载步骤 |
| `src-tauri/src/connector/channel/mod.rs` | 修改 | 导出新模块 |
| `src-tauri/tests/dingtalk_attachment_integration_test.rs` | **新增** | worker 链路集成测试 |

## 5. 组件设计

### 5.1 `dingtalk_download.rs`（新增）

```rust
pub struct DingtalkFileDownloader {
    client: reqwest::Client,
    token_cache: TokenCache,
    app_key: String,
    app_secret: String,
    dest_dir: PathBuf,           // workspace/dingtalk_downloads/
}

#[derive(Clone, Debug)]
pub struct DownloadedFile {
    pub path: PathBuf,           // 绝对路径
    pub file_name: String,       // 经 safe_filename 处理后的展示名
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("token: {0:#}")]
    Token(anyhow::Error),
    #[error("get url: status={status} body={body}")]
    GetUrl { status: u16, body: String },
    #[error("network: {0}")]
    Network(reqwest::Error),
    #[error("io: {0}")]
    Io(std::io::Error),
}

impl DingtalkFileDownloader {
    pub fn new(
        token_cache: TokenCache,
        app_key: String,
        app_secret: String,
        dest_dir: PathBuf,
    ) -> Self;

    /// 单文件两步下载 + sha256 去重落盘
    pub async fn download(
        &self,
        download_code: &str,
        robot_code: &str,
        original_file_name: &str,
    ) -> Result<DownloadedFile, DownloadError>;
}
```

下载流程：

1. `get_access_token(token_cache, app_key, app_secret)` 拿 token（10s 超时）
2. `POST {DINGTALK_API}/v1.0/robot/messageFiles/download` body `{ downloadCode, robotCode }` → 返 `{ downloadUrl }`（10s 超时）
3. `GET downloadUrl` 流式写到 `dest_dir/.tmp_<uuid>`（60s 超时；失败做 2 次 500ms 间隔的重试）
4. 边写边算 sha256
5. 重命名到 `<sha256>.<ext>`：扩展名取自 `original_file_name`，无扩展名时为 `.bin`；若目标已存在则删 `.tmp` 复用已有文件（**去重**）
6. 返回 `DownloadedFile`

文件名安全性：使用 `storage::safe_filename::ensure_safe_filename` 处理 `original_file_name`（防止 `../etc/passwd` 等），最终落盘路径只受 `<sha256>.<ext>` 控制，不会被攻击者控制目录。

### 5.2 `types.rs`（扩展 `ChannelMessage`）

```rust
#[derive(Debug, Clone)]
pub struct ChannelAttachmentSpec {
    pub kind: AttachmentKind,
    pub download_code: String,
    pub file_name: String,            // file 类用 fileName；picture 用 image_<msgId>_<idx>.jpg 兜底
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Picture,
    File,
}

pub struct ChannelMessage {
    pub msg_id: String,
    pub conversation_type: ConversationType,
    pub conversation_key: String,
    pub sender_id: String,
    pub sender_nick: String,
    pub robot_code: String,
    pub reply_group_id: String,
    pub text: String,                                 // 原有
    pub attachments: Vec<ChannelAttachmentSpec>,      // 新增
    pub session_webhook: Option<String>,              // 新增
}
```

不引入新枚举变体，单一结构体形态：

- `attachments` 为空 + `text` 非空 = 纯文字消息（包括 audio 的 recognition）
- `attachments` 非空 + `text` 任意 = 含附件消息
- 两者同空 = 不应该发生（在 `parse_im_message` 中作为 `Drop`）

### 5.3 `dingtalk_stream.rs`（重写解析）

`DingtalkImData` 增加：

```rust
#[derive(Deserialize, Default)]
struct DingtalkImContent {
    biz_custom_action_url: Option<String>,            // interactiveCard
    download_code: Option<String>,                    // picture / file / audio 单个
    file_name: Option<String>,                        // file
    recognition: Option<String>,                      // audio
    rich_text: Option<Vec<RichTextSegment>>,          // richText
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RichTextSegment {
    Picture {
        #[serde(rename = "downloadCode")]
        download_code: String,
    },
    Text { text: String },
    #[serde(other)]
    Other,                                            // 兜底未知 type
}
```

`parse_im_message` 按 msgtype 分支：

| msgtype | text | attachments | 备注 |
|---|---|---|---|
| `text` | `text.content` | `[]` | 现状 |
| `picture` | `""` | `[Picture(download_code, "image_<msgId>_0.jpg")]` | |
| `file` | `""` | `[File(download_code, file_name)]` | |
| `audio` | `recognition.unwrap_or("")` | `[]` | recognition 为空 → Drop |
| `richText` | 拼接所有非空 Text 段（`trim` 后非空，按出现顺序、空格分隔） | 所有 Picture 段，按出现顺序 | 仅 picture 段无 text 段 → text="" |
| `interactiveCard` + yunpan/doc/previewDentry URL | — | — | AutoReply（保持现状） |
| 其他 | — | — | warn + Drop |

### 5.4 `manager.rs` worker 改造

worker 在现有去重 + 路由 session 之后、`adapter.send_chat_request` 之前插入下载步骤：

```rust
// 伪码
let chat_attachments = if msg.attachments.is_empty() {
    Vec::new()
} else {
    log::info!("[channel] downloading {} attachments msgId={} session={}",
               msg.attachments.len(), msg.msg_id, session_id);
    download_specs_for_turn(&downloader, &msg.attachments, &msg.robot_code, &msg.msg_id).await
};

let download_failures: Vec<String> = /* spec.file_name 中下载失败的部分 */;

if chat_attachments.is_empty() && msg.text.trim().is_empty() && !msg.attachments.is_empty() {
    // 全失败且无 text → sessionWebhook 提示重发，不起 turn
    if let Some(webhook) = &msg.session_webhook {
        spawn(send_session_webhook_text(webhook.clone(),
            "附件下载全部失败，请重发。".into()));
    }
    continue;
}

let content = build_compound_content(&conv_type, &sender_nick, &msg.text, &download_failures);
let request = ChatTurnRequest::new(session_id.clone(), content, chat_attachments);
// 原有 register_reply + send_chat_request 流程不变
```

`build_compound_content`：群聊加 `[发送者]:` 前缀；text 为空时直接用前缀+空字符串；末尾如果有 `download_failures`，追加 `\n\n[注意：以下附件下载失败，未能加载：xxx, yyy]`。

`download_specs_for_turn`：串行 `await` 每个 spec 的 `downloader.download(...)`，把成功的转成 `ChatAttachmentRef`：

```rust
ChatAttachmentRef {
    id: downloaded.sha256.clone(),
    file_name: downloaded.file_name.clone(),
    file_path: downloaded.path.to_string_lossy().to_string(),
    kind: match spec.kind {
        AttachmentKind::Picture => "image".into(),
        AttachmentKind::File => "file".into(),
    },
    file_size: downloaded.size,
    file_type: ext_of(&downloaded.path),
    mime_type: downloaded.mime_type.clone(),
}
```

### 5.5 manager.rs 装配

`ChannelManager::start_stream`（或 `auto_connect_if_configured`）首次启动 dingtalk stream 时创建 `DingtalkFileDownloader`：

- `token_cache`：manager 自己 new 一个独立 `TokenCache::new()`（与 reply_manager 各自持有，避免跨边界依赖；`/v1.0/oauth2/accessToken` 一天内多次调被允许）
- `app_key` / `app_secret`：从 `ChannelPlatformConfig` 拿
- `dest_dir`：`chat_adapter.services.file_mgr.workspace_path().join("dingtalk_downloads")`，首次启动时 `create_dir_all`

downloader 的所有权：作为 `Arc<DingtalkFileDownloader>` 移到 worker tokio task 内。

## 6. 数据流

### 6.1 场景 A — richText（2 张图 + "你好"）

```
T0  钉钉 → ws frame { msgtype:"richText", content:{richText:[pic, \n, pic, \n, "你好"]} }
T1  parse_im_message → Forward(ChannelMessage{
        text: "你好",
        attachments: [Picture{dc1,"image_msg_0.jpg"}, Picture{dc2,"image_msg_3.jpg"}],
        ...
    })
T2  worker:
      session_id = router.get_or_create_session(...)
      emit "channel:message" 给前端（preview="[附件] 你好"）
      downloader.download(dc1, ...) → DownloadedFile{ path="<root>/<sha1>.jpg", ... }
      downloader.download(dc2, ...) → DownloadedFile{ path="<root>/<sha2>.jpg", ... }
      chat_attachments = [Ref1, Ref2]
      content = "你好"   （或群聊 "[张三]: 你好"）
      request = ChatTurnRequest::new(session_id, content, chat_attachments)
      reply_manager.register(...)
      adapter.send_chat_request(request)
T3  build_llm_content 把附件描述拼进 user content → LLM 起对话
```

### 6.2 场景 B — 单张图（`msgtype=picture`）

```
parse → Forward(ChannelMessage{
    text: "",
    attachments: [Picture{dc, "image_msg_0.jpg"}],
})
worker → 下 1 个文件 → content="" → 起 turn（attachments=1）
LLM 看到附件路径，主动询问或直接分析
```

### 6.3 场景 C — richText 中 1 张图过期

```
specs = [s1, s2, s3]
download(s1) → Ok
download(s2) → Err(GetUrl{status=410})
download(s3) → Ok
download_failures = [s2.file_name]
chat_attachments = [Ref1, Ref3]
content = "原文\n\n[注意：以下附件下载失败，未能加载：image_..._1.jpg]"
adapter.send_chat_request(request)  // 正常起 turn
```

### 6.4 时序保证

| 保证 | 机制 |
|---|---|
| ws read loop 不被下载阻塞 | worker 是独立 tokio task，下载在 worker 里 await；ws loop 只 ACK + 投递 mpsc |
| 同会话消息严格有序 | worker 串行消费 mpsc，下一条在上一条 `send_chat_request` 完成后才开始 |
| 下载去重跨会话共享 | 单一目录 `dingtalk_downloads/` 按 sha256 命名；不同会话发同一张图最终磁盘只一份 |
| 重复发同图不重下盘 | 接受重复 GET（钉钉 downloadCode 一次性，无法预判），但 sha256 命中后跳过写入 |
| 部分下载失败不导致丢消息 | 除非全失败+无 text，否则都起 turn |
| AI Card 在 LLM 首 token 前就绪 | `register_reply` 在 `send_chat_request` 前 await（保持现状） |

## 7. 错误处理

### 7.1 错误分类与策略

| 来源 | 触发 | 策略 |
|---|---|---|
| schema 解析 | `serde_json::from_str` 失败 / 必需字段缺失 | warn + Drop（钉钉协议变更风险，不通知用户） |
| 拿 accessToken | 401 / 网络异常 | 当作所有 attachment 失败；不重试 |
| messageFiles/download 拿 URL | 4xx/5xx | attachment 失败 |
| 下载文件 body | 超时 / 断流 | **2 次重试**（间隔 500ms）后失败 |
| 写盘 | 磁盘满 / 权限 | attachment 失败 |
| sha256 计算 | IO 异常 | attachment 失败 |
| richText 未知 segment type | 钉钉新增段类型 | `#[serde(other)]` 兜底 noop，picture/text 段照常处理 |
| 全 attachment 失败 + text 空 | 用户只发图全失效 | 不起 turn，sessionWebhook 提示重发 |
| 全 attachment 失败 + text 非空 | 文字尚在 | 起 turn，content 末尾追加失败说明 |
| 部分 attachment 失败 | richText 中部分图过期 | 起 turn，content 末尾追加失败说明 |
| sessionWebhook 失败 | 网络异常 / 已过期 | warn 日志，不重试 |
| `adapter.send_chat_request` 失败 | 已有逻辑 | 沿用 `log::error!`，不改 |

### 7.2 超时配置

| 操作 | 超时 |
|---|---|
| `/v1.0/oauth2/accessToken` | 10s |
| `/v1.0/robot/messageFiles/download` | 10s |
| GET 下载 body | 60s（弱网超大文件直接放弃，让用户重发） |
| sessionWebhook POST | 5s |

统一在 `DingtalkFileDownloader` 内的 `reqwest::Client::builder().timeout(...)` 设置。

### 7.3 重试

只对**下载文件 body** 阶段最多 2 次、间隔 500ms 重试，应对短暂抖动。每次重试独立计 60s 超时（总最多 ~180s + 2×500ms 间隔）。其他错误一律不重试（accessToken / downloadCode 失败重试无意义；worker 串行，重试卡后续消息）。

### 7.4 资源限制

- 不限单文件大小（按用户决策）
- 单条 IM 内的多 attachment 串行下载（避免一条消息触发并发 N 个连接）
- 跨消息天然串行（worker 自身）

## 8. 日志

每个关键事件一条 info / warn，全部带 `msgId` / `sessionId` 让事后能串联：

```
// dingtalk_stream（已有的保留）
info!("[dingtalk-stream] EVENT/CALLBACK topic=... payload=...")
info!("[dingtalk-stream] parsed kind=richText text_len=N attachments=K msgId=...")
warn!("[dingtalk-stream] drop unknown msgtype=...")

// dingtalk_download（新增）
info!("[dingtalk-download] start file_name=... download_code_prefix=<前 8 字符>")
info!("[dingtalk-download] got temp url status=200 file_name=...")
info!("[dingtalk-download] saved sha256=... size=N path=...")
info!("[dingtalk-download] dedup hit sha256=... reuse path=...")
warn!("[dingtalk-download] failed phase=token|geturl|fetch|write file_name=... err=...")

// channel/manager worker（新增）
info!("[channel] downloading N attachments msgId=... session=...")
info!("[channel] download done success=K failed=M for msgId=...")
warn!("[channel] all attachments failed and no text, replying via sessionWebhook msgId=...")
info!("[channel] starting turn session=... content_len=N attachments=K")
```

`download_code_prefix` 只取前 8 字符避免日志过长且不泄露 token。

## 9. 测试

### 9.1 单元测试

#### `dingtalk_stream::parse_im_message`（在原 `tests` mod 内）

- `parse_picture_single` — `msgtype=picture` → text="" + 1 spec
- `parse_file_single` — `msgtype=file` → text="" + 1 spec，file_name 携带
- `parse_audio_with_recognition` — `msgtype=audio` → text=recognition + attachments=[]
- `parse_audio_empty_recognition_drops` — recognition 为空或缺失 → Drop
- `parse_richtext_pictures_and_text` — `[pic,\n,pic,\n,"你好"]` → text="你好" + 2 specs
- `parse_richtext_filters_whitespace_only` — `[\n,\n,"你好"]` → text="你好"
- `parse_richtext_picture_only` — 全 picture 段无 text → text="" + N specs
- `parse_richtext_unknown_segment_type` — 含 video segment → 跳过未知，picture/text 保留
- `parse_interactive_card_doc_url_auto_reply` — 已有，保留
- `parse_text_still_works` × 4 — 保留全部已有 text 测试，回归

#### `dingtalk_download` 单测（用 `wiremock` 或 `mockito`）

- `download_two_step_happy_path`
- `download_dedup_when_same_content` — 调两次 download，磁盘只一份
- `download_token_failure_returns_error`
- `download_geturl_failure_returns_error`
- `download_retries_2x_on_get_body_timeout` — 第 1、2 次断流，第 3 次成功
- `download_safe_filename_for_dangerous_input` — `file_name="../../etc/passwd"` 落盘安全
- `download_uses_extension_from_original_filename` — `report.xlsx` → `<sha>.xlsx`；无扩展名 → `<sha>.bin`

### 9.2 集成测试 — `src-tauri/tests/dingtalk_attachment_integration_test.rs`

用 `Trait FileDownloader` 让 worker 接受替身：

- `worker_downloads_and_starts_turn_with_attachments` — 灌 ChannelMessage 含 2 specs，FakeDownloader 返 2 path → mock chat_adapter 收到 ChatTurnRequest（attachments=2）
- `worker_handles_partial_download_failure` — 3 specs 第 2 个失败 → 起 turn，content 含失败说明，attachments=2
- `worker_skips_turn_when_all_failed_and_no_text` — 1 spec 失败 + text="" → mock chat_adapter 不被调用，fake sessionWebhook 收到提示
- `worker_dedups_msg_id` — 同 msg_id 进 2 次 → 只起 1 次 turn
- `worker_does_not_block_other_messages` — 5 条带 attachments，downloader 每个 sleep 200ms → 总耗时 ≈ 1s（验证串行 + 下个消息能在前一个完成后立即处理）

### 9.3 回归

- `cargo test review_ --tests` —— 验证 runtime 模块不被钉钉改动污染
- 现有 `dingtalk_stream::tests::*` / `manager.rs::tests::*` / `reply_manager::tests::*` / `router::tests::*` 全部通过

### 9.4 手动验证矩阵（必做）

| 场景 | 期望 |
|---|---|
| 单独发 1 张图 | app 内附件气泡，LLM 正常回复 |
| 单独发 1 个 .pptx | app 内附件气泡，LLM 能 read |
| 单独发 .md | 能 read 内容 |
| richText: 2 图 + "你好" | 1 turn，attachments=2，正文="你好" |
| richText: 3 图无文字 | 1 turn，attachments=3，正文="" |
| 1.5GB 大文件 | 60s 超时 → 起 turn 时附下载失败说明 |
| 24h 前的钉盘文件 | downloadCode 过期 → 失败说明（仅它一个且无文字 → sessionWebhook 提示重发） |
| 钉钉文档分享卡 | sessionWebhook 回"暂不支持"（已实现，回归） |
| 语音 | 用 recognition 起 turn |
| 钉钉断网重连后发图 | 自动重连后正常处理 |

### 9.5 不做

- 不 mock 真实钉钉 OpenAPI 行为（变化只能靠手动发现）
- 不测多模态 LLM（不走多模态路径）
- 不做性能 / 压测（单用户场景串行已满足）

## 10. 兼容性

- 不破坏前端接口：`ChatTurnRequest` 形态保持，前端附件链路完全复用。
- 不破坏 conversation 持久化：history 中的 user message 仍按 `build_user_content_json` 形态写入，只是 attachments 由钉钉来源代替前端来源。
- 不破坏 review 测试：`runtime/` 模块不被改动；`parse_im_message` 仍同步纯函数。
- 已有 interactiveCard "暂不支持" 行为不变。

## 11. 风险

| 风险 | 缓解 |
|---|---|
| 钉钉协议字段变更（如 richText segment 命名） | `#[serde(other)]` 兜底 + warn 日志（payload 已每条 info 打印） |
| downloadCode 一次性 / 24h 过期 | 错误处理矩阵覆盖，部分失败也起 turn |
| 大文件 / 弱网 | 60s 死线 + 用户重发，不做无界等待 |
| 同一会话快速连发多消息导致 worker 排队 | worker 串行是设计选择，不修；只要单文件下载 < 30s 就能保证排队体感 |
| accessToken 缓存与 reply_manager 各持一份 | 故意为之，避免 manager → reply_manager 反向依赖；钉钉允许多次刷 token |

## 12. 后续工作

- audio 走自己的 ASR（whisper.cpp）— 当 recognition 准确率不足成为瓶颈时
- interactiveCard 钉钉文档 / 钉盘的对话内 OAuth — 单独立项，需要后端配合
- `dingtalk_downloads/` 在 app 设置里加"清理"入口
- 如果磁盘占用成为问题，再讨论是否纳入 `upload_gc`

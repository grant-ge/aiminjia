# Phase 2：企微 IM Connector（aibot WebSocket 长连接）

**日期**：2026-05-18（重写 v3 — 入站模式从 webhook 改为 aibot WebSocket）
**状态**：Design draft v3 → 待用户 review
**前置**：Phase 0（IMConnector trait + manager + shared/lock/cancel）已落地
**Scope**：在 `connector/im/wecom/` 实现 IMConnector，入站走腾讯官方 aibot WebSocket 长连接（参考 `@wecom/aibot-node-sdk` 1.0.7 MIT 协议），出站默认走 markdown / 主动 send_msg（**本期不接 replyStream 流式 AI 卡片**）；附带在 `im/shared/aicard_fallback.rs` 抽出"流式不支持"降级 buffer

## 背景

企微是接入模型跟 Phase 0 / Phase 1 不同的第一个平台。最初设计走 webhook 入站，但 **桌面 app 在用户家用/公司内网下无法暴露公网端口**——让用户自己配 ngrok / cloudflared / 反代对个人用户基本不可行。

实际可用的路径是腾讯官方维护的 **aibot WebSocket 长连接**（`@wecom/aibot-node-sdk` 协议，MIT 开源）。桌面 app 主动外连到 `wss://openws.work.weixin.qq.com`，全程**只出不进**，跟现有 `dingtalk/stream.rs` 是同一类型。Rust 端用 `tokio-tungstenite` 重写一份 client（SDK 是 Node.js，不能直接复用），协议结构非常简单（JSON-over-WebSocket）。

aibot 模式本身**支持流式 AI 卡片**（`aibot_respond_msg` + `msgtype: stream` + 同 streamId 多次刷新），但**本期不实现**——只用 markdown 主动发送即可满足 MVP，流式留待后续 Phase 评估。

## 调研结论（事实摘要）

参考代码：
- npm `@wecom/aibot-node-sdk@1.0.7` MIT 源码（github.com/WecomTeam/aibot-node-sdk）
- openclaw 实现：`~/Downloads/openclaw channel/wecom-openclaw-plugin-main/`

### 1. 入站：aibot WebSocket 长连接

- **地址**：默认 `wss://openws.work.weixin.qq.com`（可覆盖，但生产无需）
- **认证**：建连后立刻发首帧 `{ cmd: "aibot_subscribe", headers: { req_id }, body: { secret, bot_id } }`
  - **bot_id / secret 是静态凭证，不会过期**——所以 Phase 2 **不依赖** `shared::TokenCache`（Phase 1 §0 PR0a）
  - 认证成功返回 `{ headers: { req_id }, errcode: 0, errmsg: "ok" }`
- **心跳**：默认 30 秒发一次 `{ cmd: "ping", headers: { req_id } }`，连续 N 次未收到 pong 视为连接死，触发重连
- **重连**：指数退避，分两个计数器各自独立——connection drop（默认 max 10）vs auth failure（默认 max 5）
- **多账号**：用户加 N 个企微 bot = N 个 WSClient 实例 = N 个独立 WebSocket，**不共用连接**

### 2. 帧结构（统一）

```json
{ "cmd": "<command>", "headers": { "req_id": "<uuid>" }, "body": { ... } }
```

响应帧（认证 / 心跳 / 回复 ack）：

```json
{ "headers": { "req_id": "<uuid>" }, "errcode": 0, "errmsg": "ok" }
```

### 3. 入站推送帧

- **`aibot_msg_callback`** —— 用户消息推送
  - `body`: `{ msgid, aibotid, chatid?, chattype: 'single'|'group', from: { userid }, msgtype, [type-specific fields] }`
  - msgtype: `text` / `image` / `mixed` / `voice` / `file` / `video`
- **`aibot_event_callback`** —— 事件推送
  - `body`: `{ ..., msgtype: 'event', event: { eventtype } }`
  - eventtype: `enter_chat` / `template_card_event` / `feedback_event` / **`disconnected_event`**
  - **`disconnected_event` 特殊**：服务端在新连接建立时主动断开旧连接（不是网络问题）→ **不应触发重连**，否则会形成"踢断—重连—又被踢"的死循环。WSClient 收到 disconnected_event 后必须设 `isManualClose=true` 跳过 scheduleReconnect

### 4. 出站发送帧

- **`aibot_respond_msg`** —— 被动回复（透传收到的 frame.headers.req_id）
  - `{ msgtype: 'markdown', markdown: { content } }` ✅ 本期使用
  - `{ msgtype: 'stream', stream: { id, finish?, content? } }` ❌ 本期不接（流式 AI 卡片，留待后续 Phase）
  - `{ msgtype: 'file'|'image'|'voice'|'video', <type>: { media_id } }` ✅ 本期支持 image / file
- **`aibot_send_msg`** —— 主动推送（不需 frame，传 chatid）
  - 仅 markdown / template_card / 媒体（**协议层就没有流式**）
  - 用于 "AiCardChunk final" 落地时主动发完整 markdown 给会话
- **媒体上传**：`aibot_upload_media_init` → `aibot_upload_media_chunk × N` → `aibot_upload_media_finish` 三步分片上传，单分片 ≤512KB，最多 100 分片
- **媒体下载**：消息 body 含 `image.aeskey` / `file.aeskey`（AES-256 base64 key），下载文件后用 aeskey AES-256-CBC 解密
- **req_id 串行约束**：同一 req_id 的多个出站帧必须串行（前一帧 ack 后才能发下一帧），ack 默认超时 10s。流式中间帧可走 `replyStreamNonBlocking` 语义跳过未 ack 的帧

### 5. 关键错误码

- `846608` —— 流式消息 >6 分钟无更新，服务端拒绝继续 update。**本期不接 stream，所以不会遇到这个码**（但 connector error 路径仍处理一下，作为 defensive）
- `846605` —— event callback 没有有效 req_id（如 `enter_chat`），需走 `aibot_send_msg` 主动发，不要走 `aibot_respond_msg`

### 6. 凭证获取

用户在 `https://work.weixin.qq.com` 企业管理后台创建智能机器人，拿到 `bot_id` + `secret`，桌面 app 添加企微账号时填入这两个值即可。**不需要 CorpID / CorpSecret / AgentID 三件套**（那是传统 OA 应用的凭证，跟 aibot 智能机器人不同）。

## Non-Goals

1. 不接 `@wecom/aibot-node-sdk` Node.js SDK 本身——Rust 端用 `tokio-tungstenite` 自实现协议帧编解码（参考 SDK 源码 + openclaw TS 实现）
2. 不实现 `aibot_respond_msg` 的 `msgtype: stream` 流式回复（`replyStream` / `replyStreamWithCard`）——本期所有 AI 回复 buffer 到 final 再用 markdown 发一条
3. 不实现 template_card / button_interaction / vote_interaction 等富卡片——只支持 markdown / text / image / file
4. 不实现传统企微 webhook 入站（`/cgi-bin/...` HMAC+AES）—— aibot WebSocket 已覆盖所有场景
5. 不实现 voice / video 这两类入站消息的转发（**只 log + 忽略**）；出站不支持 voice / video / template_card
6. 不实现"单 connector 多 bot"——每个企微账号配置 = 1 个 connector 实例 = 1 个 WSClient = 1 个 (bot_id, secret) 组合

## 依赖关系（关键）

**Phase 2 不依赖 Phase 1 修订版 §0 任何 PR：**
- PR0a `shared/token.rs` —— ❌ 不依赖（aibot bot_id/secret 静态不过期）
- PR0b `shared/dedup.rs` —— ❌ 不依赖（msgid 由服务端保证唯一，按需查 map）
- PR0c dingtalk AI Card 接 trait —— ❌ 不依赖（aicard_fallback buffer 是新代码 + connector 内部使用，不接 dingtalk 现有 card 实现）
- PR0d ReplyTarget 平台中性 + ChannelConfigStore 多平台 —— ✅ **已落地**（Phase 0 PR4-PR6 已完成 `ReplyTarget { session_id, external_conversation_key }` + `ChannelConfigStore` 支持 Feishu/Wecom platform 枚举）

**Phase 2 内部依赖：**

```
PR1 (aibot_protocol frames)  ─┐
PR2 (aibot_client WS layer)  ─┤
PR3 (aicard_fallback)        ─┼─→ PR5 (connector 主体) → PR6 (前端 UI + 集成测试)
PR4 (parser + sender + media)─┘
```

**整条链路完全独立，可以与飞书 Phase 1 / Phase 3 Telegram 并行推进。**

## §1. aibot WebSocket 协议帧编解码（Phase 2 PR1）

新增 `connector/im/wecom/aibot_protocol.rs`，定义所有 WebSocket 帧 Rust 类型 + serde 序列化：

```rust
/// WebSocket 命令枚举，对应 SDK `WsCmd` 常量
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsCmd {
    #[serde(rename = "aibot_subscribe")]      Subscribe,
    #[serde(rename = "ping")]                  Ping,
    #[serde(rename = "aibot_respond_msg")]     Respond,
    #[serde(rename = "aibot_send_msg")]        SendMsg,
    #[serde(rename = "aibot_msg_callback")]    MsgCallback,
    #[serde(rename = "aibot_event_callback")]  EventCallback,
    #[serde(rename = "aibot_upload_media_init")]   UploadInit,
    #[serde(rename = "aibot_upload_media_chunk")]  UploadChunk,
    #[serde(rename = "aibot_upload_media_finish")] UploadFinish,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsFrame<B = serde_json::Value> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<WsCmd>,
    pub headers: FrameHeaders,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<B>,
    /// 响应帧才有
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errcode: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errmsg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHeaders {
    pub req_id: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Inbound: aibot_msg_callback body
#[derive(Debug, Clone, Deserialize)]
pub struct InboundMessageBody {
    pub msgid: String,
    pub aibotid: String,
    #[serde(default)]
    pub chatid: Option<String>,
    pub chattype: ChatType,
    pub from: From,
    pub msgtype: String,    // "text" / "image" / ...
    #[serde(default)]
    pub create_time: Option<i64>,
    #[serde(flatten)]
    pub payload: serde_json::Value,    // 留给 parser 解析具体 type
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatType { Single, Group }

#[derive(Debug, Clone, Deserialize)]
pub struct From { pub userid: String, #[serde(default)] pub corpid: Option<String> }

/// Outbound: subscribe body
#[derive(Debug, Clone, Serialize)]
pub struct SubscribeBody<'a> { pub secret: &'a str, pub bot_id: &'a str }

/// Outbound: respond_msg body — markdown variant
#[derive(Debug, Clone, Serialize)]
pub struct RespondMarkdownBody { pub msgtype: &'static str /* "markdown" */, pub markdown: MarkdownContent }

#[derive(Debug, Clone, Serialize)] pub struct MarkdownContent { pub content: String }

/// Outbound: send_msg body (主动推送)
#[derive(Debug, Clone, Serialize)]
pub struct SendMsgBody { pub chatid: String, #[serde(flatten)] pub payload: SendMsgPayload }

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SendMsgPayload {
    Markdown { msgtype: &'static str, markdown: MarkdownContent },
    Media { msgtype: WeComMediaType, /* file / image / voice / video flatten */ ... },
}

/// req_id 生成：`{prefix}_{ms_timestamp}_{8-char-random}`
pub fn generate_req_id(prefix: &str) -> String { ... }
```

**测试**：100% 单元测试覆盖 frame 序列化 / 反序列化，从 SDK 源码 / openclaw 抓真实样例帧硬编码进 test 文件作圣经测试。

## §2. aibot WebSocket Client（Phase 2 PR2）

新增 `connector/im/wecom/aibot_client.rs`，是协议帧之上的连接管理层，**结构对标 `connector/im/dingtalk/stream.rs`**（已成熟，复用 lock / cancel / reconnect 模式）：

```rust
pub struct AibotClient {
    bot_id: String,
    secret: String,                          // 静态凭证
    ws_url: String,                          // 默认 wss://openws.work.weixin.qq.com
    heartbeat_interval: Duration,            // 默认 30s
    reply_ack_timeout: Duration,             // 默认 10s
    max_missed_pong: usize,                  // 默认 3
}

pub enum AibotEvent {
    /// 服务端 push 的消息帧（aibot_msg_callback / aibot_event_callback）
    Inbound(WsFrame),
    /// 服务端踢下线（disconnected_event）—— 调用方应停止重连
    KickedOut(String),
    /// 物理连接断开（网络 / 心跳超时）—— 调用方应触发重连
    ConnectionDropped(String),
    /// 认证失败 ack —— 调用方应区分计数器
    AuthFailed(i32, String),
}

impl AibotClient {
    /// 启动连接 + 鉴权 + 心跳循环；将所有入站帧通过 mpsc::Sender<AibotEvent> 投递给调用方
    /// cancel_token 取消时主动关闭 WS（in-flight ack 允许超时，不强 abort）
    pub async fn run(
        self,
        event_tx: mpsc::Sender<AibotEvent>,
        cancel_token: CancellationToken,
    ) -> Result<(), AibotError>;

    /// 发被动回复帧（aibot_respond_msg），透传 req_id，串行队列
    pub async fn respond(
        &self,
        req_id: &str,
        body: serde_json::Value,
    ) -> Result<(), AibotError>;

    /// 发主动推送帧（aibot_send_msg）
    pub async fn send_msg(&self, body: SendMsgBody) -> Result<(), AibotError>;

    /// 上传媒体（三步分片）→ 返回 media_id
    pub async fn upload_media(
        &self,
        media_type: WeComMediaType,
        filename: &str,
        data: Vec<u8>,
    ) -> Result<String, AibotError>;
}
```

### 2.1 重连策略

复用 Phase 0 的 `shared::reconnect::ReconnectBackoff`（5/15/30/60s ladder + jitter）：

- **物理 drop**（网络 / pong miss）→ 走 ReconnectBackoff，max attempts 默认 10
- **认证失败**（errcode != 0 的 subscribe ack）→ 独立计数器，max attempts 默认 5，超限抛 `AuthExpired`（让 manager 知道是凭证错而非网络错）
- **disconnected_event** → **不重连**，直接 `KickedOut`，让 manager 知道是被踢，不要无限循环抢连接

### 2.2 串行 ack 队列

每个 (req_id, 出站帧) 进 BTreeMap<req_id, VecDeque<Frame>>，前一个收到 ack 才发下一个。复用 `tokio::sync::oneshot` 给 caller resolve。超时 10s → fail 该帧，但**不**关连接（一个 reqId 超时不影响其它）。

### 2.3 心跳

`tokio::time::interval(30s)` 任务，每次 tick 发 `{ cmd: "ping", headers: { req_id } }`，记 pending；收到对应 ack req_id 时清掉。连续 3 次未收到 pong → 视为连接死，主动 close + 触发 reconnect。

### 2.4 测试

- `aibot_protocol::tests` —— frame 序列化/反序列化圣经测试（已在 PR1）
- `aibot_client::tests` —— mock WebSocket server（用 `tokio-tungstenite` accept server side），跑这些场景：
  - 握手 → subscribe → 认证 OK → 收到 inbound 消息
  - 心跳超时 → 重连
  - 收到 disconnected_event → 不重连，发 KickedOut
  - 串行 ack：同 req_id 连发两帧，第一帧 ack 前第二帧应在内部排队
  - ack 超时 → fail 该帧不关连接
  - cancel_token 触发 → 2 秒内退出 run()

## §3. 通用流式不支持降级 Buffer（Phase 2 PR3）

新增 `connector/im/shared/aicard_fallback.rs`，给所有 `outbound_aicard: false` 的 connector 复用（Phase 2 wecom + Phase 4 whatsapp + Phase 5 个微）。

**注意**：跟"webhook vs ws"无关，是给"connector capabilities 声明不支持流式 AI 卡片"的平台用的内部 buffer。即便 wecom aibot 协议支持流式（`replyStream`），本期 capabilities 也声明 `outbound_aicard: false`，让 manager 把 AI 回复以 `AiCardChunk` 多帧形式给 connector，由 connector 在内部 buffer 起来到 final 时发一条完整 markdown。

```rust
pub struct AiCardFallbackBuffer {
    accumulated: String,
    started_at: Instant,
    placeholder_after: Duration,     // 默认 4 分钟
    placeholder_sent: bool,
}

pub enum FallbackAction {
    /// 累积，不发任何消息
    Buffer,
    /// 发"思考中..."占位（防止用户感觉 connector 卡死）
    SendPlaceholder { text: String },
    /// 发最终回复
    SendFinal { text: String },
}

impl AiCardFallbackBuffer {
    pub fn new(placeholder_after: Duration) -> Self;
    pub fn observe(&mut self, delta: &str, final_chunk: bool) -> FallbackAction;
}
```

### 策略

1. **首次 chunk**：累积，记 `started_at`
2. **后续 chunks**：累积，不发任何消息
3. **超过 `placeholder_after`（默认 4 分钟）还没 final**：发一次"思考中..."占位（`placeholder_sent` 防重）
4. **final**：发完整版

**一次 AI 回复最多 2 条消息**（思考中 + 最终），多数情况下只有 1 条（最终）。

### 为什么不做其它方案

- **每 N 秒发增量** ❌：刷屏破坏聊天历史
- **edit_via_recall** ❌：企微 recall API 不可靠，撤回时间窗也短
- **完全不发 / 等用户主动问** ❌：用户长时间没反馈会以为 bot 挂了

### 测试

`im/shared/aicard_fallback::tests` —— observe() 在不同 input pattern 下返回正确 FallbackAction：
- 边界：placeholder_after 之前 final / 之后 final / 多次 chunk 后 final / 首次就是 final（短回复）

## §4. Parser + Sender + Media（Phase 2 PR4）

### 4.1 目录结构

```
src-tauri/src/connector/im/wecom/
├── mod.rs                  # impl IMConnector for WecomConnector
├── aibot_protocol.rs       # PR1: WebSocket 帧 Rust 类型
├── aibot_client.rs         # PR2: 连接管理层
├── parser.rs               # 入站 InboundMessageBody → ChannelMessage 映射
├── sender.rs               # 出站：markdown / media 包装为 respond_msg / send_msg
├── media.rs                # 媒体上传（aibot_upload_media_*）+ 下载（aeskey 解密）
└── types.rs                # WecomConfig / WecomMediaType / SessionMap 等
```

### 4.2 parser.rs

把 `InboundMessageBody`（aibot 推送）映射到 trait 层的 `ChannelMessage`：

- text → `ChannelMessage::Text { ..., text: body.text.content }`
- image / file → 触发媒体下载（aeskey 解密） → `ChannelMessage::Attachment { ..., local_path, mime }`
- voice / video / mixed → log + 忽略（Non-Goals 5）
- event callback（enter_chat / feedback_event）→ log + 忽略；template_card_event 也忽略
- disconnected_event 不进 parser（由 aibot_client 直接发 KickedOut 给 connector）

`session_id` 构造：单聊用 `wecom:{bot_id}:single:{userid}`，群聊用 `wecom:{bot_id}:group:{chatid}`，作为 `ReplyTarget.session_id` 的 key。同时存 `external_conversation_key` = chatid（群） 或 userid（单聊），用于出站 `aibot_send_msg.chatid`。

### 4.3 sender.rs

- `send_markdown(target, text)` —— 优先尝试**被动回复**（如果该 session 有最近的 frame.req_id 记录在内存 cache 里）；否则走**主动推送** `aibot_send_msg`
- frame.req_id cache：每条入站消息 parser 时把 `(session_id, req_id, received_at)` 写入 `SessionMap`；5 分钟内有 cache hit 用被动回复，超过窗口或没记录就用主动推送（被动回复要求 req_id 在服务端还活着；超时风险用主动推送兜底）
- `send_attachment(target, attachment)` —— 走 media.rs upload_media 拿 media_id，组装 SendMediaMsgBody，调 send_msg

### 4.4 media.rs

- 上传：split 文件为 ≤ 512KB 分片，base64 编码每片，依次发 init / chunk × N / finish 帧；返回 media_id
- 下载：HTTP GET `url`（5 分钟内有效）拿密文 → AES-256-CBC（aeskey base64 decode 后用作 key + IV 取 key 前 16 字节，按 SDK `decryptFile` 实现） → 写入 workspace `attachments/wecom/{date}/{msgid}-{filename}`
- 文件名 fallback：HTTP response 的 `Content-Disposition` 没给文件名时，按 `{msgid}.{ext}` 由 mime 推 ext

### 4.5 测试

- `parser::tests` —— text / image / file 各一份样例 → ChannelMessage 映射正确
- `sender::tests` —— 用 mock AibotClient（trait 接口）验证：
  - send_markdown 在 cache hit 时走 respond_msg + 携带 req_id
  - cache miss 时走 send_msg
- `media::tests` —— 加密 buffer + 已知 aeskey → 解密结果是预期明文（参考 SDK `decryptFile` 测试向量）

## §5. WecomConnector trait 实现（Phase 2 PR5）

### 5.1 capabilities

```rust
ConnectorCapabilities {
    inbound: InboundModel::Stream,    // ✅ aibot 是 WebSocket 长连接，跟 dingtalk 同类型
    outbound_aicard: false,           // 本期不接 replyStream；走 aicard_fallback
    outbound_markdown: true,
    supports_attachments: true,
    supports_group_chat: true,
    supports_private_chat: true,
    auth_flow: AuthFlow::ApiKey,      // bot_id + secret 静态配置
}
```

### 5.2 trait `start()` 实现

```rust
impl IMConnector for WecomConnector {
    fn platform(&self) -> Platform { Platform::Wecom }

    fn capabilities(&self) -> ConnectorCapabilities { /* §5.1 */ }

    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let (msg_tx, msg_rx) = mpsc::channel::<ChannelMessage>(256);
        let (evt_tx, mut evt_rx) = mpsc::channel::<AibotEvent>(64);

        let client = self.aibot_client.clone();
        let parser = self.parser.clone();
        let session_map = self.session_map.clone();
        let cancel_token = ctx.cancel_token.clone();

        // task 1: 运行 WebSocket client 主循环
        tokio::spawn({
            let cancel_token = cancel_token.clone();
            async move { let _ = client.run(evt_tx, cancel_token).await; }
        });

        // task 2: 消费 AibotEvent → 派发到 msg_tx
        tokio::spawn(async move {
            while let Some(evt) = evt_rx.recv().await {
                match evt {
                    AibotEvent::Inbound(frame) => {
                        if let Some(msg) = parser.parse(&frame, &session_map).await {
                            let _ = msg_tx.send(msg).await;
                        }
                    }
                    AibotEvent::KickedOut(reason) => {
                        log::warn!("[wecom] kicked out by server: {reason}; stream ends");
                        break;    // 关 msg_tx → manager 看到 stream end，不重连
                    }
                    AibotEvent::ConnectionDropped(reason) => {
                        log::info!("[wecom] connection dropped: {reason}; client will reconnect");
                        // 不关 msg_tx，等 aibot_client 内部 reconnect 后继续推 Inbound
                    }
                    AibotEvent::AuthFailed(code, msg) => {
                        log::error!("[wecom] auth failed code={code} msg={msg}");
                        break;    // 关 msg_tx → manager 看到 stream end
                    }
                }
            }
        });

        Ok(ReceiverStream::new(msg_rx).boxed())
    }

    async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
        match content {
            ReplyContent::Text(t) | ReplyContent::Markdown(t) => {
                self.sender.send_markdown(&target, &t).await
            }
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                // §6 处理流式降级
                self.handle_aicard_chunk(&target, &delta, final_chunk).await
            }
            ReplyContent::AiCardFail => {
                self.sender.send_markdown(&target, "❌ 处理失败，请重试").await
            }
        }
    }
}
```

### 5.3 流式降级（aicard_fallback 接入）

```rust
async fn handle_aicard_chunk(
    &self,
    target: &ReplyTarget,
    delta: &str,
    final_chunk: bool,
) -> Result<(), ConnectorError> {
    let mut buffers = self.fallback_buffers.lock().await;
    let buffer = buffers
        .entry(target.session_id.clone())
        .or_insert_with(|| AiCardFallbackBuffer::new(Duration::from_secs(240)));
    let action = buffer.observe(delta, final_chunk);
    let cleanup_now = matches!(action, FallbackAction::SendFinal { .. });
    drop(buffers);

    match action {
        FallbackAction::Buffer => Ok(()),
        FallbackAction::SendPlaceholder { text } => {
            self.sender.send_markdown(target, &text).await
        }
        FallbackAction::SendFinal { text } => {
            let r = self.sender.send_markdown(target, &text).await;
            if cleanup_now {
                self.fallback_buffers.lock().await.remove(&target.session_id);
            }
            r
        }
    }
}
```

### 5.4 测试

- `tests/im_wecom_integration.rs` —— 起 Manager + WecomConnector + mock aibot WebSocket server，验证：
  - 收到 text inbound → ChannelMessage 出现在 manager 端
  - 触发 AI 回复 → 多个 AiCardChunk → final 时 mock server 收到一个 markdown respond_msg
  - mock server 推送 disconnected_event → connector 关流 → manager 看到 stream end 后**不**重连（与物理 drop 区分）
- `tests/review_im_layering.rs` —— `platforms` 数组追加 `"wecom"`

## §6. 前端 UI + 集成测试（Phase 2 PR6）

### 6.1 "添加企微" 流程

前端只要一个简单的"两输入框"表单：

```
[ ] Bot ID    （在企业微信管理后台 → 智能机器人 → 详情页 复制）
[ ] Secret    （同上）

[ 测试连接 ]   [ 保存 ]
```

测试连接逻辑：临时起 aibot_client → 跑一次 subscribe → 收到 ack 立刻 close。errcode != 0 显示具体错误信息（"凭证错误" / "网络异常" / "服务端拒绝"）。

**不需要**外网穿透 / 反代 / cloudflared / 复制 URL 任何东西——这是 aibot WebSocket 相对 webhook 最大的产品优势。

### 6.2 ChannelConfigStore 字段

在 Phase 0 已有的 `ChannelConfig` JSON 落盘里追加 wecom payload：

```json
{
  "platform": "wecom",
  "account_id": "<auto-uuid>",
  "display_name": "<用户填的别名>",
  "credentials": {
    "bot_id": "...",
    "secret": "<加密存储 via SecureStorage>"
  }
}
```

secret 走 `secure_storage` 加密路径（同钉钉 access_token 的处理）。

### 6.3 i18n 文案

- `src/i18n/zh-CN.json` `channels.wecom.*`：标题、字段标签、错误信息、帮助文档链接
- `src/i18n/en-US.json` 同步

### 6.4 集成测试

- 端到端：在 dev 模式启 app → 配 mock aibot server → 真实点 "测试连接" → 看到成功提示
- 持久化：保存后重启 app → 连接自动恢复
- 删除：删除 connector → WebSocket close → 不再重连

## §7. 实施 PR 切分

**完全独立可推，不依赖飞书 Phase 1 / Phase 3 Telegram：**

- **PR1** `wecom/aibot_protocol.rs` —— 帧编解码 + serde + 单元测试（圣经向量 from SDK）
- **PR2** `wecom/aibot_client.rs` —— WebSocket 连接管理 + 心跳 + 重连 + ack 队列 + 单元测试（mock WS server）
- **PR3** `shared/aicard_fallback.rs` —— 通用流式降级 buffer + 单元测试
- **PR4** `wecom/{parser, sender, media}.rs` —— 入站解析 + 出站包装 + 媒体上传下载 + 单元测试
- **PR5** `impl IMConnector for WecomConnector` —— 集成 PR1-PR4，trait 实现 + 集成测试
- **PR6** 前端 UI + ChannelConfigStore 接入 + i18n + 端到端集成测试

**估时**（单人）：

| PR | 估时 | 说明 |
|---|---|---|
| PR1 | 1 天 | 帧类型 + serde + 圣经测试，对照 SDK .d.ts 一比一抄即可 |
| PR2 | 2.5 天 | WS 连接是最复杂的——心跳 / ack 队列 / 双重连计数器 / disconnected_event 边界 |
| PR3 | 0.5 天 | buffer 逻辑简单，主要写测试 |
| PR4 | 2 天 | parser + sender + media（含 AES 解密） |
| PR5 | 1.5 天 | trait 实现 + 集成测试 |
| PR6 | 1.5 天 | 前端 + ChannelConfigStore + i18n |

**总计：~9 天单人**

## §8. 风险

| 风险 | 缓解 |
|---|---|
| aibot 协议非公开规范，全靠 SDK + openclaw 反推 | SDK 是腾讯官方 MIT 开源、有完整 TypeScript 类型定义；openclaw 在生产环境跑过；圣经测试用 SDK 抓真实 frame 锁定行为 |
| disconnected_event 误判会形成踢断—重连—又踢的死循环 | aibot_client 严格区分 KickedOut（不重连）vs ConnectionDropped（重连）；集成测试明确覆盖此场景 |
| 多账号 = 多连接，资源占用 | 桌面端典型用户 1-3 个 bot，N 不会大；每个连接基本只跑心跳 + 偶发消息，开销可忽略 |
| Anthropic LLM 响应慢 → 用户长时间没看到回复 | aicard_fallback 4 分钟发"思考中..."占位（同 spec §3） |
| 附件 size 上限（aibot 内部分片每个 ≤512KB × 100）= 50MB | 超限返回 `ConnectorError::NotSupported`；manager 提示用户"超出企微 50MB 上限" |
| WebSocket 长连接在用户切换网络 / 笔记本休眠后断 | 心跳 + 物理 drop 自动重连（ReconnectBackoff），跟 dingtalk 同样路径 |
| 用户填错凭证 / bot 被禁用 | auth failure 计数器独立 max=5 → 抛 `ConnectorError::AuthExpired`，前端弹"凭证错误，请重新登录企业微信后台获取" |

## §9. 跟其它 Phase 的关系

- **Phase 1（飞书）**：零依赖。Phase 2 不再触碰 `shared::TokenCache`（aibot 静态凭证）。Phase 1 的 §0 PR0a-d 飞书自己改，wecom 不动
- **Phase 3（Telegram）**：零依赖。Telegram spec 已定走长轮询 + webhook 双入站；Phase 2 不再实现 webhook_server，所以 Telegram **不能再依赖 Phase 2 PR1 webhook_server**——这部分要重新评估：要么 Telegram 自己实现长轮询（最简单，零公网依赖），要么 Phase 4 (WhatsApp) 真正需要 webhook 时再独立做 webhook_server
- **Phase 4（WhatsApp）**：可能需要 webhook（WhatsApp Cloud API 是 webhook-only）→ 那时再独立写 webhook_server spec
- **Phase 5（个微）**：iLink HTTP 长轮询（SelfHosted），跟本 spec 完全独立（Phase 5 调研后从 NativeDaemon 改为 SelfHosted，详见 Phase 5 spec §11）

## §10. 历史决策记录

**v1（2026-05-18 早）**：webhook 入站 + crypto。被推翻的原因：桌面 app 无法暴露公网，用户配反代/穿透不现实。

**v2（同日）**：考虑 aibot WebSocket，但被错误评估为"不划算自实现"——SDK 是 Node.js only，要 Rust 重写。

**v3（同日，本版）**：确认 aibot 协议本质是 JSON-over-WebSocket，参考 SDK 1.0.7 MIT 源码 .d.ts 完整披露了帧格式、命令枚举、错误码、心跳/重连规则，Rust 端 `tokio-tungstenite` 重写工作量约 3-5 天，对标现有 dingtalk/stream.rs 架构成熟模板。webhook 模式整体放弃；webhook_server 推迟到 Phase 4 真正用得到时再做。

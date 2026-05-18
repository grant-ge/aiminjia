# Phase 2：企微 IM Connector + 内置 Webhook 入站方案

**日期**：2026-05-18
**状态**：Design draft → 待用户 review
**前置**：Phase 0 + Phase 1 已落地
**Scope**：在 `connector/im/wecom/` 实现 IMConnector + 在 `im/shared/webhook_server.rs` 落地**全平台通用的本地 HTTP webhook 入站基础设施**

## 背景

企微是接入模型跟 Phase 0 / Phase 1 完全不同的第一个平台——**它只支持 webhook 入站，没有长连接 SDK**。所以本 spec 一并解决"桌面 app 如何接 webhook"这个 Phase 0 留给后续的问题。

webhook 入站方案落地后，Phase 3 (Telegram) / Phase 4 (WhatsApp) 都能复用。

## 调研结论（事实摘要）

参考代码：`~/Downloads/openclaw channel/wecom-openclaw-plugin-main/`

1. **入站**：HTTP POST/GET webhook，路径形如 `/plugins/wecom/bot/{accountId}`；需要公网 IP
2. **回调加密强制**：URL query 带 `msg_signature` (HMAC-SHA256) + `timestamp` + `nonce`；body `encrypt` 字段是 AES-256-CBC + PKCS#7
3. **认证**：CorpID + CorpSecret + AgentID 三件套；`/cgi-bin/gettoken` 拿 access_token，2h 有效期，自动刷新
4. **消息类型**：text / markdown / image / voice / video / file / textcard / template_card / news
5. **媒体上传**：先调 `/cgi-bin/media/upload` 换 `media_id`（图片 10MB / 视频 10MB / 语音 2MB / 文件 20MB），超限降级为"链接 + 文本"
6. **动态多 agent**：plugin 支持单连接挂多个 AgentID + 每用户独立 session，要做 token 隔离
7. **6 分钟流式超时**：`errcode=846608`，超时后必须 fallback 到非流式

## Non-Goals

1. 不实现公网穿透 / cloudflare tunnel 的内建集成——本期只起本地 HTTP server，用户自己负责暴露
2. 不支持单 connector 内**多 AgentID 并存**（openclaw 那种 dynamic-agent / 动态路由模式）；每个企微账户配置 = 1 个 connector 实例
3. 不实现 voice / video / textcard / news 这 4 类高级消息类型；只支持 text / markdown / image / file
4. 不做 template_card 的"卡片态轮询"——只支持单向发送，不接收卡片回调

## §1. 通用 Webhook Server（这是 Phase 2 的核心新增）

新增 `connector/im/shared/webhook_server.rs`，提供给所有 webhook 平台复用：

```rust
pub struct WebhookServer {
    listen_addr: SocketAddr,                // 默认 127.0.0.1:7800，可配置
    routes: Arc<RwLock<HashMap<String, WebhookHandler>>>,
}

pub type WebhookHandler =
    Arc<dyn Fn(WebhookRequest) -> BoxFuture<'static, WebhookResponse> + Send + Sync>;

impl WebhookServer {
    /// 单例，app 启动时 spawn 一次。
    pub fn global() -> &'static Arc<WebhookServer>;

    /// 平台 connector 启动时注册自己的路径前缀。
    /// path_prefix 如 "/wecom/{account_id}"，handler 收到所有匹配的请求。
    /// 返回 RAII guard：connector stop 时 drop 自动反注册。
    pub fn register(&self, path_prefix: &str, handler: WebhookHandler) -> RouteGuard;
}
```

### 1.1 监听策略

- **默认**：`127.0.0.1:7800`（仅本地回环），用户需自行配 ngrok / cloudflared 暴露公网
- **可选**：用户在设置里改 `bind_addr` 到 `0.0.0.0:7800` + 配置反代
- **不**做：UPnP / NAT-PMP 自动开端口（不可控，且很多用户的路由器不开）

### 1.2 暴露引导 UX

在前端"添加企微"流程里加一步**外网穿透引导**：

```
[ ] 我已配置反代 / Cloudflare Tunnel / ngrok，公网地址是：
    https://___.example.com/wecom/account-1
[ ] 我用 cloudflared，可以一键启动（仅 macOS / Linux，需提前 install cloudflared）
[ ] 临时跳过（不能收消息，仅测试发送）
```

cloudflared 一键启动**不**作为 Phase 2 范围，列入"后续 Polish PR"待办。

### 1.3 单 server 多 connector 路径冲突

`WebhookServer::register` 检查路径前缀冲突，重复注册返回 `Err(PathConflict)`。
平台 connector 命名约定：`/{platform}/{account_id}/...`

## §2. WecomConnector 设计

### 2.1 目录结构

```
src-tauri/src/connector/im/wecom/
├── mod.rs                  # impl IMConnector for WecomConnector
├── runtime.rs              # 启动时 register webhook 路径 + 处理回调 → 喂 stream
├── crypto.rs               # AES-256-CBC + PKCS#7 + HMAC-SHA256 签名
├── token.rs                # access_token 缓存（复用 shared/token.rs）
├── sender.rs               # 主动调用 cgi-bin/* API 发消息
├── media.rs                # media/upload + media_id 缓存
├── parser.rs               # webhook body → ChannelMessage
└── types.rs                # CorpID/AgentID/EncryptedMessage 等
```

### 2.2 capabilities

```rust
ConnectorCapabilities {
    inbound: InboundModel::Webhook,
    outbound_aicard: false,            // 不支持流式卡片
    outbound_markdown: true,
    supports_attachments: true,
    supports_group_chat: true,
    supports_private_chat: true,
    auth_flow: AuthFlow::ApiKey,        // CorpID/CorpSecret 静态配置
}
```

### 2.3 流式回复降级

收到 `ReplyContent::AiCardChunk` 时（企微不支持流式卡片）：

1. Connector 内对 `chat_turn_id` 维护 buffer
2. **不**等到 `final_chunk=true` 才一次性发——会触发 6 分钟超时
3. 改为：**每 2 秒**（或每累计 200 chars）发一次"增量 markdown"作为新消息（NOT edit），用 emoji 前缀 `🤖 (思考中...)` 提示用户
4. final 时发完整版

这条策略写进 connector 内部，外层 ReplyDispatcher 看不到。

### 2.4 access_token 隔离

支持多企微账号（每个 connector 实例一个 CorpID + AgentID 组合）。token cache key：
```
wecom-token:{corp_id}:{agent_id}
```
绝对**不**在多 corp 之间复用。

## §3. 加解密 + 签名实现

完全照搬企微官方算法（Java/Python SDK 都有现成代码可参考）：

1. **签名校验**：`sha256(token + timestamp + nonce + encrypt) == msg_signature`
2. **解密**：base64 decode `encrypt` → AES-256-CBC（IV = key 前 16 字节）→ 去 PKCS#7 padding → 拆出 4 字节 length / N 字节 message / 16 字节 corp_id 校验
3. **加密响应**：反向

新建 `im/wecom/crypto.rs`，纯函数，**100% 单元测试覆盖**（含官方文档给出的示例向量）。

## §4. 测试

- `im/wecom/crypto::tests`：官方示例向量 → 100% pass（这是企微 SDK 的"圣经"，写错企微会 502）
- `im/wecom/parser::tests`：6 种消息 type → ChannelMessage 映射
- `im/wecom/sender::tests`：mock HTTP，验证 access_token 缓存命中 + 过期刷新
- `im/shared/webhook_server::tests`：register / unregister / 路径冲突 / 并发请求
- `tests/im_wecom_integration.rs`：起 Manager + WecomConnector + mock 企微回调（含加密 body）

## §5. 实施 PR 切分

- **PR1** `im/shared/webhook_server.rs` 新增 + 单测 + 复用给后续平台（无 connector 接入）
- **PR2** `im/wecom/` 目录骨架 + crypto.rs + 单测（圣经测试通过）
- **PR3** access_token 缓存 + sender API 调用 + 单测
- **PR4** `impl IMConnector for WecomConnector` + 接 webhook_server + ChannelMessage normalize
- **PR5** 媒体上传 + media_id 缓存
- **PR6** 流式降级（2 秒节流分批 markdown 发送）
- **PR7** 前端"添加企微"UI + 外网穿透引导 + 集成测试

## §6. 风险

| 风险 | 缓解 |
|---|---|
| 用户配置公网穿透 = 摩擦大 | UI 引导 + cloudflared 一键（后续 Polish）|
| 加解密算法实现错 → 502 全失败 | 必须先用官方示例向量打通单测，再做联调 |
| 6 分钟流式超时 | 2 秒分批 markdown 而非 edit 已发消息（企微无 edit API） |
| 单 server 端口被占（多 desktop 实例同时跑） | listen 失败时探测下一个端口（7800 / 7801 / 7802），UI 显示实际端口 |
| webhook_server 是 process-global → connector 之间状态共享风险 | `register()` 返回 RAII guard，drop 时反注册；测试覆盖 |

# Phase 4：WhatsApp Cloud API IM Connector

**日期**：2026-05-18
**状态**：Design draft → 待用户 review
**前置**：Phase 0 / 1 / 2 已落地（webhook_server 必备）
**Scope**：在 `connector/im/whatsapp/` 实现 IMConnector

## 背景

WhatsApp 是这一系列中**业务约束最重**的平台。Meta Cloud API 强制 webhook 入站、强制 24h 会话窗口、强制超窗口走预审 template。这意味着"AI 任意时间回复"在 WhatsApp 上**不成立**——需要 connector 内部把会话窗口约束作为一等公民。

## 调研结论（事实摘要）

参考：[developers.facebook.com/docs/whatsapp/cloud-api](https://developers.facebook.com/docs/whatsapp/cloud-api)

1. **入站**：仅 webhook（HTTPS POST）；开发者在 Meta App Dashboard 配置 webhook URL + verify token
2. **认证**：phone_number_id + access_token；需 Meta business account；临时 token 24h 失效，生产必须用 System User Access Token（长期）
3. **消息类型**：text / template（预审） / interactive（按钮/列表） / image (≤5MB) / document (≤100MB) / audio / video / sticker
4. **24h 会话窗口强约束**：用户发消息后 24h 内可 free-form 回复；24h 外**必须发 template**——free-form 直接被 API 拒
5. **流式更新**：**完全不支持**——Cloud API 没有 edit message endpoint
6. **媒体上传**：先 POST `/{phone_number_id}/media` (multipart) 拿 `media_id`，30 天有效；或用 URL 引用

## Non-Goals

1. **不**支持 24h 窗口外的 AI 实时回复——超窗口仅允许发预定义 template
2. 不实现 WhatsApp Business API（on-premise）——本期只支持 Cloud API
3. 不实现 interactive button / list 回复——只支持 text + image + document
4. 不做 template 的设计 / 提交 / 审批工具——template 在 Meta 后台手动创建
5. 不实现 sticker / video / audio 媒体类型

## §1. 24h 会话窗口（核心设计）

这是 WhatsApp connector 跟其它平台最大的差异，**必须**在 connector 内部建模：

```rust
struct WhatsAppSession {
    wa_id: String,                   // 对方手机号
    last_inbound_at: Instant,        // 用户最后一次发消息的时刻
}

impl WhatsAppConnector {
    fn can_send_freeform(&self, wa_id: &str) -> bool {
        match self.sessions.get(wa_id) {
            Some(s) => s.last_inbound_at.elapsed() < Duration::from_hours(24),
            None => false,    // 从未收到过 → 24h 窗外
        }
    }
}
```

### 1.1 入站消息更新 last_inbound_at

每条 webhook 入站消息，无论是否产生 reply：
```rust
self.sessions.entry(wa_id).or_default().last_inbound_at = Instant::now();
```

### 1.2 出站消息分两种路径

收到 `ReplyContent::Text(s)` / `Markdown(s)`：

```rust
async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<()> {
    if self.can_send_freeform(&target.wa_id) {
        // 24h 窗内 → 走 free-form text
        self.api_send_text(target.wa_id, normalize_text(content)).await
    } else {
        // 24h 窗外 → 必须用 template
        return Err(ConnectorError::Fatal(
            "WhatsApp 24h 窗口已过，无法发送自由文本。\
             如需主动联系，请在 Meta 后台预审 template 后通过 OPS 工具调用。"
        ));
    }
}
```

**注意**：这里 fail-fast 是有意的——本期 spec 不解决"AI 在窗外自动发 template"这种业务难题。让用户感知到这个约束，再决定怎么处理。后续可以加：
- 在 chat UI 提示"WhatsApp 会话已超时，等待用户回应"
- 提供"发送预定义 template `welcome_back`"按钮

### 1.3 流式 AI Card 降级

WhatsApp 不支持 edit。收到 `AiCardChunk`：

1. **不**尝试边写边发——会变成几十条独立消息刷屏
2. 在 connector 内 buffer 直到 `final_chunk=true`
3. 一次性 `api_send_text(buffer)`

如果中间发了"思考中..."这种 ack 提示，那也是独立消息，**且会被用户视为最终回复中的一部分**——对 UX 不友好。所以：
- **不**发任何思考中提示
- 用户从发消息到看到回复完整文本是"静默间隔"
- 配置项 `whatsapp.streaming_ack_enabled: false`（默认），如果用户开启则发一句 "Working on it, just a moment..."

## §2. 目录结构 + capabilities

```
src-tauri/src/connector/im/whatsapp/
├── mod.rs                  # impl IMConnector
├── runtime.rs              # 接 webhook_server + session 管理
├── webhook.rs              # webhook verify (challenge response) + signed payload 验证
├── sender.rs               # text / template / media send API
├── media.rs                # media upload + media_id 缓存
├── session.rs              # 24h 窗口跟踪
├── parser.rs               # webhook payload → ChannelMessage
├── download.rs             # 媒体下载
└── types.rs
```

```rust
ConnectorCapabilities {
    inbound: InboundModel::Webhook,
    outbound_aicard: false,
    outbound_markdown: false,        // 不支持 markdown
    supports_attachments: true,
    supports_group_chat: false,      // Cloud API 暂不支持 group
    supports_private_chat: true,
    auth_flow: AuthFlow::ApiKey,
}
```

注：`outbound_markdown: false`——WhatsApp 的"格式化文本"只支持简单的 `*粗体*` / `_斜体_`，不是完整 markdown。Connector 内 strip markdown 转 plain text。

## §3. Webhook verify

WhatsApp 首次配 webhook URL 时，Meta 会 GET 一次带 `?hub.challenge=xxx`，期待原样回 200。

```rust
register("/whatsapp/{phone_number_id}", |req| async move {
    if req.method == GET {
        // verify token 校验
        if req.query.get("hub.verify_token") == Some(self.verify_token) {
            return WebhookResponse::text(req.query.get("hub.challenge").unwrap());
        }
        return WebhookResponse::forbidden();
    }
    // POST: 解析 message event
    ...
});
```

### 3.1 Signed payload 校验

Meta webhook POST 带 `X-Hub-Signature-256: sha256=...`，是 `app_secret` 对 body 的 HMAC-SHA256。
verify 失败 → 401。**所有**入站 POST 必须校验，不可关。

## §4. 测试

- `webhook::tests`：verify challenge + signed payload HMAC（官方文档给的示例向量）
- `session::tests`：24h 窗口边界（23:59 OK / 24:01 拒绝）
- `sender::tests`：mock HTTP，free-form vs template 路径分支
- `tests/im_whatsapp_integration.rs`：起 connector + mock Meta webhook + 模拟 24h 跨越

## §5. 实施 PR 切分

- **PR1** `im/whatsapp/` 骨架 + types + verify token / phone_number_id 配置
- **PR2** webhook verify + HMAC 校验 + 单测（官方示例向量必须 pass）
- **PR3** parser.rs（webhook payload → ChannelMessage 含 wa_id）+ session.rs（24h 窗口跟踪）
- **PR4** sender free-form text + impl IMConnector（在窗内能发，窗外 Fatal）
- **PR5** media upload / send / download
- **PR6** template send（基础——只支持 send-by-name，不做 template 编辑）
- **PR7** 集成测试 + 前端"添加 WhatsApp" UI（含 24h 窗口说明文案）

## §6. 风险

| 风险 | 缓解 |
|---|---|
| 24h 窗口约束让 AI 助手体验"残缺" | spec 明确：仅作为客服 channel，不承诺主动外呼 |
| Template 必须预审 + 跨越业务流程 | 不在 connector 内做 template 设计；UI 提示用户去 Meta 后台 |
| Phone_number_id + access_token 在 Meta Dashboard 配置摩擦大 | 设置面板加链接 → Meta 文档 + 截图教程（截图工作量 PR7） |
| 用户实际测试时 24h 窗口难复现 | 提供"模拟窗口外"开关（dev only），强制 can_send_freeform=false |
| HMAC 校验错 → 所有 webhook 全失败 | 必须用官方示例向量打通单测；上线前用 Meta debugger 验证 |

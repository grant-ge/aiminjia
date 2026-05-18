# Phase 3：Telegram IM Connector

**日期**：2026-05-18
**状态**：Design draft → 待用户 review
**前置**：Phase 0 / 1 / 2 已落地（webhook_server 可复用）
**Scope**：在 `connector/im/telegram/` 实现 IMConnector

## 背景

Telegram Bot API 是设计最简单的现代 IM 接入之一：单 token 认证、双入站模式可选、消息类型标准。**适合作为"摸底 webhook + 长轮询"两种入站模式同时实现的练手平台**。

## 调研结论（事实摘要）

参考：[core.telegram.org/bots/api](https://core.telegram.org/bots/api)

1. **入站**：长轮询 `getUpdates` **或** webhook（HTTPS POST），二选一互斥；24h 内未消费的 update 暂存
2. **认证**：@BotFather 创建 bot → 拿 token 形如 `123456789:ABC...`；无 OAuth、无 device-code、无扫码
3. **消息类型**：text / photo (≤10MB) / document (≤50MB) / video / audio / voice / inline keyboard；文本支持 MarkdownV2 / HTML / Legacy Markdown
4. **流式更新**：`editMessageText` 可反复编辑已发消息；实际限频~1-3 次/秒每条消息，超限 429 + `retry_after`
5. **附件下载**：`getFile(file_id)` → `file_path` → `https://api.telegram.org/file/bot{TOKEN}/{file_path}`；最大 50MB
6. **rate limit**：全局 30 msg/秒；单 chat 1 msg/秒；群组/频道 20 msg/分钟；429 时 backoff

## Non-Goals

1. 不实现 Telegram 业务级别的特殊功能（inline mode / payment / passport / poll）
2. 不实现 Telegram MTProto 客户端（那是用户账号 API，违反 ToS 在 bot 场景下使用）
3. 不支持 ≥ 50MB 的文件（API 硬限，告诉用户"请用云盘链接"）
4. 不做 webhook 的 HTTPS 证书自管理——用户自负

## §1. 入站模式选择

**两种都实现，UI 让用户选**：

| 模式 | 优势 | 劣势 | 推荐场景 |
|---|---|---|---|
| **long-polling** | 无需公网；零配置 | 5-30s 延迟；消耗少量 Telegram 服务器配额 | 个人 bot / 测试 / 网络受限环境（推荐默认） |
| **webhook** | 实时；无轮询成本 | 需公网 HTTPS；要走 webhook_server | 生产 + 已配公网 |

**实现策略**：
- `TelegramConnector` 内部有 `inbound_mode: TelegramInboundMode { LongPoll, Webhook }` 字段
- `capabilities()` 仅声明 `InboundModel::Stream`（对 Manager 透明——长轮询本质上也是 stream）
- 用户切换模式 = 重启 connector

### long-polling 实现

```
loop {
    let updates = getUpdates(offset, timeout=25s).await;
    for u in updates {
        offset = u.update_id + 1;
        yield normalize(u);
    }
    // 异常网络抖 → Transient backoff（复用 shared/reconnect.rs）
}
```

`offset` 持久化到 `~/.renlijia/users/{scope}/channels/telegram/{bot_id}/state.json`，重启后接续。

### webhook 实现

复用 Phase 2 的 `im/shared/webhook_server.rs`：

```
register("/telegram/{bot_id}", handler)
on POST: 验证 secret_token (X-Telegram-Bot-Api-Secret-Token 头) → normalize → 推 mpsc
```

`secret_token` 在 `setWebhook` 时设置，**未授权请求 401**。

## §2. 目录结构 + capabilities

```
src-tauri/src/connector/im/telegram/
├── mod.rs                  # impl IMConnector
├── runtime.rs              # 入口；按 inbound_mode 分发到 long-poll 或 webhook
├── long_poll.rs            # getUpdates loop + offset 持久化
├── webhook.rs              # 接 webhook_server + secret_token 校验
├── sender.rs               # sendMessage / editMessageText / sendPhoto / sendDocument
├── streaming.rs            # AI 流式 → editMessageText 节流
├── parser.rs               # Update → ChannelMessage
├── download.rs             # getFile + 下载二进制
└── types.rs
```

```rust
ConnectorCapabilities {
    inbound: InboundModel::Stream,    // 对 Manager 看是 stream，内部可能是 webhook
    outbound_aicard: false,
    outbound_markdown: true,           // 走 MarkdownV2
    supports_attachments: true,
    supports_group_chat: true,
    supports_private_chat: true,
    auth_flow: AuthFlow::ApiKey,
}
```

## §3. 流式更新（editMessageText 节流）

收到 `ReplyContent::AiCardChunk { delta, final_chunk }`：

1. 首 chunk：`sendMessage(text=delta)` → 拿 `message_id`，存 `HashMap<chat_turn_id, TgStreamSession>`
2. 后续 chunk：**节流 1 次/秒**（保守值，远低于 ~1-3/s 限频）→ `editMessageText(message_id, full_text)`
3. final：再 edit 一次完整版；清掉 session
4. **429 retry_after** 处理：所有写 API 都包一层重试，按 server 返回的 `retry_after` 等待

### MarkdownV2 转义

Telegram MarkdownV2 要转义 `_*[]()~\`>#+-=|{}.!`，规则细。新建 `escape_markdown_v2(text: &str) -> String` 纯函数 + 全量字符集单测。

## §4. 错误处理

| Telegram error | 映射 |
|---|---|
| 429 (rate limit) | `Transient`，按 `retry_after` backoff |
| 401 (invalid token) | `AuthExpired` → 强制重输 token |
| 400 (bad request) | `Fatal(msg)` 上抛 |
| 403 (bot 被踢) | 记录该 chat 进黑名单，不再发，但 connector 仍工作 |
| 500-504 | `Transient` |

## §5. 测试

- `escape_markdown_v2::tests`：所有特殊字符 + 用户输入示例
- `long_poll::tests`：mock HTTP，offset 推进正确性 + 异常重连
- `webhook::tests`：secret_token 校验 + 非法请求 401
- `streaming::tests`：429 时按 retry_after backoff + edit 节流 1次/秒
- `tests/im_telegram_integration.rs`：起 Manager + TelegramConnector 两种 inbound mode 各跑一遍

## §6. 实施 PR 切分

- **PR1** `im/telegram/` 骨架 + capabilities + impl IMConnector 空壳 + token 配置 UI
- **PR2** sender.rs + parser.rs + MarkdownV2 转义 + 单测
- **PR3** long_poll.rs + offset 持久化 + 自动重连
- **PR4** webhook.rs + secret_token 校验 + 接 webhook_server
- **PR5** streaming.rs (editMessageText 节流 + 429 backoff)
- **PR6** download.rs + 附件接收
- **PR7** 集成测试 + 前端"添加 Telegram" UI（输入 bot_token + �� inbound mode）

## §7. 风险

| 风险 | 缓解 |
|---|---|
| MarkdownV2 转义漏字符 → 消息发送 400 | 全字符单测 + 失败时 fallback 到 plain text 重试 |
| 流式 edit 触发 429 频繁 | 节流给到 1 次/秒（保守 3x 余量） |
| 用户切换 inbound mode 时已注册 webhook 未清 | 切换前调 `deleteWebhook` 清理 |
| 多 bot 同账户配置 | 不支持单 connector 多 bot；每个 bot 独立 connector 实例 |
| bot token 误泄漏（出现在日志） | 复用 dingtalk 已有的"secret 自动脱敏"机制（如尚未抽到 shared，本 PR 顺手抽） |

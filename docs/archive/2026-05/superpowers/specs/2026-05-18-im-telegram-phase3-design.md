# Phase 3：Telegram IM Connector

**日期**：2026-05-18（修订于 review 后；v4：删除 webhook 模式，仅保留 long-polling）
**状态**：Design draft v4 → 可独立执行
**前置**：Phase 0（trait + shared/* 已落）已完成；不依赖 Phase 1 / Phase 2 任何 PR
**Scope**：在 `connector/im/telegram/` 实现 IMConnector（**仅长轮询入站**）+ 在 trait 层引入 `outbound_text_streaming` capability + 重命名 `InboundModel` → `InboundDeployment`

## 背景

Telegram Bot API 是设计最简单的现代 IM 接入之一：单 token 认证、`getUpdates` 长轮询入站、消息类型标准。**本 spec 仅实现长轮询模式**——零公网、零穿透、零端口转发，桌面 app 只需出站访问 `api.telegram.org` 即可工作，对个人 / 中小用户摩擦最低。

用户接入路径：在 Telegram 里 `@BotFather` → `/newbot` → 拿 token → 填进桌面 app → 自动 `getUpdates` 收消息。

Telegram 同时暴露 Phase 0 trait 设计的一个概念错乱：

1. `outbound_aicard: bool` 不足以描述"text 流式（editMessageText）"——Telegram 不支持富卡片，但支持真正的文本流式视觉效果

> **`InboundModel` 重命名理由保留**：虽然本 spec 只用长轮询，但 Phase 4 WhatsApp 必然走 webhook，命名提前对齐是合算的；详见 §0.2。

本 spec 含一个 trait 改造 PR（**PR1.5**），同时支撑 Phase 4 (WhatsApp) / Phase 5 (微信) 后续接入。

> **webhook 模式（已删除）**：v3 包含 webhook 入站 + `im/shared/webhook_server.rs`，v4 全部移除。理由：① 桌面 app 默认无公网，webhook 必须配 ngrok / cloudflared / 反代，对绝大多数用户摩擦过高 ② Telegram 长轮询延迟 < 1s（25s long-poll，新消息立即返回），实时性足够 ③ webhook_server 这块基础设施移交给 Phase 4 WhatsApp（强制 webhook，没得选）spec 自带，本 spec 不预先垫付。

## 调研结论（事实摘要）

参考：[core.telegram.org/bots/api](https://core.telegram.org/bots/api)

1. **入站**：长轮询 `getUpdates`（HTTPS 出站请求，桌面 app 主动拉）；24h 内未消费的 update 暂存
2. **认证**：@BotFather 创建 bot → 拿 token 形如 `123456789:ABC...`；无 OAuth、无 device-code、无扫码
3. **消息类型**：text / photo (≤10MB) / document (≤50MB) / video / audio / voice / inline keyboard；文本支持 MarkdownV2 / HTML / Legacy Markdown
4. **流式更新**：`editMessageText` 可反复编辑已发消息；实际限频 ~1-3 次/秒每条消息，超限 429 + `retry_after`
5. **附件下载**：`getFile(file_id)` → `file_path` → `https://api.telegram.org/file/bot{TOKEN}/{file_path}`；最大 50MB
6. **rate limit**：全局 30 msg/秒；单 chat 1 msg/秒；群组/频道 20 msg/分钟；429 时 backoff

## Non-Goals

1. 不实现 Telegram 业务级别的特殊功能（inline mode / payment / passport / poll）
2. 不实现 Telegram MTProto 客户端（那是用户账号 API，违反 ToS 在 bot 场景下使用）
3. 不支持 ≥ 50MB 的文件（API 硬限，告诉用户"请用云盘链接"）
4. **不实现 webhook 入站**——v4 显式移除；桌面 app 默认无公网，长轮询足够
5. **不支持单 connector 多 bot**；用户加 5 个 bot = UI 添加 5 个 Telegram 账号 = 5 个 connector 实例（5 个 long-poll 协程）

## 依赖关系

```
Phase 0（已落地）：
  trait_def.rs / IMConnector / shared/{dedup, reconnect, token, config_store, ask_coordinator,
  pending_adapter, reply_manager, router} 全部就绪
  ReplyTarget 已平台中性（PR0d）

不依赖 Phase 1 / Phase 2 任何 PR：
  - 飞书 connector 是否完成不影响（PR1.5 改 trait 时会顺手补它的字段）
  - 删除了 webhook 模式后，本 phase 不产 webhook_server——这块基础设施移交给 Phase 4 WhatsApp spec

Phase 3 内部：
  PR1 (骨架 + MarkdownV2 转义 + Platform::Telegram) ─┬─→ PR2 (sender/parser) ──┬─→ PR3 (long-poll)
  PR1.5 (trait 改造)                                 ─┘                          ├─→ PR5 (streaming editMessageText)
                                                                                 └─→ PR6 (download)
  PR6.5 (log 脱敏) 独立可与任何 PR 并行
  PR7 (前端 + 集成测试 + 黑名单持久化) 依赖所有
```

## §0. trait 改造（PR1.5）

### 0.1 capabilities 新增 `outbound_text_streaming: bool`

```rust
pub struct ConnectorCapabilities {
    pub inbound: InboundDeployment,       // 改名（见 §0.2）
    pub outbound_aicard: bool,             // 富卡片流式（dingtalk AI Card / 飞书 CardKit）
    pub outbound_text_streaming: bool,     // 新增：纯文本/markdown 真流式（Telegram editMessageText）
    pub outbound_markdown: bool,
    pub supports_attachments: bool,
    pub supports_group_chat: bool,
    pub supports_private_chat: bool,
    pub auth_flow: AuthFlow,
}
```

各平台映射：

| 平台 | aicard | text_streaming | 含义 |
|---|---|---|---|
| dingtalk | true | false | 富卡片流式，无纯文本流式 |
| feishu | true | false | CardKit 流式，无纯文本流式 |
| wecom | false | false | 用 aicard_fallback buffer：占位 + 最终 |
| **telegram** | false | **true** | editMessageText 真流式 |
| whatsapp | false | false | 无任何流式（fallback buffer） |
| wechat (个微) | false | false | 无任何流式（fallback buffer） |

### 0.2 `InboundModel` 重命名为 `InboundDeployment`

```rust
pub enum InboundDeployment {
    /// 用户本地能跑，不需要公网入口（dingtalk WS / feishu WS / telegram long-poll / wechat iLink 长轮询）
    SelfHosted,
    /// 需要公网 HTTPS 入口（whatsapp）
    PublicWebhook,
    // NativeDaemon 变体已删除：Phase 5 调研结论表明个微走 iLink HTTP 长轮询（SelfHosted），
    // 不需要外部 native daemon。如未来出现真正的 PC 客户端 daemon 接入场景再加回。
    // 详见 Phase 5 spec §11 反向修订。
}
```

各平台映射：

| 平台 | InboundDeployment | UI 行为 |
|---|---|---|
| dingtalk | SelfHosted | 不引导穿透 |
| feishu | SelfHosted | 不引导穿透 |
| wecom | SelfHosted (aibot WS) | 不引导穿透 |
| **telegram** | **SelfHosted（long-poll）** | 不引导穿透 |
| whatsapp | PublicWebhook | 引导穿透 |
| wechat | SelfHosted（iLink 长轮询） | 不引导穿透；需扫码登录（AuthFlow::QRCode） |

### 0.3 Telegram capabilities（常量返回）

Telegram 入站固定 long-poll，capabilities 是常量：

```rust
fn capabilities(&self) -> ConnectorCapabilities {
    ConnectorCapabilities {
        inbound: InboundDeployment::SelfHosted,
        outbound_aicard: false,
        outbound_text_streaming: true,
        outbound_markdown: true,
        supports_attachments: true,
        supports_group_chat: true,
        supports_private_chat: true,
        auth_flow: AuthFlow::ApiKey,
    }
}
```

### 0.4 影响面（migration checklist）

PR1.5 内部要做的字段添加 / 重命名：

- `trait_def.rs`：加 `outbound_text_streaming` + 重命名 enum
- `dingtalk/connector.rs`：补 `outbound_text_streaming: false`
- 已存在的 review test：搜索 `InboundModel::` 改 `InboundDeployment::`
- Phase 1 飞书 connector（已 merge PR1 stub，capabilities 已发布）：同样补字段 + 改 `InboundModel::Stream` → `InboundDeployment::SelfHosted`
- Phase 2 wecom connector（若已 merge）：补字段 + 改 `InboundModel::Webhook` → `InboundDeployment::PublicWebhook`；若未 merge，写 wecom 时直接用新 enum

注：`types.rs` 的 `Platform` enum 当前只有 `Dingtalk/Feishu/Wechat/Wecom`，**PR1 须新增 `Telegram` 变体**（含 `as_str()` / `from_str()` 两处分支）。这步并入 PR1 而非 PR1.5。

### 0.5 测试

- `trait_def::tests::capabilities_can_be_constructed` 加 `outbound_text_streaming` 字段验证
- `review_im_layering` 不需要改
- 不需要新加测试——既有 connector tests 覆盖即可

## §1. 入站：long-polling

Telegram 桌面接入唯一入站模式：`getUpdates` 长轮询。零公网、零穿透。

| 指标 | 值 |
|---|---|
| 长轮询 timeout | 25s（API 上限 50s，留余量） |
| 新消息到达时延 | ~瞬时（Telegram 服务端有新 update 立即返回） |
| 无消息时返回间隔 | 25s（timeout 命中），然后立即下一轮 |
| 重连退避 | 复用 `shared/reconnect.rs::ReconnectBackoff` 的 5/15/30/60s ladder |

### 1.1 long-polling 实现

```rust
loop {
    let updates = client.get_updates(offset, timeout=25s).await;
    match updates {
        Ok(list) => {
            for u in list {
                offset = u.update_id + 1;
                msg_tx.send(normalize(u)).await?;
            }
            self.flush_offset_if_needed(offset).await;  // §1.2
        }
        Err(transient) => {
            // 复用 shared/reconnect.rs 的 ReconnectBackoff
            tokio::time::sleep(backoff.next_delay()).await;
            continue;
        }
    }
    if ctx.cancel_token.is_cancelled() { break; }
}
self.flush_offset(offset).await;  // 优雅终止时强制 flush
```

### 1.2 offset 持久化策略

| 时机 | 行为 |
|---|---|
| 每条 update 处理完 | 内存 offset 实时更新 |
| 每 5s 或每 10 条 update | 批量 fsync 到 `~/.renlijia/users/{scope}/channels/telegram/{bot_id}/state.json` |
| connector stop / cancel | 强制 flush |
| 启动时 | 从 state.json 读 offset；缺失则用 0（拉所有未消费 update） |

崩溃丢 offset 最差情况：~5s 内 update 重复消费。配套 `update_id` dedup（**复用** Phase 0 的 `shared::dedup::MessageDedupSet`，不再写一份）。


## §2. 目录结构 + capabilities

```
src-tauri/src/connector/im/telegram/
├── mod.rs                  # impl IMConnector
├── long_poll.rs            # getUpdates loop + offset 持久化 + dedup
├── sender.rs               # sendMessage / editMessageText / sendPhoto / sendDocument
├── streaming.rs            # AI 流式 → editMessageText 节流 + 429 backoff
├── parser.rs               # Update → ChannelMessage
├── escape.rs               # MarkdownV2 转义纯函数
├── download.rs             # getFile + 下载二进制 + 50MB 拒绝
├── blacklist.rs            # 403 黑名单（持��化 + 24h TTL）
└── types.rs
```

```rust
ConnectorCapabilities {
    inbound: InboundDeployment::SelfHosted,
    outbound_aicard: false,
    outbound_text_streaming: true,        // 走 editMessageText 真流式
    outbound_markdown: true,              // MarkdownV2
    supports_attachments: true,
    supports_group_chat: true,
    supports_private_chat: true,
    auth_flow: AuthFlow::ApiKey,
}
```

## §3. 流式更新（editMessageText 节流，PR5）

收到 `ReplyContent::AiCardChunk { delta, final_chunk }` 时（Telegram outbound_text_streaming=true）：

**关键：delta 是增量，connector 内部累积成 full_text 才能调 editMessageText**。

```rust
struct TgStreamSession {
    chat_id: i64,
    message_id: i64,
    accumulated: String,
    last_edit_at: Instant,
    last_edit_text: String,    // 上次发出去的，用来 dedup 不变内容（避免空 edit）
}

async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
    match content {
        ReplyContent::Text(t) | ReplyContent::Markdown(t) => {
            self.sender.send_message(&target, &t).await
        }
        ReplyContent::AiCardChunk { delta, final_chunk } => {
            let mut sessions = self.stream_sessions.lock().await;
            let session = sessions.entry(target.session_id.clone()).or_insert_with(|| async {
                // 首 chunk：sendMessage 拿 message_id
                let msg = self.sender.send_message(&target, &delta).await?;
                TgStreamSession { chat_id: msg.chat_id, message_id: msg.message_id, accumulated: delta.clone(), ... }
            });

            session.accumulated.push_str(&delta);

            let should_edit = final_chunk
                || (session.last_edit_at.elapsed() >= Duration::from_secs(1));

            if should_edit && session.accumulated != session.last_edit_text {
                let escaped = escape_markdown_v2(&session.accumulated);
                match self.sender.edit_message_text(session.chat_id, session.message_id, &escaped).await {
                    Ok(()) => {
                        session.last_edit_text = session.accumulated.clone();
                        session.last_edit_at = Instant::now();
                    }
                    Err(TgError::TooManyRequests { retry_after }) => {
                        // 429：按 retry_after sleep 后**不**重试本次 edit
                        // （下一个 chunk 自然会再尝试，accumulated 已包含本次内容）
                        tokio::time::sleep(retry_after).await;
                    }
                    Err(TgError::BadRequest(msg)) if msg.contains("can't parse entities") => {
                        // MarkdownV2 解析失败 → fallback 到 plain text 重试一次
                        let _ = self.sender.edit_message_text_plain(session.chat_id, session.message_id, &session.accumulated).await;
                    }
                    Err(other) => return Err(map_error(other)),
                }
            }

            if final_chunk {
                sessions.remove(&target.session_id);
            }
            Ok(())
        }
    }
}
```

### 3.1 节流参数

- **edit 间隔**：1 秒/次（Telegram 单 chat 限频 1 msg/s，保守 1x 余量）
- **429 retry_after**：服务器返回的 `retry_after` 秒数 sleep 后让下一个 chunk 自然重试
- **MarkdownV2 解析失败**：fallback 到 plain text，不让用户看到崩坏的输出

### 3.2 MarkdownV2 转义

```rust
pub fn escape_markdown_v2(text: &str) -> String { ... }
```

转义字符集（来自 Telegram 官方文档）：`_*[]()~\`>#+-=|{}.!`

测试覆盖：
- 每个特殊字符单独转义
- 已经被转义的字符不二次转义（`\\_` → `\\_`，不变成 `\\\\_`）
- 多字符组合（`hello _world_!` 期望 `hello \\_world\\_\\!`）
- 极端情况：空字符串、全特殊字符、Unicode（emoji + 中文）

### 3.3 final 后 session 清理 / 下次 AI 回复

final 触发后清掉 session。下一条 AI 回复 → 新建 message → 新 message_id —— 跟钉钉 AI Card 每次新建一张卡同形。

## §4. 错误处理

| Telegram error | 映射 |
|---|---|
| 429 (rate limit) | `Transient`，按 `retry_after` backoff |
| 401 (invalid token) | `AuthExpired` → 强制重输 token |
| 400 (bad request, 含 parse_mode) | MarkdownV2 → plain text fallback；其它 `Fatal(msg)` 上抛 |
| 403 (bot 被踢) | 记录该 chat 进 `blacklist.rs`（持久化，24h TTL），不再发，但 connector 仍工作 |
| 500-504 | `Transient` |

### 4.1 403 黑名单设计（PR2 / blacklist.rs）

- 持久化到 `~/.renlijia/users/{scope}/channels/telegram/{bot_id}/blacklist.json`
- 数据形如 `{ chat_id: -10012345678, blacklisted_at: "2026-05-18T15:30:00Z" }`
- 24h TTL：每次 send 前检查 `blacklisted_at + 24h > now`；过期则解除
- TTL 理由：用户可能把 bot 重新拉回群

## §5. 日志脱敏（PR6.5，独立 PR）

`mask_secret` 已在 `shared/config_store.rs`，但**那是 config UI 展示用的**。日志 / 调试 / 上报里 token 仍可能裸露。

新增 `im/shared/secret_string.rs`：

```rust
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: String) -> Self { SecretString(s) }
    pub fn expose(&self) -> &str { &self.0 }  // 只允许显式调用
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", mask_secret(&self.0))  // 复用既有 mask_secret
    }
}

impl std::fmt::Display for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", mask_secret(&self.0))
    }
}
```

Telegram PR1 之后**所有平台的 token / secret 字段都改为 SecretString**（dingtalk app_secret / feishu app_secret / wecom corp_secret / telegram bot_token / whatsapp access_token / wechat session_key），保证 `log::info!("config: {:?}", config)` 不再泄密。

PR6.5 独立并行——可在 Phase 1/2/3 任何阶段做。Telegram PR1 在 spec 里依赖 SecretString 是为了减少后续重构成本，但**可以先用 String 占位**，PR6.5 落地后再 sweep 替换。

## §6. 测试

- `escape::tests`：所有特殊字符 + 用户输入示例 + Unicode + 空字符串 + 已转义不二次转义
- `long_poll::tests`：mock HTTP，offset 推进正确性 + 异常重连 + offset 持久化时机（5s / 10 条触发 fsync）
- `webhook` 测试已删除（v4 不实现 webhook）
- `streaming::tests`：editMessageText 节流 1 次/秒 + 429 retry_after backoff + MarkdownV2 fallback
- `blacklist::tests`：24h TTL + 重启加载 + 边界（恰好 24h）
- `tests/im_telegram_integration.rs`：起 Manager + TelegramConnector
  - mock getUpdates 服务器，收 3 条消息（含 text / photo / 群消息）
  - cancel_token 触发后 2s 内 loop 退出 + offset 强制 flush（trait 契约 §6）
- `tests/review_im_layering.rs`：`platforms` 数组追加 `"telegram"`

## §7. 实施 PR 切分（修订）

可独立执行——不依赖 Phase 1 / Phase 2 任何 PR。

- **PR1.5** trait 改造：加 `outbound_text_streaming` + 重命名 `InboundModel`→`InboundDeployment`；扫已 merge 的 dingtalk / feishu / wecom connector 补字段（Phase 0 收尾性质）
- **PR6.5** `im/shared/secret_string.rs` + 全平台 sweep 替换 token/secret 字段类型（独立可并行）

Telegram 主链路：

- **PR1** `im/telegram/` 骨架 + capabilities 占位 + `types.rs` 加 `Platform::Telegram` 变体（含 `as_str()`/`from_str()` 分支）+ `escape.rs` MarkdownV2 转义 + 单测 + token 配置 UI（前端 stub）；占位字段先用 String，PR6.5 后 sweep 为 SecretString
- **PR2** `sender.rs`（不含 streaming）+ `parser.rs` + `blacklist.rs` + 错误码映射
- **PR3** `long_poll.rs` + offset 持久化 + 重连 backoff + dedup
- **PR5** `streaming.rs` editMessageText 节流 + 429 backoff + plain text fallback
- **PR6** `download.rs` + 附件接收 + 50MB 拒绝
- **PR7** 集成测试 + 前端 UI（输入 bot_token + 显示 bot 信息）+ `review_im_layering` 加 telegram

> 跳过的 PR 号（PR3.5 / PR4）：v3 中是 webhook 相关，v4 已删除；不重新编号以保留与之前 review 讨论的对应关系。

## §8. 风险

| 风险 | 缓解 |
|---|---|
| MarkdownV2 转义漏字符 → 消息发送 400 | 全字符单测 + 失败时 fallback 到 plain text 重试（§3.1） |
| 流式 edit 触发 429 频繁 | 节流 1 次/秒（限频 3x 余量）；429 retry_after 自动等待 |
| 长轮询 loop 退出时 offset 未 flush → 重启重复消费 | cancel 路径强制 flush + 启动时复用 `MessageDedupSet` 兜底 |
| `api.telegram.org` 国内访问不稳定 | 出错走 `ReconnectBackoff` 5/15/30/60s ladder；UI 显示连接状态供用户判断是否要开代理 |
| bot token 出现在日志 | PR6.5 引入 SecretString newtype + 全平台 sweep 替换 |
| 403 黑名单重启清空 → 浪费 API 配额 | 持久化 + 24h TTL（§4.1） |
| **PR1.5 trait 改造影响已 merge 的 dingtalk/feishu/wecom** | PR1.5 一次性扫所有 connector 补字段；CI 编译失败会立刻指路；尽早合并避免分支冲突 |
| Telegram 限制 50MB 文件 | Non-Goals 显式声明；UI 在附件发送前检查 size，超限提示用户用云盘 |

## §9. 估时

- PR1.5（trait 改造）：0.5 天（含扫描已 merge connector 补字段）
- PR6.5（SecretString）：1 天（newtype + 全平台 sweep + 测试）
- PR1（骨架 + escape + Platform::Telegram）：1 天
- PR2（sender + parser + blacklist + 错误码）：1.5 天
- PR3（long-poll + offset 持久化 + dedup）：1.5 天
- PR5（streaming + 节流 + 429 + fallback）：1.5 天
- PR6（download）：0.5 天
- PR7（前端 + 集成测试）：1 天

**总计：~8.5 天单人**

PR1.5 / PR6.5 可与主链路并行 → 工期上节省 ~1.5 天 → 实际 Phase 3 工期 **~7 天**。

## §10. trait 改造跨 Phase 影响汇总

PR1.5 一次性影响：

| 文件 | 改动 |
|---|---|
| `connector/im/trait_def.rs` | 加 `outbound_text_streaming: bool`；重命名 `InboundModel` enum + 变体语义 |
| `connector/im/dingtalk/connector.rs` | capabilities() 加 `outbound_text_streaming: false`；改 `InboundModel::Stream` → `InboundDeployment::SelfHosted` |
| `connector/im/feishu/connector.rs`（已 merge PR1 stub） | 同上 |
| `connector/im/wecom/connector.rs`（如已 merge） | 同上 + 改 `InboundModel::Webhook` → `InboundDeployment::PublicWebhook`（或按 wecom 实际入站方式定）|
| 前端 `src/types/channel.ts` | 加 `outboundTextStreaming: boolean`；改 inbound 枚举值 |
| 前端 channel UI | 根据 `inbound === 'PublicWebhook'` 显示穿透引导（仅 whatsapp 等真 webhook 平台触发） |
| `tests/review_im_layering.rs` | 不需要改 |

CI 编译会指路所有调用点。PR1.5 必须**一次性合并**，不要分多 PR——半完成状态会让其它 connector 编译失败。

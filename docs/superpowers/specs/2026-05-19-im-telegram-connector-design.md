# Telegram IM Connector — MVP（扫码配对 + 私聊）

**日期**：2026-05-19
**状态**：Design final → 可执行
**前置 spec**：`2026-05-18-im-telegram-phase3-design.md` v4（基础调研沿用，本 spec 是其 MVP 子集）
**Scope**：在 `connector/im/telegram/` 实现 IMConnector + 扫码配对（pairing code via deep-link） + 私聊；不做流式 / 不做群聊 / 不做附件 / 不做 SecretString newtype

## 背景

Phase 3 spec v4 给出了完整 Telegram 接入的设计，含 7 个 PR 的链条（含流式 editMessageText、blacklist、SecretString 等独立工程）。本 spec 把范围收敛到对非开发者用户**摩擦最低**的接入路径：

1. 用户去 @BotFather 拿 bot token（这一步绕不开，Telegram 不开放程序化建 bot）
2. AIjia 桌面端输入 token → 后端 `getMe` 验证 → 立即生成扫码二维码（`https://t.me/<bot_username>?start=<pairing_code>`）
3. 用户用手机 Telegram 扫码 → 跳转打开 bot → Telegram 自动发 `/start <pairing_code>`
4. AIjia 桌面端实时显示「待批准用户」→ 用户点「批准」→ 该 Telegram 账号加入 allowlist
5. 后续该用户私聊 bot → AIjia 用 markdown 回复

这等价于「微信扫码登录」的体验：用户看到一张二维码，扫一下，桌面端就 ready。与微信不同的是 Telegram 不开放 MTProto 给 bot 场景，必须先拿一次 token（非开发者首次接入会有 ~2 分钟障碍，但只发生一次）。

## 调研依据

参考 spec `2026-05-18-im-telegram-phase3-design.md` v4 的事实摘要。补充：

- **deep-link `/start` 协议**：[core.telegram.org/bots/features#deep-linking](https://core.telegram.org/bots/features#deep-linking) —— Bot username + `?start=<param>` 把 param 透传给 bot 的第一条 `/start` 消息。是 Telegram 官方推荐的「把外部 token 关联到内部 session」机制
- **pairing 路径参考**：[OpenClaw Telegram channel docs](https://docs.openclaw.ai/channels/telegram) —— `dmPolicy: pairing` 模式

## Non-Goals（明确不做）

1. 流式输出 editMessageText（保留给后续 PR；不做 trait 改造 PR1.5）
2. 群聊 / 频道（只私聊；群里的 Update 直接忽略）
3. 附件接收 / 发送（用户发图片给 bot 暂不处理；assistant 也只发文本 markdown）
4. SecretString newtype（继续用 String 占位）
5. 403 黑名单持久化（依赖运行时检测，被踢即移出 allowlist）
6. webhook 入站（永远不做；桌面 app 没公网）
7. MTProto 用户账号 API（违反 Telegram ToS）
8. 代理 URL UI（用户自行配系统代理或 HTTP_PROXY env）

## §1. 用户旅程

### 1.1 配置入口

频道页 Telegram 卡片 capability 从 `comingSoon` → `available`，点「配置」打开 TelegramChannelConfig 弹窗。

### 1.2 弹窗 step 1：Token 输入

- 顶部「配置 Telegram」标题 + 副文案「在 Telegram 找到 @BotFather 创建 bot，把 token 粘到这里」
- 一个 Input 框 + 一个「打��� BotFather」按钮（`open_external('https://t.me/BotFather')`）
- 「下一步」按钮 disabled 直到 token 非空
- 折叠区「使用帮助」展开后给出 5 步说明（参考 WecomChannelConfig 的 HelpPanel）

点「下一步」→ 调 `channel_telegram_save(token)` → 后端 getMe → 拿到 bot_username → 落盘 → 自动 connect → 弹窗切到 step 2。

### 1.3 弹窗 step 2：扫码配对

- 顶部展示 bot 头像占位 + bot 名称 + `@bot_username`
- 中央 QR 二维码（内容是 `https://t.me/<bot_username>?start=<pairing_code>`），白底固定 256x256
- 二维码下方倒计时「二维码 4:53 后过期」
- 倒计时归零自动刷新 code
- 二维码下方「无法扫码？在浏览器打开」按钮（同样的链接，`open_external`）

当 pending pairing 列表非空时：

- 二维码下方变成「✓ 张三 想要连接你的 bot」+ 「批准」/「拒绝」按钮
- 批准成功 → bot 向用户发欢迎消息 → 「✓ 已连接 1 个用户」状态
- 「继续添加」按钮回到 QR 显示，「完成」按钮关闭弹窗

### 1.4 弹窗 step 3：详情 / 管理

已配置状态下点卡片「···」→「配置」打开 step 3：

- bot 信息卡（avatar 占位 + name + @username + masked token）
- 已配对用户列表，每个用户右侧「移除」按钮
- 「添加更多用户」按钮回到 step 2 QR
- 底部「移除整个 Telegram 频道」按钮（destructive）

## §2. 后端架构

### 2.1 模块结构

```
src-tauri/src/connector/im/telegram/
├── mod.rs                # public exports
├── connector.rs          # impl IMConnector (start/send), session_targets
├── api.rs                # reqwest client: getMe / getUpdates / sendMessage / answerCallbackQuery
├── long_poll.rs          # getUpdates loop + offset 持久化 + ReconnectBackoff
├── parser.rs             # Update → ChannelMessage（识别 /start <code>）
├── sender.rs             # sendMessage markdown + 429 retry + parse_mode fallback
├── pairing.rs            # PairingCodeStore（in-memory） + allowlist 写盘
├── registration.rs       # begin_pairing / list_pending / approve / revoke
├── reply_forwarder.rs    # 订阅 RuntimeEventBus → connector.send(Markdown)
└── types.rs              # TelegramStoredConfig
```

跟 wecom 完全同构：连模块名都对得上。

### 2.2 capabilities

```rust
ConnectorCapabilities {
    inbound: InboundModel::Stream,        // long_poll 也走 Stream 变体（语义上是 self-hosted）
    outbound_aicard: false,               // 不做富卡片
    outbound_markdown: true,              // sendMessage parse_mode=MarkdownV2
    supports_attachments: false,          // MVP 关闭
    supports_group_chat: false,           // MVP 关闭
    supports_private_chat: true,
    auth_flow: AuthFlow::ApiKey,
}
```

> 注：暂不引入 `outbound_text_streaming` / `InboundDeployment` 重命名（Phase 3 v4 §0），保持兼容；待后续做流式 PR 时再做 trait 改造。

### 2.3 数据 schema

`users/{scope}/channels/telegram/config.json`:

```json
{
  "schemaVersion": 1,
  "platform": "telegram",
  "configured": true,
  "enabled": true,
  "credentials": {
    "botTokenEncrypted": "...",
    "botTokenStorage": "secureStorage"
  },
  "bot": {
    "botId": "8123456789",
    "botUsername": "my_aijia_bot",
    "botFirstName": "我的 AI 小家"
  },
  "allowlist": [
    {
      "userId": 12345,
      "firstName": "张三",
      "username": "zhangsan",
      "pairedAt": "2026-05-19T10:30:00Z"
    }
  ],
  "metadata": { "createdAt": "...", "updatedAt": "..." }
}
```

`users/{scope}/channels/telegram/state.json`:

```json
{ "lastOffset": 12345, "savedAt": "2026-05-19T10:30:05Z" }
```

PairingCodeStore：仅内存（重启清空，用户重新生成 code 即可），形如 `HashMap<String, PendingPairing>`：

```rust
struct PendingPairing {
    code: String,
    created_at: Instant,
    expires_at: Instant,
    pairer: Option<PairerInfo>,  // 用户扫码后填充
}

struct PairerInfo {
    user_id: i64,
    first_name: String,
    username: Option<String>,
}
```

### 2.4 主链路

```
启动 → ChannelManager::auto_connect_if_configured()
       → read_telegram_config() → 找到已 enabled config
       → connect_telegram(config, token)
          → register_telegram_connector(token, on_status)
          → connector.start(ctx) → BoxStream<ChannelMessage>
          → spawn long_poll task：
              GET /bot<token>/getUpdates?offset=N&timeout=25&allowed_updates=["message"]
              for each Update:
                ├─ if msg.text == "/start <code>":
                │    pairing.attempt_attach(code, msg.from) → pending pairing
                │    回 "等待 AIjia 桌面端批准" / "/start" 缺 code 时回提示
                │
                ├─ if msg.from.id ∈ allowlist && conversation_type == private:
                │    parser → ChannelMessage → mpsc → manager worker → chat_turn → assistant 回复
                │
                ├─ if msg.from.id ∉ allowlist (private 且非 /start):
                │    回 "请先在 AIjia 里完成扫码配对"
                │
                └─ else: 丢弃（群聊 / channel / 缺 from）
       → spawn pump task：收 ChannelMessage → manager 标准 worker

出站 → TelegramReplyForwarder 订阅 RuntimeEventBus
       → MessagePersisted(role=assistant)
       → 走 connector.has_session 过滤
       → connector.send(ReplyTarget, Markdown(text))
       → sender::send_message(chat_id, escape_markdown_v2(text), parse_mode=MarkdownV2)
          → 429 → sleep(retry_after) 重试一次
          → 400 "can't parse entities" → 用 plain text 重发（parse_mode 不传）
          → 403 → user_id 移出 allowlist + 标 ConfigError + 留 last_error
```

### 2.5 Pairing 详细协议（OpenClaw 同款手动 approve）

桌面端用户在 step 2：

1. 调 `channel_telegram_begin_pairing()` → 后端生成 8 字符 base32 code（`A-Z`, `2-9`，去歧义字符）→ 存进 PairingCodeStore，TTL 5 分钟 → 返回 `{ code, deep_link: "https://t.me/<bot_username>?start=<code>", expires_in_seconds: 300 }`
2. 前端渲染 QR + 倒计时
3. 前端每 2s 轮询 `channel_telegram_list_pending_pairings()` → 返回 `Pairing[]`（含已扫的，未 approve 的）

用户扫码（Telegram bot 收到 `/start <code>`）：

- bot 端解析出 code → `PairingCodeStore::attempt_attach(code, pairer_info)`
  - code 不存在 / 过期：bot 回「码已失效，请回到 AIjia 重新生成」
  - code 已被绑定（pairer 已填）：bot 回「码已被使用」（如果绑定到同一 user_id 则改回「等待 AIjia 桌面端批准」幂等）
  - code 有效未绑定：写入 pairer，bot 回「✓ 等待 AIjia 桌面端批准」
- 不调用 LLM，纯 connector 内部处理

用户点桌面端「批准」：

- 调 `channel_telegram_approve_pairing(code)` → 后端
  - 从 PairingCodeStore 取出 PendingPairing → 校验有 pairer
  - 把 pairer 写进 config.json 的 allowlist（atomic write + dedup by user_id）
  - 走 sender 发欢迎消息（"👋 你已连接 AIjia，可以开始对话"）
  - 从 PairingCodeStore 删除该 code
  - 返回 `{ user_id, first_name }`
- 前端刷新「已连接用户」列表

用户点「拒绝」：

- 调 `channel_telegram_reject_pairing(code)` → 后端只清 PairingCodeStore，可选向用户发"已被拒绝"消息

### 2.6 Tauri commands（新增 7 个）

```
channel_telegram_save(token: String)                  → ChannelPlatformState
channel_telegram_remove()                             → ChannelPlatformState
channel_telegram_set_enabled(enabled: bool)           → ChannelPlatformState
channel_telegram_begin_pairing()                      → TelegramPairingBeginResult
channel_telegram_list_pending_pairings()              → Vec<TelegramPendingPairing>
channel_telegram_approve_pairing(code: String)        → TelegramPairedUser
channel_telegram_reject_pairing(code: String)         → ()
channel_telegram_revoke_user(user_id: i64)            → ChannelPlatformState
```

`channel_telegram_save` 内部完成 getMe 验证；不单独导出 test_connection 命令。

返回类型：

```rust
struct TelegramPairingBeginResult {
    code: String,
    deep_link: String,            // https://t.me/<bot_username>?start=<code>
    expires_in_seconds: u64,      // 300
    bot_username: String,
}

struct TelegramPendingPairing {
    code: String,
    user_id: i64,
    first_name: String,
    username: Option<String>,
    requested_at: String,         // RFC3339
}

struct TelegramPairedUser {
    user_id: i64,
    first_name: String,
    username: Option<String>,
}
```

### 2.7 ChannelConfigView 适配

复用既有 `ChannelConfigView` 的 `app_key` 字段承载 `bot_username`，跟 wecom 用 `app_key` 装 `bot_id` 同模式：

```rust
ChannelConfigView {
    platform: Platform::Telegram,
    app_key: config.bot.bot_username,
    app_secret_masked: mask_secret(&token),
    robot_code: config.bot.bot_id,
    robot_code_source: RobotCodeSource::Registration,
    source: "TELEGRAM_BOT_TOKEN",
    created_at, updated_at,
}
```

## §3. 错误处理 + 边界场景

| 场景 | 行为 |
|---|---|
| token 错误（getMe 401） | save 命令直接返回 `Err`；前端 toast；config 不落盘 |
| 启动后 token 失效（getUpdates 401） | connector emit `NeedsReauth`；long_poll 退出不重连；前端红标 |
| 429 (sendMessage) | sleep `retry_after` 重试一次；再失败丢日志不影响 connector |
| 429 (getUpdates) | 罕见；按 `retry_after` sleep 后继续 |
| 5xx / 网络抖动 | ReconnectBackoff 5/15/30/60s ladder（复用 shared/reconnect.rs） |
| 用户从 Telegram 端 block 了 bot（send 403） | 该 user_id 移出 allowlist，前端实时刷新；下次配对需重扫 |
| pairing code 5 分钟未扫 | 自动失效；桌面 UI 倒计时到 0 → 自动调 begin_pairing 重新生成 |
| 一个 code 被同一 user 扫两次 | 幂等，再回"等待批准"提示 |
| 一个 code 被不同 user 扫第二次 | bot 回"码已被使用"；不覆盖原 pending |
| /start 不带 code | bot 回"请回到 AIjia 重新生成配对二维码" |
| /start 来自已 allowlist 用户 | bot 回"你已连接，可以开始对话"；不进 pending |
| connect 时 long_poll 第一次拉就失败 | `ConfigError` + last_error；不阻塞其它平台 |
| api.telegram.org 国内访问慢 | ReconnectBackoff 兜底；UI 文案"网络可能不畅，请检查代理" |
| 群聊消息进 connector | parser 直接丢弃（chat.type != "private"）|

## §4. 测试策略

### 4.1 Rust 单测（连同生产代码 PR）

- `api.rs`：mock `httpmock` 验证 URL 拼接 / token 不出现在 error message / 401 / 429 / 5xx 解析
- `parser.rs`：纯函数 —— text / `/start <code>` / `/start` 无 param / 群消息 / 缺字段
- `sender.rs`：MarkdownV2 转义所有特殊字符 + 已转义不二次转义 + Unicode + 429 retry_after + 400 parse fallback
- `pairing.rs`：code 生成唯一性 / 过期清理 / attempt_attach 幂等 / approve 写盘 / 重复 approve 不重复写
- `long_poll.rs`：offset 推进 / cancel_token 触发 2s 内退出 / offset 强制 flush / 5s 节流 fsync
- `connector.rs::tests`：platform 返回 Telegram / capabilities 字段

### 4.2 Rust 集成测试

`src-tauri/tests/telegram_pairing_integration_test.rs`：
- `httpmock` 起假 Bot API
- 启动 ChannelManager → 注册 telegram connector
- mock getUpdates 推一条 `/start ABC123` → 验证写入 pending
- 调 `approve_pairing(ABC123)` → 验证 allowlist 持久化 + 发欢迎 sendMessage
- 推一条普通消息 from 已配对 user → 验证 ChannelMessage 入 worker → chat_turn 处理 → assistant 持久化触发 reply_forwarder → 验证 sendMessage 调用

### 4.3 前端 Vitest

- `TelegramChannelConfig.test.tsx`：token 输入校验 / 调 save 后切到 QR / pending 列表轮询 / approve 调用
- `ChannelPage.test.tsx`：telegram 卡片渲染 + capability 切换

### 4.4 手动 e2e

- 真实 BotFather 建测试 bot → AIjia → token 输入 → 扫码 → 批准 → 私聊 → 收 AI 回复
- token 错误测试
- 二维码过期重生成
- 移除 / 重新添加用户
- disable / enable 不丢 allowlist

## §5. PR 切分

| PR | 范围 | 估时 |
|---|---|---|
| **PR1 后端骨架** | telegram/ 模块 + api + parser + sender + pairing + types；不接 manager；本地 cargo test 全过 | 1 天 |
| **PR2 后端接入** | manager 注册 connector + long_poll + auto_connect + Tauri commands + lib.rs invoke 注册 + 集成测试；后端到 Bot API 整链路通 | 1 天 |
| **PR3 前端配置 UI** | TelegramChannelConfig 组件 + ChannelPage 接线 + tauri.ts 类型 + channelStore + Vitest | 0.5 天 |
| **PR4 手动联调 + bugfix** | 真实 bot 跑完全链路 + 修发现的 bug | 0.5 天 |

**总：~3 天**

## §6. 风险

| 风险 | 缓解 |
|---|---|
| api.telegram.org 国内访问被墙 | 走系统代理；UI 显示 last_error 让用户判断 |
| MarkdownV2 转义漏字符 → 400 | 全字符单测 + 400 parse fallback plain text |
| pairing code 重复扫 → 多个 pending | attempt_attach 内部检查 pairer 字段，已绑定时幂等回包 |
| token 泄漏到日志 | 现阶段保守地不在 api.rs 的 reqwest log target 里打整个 URL；后续 PR6.5 上 SecretString 时统一治理 |
| Allowlist 写盘 race condition（同时 approve 两个 code） | atomic write tmp + rename 流程（与 wecom config 写盘同模式）+ 写盘前再读一次 dedup |
| 用户在 Telegram 里 delete chat with bot | bot 仍能 sendMessage（Telegram 行为），UI 不感知；用户重新 /start 即可 |
| getUpdates long-poll 退出时 offset 没 flush | cancel 路径强制 flush + 启动时复用 MessageDedupSet 兜底重复消费 |

## §7. 后续可选 PR（不在本 spec 范围）

- **PR5**：流式 editMessageText（同时做 trait 改造 PR1.5：`outbound_text_streaming` + `InboundDeployment` 重命名）
- **PR6**：附件接收 + 50MB 拒绝
- **PR7**：群聊支持（关闭 BotFather privacy mode 引导 + group_id allowlist）
- **PR8**：403 黑名单持久化 + 24h TTL
- **PR9**：SecretString newtype（独立全平台 sweep）

# IM Connector Trait — Phase 0 重构（多平台抽象奠基）

**日期**：2026-05-18
**状态**：Design approved → 待 writing-plans
**作者**：oayzz / Claude
**Scope**：Phase 0（trait 设计 + dingtalk 重组到独立目录，**不引入新平台**）

## 背景

桌面端已经按 [openclaw-dingtalk plugin](https://github.com/openclaw/dingtalk-openclaw-connector) 的协议实现了一套钉钉 IM 接入：

```
src-tauri/src/connector/channel/
├── manager.rs              # 1457 行：单平台 worker loop + reply dispatch
├── dingtalk_stream.rs      # 846 行：DWClient stream client
├── ask_coordinator.rs      # 818 行：跨平台共用，但当前只调 dingtalk
├── reply_manager.rs        # 663 行：reply queue
├── config_store.rs         # 611 行
├── router.rs               # 408 行
├── dingtalk_download.rs / dingtalk_card.rs / dingtalk_registration.rs / dingtalk_token.rs
└── types.rs                # Platform 枚举已预留 Feishu/Wechat/Wecom
```

接下来需要扩展到 **飞书 / 个微 / 企微 / Telegram / WhatsApp** 共 5 个平台。但若不先抽象，每个新平台都要复制粘贴 manager 和 worker loop。

**业务目标**：让"加一个新 IM 平台" = 在 `connector/im/<platform>/` 起一个目录、实现 trait，不动 manager / router / pending / coordinator。

## 参考材料

`~/Downloads/openclaw channel/` 下 4 个 openclaw TypeScript plugin：

- `dingtalk-openclaw-connector-main/`（钉钉，Stream + AI Card）
- `openclaw-lark-main/`（飞书，WebSocket + REST）
- `openclaw-weixin-main/`（个微，需外部 native daemon）
- `wecom-openclaw-plugin-main/`（企微，HTTP webhook + template card）

每个 plugin 都按 **"一平台一独立目录"** 的形态组织：自带 `channel.ts`（描述符）/ `runtime.ts`（主循环）/ `config/`（schema）/ `messaging/`（normalize）/ `media/`（附件）。没人尝试在 TS 侧抽象统一 trait——每个平台是独立 plugin、对外只 export `channel descriptor` + `runtime start` 两个东西。

本 spec **复刻这种分层**到 Rust 侧。

## 非目标（Non-Goals）

明确**不**在 Phase 0 范围内：

1. **不引入新平台**——Phase 0 结束时仍只有 dingtalk 一个 connector 实现
2. **不动外部接口**——`config.json` / `sessions.json` 落盘路径、所有 Tauri IPC 命令名、前端 store 字段全部保持二进制兼容
3. **不决定 webhook 入站方案**——Trait 只留 capability 占位（`InboundModel::Webhook`），具体怎么起 HTTP server 留给后续 spec
4. **不动 `connector/dingtalk.rs`**（那是 dws CLI sidecar，不是 IM 通道，名字撞车而已）

## 架构总览

`channel/` 重命名为 `im/`，分四层：

```
src-tauri/src/connector/im/
├── trait_def.rs         # 抽象层：IMConnector trait + Capabilities + 公共类型
├── shared/              # 公共层：跨平台辅助
│   ├── mod.rs
│   ├── router.rs            # 平移：external_id ↔ session_id 路由 + sessions.json 持久化
│   ├── pending_adapter.rs   # 平移：与 PendingQueueManager 的适配
│   ├── ask_coordinator.rs   # 平移：IMAskCoordinator
│   ├── config_store.rs      # 平移：ChannelConfigStore（多平台 channels/<platform>/config.json）
│   ├── reply_manager.rs     # 平移 + 改造：抽掉 dingtalk-specific 部分
│   └── reconnect.rs         # 新增：5s/15s/30s/60s backoff（从 manager.rs 抽出）
├── manager.rs           # 编排层：HashMap<Platform, Box<dyn IMConnector>>
└── dingtalk/            # 平台层：Phase 0 唯一实现
    ├── mod.rs               # impl IMConnector for DingtalkConnector
    ├── runtime.rs           # 整合现 dingtalk_stream.rs 的 worker loop
    ├── stream.rs            # DWClient stream client
    ├── card.rs              # AI card 增量发送
    ├── download.rs          # 附件下载
    ├── token.rs             # access_token 缓存
    ├── registration.rs      # device-code 注册
    └── types.rs             # dingtalk-specific 类型（与抽象类型隔离）
```

**依赖方向单向向下**：`manager → trait_def → shared → <platform>`。
每个 `<platform>/` 目录**只实现** trait，**不调用**其它平台的代码，跟 openclaw plugin 一一对应。

## §1. IMConnector trait（核心契约）

```rust
// connector/im/trait_def.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundModel {
    /// 长连接 / Stream（dingtalk / lark），connector 内部 spawn 长跑 task
    Stream,
    /// HTTP webhook 推送（wecom / telegram / whatsapp），connector 内部起 HTTP 端口
    Webhook,
    /// 通过外部 native daemon（个微），connector 内部 spawn / 管理 daemon 进程
    Daemon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFlow {
    /// OPEN_CLAW device-code（钉钉用）
    DeviceCode,
    /// 标准 OAuth 2.0（lark 用）
    OAuth,
    /// 静态 API key / token（telegram bot token）
    ApiKey,
    /// 扫码登录（个微）
    QRCode,
}

#[derive(Debug, Clone)]
pub struct ConnectorCapabilities {
    pub inbound: InboundModel,
    pub outbound_aicard: bool,
    pub outbound_markdown: bool,
    pub supports_attachments: bool,
    pub supports_group_chat: bool,
    pub supports_private_chat: bool,
    pub auth_flow: AuthFlow,
}

/// 注入给 connector 的"宿主能力"窄接口。
/// 不要给 connector AppHandle / 整个 ChannelManager —— 那是反模式。
#[derive(Clone)]
pub struct ConnectorContext {
    pub config_store: Arc<ChannelConfigStore>,
    pub secure_storage: Option<Arc<SecureStorage>>,
    pub ask_coordinator: Option<Arc<IMAskCoordinator>>,
    pub pending_manager: Arc<PendingQueueManager>,
    pub cancel_token: CancellationToken,
}

#[async_trait]
pub trait IMConnector: Send + Sync {
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> ConnectorCapabilities;

    /// 启动 connector。返回一个统一格式 ChannelMessage 的 Stream。
    /// 返回 stream 关闭即视为掉线，Manager 决定是否按 reconnect 策略重连。
    ///
    /// 实现者契约：必须监听 ctx.cancel_token，cancel 时 ≤ 2s 内退出。
    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError>;

    /// 发送回复。content 已是 normalize 过的 ReplyContent。
    /// connector 内部按 capabilities 自动降级（aicard → markdown，markdown → text）。
    async fn send(
        &self,
        target: ReplyTarget,
        content: ReplyContent,
    ) -> Result<(), ConnectorError>;

    /// 主动停止；默认实现 = drop 自身（让 stream 自然关）。
    async fn stop(&self) -> Result<(), ConnectorError> { Ok(()) }

    /// 注册流程入口（可选）。不支持 device-code 的平台返回 NotSupported。
    async fn begin_registration(
        &self,
        _: &RegistrationRequest,
    ) -> Result<RegistrationBegin, ConnectorError> {
        Err(ConnectorError::NotSupported("device-code begin_registration"))
    }
    async fn poll_registration(
        &self,
        _: &PollRequest,
    ) -> Result<RegistrationPoll, ConnectorError> {
        Err(ConnectorError::NotSupported("device-code poll_registration"))
    }
}
```

**为什么 trait 这么瘦**：5 大方法（连接/收/发/停/注册）已经覆盖 openclaw 4 个参考 plugin 里"对外暴露"的所有能力。所有跨平台共用的辅助（去重 / router / pending / coordinator / reply queue）住在 `shared/`，connector 不重新实现。

**为什么用 `start() -> Stream` 而不是 actor + command channel**：所有平台的入站本质都能 normalize 成"事件序列"。webhook 平台在 `start` 内部起 HTTP server 并通过 mpsc 喂入 stream，Manager 永远只看 stream。Actor + command channel 会让"connector 死了 vs 没消息"两种状态混淆，调试代价大。

**为什么 `ReplyContent` 必须 normalize 而不是给 connector 自由形态**：避免 Manager 到处 `if platform == Dingtalk { send_aicard } else { send_markdown }`。降级逻辑住在 connector 内部，对 Manager 透明。

## §2. 数据流

### 入站（IM → Lotus）

```
[平台 SDK / Webhook HTTP server / Daemon stdin]
   ↓ 平台特定解析 → ChannelMessage normalize
DingtalkConnector::start() 返回的 Stream
   ↓
ChannelManager::worker_loop（im/manager.rs，唯一）
   ↓ seen_msg_ids 去重
   ↓ shared::router::get_or_create_session（external_id → session_id）
   ↓ conv_store::create_conversation（若是新 session）
   ↓ shared::pending_adapter::enqueue_or_send
   ↓
ChatTurn 真正跑（runtime/chat/...）
```

**关键约束**：`manager.rs::worker_loop` 是**单条主链路**，对所有 connector 共用。
Connector 只负责"原始事件 → ChannelMessage 翻译"，不调度 / 去重 / 路由 / 派活。

### 出站（Lotus → IM）

```
ChatTurn 输出 streaming delta / 完整 markdown / 工具卡片
   ↓
shared/reply_manager.rs::ReplyDispatcher 按 ChannelConversation.platform 找 connector
   ↓
connector.send(ReplyTarget, ReplyContent)
   ↓ connector 内部按 capabilities 降级
[平台特定发送 API]
```

### Reply 降级表

| ReplyContent | capabilities.outbound_aicard | capabilities.outbound_markdown | 实际行为 |
|---|---|---|---|
| Text | * | * | 平台特定 send_text |
| Markdown | * | true | 平台特定 send_markdown |
| Markdown | * | false | 在 connector 内 strip markdown → send_text |
| AiCardChunk | true | * | 调平台 aicard 增量 API |
| AiCardChunk | false | true | buffer 直到 final_chunk=true，一次 send markdown |
| AiCardChunk | false | false | buffer 直到 final_chunk=true，strip → send_text |
| Attachment | - | - | 若 supports_attachments=false 则忽略并 send_text 占位 |

降级决策**全部住在 connector 内部**，Manager / ReplyDispatcher 不参与。

## §3. 错误处理 + 生命周期

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("transient error: {0}")]
    Transient(String),

    #[error("auth expired / kicked: {0}")]
    AuthExpired(String),

    #[error("fatal: {0}")]
    Fatal(String),

    #[error("shutdown requested")]
    ShutdownRequested,

    #[error("not supported: {0}")]
    NotSupported(&'static str),
}
```

**Manager 对各错误等级的处理**：

| 错误 | UI 显示 | Manager 动作 |
|---|---|---|
| `Transient` | "Reconnecting" | `shared/reconnect.rs` backoff（5s/15s/30s/60s），后调 `start()` |
| `AuthExpired` | "ConfigError"（要求重新认证） | 停止，不重连，前端弹"重新注册" |
| `Fatal(msg)` | "ConfigError: {msg}" | 停止，不重连 |
| `ShutdownRequested` | "Disconnected" | 静默清理 |
| `NotSupported` | — | 只用于注册等可选方法的 trait 默认实现，不参与运行时状态机 |

### 状态机（每个 platform 独立，Manager 管 5 个）

```
       Unconfigured ←─────────────────┐
            │ (用户填配置)             │
            ↓                         │
       Disconnected ←──────────────┐  │
            │ (用户启用)            │  │
            ↓                      │  │
       Connecting ──fail──→ ConfigError
            │                      ↑  │ (用户 reset)
            ↓                      │  │
       Connected ────Fatal/Expired─┘  │
         │   ↑                        │
   Transient│                         │
         ↓   │                        │
      Reconnecting ──超出 backoff──→ ConfigError
```

### Cancellation 契约

Manager 持有 `CancellationToken`，stop 时 cancel → connector 的 stream 必须自然 Drop → 内部 task / HTTP server 收 cancel 在 **≤ 2s** 内退出。

**这条约定写进 trait 的 doc 注释**，并由 `tests/im_connector_test.rs` 加 review case 验证：cancel 后 2.5s 内 connector 进程内 task 全清。

## §4. 测试策略

三层测试：

### 4.1 Trait 契约测试（`im/trait_def.rs::tests`）

- `MockConnector` 直接 yield 预设的 `ChannelMessage` stream
- 验证 `ChannelManager::worker_loop(mock)` 全链路工作，不依赖任何 dingtalk 代码路径
- 证明 trait 是真的抽象、不漏抽象

### 4.2 Dingtalk 实现测试（`im/dingtalk/` 内）

- 现有 5 类相关测试（`router::tests`、`config_store::tests`、`build_conversation_snapshot`、`reply_manager::tests`、`dingtalk_card::tests`）100% 保留，只改 `use` 路径
- 新增：`DingtalkConnector impls IMConnector + capabilities 字段正确`
- 新增：`DingtalkConnector::send(AiCardChunk) 走 card.rs 而非 markdown 降级`（验证 capabilities 真的驱动降级）

### 4.3 集成测试（`src-tauri/tests/im_connector_test.rs`）

- 起 `ChannelManager` + 注入 `MockConnector`，模拟完整收→派活→reply
- 验证 reply 走 `ReplyContent` 抽象，**完全不绑 dingtalk**
- 加 cancel 后 2.5s 收尾的回归 case

### 4.4 架构约束测试（`src-tauri/tests/review_im_layering.rs` 新增）

- `im/<platform>/` 任意文件**禁止** `use crate::connector::im::shared`——平台内只能从 `ConnectorContext` 拿能力
- `im/manager.rs` **禁止** `use crate::connector::im::dingtalk`——manager 只通过 trait 接触 connector
- 用 `cargo test` 跑，AST 不需要——简单 `grep -r` 即可

## §5. 迁移路径（实施 PR 切分预览）

> 详细实施步骤由 `writing-plans` 阶段产出。这里只列粗粒度 PR 切分预期。

- **PR1**：`channel/` → `im/`（pure rename + 老 import 路径全替换）+ 加 `connector/im/` 入口 mod。0 行为变化，纯目录搬家
- **PR2**：抽出 `im/shared/` 子目录，把 router / pending_adapter / ask_coordinator / config_store / reply_manager / reconnect 平移过去。0 行为变化
- **PR3**：把 `dingtalk_*.rs` 整合进 `im/dingtalk/` 子目录。0 行为变化（仍是函数调用）
- **PR4**：定义 trait（trait_def.rs）+ `impl IMConnector for DingtalkConnector`。Manager 仍走老路径，trait 是新代码
- **PR5**：Manager 改造为 `HashMap<Platform, Box<dyn IMConnector>>`，dingtalk worker loop 替换为 trait 调用。**这一期是切换点**
- **PR6**：新增 review_im_layering 测试 + 集成测试 + MockConnector，固化抽象边界

每个 PR 都保证：
- `cargo build` 通过
- `cargo test channel_ | im_` 通过
- 桌面 app 启动 + 钉钉登录 + 收发消息行为不变

## §6. 风险 + 缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| Rename PR 影响 import 路径太多 | PR1 体量大、冲突面广 | 一次性 rename，所有相关同事提前 sync |
| Trait 设计漏抽象 → 新平台被迫加方法 | trait 不稳，反复改 | PR4 必须先 mock 一遍 wecom（HTTP webhook）和 telegram（API key），证明 capabilities 够用 |
| reply 降级 buffer 在 connector 内部丢失（如崩溃） | 用户看不到 AI 回复 | buffer 限定在 stream 内存，崩溃后由 ChatTurn 重发；不引入持久化层 |
| 跨平台 worker_loop 假设"消息有 msg_id" 不一定成立 | 去重逻辑失效 | trait 要求 `ChannelMessage.msg_id` 必填，无则 connector 自行用 hash(content+timestamp) 生成 |
| `connector/dingtalk.rs`（dws CLI）名字撞车 | 误读 / 误改 | im 模块**禁止**导入 `connector/dingtalk.rs`；review test 卡 |

## §7. 后续 spec 预告（不在本期）

- `2026-MM-DD-im-feishu-design.md` — 飞书 lark-rust SDK 接入
- `2026-MM-DD-im-wecom-design.md` — 企微 HTTP webhook + 内置 HTTP server 设计
- `2026-MM-DD-im-telegram-design.md` — Telegram Bot API（webhook 或 long-polling 二选一）
- `2026-MM-DD-im-whatsapp-design.md` — WhatsApp Cloud API
- `2026-MM-DD-im-wechat-design.md` — 个微（依赖外部 daemon，最复杂、最后做）

每个新平台的 spec 都遵循同一模板：列 connector 实现细节、capabilities 表、auth flow、附件处理、reply 降级测试覆盖。Manager / shared 层在那些 spec 里**只读不改**。

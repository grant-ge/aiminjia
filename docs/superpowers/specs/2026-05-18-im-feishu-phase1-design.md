# Phase 1：飞书 IM Connector

**日期**：2026-05-18（修订于 review 后）
**状态**：Design draft v2 → 待用户 review
**前置**：Phase 0（`2026-05-18-im-connector-trait-phase0-design.md`）已落地，但下文 §0 列出的 4 项抽取（PR0a–PR0d）必须在 Phase 1 主体之前完成
**Scope**：在 `connector/im/feishu/` 下实现 `IMConnector` for 飞书

## 背景

飞书是钉钉之后用户基数最大的国内企业 IM，且**接入模型跟钉钉高度相似**——都走 WebSocket + Device Authorization Grant，是 Phase 0 trait 抽象落地后第一个该验证的平台。Phase 0 trait 是否真的够用，要靠"飞书 connector 实现能不能不动 `im/shared/` 和 `im/manager.rs`"来检验——这是 Phase 1 的硬性验收标准（由 `tests/review_im_layering.rs` 锁住）。

## 调研结论（事实摘要）

参考代码：`~/Downloads/openclaw channel/openclaw-lark-main/`

1. **入站**：飞书事件经 Lark SDK 的 `WSClient` WebSocket 长连接到达；plugin 只实现 WebSocket 模式（注释里 webhook 标记"未实现"）
2. **认证**：app_id + app_secret 走 OAuth 2.0 Device Authorization Grant（RFC 8628），跟钉钉 OPEN_CLAW 流程同形
3. **消息类型**：24 种（text/post/image/file/audio/video/sticker/interactive/share_chat/...），多数有专用 converter
4. **流式 AI Card**：通过 CardKit SDK 增量更新——`card.create()` → `cardElement.content(seq++)` → `card.update()` 完成；严格递增 sequence，乱序失败
5. **附件**：`client.im.message.resource.get()` 直接拉原始字节，无需 download token
6. **特殊点**：始终用 `tenant_access_token`（应用身份）；WebSocket 重连重放消息要 dedup

## Non-Goals

1. 不实现 Lark Webhook 入站（飞书 webhook 模式有更复杂的回调加密 + 公网入口需求，留给 Phase 2 wecom 之后的 webhook 通用方案）
2. 不实现 user_access_token 流程（暂只支持应用机器人，不做"以个人身份发消息"）
3. 不实现 24 种消息类型的全套 converter——只覆盖 text / image / file / interactive card 这 4 类高频，其它显示"[不支持的消息类型]"占位
4. 不引入飞书专有的 share_chat / share_user / vote / hongbao 等业务功能——这些属于业务集成，不属于 IM connector

---

## §0. Phase 0 实际落地差距与前置抽取 PR

Phase 0 的 6 个 PR 落地后，**有 4 处抽象 spec 假设已完成但实际没有**。Phase 1 必须先做 4 个前置抽取 PR（PR0a–PR0d），否则 PR1–PR7 会同时承担"接飞书"+"补抽取"两份工作，估时会爆。

### 已落地（Phase 0 真实状态）

- ✅ `IMConnector` trait（5 个方法）+ companion types（ConnectorCapabilities/Context/Error/ReplyTarget/ReplyContent）
- ✅ `im/shared/` 下：`router`, `ask_coordinator`, `config_store`, `pending_adapter`, `reply_manager`, `reconnect`
- ✅ `im/dingtalk/` 下：`stream`, `card`, `download`, `token`, `registration`, `connector`
- ✅ `im::factory::build_dingtalk_connector` 平台中性入口
- ✅ `tests/review_im_layering.rs`：禁止 `im/<platform>/*.rs` 直接 import `im::shared::{router, ask_coordinator, config_store, pending_adapter}`；禁止 `im/manager.rs` 直接构造平台特定 connector 类型
- ✅ `tests/im_connector_cancel_test.rs`：cancel-≤2s 契约

### Phase 0 已知 leak（必须 Phase 1 前补完）

| 缺失 | 影响 | 前置 PR |
|---|---|---|
| 没有 `im/shared/token.rs` 通用 token cache trait —— Phase 0 仅在 `dingtalk/token.rs` 内有 TokenCache，飞书的 `tenant_access_token` 没法复用 | 飞书 PR2 token 缓存只能拷贝一份 dingtalk 实现 | **PR0a** |
| 没有 `im/shared/dedup.rs` —— `seen_msg_ids: HashSet<String>` 还 inline 在 `manager.rs` worker loop 里 | 飞书 WS 重连重放消息时无处复用，会出现重复派活 | **PR0b** |
| `DingtalkReplyManager` 路径未接入 trait —— manager 仍走 `RuntimeEventBus` 订阅，不走 `connector.send(AiCardChunk)`；review_im_layering 当前**允许** `shared::reply_manager` 这一条 leak | 飞书 CardKit 流式更新无处接入，要么再写一份 `FeishuReplyManager` 并行订阅 RuntimeEventBus（**永久固化 leak**），要么完成迁移 | **PR0c** |
| `ReplyTarget` 字段是钉钉形状（`robot_code`, `reply_group_id`, `session_webhook`），飞书没这些字段 | 飞书 connector 没法用 `ReplyTarget` —— 这是 trait 层破坏性改动 | **PR0d** |
| `ChannelConfigStore` 只有 `dingtalk_dir / dingtalk_config_path / read_dingtalk_config / save_dingtalk_registration / reveal_dingtalk_secret / set_dingtalk_enabled / remove_dingtalk` 这套钉钉特化的方法 | 飞书没法存配置 | **PR0d** 一起 |
| `pending_adapter::build_pending_item_from_dingtalk` 是钉钉特化的 | 飞书 PR6 接 PendingQueueManager 时缺等价物 | 飞书 PR6 内做（不阻塞前面，但要意识到） |

### PR0a：抽 `im/shared/token.rs` 通用 token cache

**目标**：通用 `TokenCache<T>` trait + 一个默认实现，dingtalk 接入；飞书 PR2 复用。

```rust
// im/shared/token.rs (新增)
#[async_trait]
pub trait PlatformTokenSource: Send + Sync {
    /// 调远端拿一份新 token + 过期秒数。
    async fn fetch(&self) -> anyhow::Result<(String, u64)>;
}

pub struct TokenCache<S: PlatformTokenSource> {
    source: S,
    state: Mutex<Option<(String, Instant)>>,
}

impl<S: PlatformTokenSource> TokenCache<S> {
    pub async fn get(&self) -> anyhow::Result<String> { /* 提前 300s 刷新 */ }
}
```

- 写 4 个单测（首次拉取 / 缓存命中 / 临近过期刷新 / 远端失败回错）
- `dingtalk/token.rs` 的 `TokenCache` 改为 `type TokenCache = shared::TokenCache<DingtalkTokenSource>` 适配（保留旧 public API，避免动 dingtalk worker 调用点）
- 不动 `dingtalk/token.rs` 的下游用法

### PR0b：抽 `im/shared/dedup.rs`

**目标**：把 `manager.rs` worker loop 里的 `seen_msg_ids: HashSet<String>` 改为 `Arc<MessageDedupSet>` helper，飞书 connector 自带 dedup（在 connector.start() 里）。

```rust
// im/shared/dedup.rs (新增)
pub struct MessageDedupSet {
    inner: RwLock<HashSet<String>>,
    cap: usize,
}

impl MessageDedupSet {
    pub fn new(cap: usize) -> Self { /* default cap=5000 */ }
    /// 返回 true 表示首次见过这个 msg_id，false 表示重复。
    pub async fn observe(&self, msg_id: &str) -> bool { /* 满则清空 */ }
}
```

- manager.rs 改为：把 `seen_msg_ids` 字段类型从 `Arc<RwLock<HashSet<String>>>` 换成 `Arc<MessageDedupSet>`，去重逻辑改单行调用
- 写 3 个单测（首次插入 / 重复返回 false / cap 超限清空）

### PR0c：dingtalk AI Card 路径接入 `connector.send(AiCardChunk)`

**这是 Phase 1 之前最重要、风险最高的一个 PR。** 当前 `DingtalkReplyManager` 订阅 `RuntimeEventBus::StreamDelta`，在订阅闭包里调钉钉 `card.create / cardElement.content / card.update`。Phase 1 要让飞书 CardKit 走 trait 路径，就必须证明"AI Card 投放 = `connector.send(AiCardChunk)`"对 dingtalk 也通。

**设计**：
- 不删 `DingtalkReplyManager`（它还要管 register/remember_credentials 凭证缓存、AI Card 生命周期与 run_id 绑定），但**把投放点从直接调钉钉 SDK 改为调 `connector.send(target, ReplyContent::AiCardChunk { delta, final_chunk })`**
- `DingtalkConnector::send(AiCardChunk)` 内部分支调原 `dingtalk::card::stream_card / finish_card`
- `ReplyTarget` 在 PR0d 里改成平台中性后，dingtalk 特定的 `app_key/app_secret/robot_code/card_target` 由 connector 内部从 `register()` 时缓存的凭证表查出
- 完成后在 `review_im_layering.rs` 加规则：**`shared::reply_manager` 也不能 import `dingtalk::card`**（最后这一条平台 leak 关掉）

**风险**：钉钉 AI Card 当前流式响应**正常工作**——这条改动如果出 bug 用户会立刻感知到。必须配套手工冒烟（私聊/群聊各 1 条消息流式看 AI Card 字符渐进出现 + 无延迟 + final 收尾）。

### PR0d：`ReplyTarget` 平台中性化 + `ChannelConfigStore` 多平台化 + `observe_session` + `MarkdownSupport`

**Phase 5 反向需求扩展**（详见 `2026-05-18-im-wechat-phase5-design.md` §12）：原本 PR0d 只计划做 `ReplyTarget` 平台中性化 + `ChannelConfigStore` 多平台化两件事；Phase 5 调研发现 wechat / whatsapp 都需要 **router 建 session 后通知 connector 缓存 session_id → external user_id 映射**——这是 trait 没有覆盖的方向，必须加 `observe_session` trait 方法。同时 wechat 走 `StreamingMarkdownFilter` 是"部分支持 markdown"，需要把 `outbound_markdown: bool` 升级成枚举 `MarkdownSupport`。这两个改动并到 PR0d 一起做。

**`ReplyTarget` 改造**：

```rust
// trait_def.rs
#[derive(Debug, Clone)]
pub struct ReplyTarget {
    pub session_id: String,
    pub external_conversation_key: String,
    // 不再带 robot_code / reply_group_id / session_webhook —— 这些是钉钉特化
    // 字段，连接器内部从自己的 session 凭证表查
}
```

- `DingtalkConnector` 内部加 `HashMap<session_id, DingtalkSessionTarget>`（在 `register()` 时由 reply_manager 喂进来）
- 影响范围：manager.rs 里所有构造 `ReplyTarget { robot_code, reply_group_id, ... }` 的地方都要改成不带这些字段；改完后 `manager.rs` 也就**不再需要** `super::dingtalk::card::CardTarget` 这个 import（残留 leak 关掉 1 条）
- 还能不能继续走 `send_session_webhook_text` 兜底（附件全部下载失败时的钉钉特化提示）？—— **不能**，这条也通过 `connector.send(ReplyContent::Text("..."))` 走，dingtalk 内部从 session 凭证表查 webhook

**`observe_session` trait 方法（Phase 5 反向需求）**：

```rust
// trait_def.rs
pub trait IMConnector: Send + Sync {
    // ... 既有方法 ...

    /// router 建 session 后通知 connector 缓存 session_id → conversation_key 映射。
    /// 默认 no-op；只有需要在 send() 时按 session_id 反查 external user_id 的平台
    /// （wechat、whatsapp）实现此方法。
    async fn observe_session(
        &self,
        session_id: SessionId,
        conversation_key: &str,
        conversation_type: ConversationType,
    ) -> Result<(), ConnectorError> {
        let _ = (session_id, conversation_key, conversation_type);
        Ok(())
    }
}
```

- `manager.rs` 在 router `get_or_create_session` 之后 call `connector.observe_session(...)`
- `DingtalkConnector` 默认 no-op（dingtalk 用 `reply_robot_code_for_worker` 自己反查不需要这层）
- `WechatConnector`（Phase 5 PR5）/ `WhatsAppConnector`（Phase 4 PR4）实现此方法写入各自的 SessionStore

**`MarkdownSupport` 枚举（Phase 5 反向需求）**：

```rust
// trait_def.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownSupport {
    /// 平台原生不支持 markdown，全部走 plain text
    None,
    /// 流式逐字符过滤，部分 markdown 语法保留（粗体 `**x**` → `x` 等）—— wechat 走 StreamingMarkdownFilter
    Partial,
    /// 平台原生支持 markdown
    Full,
}

pub struct ConnectorCapabilities {
    // ... 既有字段 ...
    pub outbound_markdown: MarkdownSupport,  // 原本是 bool
}
```

- 既有 connector 老 `outbound_markdown: true` → `MarkdownSupport::Full`；`false` → `MarkdownSupport::None`
- 新平台 wechat 用 `MarkdownSupport::Partial`
- 上层降级逻辑：`Partial` 平台 connector 内部自行用 `StreamingMarkdownFilter` 过滤

**`ChannelConfigStore` 改造**：

```rust
impl ChannelConfigStore {
    pub fn platform_dir(&self, platform: Platform) -> PathBuf;
    pub fn platform_config_path(&self, platform: Platform) -> PathBuf;
    pub fn read_config<T: DeserializeOwned>(&self, platform: Platform) -> Result<Option<T>>;
    pub fn save_registration<T: Serialize>(&self, platform: Platform, config: &T) -> Result<...>;
    pub fn reveal_secret(&self, platform: Platform, ...) -> Result<String>;
    // ... 老的 dingtalk_* 方法保留作 deprecated 转发到新签名（避免一次性改 ~30 个调用点）
}
```

- 磁盘路径**不变**：`users/<scope>/channels/{dingtalk,feishu}/{config.json,sessions.json}` —— 钉钉路径完全兼容老用户
- 老 dingtalk_* 方法逐步迁移；Phase 1 飞书 PR2 调用新签名

---

## §1. 关键实现点（Phase 1 主体，PR0a-d 完成后）

### 1.1 SDK 选择

**用 `lark-rs` 还是直接 HTTP？**

- **先做半天的事实评估**（PR1 之前）：
  - `lark-rs` GitHub last commit / 是否支持 WSClient（ws_client crate） / 是否支持 device-code / 是否支持 CardKit
  - `larksuite-rust-sdk` 同上对比
- **推荐路线**（事实评估前的预设，可能改）：
  - Token + REST API：用 `lark-rs` 如果它支持的话，省 ~300 行
  - WebSocket：自己写 `tokio-tungstenite`，因为 Lark WS 帧协议简单，第三方 crate 适配反而麻烦
  - CardKit：必走自己写（CardKit 是新 API，第三方 crate 大概率没接）
- **fallback**：纯自己写。参考 Phase 0 dingtalk 整套实现量 **~2200 行**（stream 1100 / card 400 / download 400 / token 100 / registration 200），飞书因免去 download token 协议应该 -300，**整体 ~1700-2000 行**。不要承诺"≤1000 行"——之前那个数字是 wishful thinking。

### 1.2 Device Authorization Grant 流程

飞书的实际 endpoint：

```
POST  https://passport.feishu.cn/suite/passport/oauth/authorize/device
POST  https://passport.feishu.cn/suite/passport/oauth/token
```

参数比钉钉 OPEN_CLAW 略多（需要带 `app_id` / `scope`），但流程一致：
1. 客户端 `begin_registration()` → 返回 `verification_uri_complete` + `device_code`
2. 用户浏览器打开 URL 授权
3. 客户端按 `interval` 轮询 `poll_registration()` → 拿到 `access_token` + `refresh_token`
4. token 入 `SecureStorage`（复用 dingtalk 的 keychain 路径，但 key 改为 `aijia-feishu-...`）

### 1.3 tenant_access_token 缓存

复用 PR0a 抽出的 `im/shared/token.rs::TokenCache<S>`：

```rust
pub struct FeishuTokenSource { app_id, app_secret_storage, ... }

impl PlatformTokenSource for FeishuTokenSource {
    async fn fetch(&self) -> Result<(String, u64)> {
        // POST /open-apis/auth/v3/tenant_access_token/internal
        // 返回 (tenant_access_token, expires_in=7200)
    }
}

let cache: TokenCache<FeishuTokenSource> = TokenCache::new(source);
```

有效期 2h，TokenCache 默认提前 5 分钟刷新。

### 1.4 WebSocket 入站消息 normalize

飞书 WS 推送的事件类型多，但 IM connector **只关心**：

| 事件 type | 处理 |
|---|---|
| `im.message.receive_v1` | 翻译为 `ChannelMessage`（text/image/file/interactive） |
| `im.chat.member.bot.added_v1` | 用户加机器人 → 触发 welcome message hook（可选） |
| `card.action.trigger` | 用户点击 card 按钮 → 通过 `IMAskCoordinator` 路由回 chat turn |
| 其它 | 静默忽略（日志一行） |

**消息 dedup**：飞书 WS 重连时会重放最近消息，**connector 内部**实例化一个 `Arc<MessageDedupSet>`（PR0b 抽出），在 `start()` 返回的 stream 之前做去重。manager 一侧的 `seen_msg_ids` 也仍在（双层 dedup，不冲突，dedup 集容量极小）。

### 1.5 CardKit 增量更新

CardKit 是飞书独有能力，开放性强于钉钉 AI Card。

`connector.send(target, ReplyContent::AiCardChunk { delta, final_chunk })` 调用时：

1. **首次** chunk：调 `cardkit.v1.card.create()` → 拿 `card_id` + `seq=1`，存 connector 内部 `HashMap<session_id, FeishuCardSession>`（key 是 session_id，因为 manager 没有 chat_turn_id 概念）
2. **后续** chunk：`cardkit.v1.cardElement.content(card_id, seq++)` 推送 delta（飞书做 diff 动画）
3. **final**：`cardkit.v1.card.update(card_id, full_markdown)` 全量替换，**保留** session（同 session 下一次回复可复用 card_id？—— 不行，每次 AI 回复都建新卡，final 后清掉 session）

**rate limit 防护**：CardKit 限频 ~100ms/次。Connector 内对每个 `card_id` 起一个 mpsc + 串行 sender task，**用 `tokio::time::sleep_until` 节流，不丢 chunk**——丢 chunk 会破坏流式打字视觉效果（之前 spec 写"丢弃"是错的，已修正）。如果上游产 chunk 速度持续超过 10/s，最坏情况是流式追尾延迟（用户看到打字比 LLM 实际产出慢几秒），这比"突然刷新一整段"体验好得多。

### 1.6 24 种消息类型的"降级覆盖"

Phase 1 只接 4 种主流；其它 20 种：

```rust
match msg.msg_type {
    "text" => normalize_text(msg),
    "image" => normalize_image(msg),
    "file" => normalize_file(msg),
    "interactive" => normalize_card(msg),
    other => ChannelMessage {
        text: format!("[飞书消息类型 {} 暂不支持]", other),
        attachments: vec![],
        ...
    }
}
```

这条策略写入 spec，避免实施时拍脑袋决定。后续需要更多类型由独立小 PR 加。

## §2. 数据流（跟 Phase 0 一致，但前提是 PR0c 完成）

**入站**：`FeishuConnector::start()` 返回 `BoxStream<ChannelMessage>` → Manager 单一 worker loop（跟钉钉同一份代码）

**出站**（PR0c 完成后的新模型）：
- 普通文本 / markdown：`Manager` → `connector.send(target, ReplyContent::Text|Markdown)`
- AI Card 流式：`DingtalkReplyManager / FeishuReplyManager`（订阅 RuntimeEventBus 的角色保留）→ `connector.send(target, ReplyContent::AiCardChunk)` → connector 内部按平台决定走钉钉 AI Card 还是飞书 CardKit

完全复用 Phase 0 数据流。

## §3. 错误处理

跟 Phase 0 的 4 档错误一致。飞书特殊错误码映射：

| 飞书错误码 | 映射到 | 说明 |
|---|---|---|
| 99991661 (invalid_request 参数错) | `Fatal(msg)` | 上抛，连接器不重试，因为参数错重试也没用 |
| 99991663 (token 过期) | `Transient` | TokenCache 自动刷新后重试 |
| 99991664 (user_access_token 无效) | `Fatal(...)` | Non-Goals 不应触发；显式断言用以发现非预期路径 |
| 99991668 (token 无效) | `AuthExpired` | 强制重新走 device-code |
| 230002 (CardKit sequence 错乱) | `Transient` | 清掉本 card_id session，下次重新 create |
| 230005 (CardKit 卡片不存在) | `Transient` | 同上 |
| 其它 | `Fatal(msg)` | 上抛 |

## §4. 测试

**复用 Phase 0 PR6 已提供的契约模板** `tests/im_connector_cancel_test.rs`：mock `IMConnector` + tempdir + `PendingQueueManager` 的 fixture 模式，飞书侧测试照搬。

- `im/feishu/runtime::tests`：mock `WSClient`，验证消息 normalize 正确（4 种类型 + 20 种降级）
- `im/feishu/card::tests`：mock CardKit HTTP，验证 sequence 严格递增 + final 兜底 + rate-limit 节流（不丢 chunk）
- `im/feishu/token::tests`：tenant_access_token 提前 5min 刷新 + AuthExpired 触发 device-code
- `tests/im_feishu_integration.rs`：起 Manager + FeishuConnector + mock 飞书后端，全链路收→派活→reply

新增 `tests/review_im_layering.rs` 检查项：
- `platforms` 数组追加 `"feishu"`
- 验证 `im/feishu/*.rs` 也满足 platforms_must_not_import_shared_orchestration_helpers

## §5. 实施 PR 切分（修订）

### 前置（必做）：4 个 Phase 0 抽取补完 PR

| PR | 干啥 |
|---|---|
| **PR0a** | 抽 `im/shared/token.rs` 通用 token cache trait，dingtalk 接入 |
| **PR0b** | 抽 `im/shared/dedup.rs`，manager.rs 接入 |
| **PR0c** | dingtalk AI Card 走 `connector.send(AiCardChunk)`，review_im_layering 锁 shared::reply_manager 不能 import dingtalk |
| **PR0d** | `ReplyTarget` 平台中性化（去钉钉字段） + `ChannelConfigStore` 多平台化 |

每个 PR 独立 build + test + 钉钉手工冒烟。**PR0c 风险最高**，必须用真账号验证 AI Card 流式回复正常。

### 主体：飞书 PR1–PR7

- **PR1** `im/feishu/` 目录骨架 + 空壳 `impl IMConnector` + `capabilities()` + 单测；前端"添加飞书账号"入口 stub（按钮 disabled）
- **PR2** Device-code 注册 + tenant_access_token 缓存（复用 PR0a 的 shared TokenCache）
- **PR3** WebSocket runtime + 消息 normalize（4 种类型 + 20 种降级 + connector 内部 MessageDedupSet）
- **PR4** Reply send（text/markdown）；cardkit 留 `Err(NotSupported)` stub
- **PR5** CardKit 流式更新 + 严格 sequence + 不丢 chunk 的 rate-limit 节流
- **PR6** 附件下载 + 接入 PendingQueueManager（新增 `pending_adapter::build_pending_item_from_feishu` 或泛化既有钉钉版）
- **PR7** 集成测试 + 前端"添加飞书"UI 完整化（设置面板新增一项 + device-code 注册流） + `review_im_layering.rs` 加 `feishu` 入数组

## §6. 风险

| 风险 | 缓解 |
|---|---|
| **PR0c 改动出 bug 影响钉钉生产链路** | 必读：用真账号做手工冒烟（私聊/群聊各 1 条流式回复 + 重连 + 重启）；PR0c 必须 reversible（commit 范围窄、不夹杂其它改动） |
| PR0d `ReplyTarget` 改造影响面广 —— manager.rs 多处构造点 | 改造前先 grep 所有 `ReplyTarget { ... }` 字面构造，列清楚改动点；用类型系统逼出来（删字段后编译错误自然指路） |
| lark-rs / larksuite-rust-sdk 评估结果可能两个都不能用 | PR1 之前完成评估；fallback 是纯自己写，时间 +3-5 天 |
| CardKit 严格 sequence 模型 + 并发 chunk 乱序 | connector 内对每个 card_id 起 mpsc + 串行 sender task；用 `sleep_until` 节流而非丢弃，保流式视觉 |
| 24 种消息类型用户期待"全支持" | spec 显式声明只支持 4 种 + 占位文案，等用户反馈再扩 |
| 飞书 device-code 域名 passport.feishu.cn 在境外网络不稳 | Phase 1 不优化，标记为已知；前端注册流加超时提示 |
| 飞书 token 跟钉钉 token 字段名冲突 | PR0a 抽出的 `TokenCache<S>` 是泛型，源由 platform-specific source provider 决定，无字段名冲突；keychain key 必须带 `aijia-feishu-` 前缀，PR0d 的 `ChannelConfigStore` 改造里强制 |
| Phase 0 leak 在 Phase 1 PR0c 才迁完，期间 `review_im_layering` 不能加 `shared::reply_manager` 规则 | 路线图明确：先 PR0a/PR0b/PR0d 抽取 → PR0c 迁移 + 加锁；4 个 PR 顺序不可乱 |

## §7. 估时（修订）

- PR0a：0.5 天
- PR0b：0.5 天
- PR0c：**1.5 天**（含手工冒烟）
- PR0d：**1.5 天**（含影响面 grep + 编译错误清扫）
- 前置小计：~4 天

- PR1：0.5 天（骨架）
- PR2：1 天（device-code）
- PR3：2 天（WS + normalize）
- PR4：0.5 天（text/markdown send）
- PR5：2 天（CardKit + sequence + rate limit）
- PR6：1 天（附件下载 + pending adapter）
- PR7：1 天（集成测试 + 前端）
- 飞书主体小计：~8 天

**总计：~12 天单人**（不含 SDK 评估的 0.5 天 + 集成 review buffer）。Phase 0 是 4 天 6 PR，飞书 3 倍工作量是合理的——因为既要做新平台又要把 Phase 0 没收完的 4 条 leak 收掉。

# Phase 1：飞书 IM Connector

**日期**：2026-05-18
**状态**：Design draft → 待用户 review
**前置**：Phase 0（`2026-05-18-im-connector-trait-phase0-design.md`）已落地，`IMConnector` trait + `im/shared/` + `im/dingtalk/` 已就绪
**Scope**：在 `connector/im/feishu/` 下实现 `IMConnector` for 飞书

## 背景

飞书是钉钉之后用户基数最大的国内企业 IM，且**接入模型跟钉钉高度相似**——都走 WebSocket + Device Authorization Grant，是 Phase 0 trait 抽象落地后第一个该验证的平台。如果 trait 设计够用，飞书 connector 实现应当 ≤ 1000 行，比 dingtalk 现有量少一半（因为公共逻辑都在 `im/shared/`）。

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

## 架构契合 Phase 0

```
src-tauri/src/connector/im/feishu/
├── mod.rs               # impl IMConnector for FeishuConnector
├── runtime.rs           # WSClient 主循环 + 消息 normalize → ChannelMessage
├── stream.rs            # 飞书 WebSocket 连接管理 + 重连 + heartbeat
├── card.rs              # CardKit 增量更新 + sequence 管理
├── download.rs          # 附件下载（Lark SDK resource.get）
├── token.rs             # tenant_access_token 缓存 + 过期刷新
├── registration.rs      # OAuth Device Authorization Grant 流程
└── types.rs             # 飞书原始消息类型 + 24 种 → ChannelMessage 映射
```

**capabilities 声明**：
```rust
ConnectorCapabilities {
    inbound: InboundModel::Stream,
    outbound_aicard: true,
    outbound_markdown: true,
    supports_attachments: true,
    supports_group_chat: true,
    supports_private_chat: true,
    auth_flow: AuthFlow::DeviceCode,
}
```

跟钉钉**几乎完全一样**——这是好事，说明 trait 抽象对了。

## §1. 关键实现点

### 1.1 SDK 选择

**用 `lark-mcp` / `lark-rs` 还是直接 HTTP？**

- 推荐：直接 HTTP + 手写 `reqwest` 客户端 + `tokio-tungstenite` 处理 WS。
- 理由：① 飞书官方没有 Rust SDK ② 已有 lark-rs crate 但维护活跃度不高 ③ Lark WebSocket 协议简单（JSON over WS），HTTP API 100% REST，自己写比适配第三方 crate 稳
- spec 假设走"自己写"路线；若实施时发现 lark-rs 够用，PR3 可以改用之

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

跟钉钉的 access_token 缓存逻辑一致，但 key 字段名变为 `tenant_access_token`：

- 有效期 2h
- 提前 5 分钟刷新
- 复用 `im/shared/token.rs` 的通用 token cache trait（Phase 0 PR2 抽出）

### 1.4 WebSocket 入站消息 normalize

飞书 WS 推送的事件类型多，但 IM connector **只关心**：

| 事件 type | 处理 |
|---|---|
| `im.message.receive_v1` | 翻译为 `ChannelMessage`（text/image/file/interactive） |
| `im.chat.member.bot.added_v1` | 用户加机器人 → 触发 welcome message hook（可选） |
| `card.action.trigger` | 用户点击 card 按钮 → 通过 `IMAskCoordinator` 路由回 chat turn |
| 其它 | 静默忽略（日志一行） |

**消息 dedup**：飞书 WS 重连时会重放最近消息，复用 `seen_msg_ids: HashSet` —— Phase 0 已抽到 `im/shared/dedup.rs`。

### 1.5 CardKit 增量更新

CardKit 是飞书独有能力，开放性强于钉钉 AI Card。

接收 `ReplyContent::AiCardChunk { delta, final_chunk }` 时：

1. **首次** chunk：调 `cardkit.v1.card.create()` → 拿 `card_id` + `seq=1`，存 connector 内部 `HashMap<chat_turn_id, FeishuCardSession>`
2. **后续** chunk：`cardkit.v1.cardElement.content(card_id, seq++)` 推送 delta（飞书会做 diff 动画）
3. **final**：`cardkit.v1.card.update(card_id, full_markdown)` 全量替换，清空 session

**rate limit 防护**：CardKit 限频 100ms/次。Connector 内部对单 `card_id` 加 token bucket，超频的 chunk 直接丢弃（最后 final 兜底）。

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

## §2. 数据流（跟 Phase 0 一致）

入站：`FeishuConnector::start()` 返回 `BoxStream<ChannelMessage>` → Manager 单一 worker loop
出站：Manager → `connector.send(ReplyTarget, ReplyContent)` → connector 内部按 capabilities 走 CardKit / markdown / text

完全复用 Phase 0 数据流，本 spec 不重画。

## §3. 错误处理

跟 Phase 0 的 4 档错误一致。飞书特殊错误码映射：

| 飞书错误码 | 映射到 | 说明 |
|---|---|---|
| 99991663 (token 过期) | `Transient` | 自动刷新 token 后重试 |
| 99991668 (token 无效) | `AuthExpired` | 强制重新走 device-code |
| 230002 (CardKit sequence 错乱) | `Transient` | 清掉本 card_id session，下次重新 create |
| 230005 (CardKit 卡片不存在) | `Transient` | 同上 |
| 其它 | `Fatal(msg)` | 上抛 |

## §4. 测试

跟 Phase 0 §4 测试策略相同，飞书侧：

- `im/feishu/runtime::tests`：mock `WSClient`，验证消息 normalize 正确
- `im/feishu/card::tests`：mock CardKit HTTP，验证 sequence 严格递增 + final 兜底
- `im/feishu/token::tests`：access_token 提前 5min 刷新 + AuthExpired 触发 device-code
- `tests/im_feishu_integration.rs`：起 Manager + FeishuConnector + mock 飞书后端，全链路收→派活→reply

## §5. 实施 PR 切分

- **PR1** `im/feishu/` 目录骨架 + 空壳 `impl IMConnector` + `capabilities()` + 单测
- **PR2** Device-code 注册 + tenant_access_token 缓存（前端可以"添加飞书账号"但还收不到消息）
- **PR3** WebSocket runtime + 消息 normalize（4 种类型）
- **PR4** Reply send（text/markdown 优先，cardkit 留 stub）
- **PR5** CardKit 流式更新 + sequence 管理 + rate limit
- **PR6** 附件下载 + 接入 `im/shared/pending_adapter`（消息走 PendingQueueManager）
- **PR7** 集成测试 + 前端"添加飞书"UI（设置面板新增一项）

## §6. 风险

| 风险 | 缓解 |
|---|---|
| Lark SDK 缺位、自己写 client 工作量大 | PR3 timebox 1 周；超期就切到 lark-rs crate 加适配层 |
| CardKit sequence 模型严格、并发 chunk 会乱序 | connector 内对每个 card_id 加 mpsc 串行化 chunk，外部并发无所谓 |
| 24 种消息类型用户期待"全支持" | spec 显式声明只支持 4 种 + 占位文案，等用户反馈再扩 |
| 飞书 device-code 域名 passport.feishu.cn 在境外网络不稳 | Phase 1 不优化，标记为已知 |
| 飞书 token 跟钉钉 token 字段名冲突 | 复用 `im/shared/token.rs` 的 generic cache 时 key 必须带 platform 前缀 |

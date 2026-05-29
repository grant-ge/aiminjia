# IM Connector 路线图总览（Phase 0-5）

**日期**：2026-05-18
**状态**：所有 Phase spec 已 review v2
**目的**：在 5 份单独 spec 之上提供跨 Phase 的依赖 DAG、共享抽象演化时间线、推荐实施顺序

> 这份文档是路线图，不是细节 spec。每个 Phase 的设计细节、PR 切分、风险表都在对应 spec 里：
> - `2026-05-18-im-connector-trait-phase0-design.md`
> - `2026-05-18-im-feishu-phase1-design.md`
> - `2026-05-18-im-wecom-phase2-design.md`
> - `2026-05-18-im-telegram-phase3-design.md`
> - `2026-05-18-im-whatsapp-phase4-design.md`
> - `2026-05-18-im-wechat-phase5-design.md`

## 路线图一句话

把单平台 `connector/channel/` (钉钉 only) 演化成 trait-based 多平台架构，最终接入 6 个 IM：钉钉 / 飞书 / 企微 / Telegram / WhatsApp / 个微。每个 Phase 既加一个平台，也补一个共享抽象。

## Phase 0：trait 抽象落地（已完成）

| 落地物 | 备注 |
|---|---|
| `IMConnector` trait（5 方法）+ companion types | PR4 |
| `im/shared/` 拆出 router / ask_coordinator / config_store / pending_adapter / reply_manager / reconnect | PR2 |
| `im/dingtalk/` 拆 stream / card / download / token / registration / connector | PR3 |
| `im::factory::build_dingtalk_connector` 平台中性入口 | PR5 |
| `tests/review_im_layering.rs` 锁层 | PR6 |
| `tests/im_connector_cancel_test.rs` 锁 cancel-≤2s 契约 | PR6 |

**已知 leak（Phase 1 修订版 §0 收尾）**：
1. `shared::reply_manager` 仍然 import `dingtalk::card` —— AI Card 路径未走 trait `connector.send`
2. `ReplyTarget` 含钉钉特化字段（robot_code / reply_group_id / session_webhook）
3. `seen_msg_ids` 还 inline 在 manager.rs，没抽 `shared::dedup`
4. dingtalk `token::TokenCache` 没抽到 `shared::token`

## 跨 Phase 共享抽象时间线

每个 Phase 都给后续 Phase 留一份"工具"。下表按引入顺序排：

| # | 抽象 | 引入 Phase / PR | 给谁用 | 落地后允许下游做什么 |
|---|---|---|---|---|
| 1 | `ReconnectBackoff` (5/15/30/60s) | P0 PR2 | 所有平台 | 不再 hardcode 退避曲线 |
| 2 | `IMConnector` trait + factory | P0 PR4-5 | 所有平台 | 加新平台不动 manager |
| 3 | `shared::TokenCache<S>` | **P1 PR0a** | 飞书 / 企微 / 个微 | platform token 缓存复用 |
| 4 | `shared::MessageDedupSet` | **P1 PR0b** | 飞书 / Telegram long-poll / 个微 | webhook 重投递 / 长轮询去重 |
| 5 | dingtalk AI Card 走 `connector.send(AiCardChunk)` | **P1 PR0c** | 飞书 CardKit / wecom fallback / 所有流式平台 | reply_manager 路径通用化 |
| 6 | `ReplyTarget` 平台中性 + `observe_session` trait 方法 + `MarkdownSupport` 枚举 | **P1 PR0d** | wechat 群聊 chat_id 反查；wechat StreamingMarkdownFilter（Phase 4 修订版后 whatsapp 私聊不再需要 observe_session） | trait 真正抽象 |
| 7 | `shared::ChannelConfigStore` 多平台化 | P1 PR0d 同 PR | 飞书 / wecom / telegram / whatsapp / wechat | 每平台一份 config |
| 8 | `shared::WebhookServer` + register_or_replace + RAII guard | **P2 PR1** | wecom / telegram-webhook（Phase 4 v2 后 whatsapp 不再用 webhook，改 whatsapp-rust WS） | 接 webhook 平台 |
| 9 | `shared::AiCardFallbackBuffer` (placeholder / no-placeholder 双模式) | **P2 PR3 一次性落两个构造器（带 placeholder / no-placeholder）** | wecom / 个微（Phase 4 v2 后 whatsapp 不再用 fallback buffer，改占位+增量编辑路径） | AI Card 不支持时降级 |
| 10 | `outbound_text_streaming` capability + `InboundDeployment` 重命名（只保留 `SelfHosted / PublicWebhook`，**NativeDaemon 不实现**）+ **`ChannelConnectionState::NeedsReauth` 变体一并加入**（dingtalk / whatsapp / wechat 三家共享同款形状） | **P3 PR1.5** | telegram / 后续所有 capabilities 使用方 | trait 表达力提升 |
| 11 | `shared::SecretString` newtype（log 脱敏） | **P3 PR6.5** | 所有 secret 字段 | log 不漏 token |
| 12 | 前端 `RegistrationModal` 共抽（`url` + `qr_url` 两 mode） | **P5 PR0** | dingtalk / wechat / 未来扫码平台 | 注册 UI 复用 |
| 13 | `connector/im/wechat/` 内部 SessionGuard pause 机制 | **P5 PR4** | wechat（telegram/whatsapp 未来按需复用） | iLink session expired 不立刻终止 |

## 依赖 DAG（DOT 语法可视化）

```
P0 (已完成)
  ↓
P1-PR0a (shared/token) ─┐
P1-PR0b (shared/dedup) ─┤
P1-PR0c (dingtalk AI Card 接 trait) ─┤
P1-PR0d (ReplyTarget 中性 + observe_session + ChannelConfigStore 多平台化)
                                                                      │
P2-PR1 (webhook_server) ─┬─ (与 P0 / P1 并行) ──┐                     │
P2-PR2 (wecom crypto)   ─┘                       │                     │
                                                  ↓                     ↓
P3-PR1.5 (trait 改造: outbound_text_streaming + InboundDeployment) ────┤
P3-PR6.5 (SecretString) ─────── 独立并行 ─────────────────────────────────┘

P5-PR0 (前端 RegistrationModal 共抽) ─── 独立并行，可在任何阶段做

经过上面前置 PRs 后:
                                       ┌─ P1 (飞书) PR1-7
                                       ├─ P2 (wecom) PR3-7    (PR1/PR2 已在前置中)
                                       ├─ P3 (telegram) PR1-7  (PR1.5/PR6.5 已在前置)
                                       ├─ P4 (whatsapp) PR1-7
                                       └─ P5 (wechat) PR1-7    (PR0 已在前置)
```

## 推荐实施顺序（重要）

抽象演化有强依赖关系，但工程上可以**部分并行**。建议按 7 个阶段推进：

### 阶段 A：抽象前置（必须，~1 周）

**单纯抽取，不接新平台**：

1. P1-PR0a (shared/token.rs，dingtalk 接入)
2. P1-PR0b (shared/dedup.rs，manager 接入)
3. P3-PR6.5 (SecretString，全平台 sweep)

**风险高但收益高**：

4. P1-PR0c (dingtalk AI Card 接 trait，**真账号冒烟必跑**)
5. P1-PR0d (ReplyTarget 中性 + observe_session + ChannelConfigStore 多平台化)

阶段 A 完成后，所有 Phase 0 已知 leak 收完，trait 真正抽象。

### 阶段 B：trait 表达力 + Webhook 基础设施（与 A 并行，~3 天）

6. P3-PR1.5 (outbound_text_streaming + InboundDeployment 重命名)
7. P2-PR1 (webhook_server)
8. P2-PR2 (wecom crypto 圣经测试)
9. P5-PR0 (前端 RegistrationModal 共抽，dingtalk 切换)

### 阶段 C：飞书完整接入（~8 天）

10. P1-PR1 … PR7 (飞书 connector + 集成测试)

### 阶段 D：企微完整接入（~6-7 天，可与 C 部分并行）

11. P2-PR3 (aicard_fallback shared)
12. P2-PR4 … PR7 (wecom connector 主体)

### 阶段 E：Telegram 完整接入（~7-8 天）

13. P3-PR1 … PR7 (telegram connector 含 long-poll + webhook)

### 阶段 F：WhatsApp 完整接入（~11-12 天）

14. P4-PR1 … PR8 (whatsapp-rust WhatsApp Web 扫码路线 connector；2026-05-19 大改自 Cloud API)

### 阶段 G：个微完整接入（~9 天）

16. P5-PR1 … PR7 (wechat connector)

## 工期估算

| 阶段 | 单人工期 | 备注 |
|---|---|---|
| A 抽象前置 | 5 天 | PR0a/b 简单 1 天，PR6.5 1 天，PR0c 1.5 天（真账号冒烟），PR0d 1.5 天 |
| B trait + Webhook + UI 抽取 | 3 天 | 并行：A + B 重叠 → 总 5 天 |
| C 飞书 | 8 天 | |
| D 企微 | 6-7 天 | 与 C 部分并行（aicard_fallback 在 D 内做但不阻塞 C） |
| E Telegram | 7-8 天 | |
| F WhatsApp | 11-12 天 | 2026-05-19 路线变更：whatsapp-rust 协议实测 + AI Card 编辑路径 + TOS banner UI |
| G 个微 | 9 天 | |

**单人累加**：~54 天 ≈ 11 周
**双人并行（按 PR 边界拆）**：~7 周
**实际**：9-11 周（含 review buffer + 真账号冒烟 + 各平台账号申请等周期）

## 关键风险（跨 Phase）

| 风险 | 触发条件 | 缓解 |
|---|---|---|
| P1-PR0c 影响钉钉生产链路 | AI Card 路径迁到 connector.send 出 bug | **必须真账号冒烟**：私聊+群聊各 1 条流式 + 重连 + 重启 |
| P1-PR0d 影响面广（manager 多处构造 ReplyTarget） | 删字段后编译错误清扫 | 类型系统逼出来 + 一次性 PR 合并 |
| P3-PR1.5 影响所有已 merge connector | 加字段 + 重命名 enum | **一次性合并**不分多 PR，CI 编译失败指路 |
| P2 webhook_server guard 替换非原子 | 用户改 wecom 配置后必须重启 app | register_or_replace + generation 防错杀 |
| P5 iLink 接口变更（无 SemVer 保证） | 腾讯无预警改 API | 全程录 HTTP log 作回归资产 + canary 测试 |
| 各平台真账号申请周期 | 飞书 / wecom 企业认证 / ~~WhatsApp Meta 商业账号~~（2026-05-19 路线变更后 WhatsApp 改走扫码，不再需要 Meta Business 账号）| 提前申请 + 跑 mock 集成测试不阻塞前端 |
| **WhatsApp Web 路线特有：whatsapp-rust crate 0.1.x 协议跟进 / WhatsApp 风控 / TOS 灰区** | crate 实测 edge case 失败 / 自动化触发软风控 / 账号被封 | 前端首次扫码风险 banner + ignored 真账号 canary 测试 + 上线后实测调参 + 必要时给 upstream 提 PR；详见 Phase 4 spec §9 |

## 验收标准（每个平台收尾时）

| 标准 | 检查方法 |
|---|---|
| 单测 + 集成测试全绿 | `cargo test review_ --tests --no-fail-fast` + 各平台 `tests/im_<platform>_*` |
| `review_im_layering.rs` 含该平台 | grep `"<platform>"` in tests/review_im_layering.rs |
| 真账号冒烟通过 | 私聊 + 群聊 + 断网重连 + 重启 auto_connect（按平台特性裁剪） |
| 前端 UI 完整 | 注册 + 配置 + 删除 + 状态展示（连接中 / 已连接 / 错误 / NeedsReauth） |
| clippy 零新警告 | `cargo clippy --tests -- -D warnings` |

## 后续 Phase（不在本路线图）

| Phase | 内容 |
|---|---|
| 6 | 跨平台高级能力：workspace share link（解决 wecom 大附件超限）|
| 7 | 多账号 / 多 bot 支持（个微多账号扫码、telegram 多 bot 共存、whatsapp 多号挂同桌面）|
| 8 | ~~template 设计工具（whatsapp template 在 AIjia 内审批提交）~~（2026-05-19 WhatsApp 路线变更后无意义，已废弃；如未来恢复 Cloud API 路径再考虑） |
| 9 | webhook 模式 cloudflared 一键穿透集成 |
| 10 | WhatsApp 出站媒体（`ReplyContent::Image / File` trait 扩展 + send_image / send_document）—— Phase 4 v2 仅做入站下载，出站需 trait 改动统一规划 |
| - | 各平台 polish：高级消息类型、interactive button、richer media |

后续 Phase 不阻塞 6 个平台的 MVP 接入。

## 文档维护约定

- 每个 Phase 的 spec 修订必须**同步更新本路线图**（共享抽象列表 + DAG + 估时）
- 实施过程中如果发现某个抽象演化逆向（比如 Phase 5 删掉 NativeDaemon），更新 spec **并**在本路线图标记
- 路线图比 spec 更新频繁，spec 是设计快照，路线图是工程视图

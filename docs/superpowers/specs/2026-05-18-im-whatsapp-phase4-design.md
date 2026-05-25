# Phase 4：WhatsApp Web (Multi-Device, whatsapp-rust) IM Connector

**日期**：2026-05-18（2026-05-19 大改：从 Meta Cloud API 路线整体重写为 WhatsApp Web 扫码路线）
**状态**：Design draft v2 → 待用户 review
**前置**：Phase 0 + Phase 1（修订版 §0 PR0b/PR0d）+ Phase 3（PR1.5 trait 改造 + PR6.5 SecretString）+ Phase 5 PR0（前端 RegistrationModal 共抽）已落地
**Scope**：在 `connector/im/whatsapp/` 实现 IMConnector（基于 [whatsapp-rust](https://github.com/jlucaso1/whatsapp-rust) 纯 Rust crate）

## §0. 路线变更说明

### 0.1 为什么改

**原 spec（v1）** 走 Meta Cloud API + 公网 webhook + 24h 会话窗口 + template 预审。这条路要求用户：

- 注册 Meta Business 账号（个人 / 中小团队基本做不到）
- 配置公网 webhook URL（AIjia 是桌面 app，没有公网回调能力）
- 在 Meta 后台预审 message template（业务流，不是装一下就能用）
- 接受 24h 会话窗口约束（超窗口 AI 无法主动回复）

这跟 AIjia 桌面 app（个人 / 中小团队 AI 助手）产品形态完全不匹配，跟 Phase 5 个微（iLink HTTP 长轮询 + 扫码，`InboundModel::Stream`）也不对称。

**为什么 v1 写错了**：Phase 5 个微 spec 之所以正确，是因为它直接基于 `~/Downloads/openclaw channel/openclaw-weixin-main` 反推出协议事实。Phase 4 v1 当时没有等价 openclaw 参考目录，照 Meta 官方文档写了，方向走偏。

### 0.2 OpenClaw 官方怎么连 WhatsApp

参考 [OpenClaw 官方文档](https://docs.openclaw.ai/channels/whatsapp) + [openclaw/wacli](https://github.com/openclaw/wacli)：

- **协议**：[Baileys](https://github.com/WhiskeySockets/Baileys)（Node.js 实现的 WhatsApp Web multi-device 协议）
- **认证**：手机 WhatsApp"设置 → 已链接的设备 → 链接设备"扫码（QR linked device）
- **运行形态**：gateway 持有 WS socket + reconnect loop；凭证 `~/.openclaw/credentials/whatsapp/<accountId>/creds.json`
- **inbound 推送**：WebSocket 长连，**不**走 HTTP webhook
- **状态**：`production-ready via WhatsApp Web (Baileys). Gateway owns linked session(s).`

### 0.3 新路线：wa-rs 纯 Rust crate（whatsapp-rust 的 stable fork）

跟 Phase 5 个微同形 → `InboundModel::Stream`（WebSocket）+ 扫码 + 凭证本地存储 + 桌面形态对齐。Rust 端**实际用** [`wa-rs`](https://github.com/homunbot/wa-rs)（[jlucaso1/whatsapp-rust](https://github.com/jlucaso1/whatsapp-rust) 的 stable-Rust fork，移除 `#![feature(portable_simd)]` 和 `if_let_chains`——上游需要 nightly，桌面仓库 MSRV 1.77.2 + stable 编不过；fork 维护活跃度低，上线前考虑 vendor 一份或给 upstream 提 PR）。本质仍是 whatsmeow + Baileys 协议移植，**不**引 Node sidecar、**不**走 FFI 调 Go whatsmeow、**不**用 Meta Cloud API。

### 0.4 用户已确认产品决策（7 项）

1. **多账号**：单账号（同 Phase 5 wechat）—— 一个 AIjia 实例 = 一个 WhatsApp 号，换号必须重新扫码覆盖
2. **群聊**：MVP 私聊 only（不支持群聊；whatsapp-rust 群事件本来免费，弃用）
3. **AI Card 流式**：占位 + 增量编辑（**不**走 Phase 5 个微的静默累积路径）
4. **编辑速率上限**：保守 — 距上次编辑 ≥2s + 单 session 编辑次数 ≤6 次
5. **媒体范围**：文本 + IMAGE（≤5MB）+ FILE（≤100MB）双向；VOICE / VIDEO / STICKER 走占位
6. **凭证存储**：whatsapp-rust `SqliteStore` session.db 走 OS 文件权限 + scope 隔离，**不**额外加密
7. **Rust 实现**：whatsapp-rust crate（拒绝 Node Baileys sidecar / whatsmeow-rs FFI / Meta Cloud API）

### 0.5 已知风险（Cloud API 路线根本没有的）

- wa-rs crate 0.2.x（whatsapp-rust 0.6.x 的 stable-Rust fork），协议跟进可能滞后；部分 edge case 需实施时实测；必要时给 upstream 提 PR
- WhatsApp Web 多设备协议是 **TOS 灰区**，账号风险（封号 / 节流）用户自担——前端需明确告知
- VoIP / Google Voice 号被快速封；编辑速率风控敏感

§9 风险表逐条登记。

## §1. 部署形态 + capabilities

```rust
ConnectorCapabilities {
    inbound: InboundModel::Stream,           // whatsapp-rust 是 WS 长连
    outbound_aicard: false,                  // 不发原生 AI 卡片
    outbound_text_streaming: true,           // 走 whatsapp-rust edit API；Phase 3 PR1.5 新加字段
    outbound_markdown: false,                // 仅支持 *粗体* / _斜体_，非完整 markdown
    supports_attachments: true,              // IMAGE / FILE 双向
    supports_group_chat: false,              // MVP 私聊 only
    supports_private_chat: true,
    auth_flow: AuthFlow::QRCode,
}
```

跟其他平台的对比：

| capability | WhatsApp（新） | Phase 5 个微 | 钉钉 |
|---|---|---|---|
| `inbound` | `Stream` | `Stream` | `Stream` |
| `outbound_text_streaming` | **`true`**（编辑路径） | `false` | `true`（AI Card） |
| `outbound_aicard` | `false` | `false` | `true` |
| `supports_group_chat` | `false` | `true` | `true` |
| `auth_flow` | `QRCode` | `QRCode` | `DeviceCode` |

**为什么 `outbound_text_streaming: true`**：跟其他无 edit API 的平台不同，whatsapp-rust 提供 `SendOptions::edit(target_msg_id, new_body)`，所以 AI Card ��式可以走"占位 + 增量编辑"路径而不需要降级到 final-only。这是该 capability 在 trait 里第一个被设成 `true` 的**非钉钉**平台。

**反向影响 Phase 3 PR1.5**：`outbound_text_streaming` capability 必须由 Phase 3 PR1.5 实际加入 trait，Phase 4 才能正确填写。路线图共享抽象表第 10 行涵盖。

**`Platform` enum 扩展**：当前 `src-tauri/src/connector/im/types.rs::Platform` 只有 4 个变体（Dingtalk / Feishu / Wechat / Wecom），需要 Phase 4 PR1 加 `Whatsapp` 变体 + `as_str() / from_str() / all()` 同步更新。

## §2. 目录结构

```
src-tauri/src/connector/im/whatsapp/
├── mod.rs              # impl IMConnector for WhatsAppConnector + factory 入口
├── connector.rs        # WhatsAppConnector 结构体 + Bot 生命周期 + ConnectorContext 注入
├── login.rs            # begin_registration / poll_registration（QR 扫码流程）
├── runtime.rs          # bot.run() event loop + 事件 dispatch + mpsc 桥接 BoxStream
├── sender.rs           # send text/markdown 入口 + 错误映射 + ReplyContent 路由
├── aicard.rs           # 占位 + 增量编辑 状态机（2s 间隔 / ≤6 次）
├── parser.rs           # whatsapp-rust Event::Message → ChannelMessage（群事件 drop）
├── session.rs          # SqliteStore 路径管理 + Pairing 过程状态
├── markdown.rs         # strip → WhatsApp 受限格式（*粗体* / _斜体_）
└── types.rs            # 内部 enum / newtype（JID / MessageRef / PairingState）
```

**跟 Phase 5 个微对照**：同样 10 个文件，结构同形。差异点：

- 多了 `aicard.rs`（个微走静默累积复用 shared buffer，whatsapp 是独有编辑状态机）
- 没有 `crypto.rs`（个微的 AES-128-ECB 是 iLink 媒体协议特化；whatsapp-rust 内部已封装 Signal Protocol 加解密）
- 没有 `media.rs`（whatsapp-rust 直接暴露 send_image / download_media，媒体处理在 sender.rs / parser.rs 内）

**`aicard.rs` 不抽 shared**：当前只有 whatsapp 需要（钉钉走原生 AI Card / 飞书走 CardKit / 个微+企微+telegram-webhook 走静默累积）。如果将来真有第二个平台用同款，再抽。这是 CLAUDE.md 的 YAGNI 原则。

## §3. 扫码登录流程（PR2-3，v3 OpenClaw-aligned）

### 3.0 v3 修订说明（2026-05-20）

原 spec v2 设计了 `_pairing/session.db → {jid}/session.db` rename + `bot.stop().await` race 防护 + 8 状态 `PairingState`。实施 PR2 brainstorm 阶段读 wa-rs 0.2 源码发现：

- wa-rs `Bot` 没有 `stop()` 方法；只能 `JoinHandle::abort()`
- wa-rs `SqliteStore` 没有 `close()`/`flush()`；drop 时不等 in-flight `spawn_blocking` 写入落盘
- 这两个事实让 spec v2 的 race 防护设计直接落空

同时参考了 [openclaw/extensions/whatsapp/src/](https://github.com/openclaw/openclaw) 真实实现（Baileys + `useMultiFileAuthState`）：

| OpenClaw 做法 | 我们对齐 |
|---|---|
| 凭证目录 `oauth/whatsapp/{accountId}/`，单账号 `accountId="default"` | 我们单账号下连 `default/` 子目录也省，直接 `channels/whatsapp/` |
| `creds.json` + `creds.json.bak`（启动时自动备份+回退） | 我们 `session.db` + `session.db.bak` |
| 凭证目录跟 JID 无关，JID 只是 creds 里一个字段 | 我们 JID 写在 `config.json` 元数据 |
| 不 rename，全程固定路径 | 同 |
| `replaceFileAtomic` 写 creds（tmp+rename+fsync） | wa-rs 的 SQLite WAL + synchronous=NORMAL 已提供同等耐久性 |

**v3 决策**：抄 OpenClaw 形态，砍掉 `_pairing/` 临时路径 + rename + race 防护 + 8 状态 PairingState。

### 3.1 凭证存储路径

```
~/.renlijia/users/{scope}/channels/whatsapp/
├── session.db          # wa-rs SqliteStore（Noise key / Signal session / device id 等）
├── session.db.bak      # 启动前自动备份；启动后如 session.db 损坏可回退（OpenClaw 同款思路）
└── config.json         # AIjia 元数据：{ schemaVersion: 1, jid, pushName, pairedAt }
```

**单账号约束**（spec §0.4 决策 #1）下永远只有一组文件，路径完全固定。换号 = 删 `session.db` + `config.json`（保留 `.bak` 给紧急回退）→ 重新扫码。

JID 不出现在路径里。运维查"这是哪个 WhatsApp 号"读 `config.json`，比看目录名更可靠（用户改不动元数据字段名）。

### 3.2 不额外加密 session.db

跟 dingtalk 现有 secret 存储一致——靠 OS 文件权限（chmod 600 在 Unix；ACL 限制在 Windows）+ scope 隔离。**不**走 SecureStorage AES-256-GCM 整体加密（wa-rs SqliteStore 写入路径无法 wrap）。

设备私钥（noise_key / signed_pre_key 等）也不单独冷备份；`.bak` 是整文件复制，简单。

### 3.3 启动备份策略

`WhatsAppConnector::start()` 启动 Bot **之前**：

```
if session.db.存在 && size > 0:
    fs::copy(session.db, session.db.bak)   // 覆盖旧 .bak
```

启动后**不**检查 session.db 是否能正常打开；wa-rs 自己会在 `SqliteStore::new` 里报错，这种情况上层把 `.bak` 回滚回去（PR2 只做"写 bak"，"启动失败回滚"留到 PR4 集成测试发现实际损坏概率后再决定）。

### 3.4 关闭 Bot 的实现

wa-rs 没 graceful shutdown。我们的策略：

```rust
async fn stop(&self) -> Result<(), ConnectorError> {
    if let Some(handle) = self.bot_handle.lock().await.take() {
        handle.abort();
        // 不 await handle.await——abort 后 await 会返回 Err(JoinError::Cancelled)
        // 这正是我们想要的；不需要等"任务全跑完"
    }
    // 让 SqliteStore 的 Arc 自然回收（r2d2 池会在最后一个 clone drop 时关闭连接）
    Ok(())
}
```

SQLite WAL + `synchronous = NORMAL`（wa-rs 默认 PRAGMA）保证已 fsync 的写入不会丢；abort 期间正在 `spawn_blocking` 的事务可能丢失最后几条 Signal session 增量。**这种丢失等价于断网**——下次起 Bot 时 wa-rs 自己用 Signal 协议重新 fetch 缺失部分。**.bak 是兜底**：如果 session.db 因 abort 时机不巧损坏，下次启动 wa-rs 报错，上层从 `.bak` 恢复。

### 3.5 PairingState（4 状态）

```rust
enum PairingState {
    Idle,                                              // 没开始
    AwaitingQr { started_at: Instant },                // run() 起来但 PairingQrCode 还没到
    QrIssued { code: String, expires_at: Instant },    // QR 已下发给前端
    Connected { jid: String, push_name: String },      // PairSuccess + Connected 都到了
}
```

`AwaitingDeviceConfirm` / `Expired` / `Cancelled` / `Failed` 砍掉——从超时 / 错误 event 派生，不需要单独存：

- "用户扫了码但 Connected 还没到" → QrIssued 状态不变（等收到 `Event::PairSuccess` + `Event::Connected` 直接跳 Connected）
- "QR 过期" → `QrIssued.expires_at` 在 poll 时算
- "用户取消" → connector cancel_token cancelled → state reset 到 Idle
- "失败" → 错误冒泡成 `ConnectorError::Fatal`，poll 返回 `Fail`

`poll_registration` 状态映射：

| PairingState | ChannelRegistrationPollState |
|---|---|
| Idle / AwaitingQr | Waiting |
| QrIssued（未过期） | Waiting（QR 通过 `verification_uri_complete` 字段返回） |
| QrIssued（已过期） | Expired |
| Connected | Success |

### 3.6 扫码流程（PR3 真做）

```
用户点"添加 WhatsApp 账号"
   ↓
（如果 config.json 已存在 = 重新扫码场景）
   manager 删 config.json + session.db（保留 session.db.bak 兜底）
   ↓
WhatsAppConnector::begin_registration()
   ↓ paths.ensure_base_dir() + session::backup_session_db_if_present()
   ↓ SqliteStore::new(session.db).await
   ↓ Bot::builder().with_backend(store).on_event(closure 捕获 pairing_state Arc<Mutex>)
   ↓   .build().await → bot.run() → JoinHandle 存到 self.bot_handle
   ↓ *pairing_state = AwaitingQr { started_at: now }
   ↓ **立即返回**（不等 PairingQrCode）：
     RegistrationBegin {
        source: "whatsapp_web",
        verification_uri_complete: "",       // 占位；QR 还没生成
        device_code: "whatsapp",             // 单账号约定常量
        expires_in_seconds: 60,
     }
   ↓
前端 RegistrationModal (mode='qr_url') 开始 poll
   ↓
（异步）wa-rs 启动建连 → Event::PairingQrCode { code, timeout } 到达
   ↓ on_event 闭包：*pairing_state = QrIssued { code, expires_at: now + timeout }
   ↓
poll_registration() 每 2s 调一次：
  - PairingState::AwaitingQr → Waiting，verification_uri_complete=None
  - PairingState::QrIssued（未过期）→ Waiting，verification_uri_complete=Some(code)
  - PairingState::QrIssued（已过期）→ Expired
  - PairingState::Connected → Success + config_view
   ↓
前端拿到 QR string 用 qrcode lib 渲染图像（同 wechat mode='qr_url' 路径）
   ↓ 用户在手机 WhatsApp"设置 → 已链接的设备 → 链接设备"扫码
   ↓
wa-rs 内部：扫码完成 → Event::PairSuccess { id: jid, push_name, ... } → Event::Connected
   ↓ on_event 收到 PairSuccess：
     - 写 config.json { jid, push_name, paired_at: now }
     - *pairing_state = Connected { jid, push_name }
   ↓
下一次 poll → ChannelRegistrationPollState::Success

**关键设计点**：begin_registration 立即返回，不等 PairingQrCode 事件（跟 wechat
begin_wechat_registration 同形）。原因：① wa-rs 起 Bot + 建连有几百毫秒延迟，
让 begin 同步等会阻塞 Tauri 调用；② 用 PairingState 做"事件 → 拉取"桥接更解耦，
poll 拿到 QR 时机由 wa-rs 自己决定。
```

### 3.7 复用现有 Tauri 命令

`channel_begin_registration` / `channel_poll_registration`（`src-tauri/src/commands/channel.rs`）不需要新增，只把 platform 参数走 `"whatsapp"` 分支。

### 3.8 复用前端 RegistrationModal

复用现有 `mode='qr_url'`（wechat 的同款）—— wa-rs `Event::PairingQrCode.code` 是 raw string，前端 `qrcode` lib 客户端渲染成图像。**不**新加 `qr_image` mode（PR4 spec v2 旧设计的需求点已不存在）。

### 3.9 重新扫码（NeedsReauth → 新设备）

```
NeedsReauth 卡片点击"重新扫码"
   ↓
前端调 channel_begin_registration
   ↓
后端：
  1. 旧 connector.stop()（JoinHandle.abort）
  2. cp session.db session.db.bak（启动前备份兜底）
  3. fs::remove_file(session.db)（让 wa-rs 走 fresh pairing）
  4. fs::remove_file(config.json)
  5. 起新 connector → Event::PairingQrCode → 返回 QR
   ↓ 前端弹 RegistrationModal
   ↓ 用户扫码成功 → wa-rs 写新 session.db + PairSuccess → 写新 config.json
   ↓ Connected
```

**比 v2 简化的点**：v2 在 `_pairing/session.db` 扫码成功后 rename 到 `{new_jid}/`，且"扫码成功后才删旧 db"防止误操作丢凭证。v3 直接用 `.bak` 兜底——如果用户重新扫码失败，旧 `session.db.bak` 还在，可以手动改名回去；自动化的"扫码成功后才删旧 db"不需要写代码维护。

### 3.10 allowFrom allowlist（v3 新增，跟 OpenClaw 对齐）

**目的**：降低封号风险 + 避免对推销/陌生人发的 WhatsApp 都立刻 AI 回复（机器人特征更不明显）。

**数据形态**：`config.json` 加字段：

```jsonc
{
  "schemaVersion": 1,
  "jid": "8613800138000@s.whatsapp.net",
  "pushName": "Alice",
  "pairedAt": "2026-05-20T10:30:00Z",
  "allowFrom": ["+8613912345678", "+8613987654321"]   // 可选；空数组 / 不存在 = 回复所有
}
```

**号码格式**：E.164（+86 + 11 位手机号）。前端 UI 一行一个，自动规整：
- 去掉空格 / 短横
- 加 `+` 前缀如果用户没填
- WhatsApp JID 形如 `8613800138000@s.whatsapp.net`，过滤时比对 jid 前缀对应的 E.164

**filter 时机**：PR4 入站 worker `Event::Message` 进 `parser.normalize` 之前。
- `allowFrom` 缺失或空数组 → 放行所有（默认行为，向后兼容）
- 已配置 allowlist 但来源不在列表 → drop（log debug，不返 error）
- 自己发的（`info.source.is_from_me=true`）总是 drop（既不在 allowlist 也不该 AI 自己回自己）

**重新配置不需要重启 bot**：connector 每次入站事件**重读** config.json 中的 allowFrom（或 manager 在 config 改变时调 `WhatsAppConnector::refresh_config()` 重新加载）。MVP 实现：connector 持 `Arc<RwLock<Vec<String>>>`，PR3 配置 UI 改 allowlist 时也调 `refresh`。

### 3.11 Reaction 作为"AI 收到了"信号（v3 新增）

**目的**：替代 §6.4 现在的 `_正在生成回复..._` 斜体占位文案。emoji reaction（⏳）跟 WhatsApp 默认行为更接近，机器人特征更不明显。

**实现**：PR6 时实施。wa-rs 0.2 **没有** `send_reaction()` API；需自己构造 `wa::Message { reactionMessage: ReactionMessage { key: <target_msg_key>, text: "⏳" } }` 然后调 `client.send_message(jid, msg)`。

**状态机调整**（§6.1 占位 + 编辑改为 reaction + 编辑）：
1. 第一条 chunk 到 → 发 ⏳ reaction 给用户那条原消息（不再发占位文本消息）
2. 后续 chunk 到达 → 累积到 `accumulated_text`，**不**编辑 reaction（reaction 只是"收到"信号，AI 真实回复才走 send_text）
3. 达到 §6.2 触发条件（≥2s 距上次 / ≤6 次）→ 发 send_text + edit_message 路径
4. final_chunk 到 → 把 ⏳ reaction 换成 ✅（成功）/ ❌（失败）

**降级**：如果 reaction 调用失败（wa-rs 不支持 / WhatsApp 版本旧），自动回退到 §6.4 文本占位。降级策略由 PR6 实施时实测决定。

### 3.12 入站 quoted reply 解析（v3 新增）

**目的**：用户在 WhatsApp 上 quote 一条旧消息发给 AI 时，AI 知道引用的内容是什么会更智能。

**实现**：PR4 parser 时实施。WhatsApp protobuf 的 quoted 信息在 `ExtendedTextMessage.contextInfo.quotedMessage`（也可能在 ImageMessage 等各 message 变体里的 contextInfo）。parser 提取后塞到 `ChannelMessage.text` 前面：

```
[引用了消息："这是被引用的内容"]
用户发的实际消息体
```

**回退**：如果 contextInfo 缺失或 quotedMessage 解析失败，普通 message 不 prefix，照常处理。protobuf 字段是 optional，不存在时不报错。

## §4. 入站事件 → ChannelMessage（PR4）

### 4.1 worker task

```rust
async fn start(
    &self,
    ctx: ConnectorContext,
) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
    let (tx, rx) = mpsc::channel::<ChannelMessage>(64);
    let bot = self.bot.clone();              // Arc<Bot>，PR2 在 connector.rs 构造
    let parser = self.parser.clone();
    let dedup = Arc::clone(&self.dedup);     // shared::MessageDedupSet（Phase 1 PR0b）
    let cancel = ctx.cancel_token.clone();

    self.install_event_handler(tx, parser, dedup);

    tokio::spawn(async move {
        let mut backoff = ReconnectBackoff::default_schedule();  // shared (Phase 0 PR2)
        loop {
            tokio::select! {
                _ = cancel.cancelled() => { let _ = bot.stop().await; break; }
                res = bot.run() => {
                    match res {
                        Ok(()) => break,
                        Err(WaError::AuthRevoked) => {
                            log::warn!("[whatsapp] session revoked, ending stream");
                            break;  // stream 关 → manager 设 NeedsReauth
                        }
                        Err(WaError::Transient(e)) => {
                            let d = backoff.next_delay();
                            log::info!("[whatsapp] transient: {e}, sleep {d:?}");
                            tokio::time::sleep(d).await;
                        }
                        Err(WaError::Fatal(e)) => {
                            log::error!("[whatsapp] fatal: {e}");
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(Box::pin(ReceiverStream::new(rx)))
}
```

### 4.2 事件 dispatch（`runtime.rs`）

```rust
fn install_event_handler(&self, tx, parser, dedup) {
    self.bot.on_event(move |event| {
        let tx = tx.clone();
        let parser = parser.clone();
        let dedup = Arc::clone(&dedup);
        async move {
            match event {
                Event::Message(msg, info) => {
                    if info.is_group { return; }                 // 群事件 drop（私聊 only）
                    if !dedup.observe(&info.id).await { return; } // 去重
                    if let Some(cm) = parser.normalize(&msg, &info) {
                        let _ = tx.send(cm).await;
                    }
                }
                Event::PairingQrCode { code, .. } => {
                    self.pairing.observe_qr(code).await;        // 走 §3 PairingState
                }
                Event::Connected => {
                    self.pairing.observe_connected().await;
                }
                Event::Disconnected { reason } => {
                    log::info!("[whatsapp] disconnected: {reason:?}");
                    // whatsapp-rust 内部会重连；不破坏 stream
                }
                Event::Receipt(_) | Event::PresenceUpdate(_) => { /* drop */ }
                _ => {}
            }
        }
    });
}
```

### 4.3 parser 规则（`parser.rs`）

| 入站类型 | 映射 |
|---|---|
| 私聊 TEXT | `ChannelMessage { conversation_type: Private, text, attachments: [] }` |
| 私聊 IMAGE | `ChannelMessage { text: caption.unwrap_or(""), attachments: [Image] }` |
| 私聊 DOCUMENT | `ChannelMessage { text: caption.unwrap_or(""), attachments: [File] }` |
| 私聊 VOICE | `ChannelMessage { text: "[不支持的消息类型：语音]", attachments: [] }` |
| 私聊 VIDEO | `ChannelMessage { text: "[不支持的消息类型：视频]", attachments: [] }` |
| 私聊 STICKER | `ChannelMessage { text: "[不支持的消息类型：表情贴纸]", attachments: [] }` |
| 私聊 Location | `ChannelMessage { text: "[不支持的消息类型：位置]", attachments: [] }` |
| 私聊 Contact | `ChannelMessage { text: "[不支持的消息类型：联系人]", attachments: [] }` |
| 群事件（任意类型） | **drop**，log debug |
| 编辑入站 / 撤回入站 | drop（MVP 不处理） |

不支持类型保留 text + sender 信息，让 AI 知道用户发过东西但内容不可用，可以 contextual 回复（"我看不到你的语音，能打字说吗"）。

### 4.4 dedup + reconnect 策略

- **dedup**：`shared::MessageDedupSet`（Phase 1 PR0b）防 whatsapp-rust 在边界重投递；key 用 WhatsApp 自带的 `message.key.id`
- **reconnect**：whatsapp-rust **内部**有断线重连；**外层** `bot.run()` 返回 Err 才走 `shared::ReconnectBackoff`。避免双层退避相互踩
- **`Event::Disconnected` drop**：whatsapp-rust 自带重连会接管；外层只 log，不破坏 stream

### 4.5 没有 observe_session 依赖

私聊 only，`ReplyTarget.session_id` → `external_conversation_key`（jid）的反查只需 connector 内存 map，**不**需要 trait `observe_session`（这点跟 Phase 1 PR0d 范围相关：whatsapp 不再要求该 trait 方法，仅 Phase 5 wechat 群聊需要）。

## §5. 出站 text / markdown（PR5）

### 5.1 send 入口

```rust
async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
    let jid = self.resolve_jid(&target.session_id).await
        .ok_or_else(|| ConnectorError::Fatal(
            format!("whatsapp: unknown session_id {}", target.session_id)
        ))?;

    match content {
        ReplyContent::Text(s) => self.send_text(&jid, &s).await,
        ReplyContent::Markdown(s) => {
            let plain = markdown::strip_to_wa(&s);  // 仅保留 *粗体* / _斜体_
            self.send_text(&jid, &plain).await
        }
        ReplyContent::AiCardChunk { delta, final_chunk } => {
            self.aicard_handle(&target, &jid, delta, final_chunk).await
        }
        ReplyContent::AiCardFail => {
            self.aicard_fail(&target, &jid).await
        }
    }
}

async fn send_text(&self, jid: &str, body: &str) -> Result<(), ConnectorError> {
    match self.bot.send_text(jid, body).await {
        Ok(_) => Ok(()),
        Err(WaError::AuthRevoked)         => Err(ConnectorError::AuthExpired("session revoked".into())),
        Err(WaError::RateLimited)         => Err(ConnectorError::Transient("WA rate limit".into())),
        Err(WaError::NetworkTransient(e)) => Err(ConnectorError::Transient(e.to_string())),
        Err(WaError::Fatal(msg))          => Err(ConnectorError::Fatal(msg)),
    }
}
```

### 5.2 markdown strip 规则（`markdown.rs`）

| 输入 markdown | 输出 |
|---|---|
| `**粗体**` | `*粗体*` |
| `*斜体*` 或 `_斜体_` | `_斜体_` |
| `# 标题 / ## 二级` | 标题文字（前缀去掉） |
| `` `code` `` | `code`（反引号去掉） |
| ` ```block``` ` | ``` `block` ```（WhatsApp 不支持多行 code，转单行 + 换行原样保留） |
| `[link](url)` | `link (url)` |
| `> 引用` | 引用文字 |
| `- 列表` / `1. 列表` | `• 列表`（统一 bullet） |

跟 dingtalk / feishu 的 markdown 处理路径完全独立——这是 whatsapp 内部规则，不抽 `shared::markdown_simple`（Phase 5 spec 提过的"将来再抽"伏笔保留，但本期不做）。

### 5.3 错误映射

| whatsapp-rust 错误 | ConnectorError | manager 行为 |
|---|---|---|
| `AuthRevoked`（device unlinked / session expired） | `AuthExpired(reason)` | 设 `NeedsReauth` 状态 + 前端 ⚠️ + "重新扫码"按钮 |
| `RateLimited`（WhatsApp 服务端节流） | `Transient` | shared::ReconnectBackoff 退避 + 重试 |
| 网络 timeout / 连接抖 | `Transient` | 同上 |
| 4xx 永久错（无效 jid / 消息过长） | `Fatal(msg)` | 不重试 |

### 5.4 送达成功定义

`send_text` 返回 `Ok` 即视为成功；**不**等 ACK（送达 receipt），**不**等 read receipt。跟 Phase 5 个微一致。如果未来产品需要"已读"反馈，单独建 issue。

### 5.5 markdown / aicard 路径分离

`ReplyContent::Markdown` 直接 strip + send_text（**不**走编辑路径），因为 markdown 是"已完成的内容一次性发"，不需要流式编辑。`ReplyContent::AiCardChunk` 才是流式入口（§6）。

## §6. 出站 AI Card：占位 + 增量编辑（PR6）

### 6.1 状态机

```rust
struct WhatsAppAiCardSession {
    placeholder_msg_id: Option<MessageId>,   // 第一条 chunk 发出后填入
    accumulated_text: String,
    last_edit_at: Option<Instant>,
    edit_count: u32,
    finalized: bool,                          // 防 final 后又收到 chunk
}

// connector 内部
fallback_buffers: Mutex<HashMap<SessionId, WhatsAppAiCardSession>>
```

### 6.2 触发流程

```rust
async fn aicard_handle(
    &self,
    target: &ReplyTarget,
    jid: &str,
    delta: String,
    final_chunk: bool,
) -> Result<(), ConnectorError> {
    let mut sessions = self.fallback_buffers.lock().await;
    let session = sessions.entry(target.session_id.clone())
        .or_insert_with(WhatsAppAiCardSession::default);

    if session.finalized {
        log::warn!("[whatsapp] chunk after finalized for {}", target.session_id);
        return Ok(());
    }

    session.accumulated_text.push_str(&delta);

    match (session.placeholder_msg_id.as_ref(), final_chunk) {
        // 1st chunk + 不是 final：发占位
        (None, false) => {
            let msg_id = self.bot.send_text(jid, "_正在生成回复..._").await?;
            session.placeholder_msg_id = Some(msg_id);
            session.last_edit_at = Some(Instant::now());
            session.edit_count = 1;        // 占位发出 = 1 次"编辑额度"
            Ok(())
        }
        // 1st chunk + final：直接发完整文本，不走占位
        (None, true) => {
            self.bot.send_text(jid, &session.accumulated_text).await?;
            session.finalized = true;
            drop(sessions);
            self.cleanup_aicard(target).await;
            Ok(())
        }
        // 后续 chunk：判断是否触发编辑
        (Some(msg_id), is_final) => {
            let elapsed = session.last_edit_at
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO);
            let should_edit = is_final
                || (elapsed >= Duration::from_secs(2) && session.edit_count < 6);
            if !should_edit {
                return Ok(());  // 静默累积，等下一次触发
            }
            self.bot.edit_message(jid, msg_id, &session.accumulated_text).await?;
            session.last_edit_at = Some(Instant::now());
            session.edit_count += 1;
            if is_final {
                session.finalized = true;
                drop(sessions);
                self.cleanup_aicard(target).await;
            }
            Ok(())
        }
    }
}

async fn aicard_fail(&self, target: &ReplyTarget, jid: &str) -> Result<(), ConnectorError> {
    let mut sessions = self.fallback_buffers.lock().await;
    if let Some(session) = sessions.get(&target.session_id) {
        if let Some(msg_id) = session.placeholder_msg_id.as_ref() {
            let _ = self.bot.edit_message(jid, msg_id, "_[生成失败]_").await;
        }
    }
    sessions.remove(&target.session_id);
    Ok(())
}
```

### 6.3 速率上限触发后的退化

单 session `edit_count` 达到 6 后，后续 chunk **静默累积**（不编辑），**直到 final** 强制再编辑一次（把完整结果落到占位消息）。即"6 次"上限被 final 突破一次，保证用户最终看到的是完整答案。

### 6.4 占位文案选择

`_正在生成回复..._`（斜体）：

- 跟 WhatsApp 默认 typing 提示形态接近
- 不用 emoji（避免风控启发式识别"机器人回复"特征更显著）
- 不用 `[`...`]`（避免与 Phase 5 个微的 `[不支持的消息类型]` 占位混淆）

### 6.5 cleanup

- final 后立即从 `fallback_buffers` 移除该 session
- connector restart 时所有 in-flight session 直接丢弃（不持久化，跟 Phase 5 个微 fallback 策略一致）
- restart 后用户看到的"正在生成回复..."不会再更新——这是已知降级，§9 风险表登记

## §7. 媒体（仅入站，PR7）

### 7.1 范围

**仅入站**：IMAGE / FILE 下载到本地 tmp + ChannelAttachmentSpec 喂给 AI。

**出站**：当前 `ReplyContent` enum 没有 Image / File 变体——主链路里 AI 不会主动给用户回图。出站留给后续 Phase 配合 trait 扩展统一加，**不**在本期推动 trait 改动。

### 7.2 下载（入站）

```rust
// parser.rs 内
async fn parse_image(&self, msg: &Message, info: &MessageInfo) -> Option<ChannelMessage> {
    let image = msg.image.as_ref()?;
    // 走 whatsapp-rust 的下载 API（自动解密 + 解压）
    let bytes = self.bot.download_media(&image.media_ref).await
        .map_err(|e| log::warn!("[whatsapp] download failed: {e}"))
        .ok()?;
    let path = self.write_tmp_file(&bytes, &image.mime_type).await.ok()?;
    Some(ChannelMessage {
        text: image.caption.clone().unwrap_or_default(),
        attachments: vec![ChannelAttachmentSpec::Image { path, ... }],
        ..base
    })
}
```

- **临时文件路径**：`~/.renlijia/tmp/whatsapp_downloads/{msg_id}.{ext}`
- **ext** 从 mime_type 推（image/jpeg → .jpg / application/pdf → .pdf）
- **老化清理**：跟 Phase 5 个微同款，每 1h 扫描一次，删 25h 前的（manager 层 cron 已有就复用；没有的话本期不抽 shared）

### 7.3 Size 检查（仅占位）

```rust
const IMAGE_SIZE_LIMIT_BYTES: usize = 5 * 1024 * 1024;     // 5MB
const FILE_SIZE_LIMIT_BYTES: usize = 100 * 1024 * 1024;    // 100MB
```

下载时**不**做 size 检查（来源是用户，size 由 WhatsApp 服务端约束）。上限常量为未来出站链路预留。

### 7.4 不支持的媒体类型

见 §4.3 parser 规则表——VOICE / VIDEO / STICKER / Location / Contact 都走具体占位文案。

## §8. NeedsReauth 状态链路（PR4 + PR5 + PR8）

跟 Phase 5 个微 §1.2 同形。WhatsApp Web 触发场景比个微多。

### 8.1 触发场景

| 场景 | whatsapp-rust 信号 |
|---|---|
| 用户在手机端"已链接设备"里主动登出 AIjia | `WaError::AuthRevoked` |
| WhatsApp 服务端 token 过期（长期未活跃 ~14 天） | `WaError::AuthRevoked` |
| 手机端 WhatsApp 卸载 / 解绑账号 | `WaError::AuthRevoked` |
| 同一桌面被另一台 AIjia 扫码替换（multi-device 冲突） | `WaError::AuthRevoked` |
| WhatsApp 风控直接封掉 device（封号情况） | `WaError::AuthRevoked` |

### 8.2 链路

```
runtime.rs bot.run() → Err(AuthRevoked)
   ↓
spawn task break out of loop → stream 关闭
   ↓
manager 检测 stream None → set_connection_state(NeedsReauth)
   ↓
emit channel:platform-state { state: 'needsReauth', last_error: <场景文案> }
   ↓
前端 channel 列表显示 ⚠️ NeedsReauth + "重新扫码"按钮
   ↓
用户点击 → §3.5 重新扫码流程（扫码成功后才删旧 db）
```

### 8.3 `ChannelConnectionState::NeedsReauth` 变体归属

当前 `src-tauri/src/connector/im/types.rs::ChannelConnectionState` 只有 6 个变体（Unconfigured / Disconnected / Connecting / Connected / Reconnecting / ConfigError），**没有** `NeedsReauth`。

**新增位置**：**Phase 3 PR1.5 trait 改造时统一加**（dingtalk device_code 过期 / whatsapp AuthRevoked / wechat session expired 三家共享）。Phase 5 spec 原本写"Phase 5 PR4 顺手加"，本次 §11 修订把这个责任移到 Phase 3 PR1.5。

### 8.4 前端 UI（PR8）

```
┌─ Channel 卡片 ──────────────────────────────────┐
│ [WhatsApp icon] WhatsApp                       │
│ ⚠️  会话已失效（device unlinked）              │
│ jid: 8613800138000@s.whatsapp.net             │
│ ────────────────────────────────────────────  │
│ [重新扫码] [删除账号]                          │
└─────────────────────────────────────────────────┘
```

文案区分场景（manager 透传 last_error）：

- "会话已失效"（一般 AuthRevoked）
- "已在其他设备登录"（multi-device 冲突）
- "已被服务端登出"（风控嫌疑）→ 这种情况"重新扫码"也可能立刻又被踢，提示用户考虑换号

### 8.5 重新扫码 race 防护

跟 §3.5 一致：

- **扫码成功后才删旧 db**（避免误删后扫码又失败 → 旧 session 也没了）
- rename `_pairing/session.db` → `{new_jid}/session.db` 完成后才删 `{old_jid}/session.db`

## §9. 风险表

| 风险 | 等级 | 缓解 |
|---|---|---|
| wa-rs crate 0.2.x（whatsapp-rust fork）协议跟进滞后 | 高 | 实施时全程实测；上线前给 upstream 提 issue/PR；canary 真账号长期挂测；§0.5 已声明 |
| WhatsApp 风控（自动化检测） | 高 | 发送间隔节流 ≥500ms；编辑频率 ≤6 次/session；不主动外呼；不群发；§6 实测后调参 |
| TOS 灰区——账号风险用户自担 | 高 | 首次扫码弹窗 banner：风险提示 + "我已知晓"勾选才能进入扫码界面（§9.1） |
| VoIP / Google Voice 号被快速封 | 中 | 设置面板首次扫码前显著 banner：仅推荐真实手机号 |
| AI Card 编辑速率触发软风控 | 中 | §6 速率上限；监控触发后退化到 final-only（PR6 PR 描述说明实测路径） |
| multi-device 冲突踢号 | 中 | §8 NeedsReauth "已在其他设备登录" 文案；不在 connector 处理多桌面同号 |
| wa-rs send_text / edit_message 在新版本协议变更失效 | 中 | 集成测试 + 真账号每周冒烟；CI 加 ignored 实账号测试（§9.2） |
| 服务端封号后用户期待 AIjia 提示 | 中 | §8 NeedsReauth "已被服务端登出" + 建议换号文案 |
| session.db 跨设备 / 跨用户泄漏 | 低 | scope 隔离 + 文件 OS 权限；不额外加密（用户决策 #6） |
| 重新扫码失败导致 session.db 损坏 | 低 | §3.3 启动备份：每次 start 之前 cp session.db session.db.bak；如 wa-rs 启动失败手动恢复 .bak（v3 OpenClaw 同款思路） |
| placeholder_msg_id 在 connector restart 后丢失 | 低 | §6.5 已说明；下次重启该 session 已不 in-flight |
| 媒体下载临时文件占盘 | 低 | 25h 老化清理（跟 Phase 5 个微同款） |

### 9.1 首次扫码风险 banner（PR8 必须实现）

弹窗，必须勾选"我已知晓"才能继续：

> **WhatsApp Web 接入说明**
>
> AIjia 通过 WhatsApp Web 多设备协议连接你的账号。这是 **WhatsApp 官方未授权的接入方式**，账号可能因自动化行为被 WhatsApp 限速或封禁——风险由用户自行承担。
>
> **强烈建议**：
> - 使用真实手机号，**不要**用 Google Voice / 虚拟号
> - 不在 AIjia 中群发或频繁主动外呼
> - 配合 AI 在工作场景使用，避免触发风控
>
> ☐ 我已了解上述风险，继续扫码

### 9.2 实测义务（PR8 验收）

- 真账号至少跑 24h 无非预期 disconnect 触发
- 真账号收发 ≥50 条 text 无风控
- AI Card 编辑路径触发 ≥3 次（验证占位+编辑链路）
- 主动登出 / multi-device 切换 / 重新扫码 三个 NeedsReauth 场景人工验证
- 添加 `tests/im_whatsapp_live.rs --ignored` 作为可选 canary 测试骨架（CI 不跑）

## §10. PR 切分 + 估时

### 10.1 前置依赖（4 项 + 1 项弱依赖，比 Cloud API 版本少 1 项）

- Phase 1 PR0a (shared/token.rs) — **弱依赖**（whatsapp-rust 自管 token），保留
- Phase 1 PR0b (shared/dedup.rs) — **阻塞**（parser dedup）
- Phase 1 PR0c (dingtalk AI Card 接 trait) — **不依赖**（whatsapp 不发原生 AI Card）
- Phase 1 PR0d (ReplyTarget 平台中性) — **已落地**（commit `58f801f7`），不阻塞
- Phase 2 PR3 (aicard_fallback) — **不依赖**（whatsapp 走编辑路径，不用静默累积 buffer）
- Phase 3 PR1.5 (trait 改造：`outbound_text_streaming` + `InboundDeployment` 重命名 + `NeedsReauth` 变体新增) — **阻塞**
- Phase 3 PR6.5 (SecretString) — **推荐先合并**（pair-key / 设备私钥脱敏）
- Phase 5 PR0 (前端 RegistrationModal 共抽) — **阻塞**（扫码 UI）

### 10.2 PR 列表

| PR | 内容 | 估时 |
|---|---|---|
| **PR1** 骨架 | `im/whatsapp/` 目录 + `Platform::Whatsapp` enum 变体（含 `as_str / from_str / all`）+ capabilities + factory 入口 + `Cargo.toml` 加 `wa-rs = "0.2"` 依赖（whatsapp-rust 的 stable Rust fork） | 0.5 天 |
| **PR2** Bot 生命周期 | `connector.rs` `WhatsAppConnector` 结构升级（持 `JoinHandle` + `PairingState`）+ `session.rs` 固定路径解析（`session.db` / `session.db.bak` / `config.json`）+ `config.rs` 元数据读写 + `types.rs` PairingState 4 状态（v3 简化，OpenClaw-aligned） | 1 天 |
| **PR3** 扫码登录 | `login.rs` begin/poll_registration + `runtime.rs` Event::PairingQrCode → PairingState 状态机 + Tauri 命令 `"whatsapp"` 分支接入 | 2 天 |
| **PR4** 入站 | `runtime.rs` bot.run() worker + `shared::ReconnectBackoff` 包裹 + Event::Message dispatch + `parser.rs` 私聊 only / 群事件 drop / 不支持类型占位 + `shared::MessageDedupSet` 接入 + impl IMConnector::start | 1.5 天 |
| **PR5** 出站 text | `sender.rs` + `markdown.rs` strip + impl IMConnector::send（Text/Markdown）+ 错误映射 + AuthRevoked → AuthExpired 链路 | 1.5 天 |
| **PR6** 出站 AI Card | `aicard.rs` 占位 + 增量编辑状态机（2s / ≤6 次 + final 强制突破）+ ReplyContent::AiCardChunk / AiCardFail 路径 + cleanup 链路 | 2 天 |
| **PR7** 入站媒体 | parser 内 IMAGE / FILE download_media + write tmp + ChannelAttachmentSpec + size 检查常量 + 25h 老化清理（复用 manager cron） | 1 天 |
| **PR8** 集成测试 + UI | 前端 platform card + 首次扫码风险 banner + NeedsReauth UI（3 文案分支）+ `review_im_layering.rs` 加 whatsapp + ignored 真账号 canary 测试骨架 | 1.5 天 |

**总计：~11.5 天单人**（vs 旧 Cloud API spec 8.5 天 +3 天，多在 whatsapp-rust 协议实测 + AI Card 编辑路径调参 + TOS banner UI）

**关键 PR 依赖**：PR2 → PR3 → PR4 → PR5 → PR6（顺序强依赖）；PR7 可与 PR5/PR6 并行；PR8 跟在最后。

**真账号实测时间不算在估时内**：§9.2 列的 24h 挂测 + 50 条收发 + 3 个 NeedsReauth 场景人工验证，至少额外 1-2 个工作日 buffer。

### 10.3 crate 版本 pin 策略

`Cargo.toml` 写 `wa-rs = "0.2"`（接受 0.2.x patch 更新，拒绝 0.3 自动升）。crate 是 [homunbot/wa-rs](https://github.com/homunbot/wa-rs)——上游 [jlucaso1/whatsapp-rust](https://github.com/jlucaso1/whatsapp-rust) 的 stable-Rust fork，移除 `#![feature(portable_simd)]` 和 `if_let_chains`。

2026-05-20 实测决策：原计划 `whatsapp-rust = "0.6"` 实际无法在桌面仓库 stable Rust（MSRV 1.77.2）编过，wacore 依赖链有 nightly-only features。`/tmp/wa-rs-probe` 隔离工程实测 `wa-rs = "0.2"` 44s 编过。

**fork 风险**：homunbot/wa-rs 维护活跃度低（4 commits / 7 stars）。**缓解**：① 上线前 vendor 一份到 `vendor/` 锁源码；② 同时给 jlucaso1/whatsapp-rust upstream 提 PR 去 SIMD + if_let_chains，将来回归上游；③ 风险登记在 §0.5。

upgrade 到 0.3 时单独 PR 评估。

## §11. 跨 spec 联动修订

### 11.1 Phase 5 个微 spec 修订（解耦 Phase 4 PR3 依赖）

| 行号 | 修订 |
|---|---|
| L5 前置 | 删 "Phase 4（PR3 `AiCardFallbackBuffer::new_no_placeholder()` 扩展）"；改为 "Phase 0 + Phase 1（修订版 §0 PR0d）+ Phase 2（PR3 aicard_fallback **含 new_no_placeholder 构造器**）+ Phase 3（PR1.5 trait 改造 + PR6.5 SecretString）已落地" |
| L17 | 删 "比 Phase 4 WhatsApp 还简单" 句（Phase 4 现在跟 Phase 5 难度相当） |
| L53-54 依赖 DAG | 删 "Phase 4：/ PR3 (AiCardFallbackBuffer::new_no_placeholder) → 阻塞依赖" 两行 |
| L418 | "Phase 4 PR3 加的构造器" → "Phase 2 PR3 提供的构造器" |
| L543 | "依赖 Phase 1 PR0d + Phase 2 PR3 + Phase 3 PR1.5 + Phase 4 PR3 全部合并" → "依赖 Phase 1 PR0d + Phase 2 PR3 + Phase 3 PR1.5 全部合并" |
| L149 NeedsReauth | "Phase 5 PR4 顺手加" → "**Phase 3 PR1.5 改造时统一加**（dingtalk device_code 过期 / whatsapp AuthRevoked / wechat session expired 三家共享）" |
| L630-632 trait 改动表 | WhatsApp 那行改为 "Phase 4 修订版改为私聊 only，**不需要 observe_session**；observe_session 仅为 Phase 5 wechat 群聊保留" |

### 11.2 Roadmap 修订

| 行号 | 修订 |
|---|---|
| L47 第 6 项抽象 | "给谁用" 列删 "whatsapp"；只留 "wechat 群聊 chat_id 反查" |
| L50 第 9 项抽象 | 引入位置改为 "**P2 PR3 一次性落两个构造器（带 placeholder / no-placeholder）**"；"给谁用" 列删 "whatsapp"，改为 "wecom / 个微" |
| L77 DAG | P4 节点保留（whatsapp 仍是 P4），但移除 "P4 ← P2-PR3" 依赖箭头（whatsapp 不再用 AiCardFallbackBuffer） |
| L121 阶段 F 标题工期 | "WhatsApp 完整接入（~7-8 天）" → "（~11-12 天）" |
| L122 | 删 "P4-PR3 (AiCardFallbackBuffer::new_no_placeholder() 扩展)" 整行 |
| L123 | "P4-PR1 … PR7" → "**P4-PR1 … PR8**" |
| L138 估时表 F 行 | "7-8 天" → "11-12 天" |
| L141-143 总估时 | "~50 天 ≈ 10 周" → "~54 天 ≈ 11 周"；"双人 ~6-7 周" → "~7 周"；"实际 8-10 周" → "9-11 周" |
| L153 风险表追加 | 新行: "whatsapp-rust crate 0.1.x 协议跟进 / WhatsApp 风控 / TOS 灰区" + 缓解: "前端首次扫码风险 banner + ignored 真账号 canary 测试 + 上线后实测调参 + 必要时给 upstream 提 PR" |
| 共享抽象表第 10 行附注 | "**+ `ChannelConnectionState::NeedsReauth` 变体一并加入**（dingtalk / whatsapp / wechat 三家共享）" |
| §"后续 Phase" 新增 | "**Phase 10 (新)**：WhatsApp 出站媒体（`ReplyContent::Image / File` trait 扩展 + send_image / send_document 实现）" |

### 11.3 `AiCardFallbackBuffer::new_no_placeholder()` 归属变更

由 **Phase 4 PR3** → **Phase 2 PR3**（buffer 诞生地一次性落两个构造器）。Phase 2 spec 本次不动（用户未要求），但 roadmap 共享抽象表第 9 行的注释会让 Phase 2 实施时自然加上 `new_no_placeholder()`，Phase 5 spec 引用也会指向 Phase 2 PR3。

## §12. 测试

### 12.1 单测覆盖

- `parser::tests`：私聊 5 种消息类型映射 + 群事件 drop + 不支持类型具体占位文案
- `markdown::tests`：strip 8 行规则 + 边界（连续 ** / 嵌套 markdown / URL 含括号）
- `aicard::tests`：1st chunk 占位 / 1st chunk + final 跳过占位 / 后续 chunk 2s 触发 / 6 次上限 / final 突破上限 / finalized 后 chunk drop / fail 编辑占位
- `session::tests`：`_pairing` 路径 → rename → 最终路径；rename 期间 bot.stop 等待
- `login::tests`：mock whatsapp-rust event → PairingState 状态机 5 个分支（waiting / qr_issued / device_confirm / connected / expired）
- `sender::tests`：mock bot 错误映射 4 类 + AuthRevoked → AuthExpired

### 12.2 集成测试

- `tests/im_whatsapp_integration.rs`：起 connector + mock whatsapp-rust 事件源 → 完整收发（text 1 条 + image 1 条 + AiCardChunk 占位+2 次编辑+final）+ AuthRevoked → stream 关 → NeedsReauth
- `tests/im_whatsapp_live.rs --ignored`：真账号 canary 测试骨架（默认不跑，PR8 加）
- `tests/review_im_layering.rs`：`platforms` 数组追加 `"whatsapp"`

## §13. 后续扩展（不在 Phase 4 范围）

- VOICE / VIDEO 媒体接收（需 STT 工具协作）
- 群聊支持（Baileys 群事件本就免费，但需要 `observe_session` 机制 + 群 @ 触发逻辑）
- 出站 IMAGE / FILE（需 trait `ReplyContent::Image / File` 变体——见 Roadmap Phase 10）
- 多账号扫码（同桌面挂多个 WhatsApp 号）
- WhatsApp Business 账号 / Cloud API 路径（如果企业用户有强需求，可以单独走 v1 当时的 Cloud API spec）
- 出站 @提及（@mention）/ quoted reply（v3 仅做入站 quoted 解析，出站不做）
- 入站 Reaction event 处理（v3 仅做出站 reaction "AI 收到"信号，不解析对方发来的 reaction）
- approval-before-reply / command policy（OpenClaw 有，AIjia AI 助手场景不需要）

这些都不在 Phase 4 MVP 范围。

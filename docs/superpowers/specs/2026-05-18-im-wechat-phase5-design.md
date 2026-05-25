# Phase 5：个人微信 (iLink) IM Connector

**日期**：2026-05-18（修订于 review 后）
**状态**：Design draft v2 → 待用户 review
**前置**：Phase 0 + Phase 1（修订版 §0 PR0d）+ Phase 2（PR3 aicard_fallback **含 new_no_placeholder 构造器**）+ Phase 3（PR1.5 trait 改造 + PR6.5 SecretString）已落地
**Scope**：在 `connector/im/wechat/` 实现 IMConnector（基于腾讯 iLink HTTP API）

## 背景

调研之前我们以为个微需要依赖第三方逆向 daemon（合规风险大）。实际**腾讯有官方的 iLink AI 服务**（`ilinkai.weixin.qq.com`），openclaw weixin plugin 就是基于它做的。所以：

- **无需** 外部 native daemon
- **无需** 逆向协议 / ipad 协议 / wxbot 等灰产
- **无** 封号风险（按官方协议走）
- 本质上是个 HTTP client + 扫码登录 + AES-128-ECB 媒体加密

这把 Phase 5 从"最复杂、最敏感"降到"中等难度"——技术栈是 HTTP + 扫码 + 对称加密，比 Phase 1/2 的钉钉 / 企微更直观（Phase 4 WhatsApp Web 路线 2026-05-19 修订后跟 Phase 5 难度相当）。

**对 Phase 3 InboundDeployment 设计的反向验证**：调研结论表明 iLink 是 HTTP 长轮询，跟 dingtalk WS / telegram long-poll 同形，应当映射为 `InboundDeployment::SelfHosted`。Phase 3 PR1.5 §0.2 原计划保留 `NativeDaemon` 变体作为"未来扩展点"，但 Phase 5 调研确认**没有任何已知平台真正需要它**——建议 Phase 3 PR1.5 实施时**删除该变体**，`InboundDeployment` 只保留 `SelfHosted / PublicWebhook` 两值。如果未来出现真正的本地 daemon 场景（比如某天接 PC 微信桌面客户端协议），再加回。

## 调研结论（事实摘要）

参考：`~/Downloads/openclaw channel/openclaw-weixin-main/`

1. **入站**：调用腾讯 iLink HTTP API `getUpdates` 长轮询；plugin 本身是 NodeJS，运行在 OpenClaw gateway 进程内
2. **通信协议**：5 个 HTTP POST endpoint —— `getUpdates` / `sendMessage` / `getUploadUrl` / `getConfig` / `sendTyping`
3. **认证**：扫码 QR 登录：`startWeixinLoginWithQr` → 返回 QR 二维码图片数据 → `waitForWeixinLogin` 长轮询 QR 状态 → 拿 `bot_token` + `ilink_bot_id`，本地持久化
4. **消息类型**：TEXT(1) / IMAGE(2) / VOICE(3) / FILE(4) / VIDEO(5)
5. **媒体**：通过 CDN URL + AES-128-ECB 加密；接收方用 `encrypt_query_param` + `aes_key` 下载后本地解密
6. **合规**：README 无任何"风险"关键词；走官方协议
7. **token 生命周期**：iLink 后端隐式管理，**没有 refresh_token 流程**；session 超时必须**重新扫码**——这是核心 UX 约束

## Non-Goals

1. 不实现 VOICE / VIDEO 消息接收（仅 TEXT / IMAGE / FILE）
2. 不实现公众号文章 / 链接卡片之类的特殊消息（iLink 是否支持也不清楚）
3. 不引入"多账号扫码"——一个 connector 实例 = 一个个微账号
4. 不做 contact / chat list 同步（用户不需要拉通讯录，仅做"被动应答"）

## 依赖关系

```
Phase 1 修订版 §0：
  PR0a (shared/token.rs) → 不严格需要（iLink 不刷新 token）
  PR0b (shared/dedup.rs) → 复用：长轮询断线重连时 msg_id dedup
  PR0c (dingtalk AI Card 接 trait) → 不依赖（个微无流式）
  PR0d (ReplyTarget 平台中性) → 阻塞依赖：chat_id 从 connector 内 session 表反查
Phase 2：
  PR3 (aicard_fallback **含 new_no_placeholder 构造器**) → 阻塞依赖：复用 AiCardFallbackBuffer
Phase 3：
  PR1.5 (trait 改造) → 阻塞依赖：InboundDeployment::SelfHosted + outbound_text_streaming: false；同时删除 NativeDaemon 变体
  PR6.5 (SecretString) → 强烈建议先合并：bot_token 是身份凭证

Phase 5 内部：
  PR0 (前端 RegistrationModal 共抽，含 url + qr_image 两 mode) ──→ PR3
  PR1 (骨架 + types) ──→ PR2 (crypto) ──→ PR3 (login) ──→ PR4 (api + 长轮询 + impl IMConnector) ──→ PR5 (sender + parser + AiCardChunk 降级) ──→ PR6 (media) ──→ PR7 (集成测试 + UI)
```

## §0. 前端 RegistrationModal 共抽（PR0，独立可并行）

**问题**：Phase 5 §1 原 spec 假设"复用 dingtalk 二维码组件"——但代码扫描确认 dingtalk OPEN_CLAW 流程在 `ChannelConfig.tsx` 内 **inline**，没抽公共组件。dingtalk 走 URL + 用户码模式（用户在浏览器打开），个微走 QR URL 模式（iLink 返回的是 URL 字符串，**前端用 `qrcode` 库 client-side 渲染成 canvas/svg**，不是后端推 PNG），两种 mode 都需要 modal + 倒计时 + 状态轮询。

**抽取**：

```ts
// src/components/registration/RegistrationModal.tsx
type RegistrationModalProps = {
  mode: 'url' | 'qr_url';
  title: string;
  // mode='url' 时展示（用户在浏览器打开 url，输入 userCode 确认）
  url?: string;
  userCode?: string;       // 可选：dingtalk 有用户码确认
  // mode='qr_url' 时展示（前端用 qrcode 库渲染成二维码给用户用手机扫）
  qrUrl?: string;          // 后端返回的 URL 字符串；前端 `qrcode` 渲染为 canvas
  // 共用
  expireSeconds: number;
  pollState: () => Promise<'waiting' | 'confirmed' | 'cancelled' | 'expired'>;
  onConfirmed: () => void;
  onCancel: () => void;
};
```

**关键修正（v2 → v3）**：原 spec 写 `mode: 'qr_image' / qrImageDataUrl` 假设后端推 base64 PNG。实测 openclaw `fetchQRCode` 返回的 `qrcode_img_content` 是**普通 URL 字符串**（被嵌入到二维码图像里的 payload），二维码图像本身在前端渲染。改 `qr_url` 更准确。

**dingtalk 切换**：`ChannelConfig.tsx` 当前的 inline registration UI 改用 `<RegistrationModal mode="url" url={...} userCode={...} />`。原行为 byte-for-byte 不变。

**测试**：vitest 渲染两种 mode + 倒计时 + 状态切换 fixture。

**PR0 独立并行**：可与 Phase 1/2/3/4 任何 PR 并行。Phase 5 PR3 需要 PR0 合并。

## §1. 扫码登录流程（核心 UX）

```
用户在设置面板点"添加个微账号"
   ↓
WechatConnector::begin_registration() 调 ilink/bot/get_bot_qrcode (GET, query: bot_type=3)
   ↓ 返回 qrcode + qrcode_img_content (URL 字符串，前端 qrcode 库渲染为二维码)
前端 RegistrationModal (mode='qr_url') 渲染 QR + 倒计时
   ↓ 用户用微信扫码 + 在手机上确认
后台 poll_registration() 长轮询 ilink/bot/get_qrcode_status?qrcode=<x>
   ↓ 状态：wait / scaned / scaned_but_redirect / confirmed / expired
   ↓ 收到 scaned_but_redirect 时切换 base_url 到 redirect_host
   ↓ expired 自动 refresh QR，最多 3 次
确认后拿 bot_token + ilink_bot_id + ilink_user_id + baseurl
   ↓
**bot_token 走 SecureStorage 加密**（key: `aijia-wechat-bot-token-{bot_id}`）
**ilink_bot_id + ilink_user_id + baseurl 走 auth.json 明文**
**ilink_user_id 自动加入 allowFrom 白名单**（§1.4）
   ↓
状态机进入 Connected；后续所有 API 走 baseurl（不是固定 ilinkai.weixin.qq.com）
```

### 1.1 凭证存储拆分

```
~/.renlijia/users/{scope}/channels/wechat/{bot_id}/
├── auth.json              # ilink_bot_id + ilink_user_id + baseurl + bot_token_storage_kind: "keychain"
├── state.json             # get_updates_buf cursor（高频写）
├── sessions.json          # session_id → ilink_user_id 反查表
├── context_tokens.json    # ilink_user_id → context_token 反查表（每条消息回带，§3.4）
└── allow_from.json        # 授权用户白名单（§1.4）
```

`auth.json` 含 `bot_token` 的 keychain key 引用，不含明文 token；token 实体放在 macOS Keychain / Windows Credential Manager（复用 dingtalk app_secret 路径，参考 `SecureStorage` 抽象）。

理由：
- bot_token 是身份凭证（拿到就能控制用户微信）—— **必须** 加密存储
- get_updates_buf cursor 是高频写（每条消息推进），跟 bot_token 拆开避免 fsync 风暴误伤 token
- `context_tokens.json` 单独存：高频写 + 数据量随会话用户增长，跟一次性写的 `auth.json` 拆开

### 1.2 token 失效 → pause → NeedsReauth 两阶段链路

iLink **没有 refresh_token 流程**——token 过期必须重新扫码。但实际不是"一见 401 就跳 NeedsReauth"——openclaw 走两阶段：

```
Connector worker loop 中 getUpdates 收到 errcode = -14 (SESSION_EXPIRED)
   ↓
SessionGuard::pause(account_id, duration)  // 默认 N 分钟（实施时按 openclaw 实测值定）
   ↓
所有出站请求前 assert_session_active() —— pause 期间快速失败
   ↓
pause 时间到 → 自动重试 getUpdates
   ↓
连续 K 轮重试仍 -14 → 升级到 NeedsReauth（真正进设置面板提示用户重新扫码）
```

`ChannelConnectionState` 当前没有 `NeedsReauth` 变体——**Phase 3 PR1.5 trait 改造时统一加**（dingtalk device_code 过期 / whatsapp AuthRevoked / wechat session expired 三家共享同款形状）。新增 `connector/im/shared/session_guard.rs` —— pause 机制是个**跨平台 shared 模块**（telegram bot deleted by user、whatsapp token revoked 都是同款形状），但实际抽出在 Phase 5 PR4，因为只有 wechat 真正需要。其他平台未来用到时复用。

前端 channel 状态展示：
- pause 期间：⏸️ 临时暂停，倒计时显示恢复时间
- NeedsReauth：⚠️ 需要重新扫码（按钮）

### 1.3 必带请求头（关键 spec 遗漏，必须实施时对齐）

iLink API 每个请求（含登录的 GET 和 8 个业务 endpoint）都需要以下 headers，**spec v2 完全没提**：

```
iLink-App-Id: <APPID>                                # 腾讯发的 app id，见 §1.5 合规
iLink-App-ClientVersion: <uint32>                    # major<<16 | minor<<8 | patch
AuthorizationType: ilink_bot_token                   # 固定字符串
Authorization: Bearer <bot_token>                    # 仅业务 endpoint 需要
X-WECHAT-UIN: <base64(decimal_string(random_uint32))> # 每次请求随机
SKRouteTag: <route_tag>                              # 可选，从 config 读，IDC 路由用
Content-Type: application/json                       # POST endpoint
```

body 还要套一层 `base_info: { channel_version: "<plugin version>" }`。

`api.rs` 用一个 `build_headers(token, body) -> HeaderMap` 集中函数生成，禁止散落在各 endpoint。

### 1.4 allowFrom 白名单（关键 spec 遗漏，安全模型）

**问题**：扫码登录后任何加你微信的人都能跟你的 bot 对话，包括陌生人、营销号、风控测试号。**spec v2 完全没有这层访问控制**。

**机制**：每账号一份 `allow_from.json`：

```json
{ "version": 1, "allowFrom": ["wxid_xxx@im.wechat", "wxid_yyy@im.wechat"] }
```

- 扫码登录时拿到的 `ilink_user_id`（QR 扫码人自己）**自动加入白名单**
- inbound message 到达时，parser 检查 `from_user_id` 是否在 `allowFrom`，**不在则丢弃（log info 不报错）**
- 用户可在前端管理面板手动增删（Phase 5 PR7 UI）

实现位置：`session.rs` 的 `WechatAllowFromStore`；inbound 过滤在 `runtime.rs` 的 worker loop 里、parser 之后 / observe_session 之前。

### 1.5 iLink-App-Id 合规说明

`iLink-App-Id` 是腾讯 iLink 后端发给接入方的标识符。openclaw plugin 自己有一个 appid（从 `package.json` 顶层 `ilink_appid` 字段读）。腾讯 iLink AI 服务（`ilinkai.weixin.qq.com`）**没有公开的开发者门户**——微信开放平台 / 腾讯云开发者中心 / 企业微信开放平台都搜不到对应申请入口，openclaw 的 appid 大概率是腾讯主动给的合作位。

**决策（2026-05-18 与 oayzz 确认）**：走方案 C，**MVP 复用 openclaw 的 appid + 后期单独申请**。

**实施约定**：
- **PR1**：appid 来源做成**运行时可配置**，不硬编码。优先级 `~/.renlijia/config.json::wechat.ilink_app_id` > 编译期常量（默认值 = openclaw 的 appid）。这样切换自有 appid 时不用改代码、不用发版。
- **MVP 阶段**：用 openclaw 的 appid 跑通；日志 / 错误消息 / 用户可见文案**禁止出现 "openclaw" 字样**，避免暴露
- **流量底线**：MVP 期间只允许内部测试 + 少量 design partner 真人扫码验证；**不上 OSS 公开下载、不发 beta**，避免被腾讯风控按"openclaw 流量异常"追责
- **正式发布前置条件**：oayzz 后期负责跟腾讯渠道沟通拿到 AIjia 自己的 appid，写入 `wechat.ilink_app_id` 配置后才能发 beta
- **如果申请走不通**：要么不做个微（用户引导到 Phase 2 企微），要么承担合规风险走灰名单——产品决策点，不在工程范围


## §2. 目录结构 + capabilities

```
src-tauri/src/connector/im/wechat/
├── mod.rs                  # impl IMConnector for WechatConnector
├── runtime.rs              # getUpdates 长轮询循环 + ReconnectBackoff + SessionGuard 检查 + allowFrom 过滤
├── api.rs                  # 7 个 iLink endpoint 封装（5 POST + 2 GET 扫码）+ build_headers
├── login.rs                # 扫码登录 begin + poll（含 scaned_but_redirect 处理 + 3 次 expired 自动刷新）
├── sender.rs               # sendMessage / sendTyping
├── media.rs                # getUploadUrl + AES-128-ECB 加解密 + 上传下载
├── parser.rs               # iLink raw message → ChannelMessage（VOICE 有 text 字段时按文本走，§6）
├── session.rs              # WechatSessionStore + WechatContextTokenStore + WechatAllowFromStore
├── crypto.rs               # AES-128-ECB 加解密纯函数 + 单测（圣经 fixture 来自 openclaw 实测对）
└── types.rs                # 区分 UploadMediaType {IMAGE=1,VIDEO=2,FILE=3,VOICE=4} 与 MessageItemType {TEXT=1,IMAGE=2,VOICE=3,FILE=4,VIDEO=5} 两套枚举
```

```rust
ConnectorCapabilities {
    inbound: InboundDeployment::SelfHosted,         // HTTP 长轮询，不需要公网
    outbound_aicard: false,
    outbound_text_streaming: false,                  // iLink 无 edit API
    outbound_markdown: MarkdownSupport::Partial,     // StreamingMarkdownFilter 流式过滤，部分支持
    supports_attachments: true,
    supports_group_chat: false,                      // Phase 5 仅私聊：iLink group_id 字段语义未知，openclaw 也只开 chatTypes:["direct"]
    supports_private_chat: true,
    auth_flow: AuthFlow::QRCode,
}
```

**关于 `outbound_markdown`**：spec v2 写 `false`。实测 openclaw 2.1.3 起改为 `StreamingMarkdownFilter`（`src/messaging/markdown-filter.ts`）——流式逐字符过滤，部分 markdown 语法可以保留（粗体 `**x**` → `x`、列表 `-`/`1.` 保留前缀字符等）。Phase 5 沿用这个策略，capability 表达为 `Partial`。如 trait 当前只有 `bool`，PR1 顺手扩成枚举 `MarkdownSupport { None, Partial, Full }`。

**关于 `supports_group_chat: false`**：iLink 协议里 `WeixinMessage.group_id` 字段是存在的，但 openclaw plugin 实际 `chatTypes: ["direct"]` 只支持私聊。群聊 chat_id 格式、向群发送的 API 形状、群消息 from_user_id 语义全是未知数。**Phase 5 范围缩小到私聊**，群聊放 §10 后续扩展。这反向影响 §3.2 `WechatSessionStore` 设计——现在只��要 `session_id → ilink_user_id` 一维表，不需要 `chat_type`。


## §3. 长轮询入站

### 3.1 worker loop

```rust
async fn start(
    &self,
    ctx: ConnectorContext,
) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
    let token = self.load_bot_token().await?;  // 从 SecureStorage 解密
    let initial_buf = self.state.load_get_updates_buf().await.unwrap_or_default();
    let api = self.api.clone();
    let parser = self.parser.clone();
    let allow_from = Arc::clone(&self.allow_from);
    let context_tokens = Arc::clone(&self.context_tokens);
    let state_store = Arc::clone(&self.state);
    let session_guard = Arc::clone(&self.session_guard);
    let account_id = self.account_id.clone();

    // 用 futures::stream::unfold 而非 async-stream macro，保持跟 Phase 0
    // 现有代码风格一致（项目当前不依赖 async-stream crate）。
    // 注意：服务器会通过 longpolling_timeout_ms 字段返回建议超时，我们据此调下一轮 timeout。
    let init = (initial_buf, ReconnectBackoff::default_schedule(), 0u64, 35_000u64);
    let stream = futures::stream::unfold(init, move |(mut buf, mut backoff, mut since_last_flush, mut next_timeout_ms)| {
        let cancel = ctx.cancel_token.clone();
        let api = api.clone();
        let parser = parser.clone();
        let token = token.clone();
        let allow_from = Arc::clone(&allow_from);
        let context_tokens = Arc::clone(&context_tokens);
        let state_store = Arc::clone(&state_store);
        let session_guard = Arc::clone(&session_guard);
        let account_id = account_id.clone();
        async move {
            loop {
                if cancel.is_cancelled() {
                    let _ = state_store.flush_get_updates_buf(&buf).await;
                    return None;
                }

                // pause 期间快速 sleep（不发请求）
                if let Some(remaining) = session_guard.remaining_pause(&account_id) {
                    log::info!("[wechat] session paused, sleeping {:?}", remaining);
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            let _ = state_store.flush_get_updates_buf(&buf).await;
                            return None;
                        }
                        _ = tokio::time::sleep(remaining) => continue,
                    }
                }

                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = state_store.flush_get_updates_buf(&buf).await;
                        return None;
                    }
                    resp = api.get_updates(&token, &buf, Duration::from_millis(next_timeout_ms)) => {
                        match resp {
                            Ok(resp) => {
                                backoff.reset();
                                if let Some(ms) = resp.longpolling_timeout_ms {
                                    next_timeout_ms = ms;  // 服务器建议的下一轮超时
                                }
                                if !resp.get_updates_buf.is_empty() {
                                    buf = resp.get_updates_buf.clone();
                                }
                                for raw in resp.msgs {
                                    // 1. 持久化 context_token（每条消息都带，回复时 echo）
                                    if let (Some(uid), Some(ct)) = (&raw.from_user_id, &raw.context_token) {
                                        context_tokens.set(&account_id, uid, ct).await;
                                    }
                                    // 2. allowFrom 白名单过滤
                                    if let Some(uid) = &raw.from_user_id {
                                        if !allow_from.is_allowed(uid).await {
                                            log::info!("[wechat] dropped message from non-allowlisted user {uid}");
                                            continue;
                                        }
                                    }
                                    // 3. parser 输出 ChannelMessage
                                    if let Some(msg) = parser.normalize(&raw) {
                                        since_last_flush += 1;
                                        if since_last_flush >= 10 {
                                            let _ = state_store.flush_get_updates_buf(&buf).await;
                                            since_last_flush = 0;
                                        }
                                        return Some((msg, (buf, backoff, since_last_flush, next_timeout_ms)));
                                    }
                                }
                                // 这一轮没拿到消息（或全被过滤），继续 loop。
                            }
                            Err(WechatApiError::SessionExpired) => {
                                // errcode = -14 → pause 而不是立刻 NeedsReauth（§1.2）
                                session_guard.pause(&account_id, DEFAULT_PAUSE_DURATION).await;
                                let count = session_guard.consecutive_pause_count(&account_id);
                                if count >= MAX_PAUSE_BEFORE_REAUTH {
                                    log::warn!("[wechat] {count} consecutive pauses, escalating to NeedsReauth");
                                    let _ = state_store.flush_get_updates_buf(&buf).await;
                                    return None;  // stream 结束 → manager 进入 NeedsReauth
                                }
                                // pause 期间继续 loop，下一轮检查 remaining_pause
                            }
                            Err(WechatApiError::Transient(e)) => {
                                let delay = backoff.next_delay();
                                log::info!("[wechat] transient error, sleeping {:?}: {e}", delay);
                                tokio::time::sleep(delay).await;
                            }
                            Err(WechatApiError::Fatal(e)) => {
                                log::error!("[wechat] fatal error: {e}");
                                return None;
                            }
                        }
                    }
                }
            }
        }
    });

    Ok(Box::pin(stream))
}
```

**关键设计点**：
- **ReconnectBackoff 复用** Phase 0 PR2 的 5/15/30/60 秒指数退避，不再固定 5 秒
- **`get_updates_buf` 持久化**（不是 `cursor` —— iLink 协议词是 `get_updates_buf`，是个 base64-encoded 全量 context buf 不是简单游标）：每 10 条 update 批量 fsync + cancel 时强制 flush + 启动时加载
- **长轮询超时自适应**：服务器在 `getUpdates` 响应里给 `longpolling_timeout_ms` 建议值，client 据此调下一轮超时。默认 35s（openclaw 实测值）。**spec v2 写固定 30s 是错的**。
- **`errcode = -14` 走 pause 而非立刻 NeedsReauth**（§1.2 两阶段）：默认 pause N 分钟 → 反复失败 K 次才升级 NeedsReauth。`SessionGuard::pause()` + 出站请求前 `assert_session_active()` 熔断。
- **context_token 必须持久化**（§3.4）：每条 inbound 消息都带，回复时必须 echo 回去；in-memory map + 落盘 `context_tokens.json`，启动时 restore。
- **allowFrom 白名单过滤**在 parser 之前：从 `from_user_id` 看，不在白名单直接 drop（log info）。
- **无 async-stream 依赖**：用 `futures::stream::unfold`，跟 Phase 0 PR6 cancel test 同款写法

### 3.2 私聊路径 + session 反查

**Phase 5 仅支持私聊**（§2 capabilities）。parser 输出 ChannelMessage：

| chat 类型 | conversation_type | conversation_key |
|---|---|---|
| 私聊 | Private | `from_user_id`（形如 `wxid_xxx@im.wechat`） |

connector 内部维护 `WechatSessionStore`：

```rust
pub struct WechatSessionStore {
    // session_id → ilink_user_id
    inner: RwLock<HashMap<String, String>>,
    persist_path: PathBuf,
}

impl WechatSessionStore {
    pub async fn observe(&self, session_id: &str, ilink_user_id: &str);
    pub async fn get_user_id(&self, session_id: &str) -> Option<String>;
}
```

`send()` 时通过 `target.session_id` 反查 `ilink_user_id`，再从 `WechatContextTokenStore` 拿 `context_token`：

```rust
async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
    self.session_guard.assert_active(&self.account_id)?;
    let to_user_id = self.sessions.get_user_id(&target.session_id).await
        .ok_or_else(|| ConnectorError::Fatal(
            format!("wechat connector: unknown session_id {}", target.session_id)
        ))?;
    let context_token = self.context_tokens.get(&self.account_id, &to_user_id).await;
    // context_token 缺失时 log warn，不阻断（openclaw 实测降级行为）

    match content {
        ReplyContent::Text(s) | ReplyContent::Markdown(s) => {
            // 走 StreamingMarkdownFilter 而不是整段 strip（§3.3 注释）
            let filtered = StreamingMarkdownFilter::default().feed_and_flush(&s);
            self.api.send_message(&to_user_id, &filtered, context_token.as_deref()).await
        }
        ReplyContent::AiCardChunk { delta, final_chunk } => {
            self.handle_aicard_chunk(&target, &to_user_id, context_token.as_deref(), delta, final_chunk).await
        }
    }
}
```

**关于 trait `observe_session`**：原 spec 写"两个选择"，结论已经定了——走 **(a)**：trait 加 `async fn observe_session(&self, session_id, conversation_key, conversation_type) -> Result<()>`，默认 no-op，wechat / whatsapp 实现。**这是 Phase 1 PR0d 范围扩展项**，跨 Phase 反向需求详见 §12。

### 3.3 AI Card 流式降级（复用 Phase 2/4 抽出的 buffer）

个微跟 WhatsApp Web 路线**部分同形**——iLink 没 edit API + 没平台超时压力，所以**仍**走静默累积（WhatsApp 修订版 v2 改走"占位+增量编辑"，故双方在 AI Card 路径分叉）。复用 `AiCardFallbackBuffer::new_no_placeholder()`（**Phase 2 PR3 提供的构造器**）：

```rust
async fn handle_aicard_chunk(
    &self,
    target: &ReplyTarget,
    to_user_id: &str,
    context_token: Option<&str>,
    delta: String,
    final_chunk: bool,
) -> Result<(), ConnectorError> {
    let mut buffers = self.fallback_buffers.lock().await;
    let buffer = buffers.entry(target.session_id.clone())
        .or_insert_with(|| AiCardFallbackBuffer::new_no_placeholder());

    match buffer.observe(&delta, final_chunk) {
        FallbackAction::Buffer => Ok(()),
        FallbackAction::SendPlaceholder { .. } => unreachable!("no_placeholder mode"),
        FallbackAction::SendFinal { text } => {
            buffers.remove(&target.session_id);
            drop(buffers);
            // 仍走 sender.send_message，带 context_token
            let filtered = StreamingMarkdownFilter::default().feed_and_flush(&text);
            self.api.send_message(to_user_id, &filtered, context_token).await
        }
    }
}
```

### 3.4 context_token 持久化（关键 spec 遗漏）

**openclaw 实测**：每条 `getUpdates` 拉到的 `WeixinMessage` 都带一个 `context_token` 字段；发回消息时**必须 echo 这个 token**，否则 server 不接收或回错。这是 iLink 协议核心机制 spec v2 完全没提。

存储结构（`session.rs::WechatContextTokenStore`）：

```rust
pub struct WechatContextTokenStore {
    // (account_id, ilink_user_id) → context_token
    inner: RwLock<HashMap<(String, String), String>>,
    persist_dir: PathBuf,  // 每 account 一份 `{bot_id}/context_tokens.json`
}

impl WechatContextTokenStore {
    pub async fn set(&self, account_id: &str, user_id: &str, token: &str);
    pub async fn get(&self, account_id: &str, user_id: &str) -> Option<String>;
    /// 启动时调一次，从落盘文件加载所有 token 到 in-memory map
    pub async fn restore(&self, account_id: &str) -> Result<usize>;
    /// 删除账号时调
    pub async fn clear_account(&self, account_id: &str) -> Result<()>;
}
```

持久化策略：写 in-memory 后**立即落盘**（同步写）——单条 message 影响一个 user 的一行，不会有 fsync 风暴。重启后 `restore()` 拉回内存。

错误处理：`send()` 时 `context_token` 缺失（用户从未发过消息，bot 试图主动推送）→ log warn + 仍然尝试发送（openclaw 实测降级行为）；如果 server 因此拒绝，走 Transient 错误重试。

## §4. AES-128-ECB 媒体加密（PR2）

iLink 媒体走 CDN，URL 形如：

```
https://wx.qlogo.cn/...?encrypt_query_param=xxx
+ msg.media.aes_key (16 字节 hex)
```

下载流程：

1. HTTP GET CDN URL（带 `encrypt_query_param` 在 query）
2. 拿到的 body 是 AES-128-ECB + PKCS#7 padding 加密的原始字节
3. `aes_key` 解密 → 原始文件字节

上传反向走：`getUploadUrl()` → 拿 signed URL + `aes_key` → 本地 AES-128-ECB 加密 → PUT 上去。

**注意 inbound 优先用 `image_item.aeskey`（hex 字符串）而不是 `image_item.media.aes_key`（base64）——openclaw 注释指出前者是更稳定的 inbound key 字段**。`types.ts::ImageItem` 同时有两个 key 字段是历史遗留，parser 实现时要按 openclaw 顺序兜底。

### 4.0 两套媒体枚举（实施陷阱）

iLink 协议里**同一个媒体概念用两套不同的数字**：

```rust
/// 用于 getUploadUrl 的 media_type 字段
pub enum UploadMediaType {
    Image = 1,
    Video = 2,
    File = 3,
    Voice = 4,
}

/// 用于 MessageItem.type 字段
pub enum MessageItemType {
    Text = 1,
    Image = 2,
    Voice = 3,
    File = 4,
    Video = 5,
}
```

**实施约定**：`types.rs` 两个枚举严格分开命名，禁止互相 `as i32` 强转；`media.rs` 内部从一种转到另一种必须走 `pub fn upload_type_from_item_type(t: MessageItemType) -> Option<UploadMediaType>` 显式查表函数（含一个 TEXT → None 的分支）。否则一定会有人写 `1 → 1` 这种"看起来对但实际错"的代码。

### 4.1 关于 ECB 模式

`crypto.rs` 用 ECB 模式——技术上是已知不安全的（不带随机 IV，相同明文产生相同密文）。**但这是 iLink 协议规定的算法，不是选型**：

```rust
//! AES-128-ECB encryption for iLink media transport.
//!
//! **Why ECB?** This is the algorithm the iLink server expects on the wire.
//! ECB is generally not recommended (no IV, plaintext patterns leak), but
//! we don't get to choose — the server-side protocol mandates it. If iLink
//! ever migrates to CBC/GCM we'll update; until then this matches the
//! reference NodeJS plugin byte-for-byte.

use aes::Aes128;  // feature = "ecb" must be enabled in Cargo.toml
```

Rust `aes` crate 默认 deny ECB —— Cargo.toml 必须显式 `aes = { version = "...", features = ["ecb"] }`。

### 4.2 圣经测试 fixture

**fixture 必须来自 openclaw NodeJS plugin 的真实加解密对**，不能凭空构造。流程：

1. 跑 NodeJS plugin 真实账号 → 截一段 iLink 媒体下载完整流程 → 拿到 (aes_key, ciphertext, plaintext) 三元组
2. 三元组硬编码进 `crypto::tests` —— 每次 PR2 修改 crypto.rs 都必须通过

如果 fixture 不在手，PR2 不能 merge。这是"圣经"的含义。

## §5. 错误处理

| iLink 错误 | 映射 | 处理 |
|---|---|---|
| `errcode = -14` (`SESSION_EXPIRED_ERRCODE`) | `SessionExpired` | `SessionGuard::pause(N min)` → 反复 K 次升级 `NeedsReauth`（§1.2） |
| `ret != 0` 其他 errcode | `Fatal(errcode, errmsg)` 或 `Transient` 由 code 决定 | 实施时按 openclaw 实测 errcode 表分类；未知 code 默认 Transient |
| `AbortError` / 网络抖动 / 5xx | `Transient` | ReconnectBackoff 5/15/30/60s 退避 |
| 网关 timeout（Cloudflare 524 等） | `Transient` | 等同长轮询 timeout，立刻重试 |
| 4xx (HTTP-level，参数错) | `Fatal(msg)` | log error 并 panic worker（结构性 bug） |
| Rate limit (具体 code 由实施时实测确定) | `Transient` | 默认指数退避，必要时按 code 加固定延迟 |
| 多端登录冲突（手机 + iLink + plugin 同时） | `SessionExpired`（iLink 通常把后登录的踢掉）→ 走 pause 路径 |  |

## §6. 测试

- `crypto::tests`：AES-128-ECB 圣经 fixture（NodeJS plugin 实测对） + 自洽往返加解密 + PKCS#7 padding 边界（0/15/16 字节）
- `parser::tests`：5 种 MessageItemType → ChannelMessage 映射；TEXT/IMAGE/FILE 正常路径 + **VOICE 有 `voice_item.text` 时按文本走（§6 注释）** + VOICE 无 text / VIDEO 走 `[不支持的消息类型]` 占位 + 私聊 conversation_type=Private（Phase 5 无群聊路径）+ `ref_msg` 引用消息 prefix 处理
- `session::tests`：observe → get + 持久化 + 重启加载（含 `WechatSessionStore` + `WechatContextTokenStore` + `WechatAllowFromStore` 三个 store 各自的 round-trip 测）
- `login::tests`：mock iLink，begin → poll wait → poll scaned → poll **scaned_but_redirect**（验证 base_url 切换）→ poll confirmed → 拿 token + 写 SecureStorage；expired 自动 refresh（最多 3 次）边界
- `api::tests`：mock HTTP，`get_updates_buf` 推进正确性 + **`longpolling_timeout_ms` 自适应**（验证服务器建议被采用）+ ReconnectBackoff 退避路径 + `errcode = -14` → pause 而不是立刻终止 + 连续 K 次 pause 后升级 NeedsReauth
- `session_guard::tests`：pause 后 `assert_active` 失败、过期后恢复、`consecutive_pause_count` 计数与重置
- `tests/im_wechat_integration.rs`：起 connector + mock iLink + 完整收发（私聊 2 条 + AiCardChunk fallback final + context_token 持久化 + allowFrom 过滤一条陌生人消息）+ mode 切换到 NeedsReauth
- `tests/review_im_layering.rs`：`platforms` 数组追加 `"wechat"`

**VOICE 文本处理（spec 修正）**：iLink server 已经做了语音转文字，`voice_item.text` 字段存在时直接当文本消息走，**不再统一占位**。`voice_item.text` 为空才走 `[不支持的消息类型]` 占位。openclaw `inbound.ts::bodyFromItemList` 已经这么做了。

## §7. 实施 PR 切分（修订）

**独立可并行**：
- **PR0** 前端 RegistrationModal 共抽（含 `url` + `qr_url` 两 mode）+ dingtalk 切到新组件

**依赖 Phase 1 PR0d + Phase 2 PR3 + Phase 3 PR1.5 全部合并**：

- **PR1** `im/wechat/` 骨架 + types（两套媒体枚举严格分开，§4.0）+ iLink endpoint 常量（7 个）+ `build_headers` 集中函数（§1.3）+ capabilities（`InboundDeployment::SelfHosted` + `outbound_text_streaming: false` + `outbound_markdown: Partial` + `supports_group_chat: false`）+ **iLink-App-Id 来源决策**（§1.5）
- **PR2** `crypto.rs` (AES-128-ECB) + PKCS#7 padding + 圣经 fixture（NodeJS plugin 实测对必须 pass）+ Cargo.toml 加 `aes` crate ecb feature
- **PR3** `login.rs` begin/poll 扫码（含 `scaned_but_redirect` 切 base_url + `expired` 3 次自动刷新）+ bot_token 走 SecureStorage 加密 + auth.json 存 ilink_bot_id + ilink_user_id + baseurl + 自动将 ilink_user_id 加入 allowFrom 白名单（依赖 PR0）
- **PR4** `api.rs` + `runtime.rs` getUpdates 长轮询（含 `longpolling_timeout_ms` 自适应）+ `get_updates_buf` 持久化（Phase 3 策略）+ ReconnectBackoff 集成 + `SessionGuard` 模块 + `impl IMConnector for WechatConnector` + `ChannelConnectionState` 加 `NeedsReauth` 变体 + `errcode=-14 → pause → 多次升级 NeedsReauth` 链路（顺手做，dingtalk 受益）
- **PR5** `sender.rs` + `parser.rs`（TEXT/IMAGE/FILE + VOICE 含 text 字段 + 私聊 conversation_type 路径）+ `session.rs` 三个 store（WechatSessionStore + WechatContextTokenStore + WechatAllowFromStore）+ AiCardChunk 走 `AiCardFallbackBuffer::new_no_placeholder()` + `StreamingMarkdownFilter` 过滤（不是整段 strip）+ inbound allowFrom 过滤
- **PR6** `media.rs` 上传下载 + crypto 接入 + 两套媒体枚举之间的 `upload_type_from_item_type` 显式查表
- **PR7** 集成测试 + 前端"添加个微" UI（用 RegistrationModal `mode=qr_url`）+ allowFrom 管理 UI + `review_im_layering.rs` 加 wechat + NeedsReauth 状态 UI（dingtalk 同步受益）

## §8. 风险

| 风险 | 缓解 |
|---|---|
| iLink 服务 SLA / 文档不公开（openclaw plugin 是实测得来） | 实施时先用一个真实账号跑通 + 全程录 HTTP log 作回归资产；列入 spec 附录 |
| iLink 接口变更（无 SemVer 保证） | 同上：用真实账号 + canary 测试，发现变化时调 parser |
| AES-128-ECB 跟 NodeJS plugin 实现微差异 | 圣经 fixture 必须用 plugin 的真实 ciphertext，不能凭空构造（§4.2） |
| **iLink-App-Id 长期依赖 openclaw 的标识** | MVP 阶段复用 + 流量底线（不发 OSS / 不发 beta）；oayzz 后期负责申请 AIjia 自有 appid，PR1 把 appid 来源做成运行时可配置以零成本切换（§1.5） |
| **bot_token 误泄漏到日志** | PR6.5 SecretString newtype + SecureStorage 加密存储双重保护 |
| **token 失效用户不知道为啥 AI 突然不回话** | `errcode=-14 → pause → NeedsReauth` 两阶段链路 + 前端 ⏸️ pause 倒计时 / ⚠️ NeedsReauth + "重新扫码"按钮（§1.2） |
| **陌生人加微信就能调你的 bot** | `allowFrom` 白名单（§1.4）+ 扫码人自动入白 + 前端管理 UI |
| **context_token 缺失导致回复被拒** | 必须持久化（§3.4）：每条 inbound 都 set + 启动 restore + send 时 echo |
| **请求头不全（缺 iLink-App-Id / X-WECHAT-UIN 等）导致全量请求 403** | `api.rs::build_headers` 集中函数 + 单测验证所有必带 header（§1.3） |
| **`UploadMediaType` ↔ `MessageItemType` 数字撞车（同 1/2 不同义）** | `types.rs` 两枚举强类型分开 + 显式查表函数（§4.0） |
| 扫码登录被微信风控（罕见但有可能） | spec 不优化；记录现象交给用户切到企微 |
| 多端登录冲突（手机 + iLink + plugin 同时） | 不在 connector 处理；按 iLink 报错走 pause 路径 |
| VOICE 没 text 字段 / VIDEO 接收不支持 → 用户期待落空 | spec 明确占位文案 + 未来扩展点（§10） |
| **`get_updates_buf` 持久化频繁 fsync 抢占 bot_token 写盘** | auth.json 与 state.json 拆分（§1.1）；高频写不会影响 token |
| **私聊场景 session 反查依赖 trait observe_session 方法** | Phase 1 PR0d 范围扩展，加 `observe_session` 默认 no-op trait 方法；wechat / whatsapp 实现 |

## §9. 估时

- PR0（前端 RegistrationModal 抽取 + dingtalk 切换）：1.5 天（含 vitest）
- PR1（骨架 + 两套枚举 + 7 endpoint 常量 + headers 函数 + appid 决策）：1 天（+0.5 天因为 headers / appid 调研）
- PR2（crypto + 圣经 fixture）：1 天（fixture 准备占大头）
- PR3（扫码登录 + redirect/expired 状态机 + SecureStorage + 自动入白）：2 天（+0.5 天因为 state 机变复杂）
- PR4（长轮询 + 自适应超时 + ReconnectBackoff + SessionGuard + IMConnector + NeedsReauth 状态链路）：2.5 天（+0.5 天因为 pause 两阶段）
- PR5（sender + parser + 3 个 store + aicard fallback + StreamingMarkdownFilter + allowFrom 过滤）：2 天（+0.5 天因为 context_token + allowFrom）
- PR6（media 上下行 + 两套枚举查表）：1 天
- PR7（集成测试 + 前端 UI + allowFrom 管理 UI + NeedsReauth UI）：2 天（+0.5 天因为 allowFrom UI）

**总计：~13 天单人**（spec v2 估 10.5 天，v3 因新发现的 context_token / allowFrom / pause 机制 / redirect / 自适应超时 / appid 决策 +2.5 天）

PR0 可与 Phase 1-4 任何 PR 并行 → 实际 Phase 5 工期 ~11.5 天。

## §10. 后续扩展

- VOICE（无 `voice_item.text` 时）/ VIDEO 媒体接收（需要语音转写工具协作）
- **群聊支持**（`group_id` 格式调研 + 群消息 from_user_id 语义 + 向群 sendMessage API）
- 公众号文章卡片 / 链接卡片解析
- 多账号扫码（同一桌面 app 挂多个个微号）
- contact / chat list 同步（用户主动选择"群"才发消息）
- `SessionGuard` 抽到 `connector/im/shared/`（telegram bot deleted / whatsapp token revoked 未来用到时复用）

这些都不在 Phase 5 范围。

## §11. Phase 3 InboundDeployment 反向修订

Phase 5 调研结论触发对 Phase 3 PR1.5 §0.2 的修订：

**修订前**：

```rust
pub enum InboundDeployment {
    SelfHosted,
    PublicWebhook,
    NativeDaemon,  // ← 原计划保留，给 wechat 用
}
```

**修订后**：

```rust
pub enum InboundDeployment {
    SelfHosted,
    PublicWebhook,
    // NativeDaemon 删除：Phase 5 个微走 iLink HTTP 长轮询，
    // 是 SelfHosted。如未来出现真正的本地 daemon 平台（PC 微信桌面客户端协议
    // 之类），再加回。
}
```

Phase 3 PR1.5 实施时直接按修订后枚举写 —— **不要先实现 NativeDaemon 再删，浪费工作量**。

**同步修订的下游 spec**（实施时务必同步改文案）：
- `2026-05-18-im-connector-trait-phase0-design.md` §91 注释"通过外部 native daemon（个微）"和 §103"扫码登录（个微）"附近——删除 NativeDaemon 变体描述，改为 SelfHosted
- `2026-05-18-im-telegram-phase3-design.md` §89 capabilities 表 wechat 行：inbound 改 `SelfHosted`；§99-100 枚举定义删 NativeDaemon；§113 capabilities 路由表删 wechat NativeDaemon 行
- `2026-05-18-im-wecom-phase2-design.md` §587 风险表"Phase 5（个微）：daemon 模式"改为"Phase 5（个微）：iLink HTTP 长轮询，跟本 spec 完全独立"

## §12. trait 改动跨 Phase 影响汇总（更新）

Phase 1 PR0d 扩展范围（Phase 5 反向需求）：

| 文件 | 改动 |
|---|---|
| `connector/im/trait_def.rs` | `ReplyTarget` 平台中性化（删 robot_code/reply_group_id/session_webhook）+ **加 `async fn observe_session(&self, session_id: SessionId, conversation_key: &str, conversation_type: ConversationType) -> Result<()>`，默认实现 no-op** + `MarkdownSupport { None, Partial, Full }` 枚举替换 `outbound_markdown: bool` |
| `connector/im/manager.rs` | router 建 session 后调 `connector.observe_session(...)` |
| `connector/im/dingtalk/connector.rs` | observe_session 默认 no-op（dingtalk 用 reply_robot_code_for_worker 反查） |
| `connector/im/wechat/connector.rs`（Phase 5 PR5） | 实现 observe_session 写入 WechatSessionStore（仅私聊路径） |
| `connector/im/whatsapp/connector.rs`（Phase 4） | **2026-05-19 Phase 4 修订版改为私聊 only，不需要 observe_session**；observe_session 仅为 Phase 5 wechat 私聊 session 反查保留 |

Phase 4 spec 同款修订：~~WhatsApp PR4 也用 `observe_session` 而非 parser 直推 sessions~~ 已废弃——Phase 4 v2 改走 whatsapp-rust + 私聊 only 后，jid 反查走 connector 内存 map，不再需要 trait `observe_session`。

**Phase 1 PR0d spec（`2026-05-18-im-feishu-phase1-design.md` §0）必须更新**：当前 §115-131 PR0d 章节只写了 `ReplyTarget` 改造；实施时**还必须加 `observe_session` trait 方法 + `MarkdownSupport` 枚举改造**——否则 wechat / whatsapp 都没法实现 session 反查，wechat 也没法表达 `Partial` markdown。本 spec 是 source of truth，PR1 实施前先回去把 Phase 1 PR0d 章节扩展掉。

## §13. spec 版本说明

**v3 (2026-05-18 二次 review，参考 openclaw-weixin-main 源码全量过一遍)** 在 v2 之上的主要修订：

1. §0 RegistrationModal mode `qr_image` → `qr_url`，澄清前端渲染机制（后端不推 PNG，推 URL 字符串）
2. §1 扫码状态机加 `scaned_but_redirect` + IDC base_url 切换 + `expired` 3 次自动刷新；登录返回 `baseurl` 字段后续 API 走该地址
3. §1.1 目录结构新增 `context_tokens.json` 和 `allow_from.json`
4. §1.2 token 失效改为 `pause N min → 升级 NeedsReauth` 两阶段，新增 `SessionGuard` 模块
5. §1.3 新增"必带请求头规范"——`iLink-App-Id` / `X-WECHAT-UIN` / `base_info` / `AuthorizationType` 等（spec v2 完全没提）
6. §1.4 新增 allowFrom 白名单（关键安全模型，spec v2 缺失）
7. §1.5 新增 iLink-App-Id 合规说明（PR1 前置决策）
8. §2 capabilities：`supports_group_chat: true → false`（Phase 5 仅私聊，openclaw 也只开 direct）；`outbound_markdown: bool → MarkdownSupport::Partial`（流式过滤而非整段 strip）
9. §3.1 worker loop 重写：`cursor → get_updates_buf`、加 SessionGuard pause 检查、加 allowFrom 过滤、加 `longpolling_timeout_ms` 自适应、加 context_token 持久化、`errcode=-14` 不立刻终止改 pause
10. §3.2 删除群聊/chat_id 反查设计，简化为私聊单维 session_id → ilink_user_id
11. §3.4 新增 `WechatContextTokenStore` 设计（每条 inbound 都带 token，发回时必须 echo——关键 spec 遗漏）
12. §4.0 新增"两套媒体枚举"陷阱说明（`UploadMediaType` vs `MessageItemType` 数字撞车）
13. §6 测试 VOICE 加分支（有 `text` 字段按文本走，server 已经语音转写）
14. §7 PR 切分按新增内容调整
15. §8 风险表新增 5 项（appid 合规 / 陌生人 / context_token / headers / 枚举撞车）
16. §9 估时 10.5 → 13 天单人
17. §11 反向修订下游 spec 列出文件 + 行号（实施时一并改：phase 0 §91/§103、phase 3 §89/§99-100/§113、phase 2 §587）
18. §12 Phase 1 PR0d cross-reference 显式标注"必须扩展"+ 新增 `MarkdownSupport` 枚举改动

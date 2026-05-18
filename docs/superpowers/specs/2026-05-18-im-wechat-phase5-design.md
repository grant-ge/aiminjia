# Phase 5：个人微信 (iLink) IM Connector

**日期**：2026-05-18
**状态**：Design draft → 待用户 review
**前置**：Phase 0 / 1 / 2 / 3 / 4 已落地
**Scope**：在 `connector/im/wechat/` 实现 IMConnector（基于腾讯 iLink HTTP API）

## 背景

调研之前我们以为个微需要依赖第三方逆向 daemon（合规风险大）。实际**腾讯有官方的 iLink AI 服务**（`ilinkai.weixin.qq.com`），openclaw weixin plugin 就是基于它做的。所以：

- **无需** 外部 native daemon
- **无需** 逆向协议 / ipad 协议 / wxbot 等灰产
- **无** 封号风险（按官方协议走）
- 本质上是个 HTTP client + 扫码登录 + AES-128-ECB 媒体加密

这把 Phase 5 从"最复杂、最敏感"降到"中等难度"——比 Phase 4 WhatsApp 还简单。

## 调研结论（事实摘要）

参考：`~/Downloads/openclaw channel/openclaw-weixin-main/`

1. **入站**：调用腾讯 iLink HTTP API `getUpdates` 长轮询；plugin 本身是 NodeJS，运行在 OpenClaw gateway 进程内
2. **通信协议**：5 个 HTTP POST endpoint —— `getUpdates` / `sendMessage` / `getUploadUrl` / `getConfig` / `sendTyping`
3. **认证**：扫码 QR 登录：`startWeixinLoginWithQr` → 显示二维码 → `waitForWeixinLogin` 长轮询 QR 状态 → 拿 `bot_token` + `ilink_bot_id`，本地持久化
4. **消息类型**：TEXT(1) / IMAGE(2) / VOICE(3) / FILE(4) / VIDEO(5)
5. **媒体**：通过 CDN URL + AES-128-ECB 加密；接收方用 `encrypt_query_param` + `aes_key` 下载后本地解密
6. **合规**：README 无任何"风险"关键词；走官方协议
7. **特殊**：长轮询模式 + context token 维护会话上下文；token 生命周期由 iLink 后端隐式管理，session 超时退避重试而非主动续期

## Non-Goals

1. 不实现 VOICE / VIDEO 消息接收（仅 TEXT / IMAGE / FILE）
2. 不实现公众号文章 / 链接卡片之类的特殊消息（iLink 是否支持也不清楚）
3. 不引入"多账号扫码"——一个 connector 实例 = 一个个微账号
4. 不做 contact / chat list 同步（用户不需要拉通讯录，仅做"被动应答"）

## §1. 扫码登录流程（核心 UX）

这是 connector 内最特殊的部分——需要在桌面 app 内**展示二维码**。

```
用户在设置面板点"添加个微账号"
   ↓
WechatConnector::begin_registration() 调 iLink startWeixinLoginWithQr
   ↓ 返回 qr_url + login_id
前端拉取 qr_url 渲染成图片，桌面 app 模态框显示
   ↓ 用户用微信扫码 + 在手机上确认
后台 poll_registration() 长轮询 waitForWeixinLogin
   ↓ 状态：waiting / confirmed / cancelled / expired
确认后拿 bot_token + ilink_bot_id → SecureStorage 持久化
   ↓
状态机进入 Connected
```

**spec 决议**：扫码 UI **复用** Phase 0 dingtalk 已有的"设备码 + 二维码"组件（dingtalk OPEN_CLAW 也是扫码场景）。前端组件签名：

```ts
showQrCodeModal({
  title: '添加个人微信账号',
  qrUrl: string,
  pollState: () => Promise<'waiting' | 'confirmed' | 'cancelled' | 'expired'>,
  expireSeconds: number,
})
```

如果 dingtalk 那条路径还没抽公共组件，本期顺手抽（小 PR）。

## §2. 目录结构 + capabilities

```
src-tauri/src/connector/im/wechat/
├── mod.rs                  # impl IMConnector for WechatConnector
├── runtime.rs              # getUpdates 长轮询循环
├── api.rs                  # 5 个 iLink endpoint 封装
├── login.rs                # 扫码登录 begin + poll
├── sender.rs               # sendMessage / sendTyping
├── media.rs                # getUploadUrl + AES-128-ECB 加解密 + 上传下载
├── parser.rs               # iLink raw message → ChannelMessage
├── crypto.rs               # AES-128-ECB 加解密（纯函数 + 单测）
└── types.rs
```

```rust
ConnectorCapabilities {
    inbound: InboundModel::Stream,    // 长轮询，对 Manager 透明
    outbound_aicard: false,
    outbound_markdown: false,         // 个微不支持 markdown
    supports_attachments: true,
    supports_group_chat: true,
    supports_private_chat: true,
    auth_flow: AuthFlow::QRCode,
}
```

## §3. 长轮询入站

```rust
async fn start(&self, ctx: ConnectorContext) -> Result<BoxStream<ChannelMessage>> {
    let token = self.load_token()?;
    let stream = async_stream::stream! {
        let mut cursor = self.load_cursor();    // 从磁盘恢复
        loop {
            tokio::select! {
                _ = ctx.cancel_token.cancelled() => break,
                resp = self.api.get_updates(token, cursor, timeout=30s) => {
                    match resp {
                        Ok(updates) => {
                            for msg in updates {
                                cursor = msg.cursor;
                                yield self.parser.normalize(msg);
                            }
                            self.save_cursor(cursor);
                        }
                        Err(Transient) => {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                        Err(AuthExpired) => return,   // 跳出 stream，触发 ConfigError
                        Err(e) => log::error!("get_updates: {e}"),
                    }
                }
            }
        }
    };
    Ok(Box::pin(stream))
}
```

**cursor 持久化**：跟 Telegram offset 同形，放 `~/.renlijia/users/{scope}/channels/wechat/{bot_id}/state.json`。

## §4. AES-128-ECB 媒体加密

iLink 媒体走 CDN，URL 形如：

```
https://wx.qlogo.cn/...?encrypt_query_param=xxx
+ msg.media.aes_key (16 字节 hex)
```

下载流程：

1. HTTP GET CDN URL（带 `encrypt_query_param` 在 query）
2. 拿到的 body 是 AES-128-ECB + PKCS#7 padding 加密的原始字节
3. `aes_key` 解密 → 原始文件字节

`crypto.rs`：纯函数 `aes128_ecb_decrypt(ciphertext: &[u8], key: &[u8]) -> Result<Vec<u8>>` + 全量单测。

上传反向走：`getUploadUrl()` → 拿 signed URL + `aes_key` → 本地 AES-128-ECB 加密 → PUT 上去。

## §5. 错误处理

| iLink 错误 | 映射 |
|---|---|
| Session 超时 | `AuthExpired` → 触发重新扫码 |
| 网络抖动 / 5xx | `Transient` → backoff |
| 4xx (参数错) | `Fatal(msg)` |
| Rate limit (具体 code 由实施时实测确定) | `Transient` |

## §6. 测试

- `crypto::tests`：AES-128-ECB 自洽 + 与 NodeJS plugin 共享一个 fixture（同一 key+plaintext 在两侧产出相同 ciphertext）
- `parser::tests`：5 种消息类型 → ChannelMessage 映射（VOICE / VIDEO 走 `[不支持的消息类型]` 占位）
- `login::tests`：mock iLink，begin → poll waiting → poll confirmed → 拿 token
- `api::tests`：mock HTTP，cursor 推进 + 异常重试
- `tests/im_wechat_integration.rs`：起 connector + mock iLink + 完整收发

## §7. 实施 PR 切分

- **PR1** `im/wechat/` 骨架 + types + iLink endpoint 常量
- **PR2** crypto.rs (AES-128-ECB) + 单测（NodeJS plugin fixture 必须 pass）
- **PR3** login.rs（begin/poll 扫码）+ 前端二维码模态框（顺手抽 dingtalk 共用组件）
- **PR4** api.rs + getUpdates 长轮询 + cursor 持久化 + impl IMConnector
- **PR5** sender.rs + parser.rs（TEXT / IMAGE / FILE 3 类）
- **PR6** media.rs（上传下载 + crypto 接入）
- **PR7** 集成测试 + 前端"添加个微" UI

## §8. 风险

| 风险 | 缓解 |
|---|---|
| iLink 服务 SLA / 文档不公开（openclaw plugin 是实测得来） | 实施时先用一个真实账号跑通 + 全程录 HTTP log 作回归资产 |
| iLink 接口变更（无 SemVer 保证） | 同上：用真实账号 + canary 测试，发现变化时调 parser |
| AES-128-ECB 跟 NodeJS plugin 实现微差异 | fixture 测试必须用 plugin 的真实 ciphertext，不能凭空构造 |
| 扫码登录被微信风控（罕见但有可能） | spec 不优化；记录现象交给用户切到企微 |
| 多端登录冲突（手机+iLink+plugin 同时） | 不在 connector 处理；按 iLink 报错 fail-fast |
| VOICE / VIDEO 接收不支持 → 用户期待落空 | spec 明确写了占位文案 + 未来扩展点 |

## §9. 后续扩展

- VOICE / VIDEO 媒体接收（需要语音转写工具协作）
- 公众号文章卡片 / 链接卡片解析
- 多账号扫码（同一桌面 app 挂多个个微号）
- contact / chat list 同步（用户主动选择"群"才发消息）

这些都不在 Phase 5 范围。

# 2026-05-19 — Wechat phase 5 修复 from_is_self 误过滤 + image/file CDN 下载落地

## Context

phase 5 PR4-5 把 wechat (iLink 个人微信) 的扫码登录 + long-poll + 发文本通路接上之后，桌面 app 实测**完全收不到自己微信号发给自家 bot 的任何消息**——既不报错，也不进 candidate 日志，bot 像哑巴。同时图片 / 文件附件被 `flatten_item_list_to_text` 替成 `[图片] / [文件]` 占位串发给 LLM，附件根本没下载。

本次会话定位并修复了 2 个问题：
1. **`from_is_self` 误过滤**（root cause bug，已 commit `caf59c89`）
2. **图片 / 文件 CDN 下载链路落地**（功能补齐，对齐 wecom 的实现形态）

## 问题 1：`from_is_self` 误过滤

**症状**：扫码登录成功、long-poll 进入正常循环（每 18s 一轮空响应），但用户从扫码登录的同一个微信号给自己的 bot 发"你好"后，日志静默，没有任何 inbound 事件。

**Root cause**：`src-tauri/src/connector/im/wechat/runtime.rs`（PR4 当初引入）有一条多余的"自己 echo 过滤"：

```rust
if from_user_id == cfg.self_user_id {
    continue;
}
```

设计意图是过滤 bot 自己 echo 回来的消息，但在 iLink 个人微信协议下，`from_user_id` 是会话**对端**的 wxid，而 `ilink_user_id`（即 `cfg.self_user_id`）是登录扫码的微信号本身。用户用同一个微信号自测自己的 bot 时，这两个值天然相同——所有用户消息都被这条判断吃掉。

参考 openclaw-weixin-main `src/monitor/monitor.ts` 的 inbound 处理循环，里面**没有**任何 `from_user_id == self` 判断，证明设计上不需要。Bot 自己 echo 的正确识别是 `message_type == 2`（BOT），那条分支已经在前面 `continue`，不需要叠 self-id 校验。

**修复**（在 `caf59c89`）：删除这条过滤，保留 NOTE 注释解释为什么不要。同时把 long-poll 内部所有静默 `debug!` 跳过路径升级到 `info!` + 加每轮 getUpdates 响应摘要 + 临时加 raw body diag log（任务结束后撤），后续 wechat connector 类似问题能直接从日志定位。

## 问题 2：图片 / 文件 CDN 下载链路

phase 5 main plan 把媒体上下行规划在 PR6，本次提前把**下载**方向（image / file）做掉，对齐 wecom 同等能力。voice / video 留 PR6 后续。

### 协议要点（从抓的 raw body 验证）

iLink CDN 下发的 `MessageItem` 结构：

```json
{
  "type": 2,                       // 1 TEXT, 2 IMAGE, 3 VOICE, 4 FILE, 5 VIDEO
  "is_completed": true,
  "image_item": {
    "aeskey": "7716ac836c2fc1faae223956cff3dbf7",        // hex 32 字符（裸 key）
    "media": {
      "encrypt_query_param": "...",                       // 已嵌在 full_url
      "aes_key": "NzcxNmFjODM2YzJmYzFmYWFlMjIzOTU2Y2ZmM2RiZjc=",  // base64(hex)，等价于上面 aeskey
      "full_url": "https://novac2c.cdn.weixin.qq.com/c2c/download?encrypted_query_param=...&taskid=..."
    },
    "mid_size": 35447, "thumb_size": 7735, ...
  }
}
```

```json
{
  "type": 4,
  "is_completed": true,
  "file_item": {
    "media": { "aes_key": "...", "full_url": "..." },
    "file_name": "王瀚坤简历.pdf",
    "md5": "...",
    "len": "518728"
  }
}
```

关键点：

- **下载**：直接 GET `media.full_url`（已带好 query string），response body 是密文
- **加密**：**AES-128-ECB + PKCS#7 padding**（注意：跟 wecom 的 CBC + 32 字节 padding 不一样）
- **aes_key 两种编码**，`media::parse_aes_key` 都兼容：
  - base64-decode → 16 字节 → 直接当 key
  - base64-decode → 32 ASCII hex → 再 hex-decode → 16 字节
- **优先级**：图片用 `image_item.aeskey`（裸 hex，最直接），不存在再 fallback 到 `image_item.media.aes_key`（base64(hex)）；文件只有 `media.aes_key` 一条路径

### 文件改动

| 文件 | 改动 |
|------|------|
| `src-tauri/Cargo.toml` | + `ecb = "0.1"` 依赖（RustCrypto，跟现有 `cbc = "0.1"` 同体系） |
| `src-tauri/src/storage/aijia_home.rs` | + `tmp_wechat_downloads_dir()` helper |
| `src-tauri/src/connector/im/wechat/mod.rs` | + `pub mod media;` |
| `src-tauri/src/connector/im/wechat/media.rs` | **新增**：镜像 `wecom::media` 结构。`WechatDownloadedFile` / `decode_download_code` / `parse_aes_key` / `hex_aeskey_to_base64` / `decrypt_aes_ecb` / `download_and_decrypt` / `download_and_save` + sha256 内容寻址 + tmp/rename 原子写。9 个 unit test 覆盖 key 解析 + ECB roundtrip + download_code 切分。 |
| `src-tauri/src/connector/im/wechat/api.rs` | 扩展 `MessageItem` 反序列化：补 `image_item` / `file_item` / `is_completed` 字段 + `CdnMedia` / `ImageItem` / `FileItem` 结构体。新增 `pub fn extract_attachments_from_item_list()`：跳过 `is_completed: false`；图片优先 `aeskey` (hex) fallback `media.aes_key`；文件用 `media.aes_key`；统一 `download_code` 形式 `wechat://{aes_key_b64}@{full_url}`。撤掉前一阶段的 raw body diag log。 |
| `src-tauri/src/connector/im/wechat/runtime.rs` | 构造 `ChannelMessage` 时调 `extract_attachments_from_item_list` 拿 attachments；有附件时清空 text（避免占位串污染 LLM input），全无附件无文本时 skip。`inbound` log 加 `attachments=N` 字段。 |
| `src-tauri/src/connector/im/manager.rs` | + `wechat_downloads_dir()` helper；+ `downloaded_to_chat_attachment_wechat` shape adapter；+ `download_specs_for_turn_wechat`（串行下载 + warn-on-fail）；wechat worker 在 `build_channel_chat_request` 前加 attachments 下载 block + 全失败兜底回信"附件下载全部失败，请重发。"，对称 wecom L780 实现。 |

### 复用

- `super::wecom::media::extension_or_bin` 直接 re-export（扩展名推断完全一样）
- `super::wecom::media::mime_from_ext` 因为是私有的，inline 镜像一份（避免改 wecom 公开 API；wechat 将来要加 audio/silk 等独立路径也好演进）
- `wechat::media::download_and_save` 的 sha256 内容寻址 + tmp/rename 原子写策略沿用 wecom
- manager 的 `download_specs_for_turn_wechat` 是 wecom 同名函数的拷贝改名

### 不在本期范围

- **voice (type=3)**：需 silk → wav transcode（依赖 `libsilk` 或外部 `ffmpeg`），跨平台依赖复杂
- **video (type=5)**：解密路径同图片，但 LLM vision 不消费视频，落盘后还需前端预览
- **多分辨率图片**：当前一律用 `media.full_url`（应该是中等大小），`hd_size > 0` 时是否拉高清留观察
- **缩略图 (thumb_*)**：用不上

后续 PR 一起补。

## Verification

1. `cargo test --lib wechat::` —— 40 passed（含本次新增 11 个：9 个 media + 4 个 api `extract_attachments` + 2 个 api real-body 反序列化）
2. `cargo check --lib` —— 编译干净，0 新警告
3. **手测**（已完成）：
   - 个人微信账号给自家 bot 发"你好" → 正常入站 + LLM 回复（修复 problem 1 验证）
   - 发图片 (94951 字节 JPEG 810×1278) → 落到 `~/.renlijia/tmp/wechat_downloads/{sha256}.jpg`，`file` 命令确认 JPEG magic bytes 正确
   - 发文件 (王瀚坤简历.pdf 518728 字节) → 落到同目录 `{sha256}.pdf`，`file` 命令确认 PDF 1.7 正常
   - 大小完全匹配 raw body 的 `len: "518728"` —— ECB PKCS7 反填充没多吃也没少吃字节
   - `build_turn_permission_ctx` 自动把 `wechat_downloads` 加入 `additional_working_dirs`，LLM 能读

## 关键 commit

- `caf59c89` — fix(connector/im/wechat): drop spurious from-is-self filter + add long-poll diag
- 本次 commit（image/file CDN 下载）—— 待提交

//! Telegram Bot API IM connector — long-poll 私聊 + 加固版。
//!
//! 实现 `IMConnector`：入站 Bot API `getUpdates` 长轮询（零公网入口），
//! 出站 `sendMessage` HTML + `sendDocument` 附件。Pairing 协议参考 OpenClaw
//! `dmPolicy: pairing`。
//!
//! See `docs/superpowers/specs/2026-05-19-im-telegram-connector-design.md` (MVP)
//! 和 `docs/superpowers/specs/2026-05-20-im-telegram-hardening-design.md` (加固).
//!
//! ## PR1-4 已落地的加固
//!
//! - **PR1 传输层**：
//!   - `sender::split_telegram_html` 按 4000 byte 上限分片，保留 `<pre><code>` 完整
//!   - `long_poll::run_watchdog` 30s tick / 120s 阈值 stall watchdog → `api::rebuild_client`
//!   - `TelegramApiError::TransportConnect`（可重试）vs `TransportConnected`（不可重试）
//! - **PR2 入站类型**：parser 识别 voice/audio/video/video_note/sticker/animation
//!   6 种类型，long_poll 每条都回提示给已配对用户
//! - **PR3 出站附件 + 引用**：`api::send_document` multipart + `sender::extract_local_paths`
//!   自动从 markdown 提取本地路径 + 50MB 上限提示 + reply_to_message_id（首条 chunk）
//! - **PR4 可靠性**：pairing pending 落盘到 `pending-pairings.json` 抗重启 +
//!   `download_file` SSRF host 检查（仅生产 api_base）

pub mod api;
pub mod connector;
pub mod download;
pub mod long_poll;
pub mod pairing;
pub mod parser;
pub mod registration;
pub mod reply_forwarder;
pub mod sender;
pub mod types;

pub use connector::TelegramConnector;

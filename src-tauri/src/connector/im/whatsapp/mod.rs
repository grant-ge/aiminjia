//! WhatsApp connector implementation —— OpenClaw 同款方案
//! ([docs.openclaw.ai/channels/whatsapp](https://docs.openclaw.ai/channels/whatsapp))
//! via Rust crate [wa-rs](https://github.com/homunbot/wa-rs)
//! (jlucaso1/whatsapp-rust 的 stable-Rust fork，whatsmeow + Baileys 协议移植)。
//!
//! ⚠️ 协议是 WhatsApp Web 多设备协议，**TOS 灰区**。账号有被 WhatsApp
//! 限速 / 封禁的风险，必须在前端首次扫码时显示 §9.1 风险 banner，
//! 用户勾选"已知晓"才能进入扫码界面。
//!
//! Phase 4 PR 切分（spec §10.2）：
//!   PR1 —— 骨架 + Cargo deps + capability 字段（**本 PR**）
//!   PR2 —— Bot 生命周期 + SqliteStore + _pairing 路径
//!   PR3 —— 扫码登录（begin/poll_registration + PairingState 状态机）
//!   PR4 —— 入站（bot.run() worker + Event::Message dispatch + parser）
//!   PR5 —— 出站 text/markdown + 错误映射
//!   PR6 —— 出站 AI Card 占位 + 增量编辑
//!   PR7 —— 入站媒体 download_media
//!   PR8 —— 集成测试 + UI + banner
//!
//! 详见 `docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md`。

pub mod aicard;
pub mod config;
pub mod connector;
pub mod download;
pub mod gc;
pub mod markdown;
pub mod parser;
pub mod proxy_transport;
pub mod reply_forwarder;
pub mod runtime;
pub mod sender;
pub mod session;
pub mod types;

pub use connector::WhatsAppConnector;

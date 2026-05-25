//! 企业微信智能机器人（aibot）IM connector。
//!
//! 入站走腾讯官方 aibot WebSocket 长连接（`wss://openws.work.weixin.qq.com`）。
//! 协议参考：`@wecom/aibot-node-sdk@1.0.7` MIT 开源 SDK。
//!
//! See `docs/superpowers/specs/2026-05-18-im-wecom-phase2-design.md`.

pub mod aibot_client;
pub mod aibot_protocol;
pub mod connector;
pub mod media;
pub mod parser;
pub mod registration;
pub mod reply_forwarder;
pub mod sender;
pub mod types;

pub use connector::WecomConnector;

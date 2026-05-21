//! WeChat (iLink HTTP API) connector — MVP scaffold for QR-code login UI.
//!
//! Spec: docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md
//!
//! Current scope (MVP for testing the login UI):
//!   - endpoints + headers + appid + LoginSession state machine
//!   - real HTTP calls against ilinkai.weixin.qq.com (no mocking)
//!   - `WechatConnector` implements `IMConnector` with start() returning
//!     empty stream and send() returning NotSupported — actual inbound /
//!     outbound implementation comes in Phase 5 PR4+.

pub mod api;
pub mod appid;
pub mod connector;
pub mod endpoints;
pub mod headers;
pub mod login;
pub mod media;
pub mod registration;
pub mod reply_forwarder;
pub mod runtime;
pub mod types;

pub use connector::WechatConnector;

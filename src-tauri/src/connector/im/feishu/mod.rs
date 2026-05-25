//! Feishu (Lark) connector implementation. Mirrors `dingtalk/` structure.
//! Phase 1 PRs:
//!   PR1 — skeleton + impl IMConnector with stubbed methods + frontend stub button
//!   PR2 — device-code registration + tenant_access_token cache
//!   PR3 — WebSocket runtime + message normalize
//!   PR4 — Text/Markdown send + webhook reply path
//!   PR5 — CardKit streaming (create / stream / finish / fail) with rate limit
//!   PR6 — attachment download + PendingQueueManager integration
//!   PR7 — integration test + UI

pub mod card;
pub mod connector;
pub mod download;
pub mod pbbp2;
pub mod registration;
pub mod reply_forwarder;
pub mod stream;
pub mod token;
pub mod types;

pub use connector::FeishuConnector;

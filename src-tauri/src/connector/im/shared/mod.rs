//! Cross-platform helpers shared by all IM connector implementations.
//!
//! Files in this module are platform-agnostic: they operate on the abstractions
//! defined in `super::types` (or on runtime/storage primitives) and must not
//! depend on any specific platform sub-module (e.g. `super::dingtalk::*`).
//!
//! Exception: `reply_manager` still imports `super::super::dingtalk::card` and
//! `super::super::dingtalk::token` for AI-card streaming. That coupling is
//! staged for removal in PR5 of the IM connector trait refactor, when the
//! card-streaming path will route through `IMConnector::send`.

pub mod aicard_fallback;
pub mod app_feedback;
pub mod ask_coordinator;
pub mod config_store;
pub mod dedup;
pub mod envelope;
pub mod pending_adapter;
pub mod proxy;
pub mod reconnect;
pub mod reply_manager;
pub mod router;
pub mod token;

pub use dedup::MessageDedupSet;
pub use reconnect::ReconnectBackoff;
pub use token::{PlatformTokenSource, TokenCache as SharedTokenCache};

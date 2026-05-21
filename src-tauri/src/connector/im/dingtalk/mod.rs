//! DingTalk connector implementation. The `IMConnector` trait impl lives here
//! (added in Phase 0 PR4). For now this module just groups the existing
//! dingtalk support files.

pub mod card;
pub mod connector;
pub mod download;
pub mod registration;
pub mod stream;
pub mod token;

pub use connector::DingtalkConnector;

pub mod dingtalk;
pub mod factory;
pub mod feishu;
pub mod manager;
pub mod shared;
pub mod telegram;
pub mod trait_def;
pub mod types;
pub mod wechat;
pub mod wecom;
pub mod whatsapp;

// Backwards-compat re-exports: external consumers reach into these submodules
// via `crate::connector::im::ask_coordinator::*` etc. PR2 moved the files into
// `shared/`; these re-exports keep the old paths resolvable so consumers don't
// have to change their import sites.
pub use shared::ask_coordinator;
pub use shared::config_store;
pub use shared::pending_adapter;
pub use shared::reply_manager;
pub use shared::router;

pub use manager::ChannelManager;
pub use shared::config_store::ChannelConfigStore;
pub use shared::reply_manager::DingtalkReplyManager;
pub use trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    PollRequest, RegistrationBegin, RegistrationPoll, RegistrationRequest, ReplyContent,
    ReplyTarget,
};
pub use types::{
    ChannelCapability, ChannelConfigView, ChannelConnectionState, ChannelConversation,
    ChannelMessagePayload, ChannelPlatformState, ChannelPlatformStatePayload,
    ChannelRegistrationBeginResult, ChannelRegistrationPollResult, ChannelRegistrationPollState,
    DingtalkStoredConfig, Platform, RobotCodeSource,
};

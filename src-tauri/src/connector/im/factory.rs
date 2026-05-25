//! Platform-neutral factory entry points the manager uses to construct
//! `Arc<dyn IMConnector>` without taking a hard dependency on any specific
//! platform module.
//!
//! Architecture rule (enforced by `tests/review_im_layering.rs`): manager.rs
//! MUST NOT contain `use super::dingtalk::*` or `use crate::connector::im::dingtalk::*`.
//! Adding a new platform (e.g. 飞书) means adding a sibling factory function
//! here, NOT editing manager.

use std::sync::Arc;

use crate::connector::im::dingtalk::connector::{DingtalkConnector, StatusCallback};
use crate::connector::im::dingtalk::token::TokenCache;
use crate::connector::im::feishu::connector::FeishuConnector;
use crate::connector::im::shared::reply_manager::DingtalkReplyManager;
use crate::connector::im::trait_def::IMConnector;
use crate::connector::im::types::ChannelConnectionState;
use crate::connector::im::wechat::connector::WechatConnector;
use crate::connector::im::wecom::connector::WecomConnector;

/// Build a `DingtalkConnector` boxed behind `Arc<dyn IMConnector>` AND keep a
/// concrete `Arc<DingtalkConnector>` handle (returned alongside) for manager-
/// side calls to `remember_session` that the trait does not expose.
pub fn build_dingtalk_connector(
    app_key: String,
    app_secret: String,
    robot_code: String,
    reply_manager: Arc<DingtalkReplyManager>,
    on_status: StatusCallback,
) -> (Arc<dyn IMConnector>, Arc<DingtalkConnector>) {
    let concrete = Arc::new(DingtalkConnector::with_status_callback(
        app_key,
        app_secret,
        robot_code,
        reply_manager,
        Arc::new(TokenCache::new()),
        on_status,
    ));
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    (dyn_handle, concrete)
}

/// Build a `FeishuConnector` plus its concrete handle for `remember_session`.
/// PR1 returns a stub connector; PR2-PR7 fill in actual functionality.
pub fn build_feishu_connector(
    app_id: String,
    app_secret: String,
    on_status: FeishuStatusCallback,
) -> (Arc<dyn IMConnector>, Arc<FeishuConnector>) {
    let concrete = Arc::new(FeishuConnector::with_status_callback(
        app_id, app_secret, on_status,
    ));
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    (dyn_handle, concrete)
}

/// Re-export the StatusCallback alias for manager-side closure typing.
pub use crate::connector::im::dingtalk::connector::StatusCallback as DingtalkStatusCallback;
pub type FeishuStatusCallback =
    Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>;
pub type WecomStatusCallback =
    Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>;
pub type WechatStatusCallback =
    Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>;
pub type TelegramStatusCallback =
    Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>;
pub type WhatsappStatusCallback =
    Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>;

/// Build a `WecomConnector` plus its concrete handle, mirroring the
/// feishu factory shape. Manager-side: PR6 will wire register / refresh.
pub fn build_wecom_connector(
    bot_id: String,
    secret: String,
    on_status: WecomStatusCallback,
) -> (Arc<dyn IMConnector>, Arc<WecomConnector>) {
    let concrete = Arc::new(WecomConnector::with_status_callback(
        bot_id, secret, on_status,
    ));
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    (dyn_handle, concrete)
}

/// Build a `WechatConnector` plus its concrete handle (Phase 5).
/// iLink 凭证不只是单 token —— 还需要 ilink_bot_id / ilink_user_id /
/// effective_base_url / app_id / client_version 才能起 long-poll worker。
#[allow(clippy::too_many_arguments)]
pub fn build_wechat_connector(
    bot_token: String,
    ilink_bot_id: String,
    ilink_user_id: String,
    base_url: String,
    app_id: String,
    client_version: String,
    on_status: WechatStatusCallback,
) -> (Arc<dyn IMConnector>, Arc<WechatConnector>) {
    let concrete = Arc::new(WechatConnector::new(
        bot_token,
        ilink_bot_id,
        ilink_user_id,
        base_url,
        app_id,
        client_version,
        on_status,
    ));
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    (dyn_handle, concrete)
}

/// Build a `TelegramConnector` plus its concrete handle. Diverges from the
/// other factories: `TelegramConnector::new` is fallible because
/// `TelegramApi::new` builds a `reqwest::Client` internally, so we propagate
/// that error to the manager (which surfaces it as `ConfigError`).
pub fn build_telegram_connector(
    bot_id: String,
    bot_username: String,
    token: String,
    config_store: Arc<crate::connector::im::shared::config_store::ChannelConfigStore>,
    on_status: TelegramStatusCallback,
) -> anyhow::Result<(
    Arc<dyn IMConnector>,
    Arc<crate::connector::im::telegram::connector::TelegramConnector>,
)> {
    use crate::connector::im::telegram::connector::TelegramConnector;
    let concrete = Arc::new(TelegramConnector::new(
        bot_id,
        bot_username,
        token,
        config_store,
        on_status,
    )?);
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    Ok((dyn_handle, concrete))
}

/// Build a `WhatsAppConnector` plus its concrete handle. PR1 stub —— concrete
/// 类型 PR2-PR8 会带上 Bot/SqliteStore 等内部状态。Manager wiring（包括
/// 注册路径、register_whatsapp_connector、reply_forwarder）留到 PR3。
/// PR7：加 `attachments_dir` 参数，由 manager 从 AiJiaHome::tmp_whatsapp_downloads_dir()
/// 解析后传入，用于 WhatsAppMediaDownloader 的下载目标目录。
pub fn build_whatsapp_connector(
    on_status: WhatsappStatusCallback,
    attachments_dir: std::path::PathBuf,
) -> (
    Arc<dyn IMConnector>,
    Arc<crate::connector::im::whatsapp::connector::WhatsAppConnector>,
) {
    use crate::connector::im::whatsapp::connector::WhatsAppConnector;
    let concrete = Arc::new(WhatsAppConnector::with_status_callback(
        on_status,
        attachments_dir,
    ));
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    (dyn_handle, concrete)
}

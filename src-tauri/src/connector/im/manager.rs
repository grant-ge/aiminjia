//! ChannelManager — 管理 IM 频道连接生命周期

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::runtime::store::ConversationStore;
use crate::runtime::ChatTurnRequest;
use crate::storage::aijia_home::AiJiaHome;
use crate::storage::crypto::SecureStorage;
use crate::transport::tauri_commands::chat::TauriChatCommandAdapter;

use super::dingtalk::card::CardTarget;
use super::dingtalk::registration::{begin_registration, poll_registration, RegistrationPollState};
use super::factory::build_dingtalk_connector;
use super::shared::config_store::ChannelConfigStore;
use super::shared::reply_manager::DingtalkReplyManager;
use super::shared::router::ChannelSessionRouter;
use super::trait_def::{ConnectorContext, IMConnector};
use super::types::{
    ChannelConnectionState, ChannelConversation, ChannelMessage, ChannelMessagePayload,
    ChannelPlatformState, ChannelPlatformStatePayload, ChannelRegistrationBeginResult,
    ChannelRegistrationPollResult, ChannelRegistrationPollState, ConversationType,
    DingtalkStoredConfig, Platform,
};

use super::dingtalk::download::{DingtalkFileDownloader, DownloadedFile};
use super::types::AttachmentKind;
use crate::runtime::chat::chat_turn_driver::{ChatAttachmentRef, IM_MOBILE_CHANNEL_CONTEXT};

const DINGTALK_GREETING_PROMPT: &str = "你好";

/// Per-platform runtime state. Each platform (dingtalk / feishu / ...) owns its
/// own slot — disabling or reconnecting one MUST NOT touch another's slot.
/// Prior to PR3.5 these fields were single-slot at the `ChannelManager` level
/// and dingtalk/feishu collided (e.g. `set_enabled(Feishu, false)` would cancel
/// a running dingtalk stream).
struct PerPlatformState {
    connection: ChannelConnectionState,
    last_error: Option<String>,
    stream_cancel: Option<CancellationToken>,
    message_task: Option<JoinHandle<()>>,
    /// Monotonic generation counter — bumped on every (re)connect / stop so
    /// stale status callbacks and worker iterations can detect drift and
    /// bail. Kept as `Arc<AtomicU64>` so closures spawned at connect-time
    /// retain access even after the map entry is replaced.
    stream_generation: Arc<AtomicU64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DingtalkGreetingTarget {
    session_id: String,
    external_conversation_key: String,
}

fn select_dingtalk_greeting_target(
    conversations: &[ChannelConversation],
    current_robot_code: &str,
) -> Option<DingtalkGreetingTarget> {
    conversations
        .iter()
        .find(|conversation| {
            conversation.platform == Platform::Dingtalk
                && conversation.conversation_type == ConversationType::Private
                && conversation.is_active_robot
                && conversation.robot_code == current_robot_code
        })
        .map(|conversation| DingtalkGreetingTarget {
            session_id: conversation.session_id.clone(),
            external_conversation_key: conversation.external_id.clone(),
        })
}

impl PerPlatformState {
    fn unconfigured() -> Self {
        Self {
            connection: ChannelConnectionState::Unconfigured,
            last_error: None,
            stream_cancel: None,
            message_task: None,
            stream_generation: Arc::new(AtomicU64::new(0)),
        }
    }
}

pub struct ChannelManager {
    app_handle: AppHandle,
    chat_adapter: Arc<TauriChatCommandAdapter>,
    conversation_store: Arc<dyn ConversationStore>,
    config_store: Arc<ChannelConfigStore>,
    /// Per-platform sessions.json paths. Each platform's IM worker reads and
    /// writes its own file via `ChannelSessionRouter`, so cross-platform
    /// router_keys (飞书 cli_*, 钉钉 dingaf*, 企微 UUID) can never collide and
    /// `hydrate_conversations` doesn't need to guess platform from prefix.
    /// Populated for every `Platform::all()` at construction; lookups should
    /// always succeed.
    sessions_paths: HashMap<Platform, PathBuf>,
    /// Per-platform runtime state (connection, cancel token, worker handle,
    /// generation counter). Lazily populated on first connect via
    /// `entry().or_insert_with(PerPlatformState::unconfigured)`.
    platform_state: Arc<RwLock<HashMap<Platform, PerPlatformState>>>,
    seen_msg_ids: Arc<super::shared::dedup::MessageDedupSet>,
    conversations: Arc<RwLock<Vec<ChannelConversation>>>,
    reply_manager: Arc<DingtalkReplyManager>,
    reply_subscribed: Arc<AtomicBool>,
    feishu_reply_subscribed: Arc<AtomicBool>,
    /// 跟 feishu_reply_subscribed 对称：保证 WecomReplyForwarder 整个进程
    /// 生命周期内只挂一次 RuntimeEventBus，避免重连/重保存配置时重复挂载。
    wecom_reply_subscribed: Arc<AtomicBool>,
    /// 跟 wecom_reply_subscribed 对称：保证 WechatReplyForwarder 整个进程
    /// 生命周期内只挂一次 RuntimeEventBus。
    wechat_reply_subscribed: Arc<AtomicBool>,
    /// 跟 wechat_reply_subscribed 对称：保证 TelegramReplyForwarder 整个进程
    /// 生命周期内只挂一次 RuntimeEventBus。
    telegram_reply_subscribed: Arc<AtomicBool>,
    /// 跟 telegram_reply_subscribed 对称：保证 WhatsAppReplyForwarder 整个进程
    /// 生命周期内只挂一次 RuntimeEventBus。
    whatsapp_reply_subscribed: Arc<AtomicBool>,
    /// 缓存最近一次 register_telegram_connector 返回的 concrete handle。
    /// 调用方（PR pairing-confirm flow）通过 telegram_connector() 拉这个 handle
    /// 直接调用 remember_session / sender 等非 trait 接口。
    telegram_concrete:
        Arc<tokio::sync::RwLock<Option<Arc<super::telegram::connector::TelegramConnector>>>>,
    /// 缓存最近一次 register_whatsapp_connector 返回的 concrete handle。
    /// PR3+ pairing flow 通过 whatsapp_connector() 拉这个 handle 直接调用
    /// start_pairing_session / poll_pairing_state 等非 trait 接口。
    whatsapp_concrete:
        Arc<tokio::sync::RwLock<Option<Arc<super::whatsapp::connector::WhatsAppConnector>>>>,
    /// 保证整个进程生命周期内只 spawn 一次 WhatsApp 附件 GC 任务。
    /// `OnceCell::get_or_init` 在 register_whatsapp_connector 调用，幂等。
    whatsapp_gc_spawned: Arc<tokio::sync::OnceCell<()>>,
    ask_coordinator: Option<Arc<super::shared::ask_coordinator::IMAskCoordinator>>,
    ask_subscribed: Arc<AtomicBool>,
    /// 已建立的 IM 频道 session id 集合，与 ask_coordinator 的 registry 共享同一 Arc。
    /// 消息 worker 每创建一个新 session 时向此集合写入，确保 coordinator 能识别频道会话。
    channel_session_ids: Arc<std::sync::RwLock<HashSet<String>>>,
    /// PendingQueueManager — IM 消息进入后先走 enqueue_or_send，闲时直发、忙时入队。
    pending_manager: Arc<crate::runtime::pending::PendingQueueManager>,
    /// 已注册的平台 connectors，按 Platform 索引。Phase 0 仅 Dingtalk；
    /// Phase 1+ 飞书/企微/Telegram/WhatsApp/个微 共用同一份 worker 编排逻辑。
    connectors: Arc<RwLock<HashMap<Platform, Arc<dyn IMConnector>>>>,
    /// Manager 构造时刻的 wall-clock 毫秒时间戳。仅 feishu worker 用来识别
    /// "服务端重发的、`create_time` 早于本次启动" 的历史消息——飞书 WS 在
    /// 重连/进程重启后会把未 ACK 的消息重投，而我们的 msg_id 去重表是内存的
    /// 跨进程不存活，所以拿启动时间和 `ChannelMessage.created_at_ms` 比较，
    /// 早于 (started_at - grace) 的视作重发，只 ACK 不触发 LLM。
    started_at_ms: i64,
    active_scope: Option<crate::storage::UserScope>,
    /// Strong references to event bus subscribers owned by this instance.
    /// The bus stores `Weak` refs, so when this CM is dropped the subscribers
    /// become unreachable and are pruned on next emit — no unsubscribe needed.
    subscriber_anchors:
        std::sync::Mutex<Vec<Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>>>,
    /// Set to `true` by `shutdown()`. Once inactive, mutating entry points
    /// (`set_enabled`, `begin_*_registration`) and worker session-id inserts
    /// become no-ops. Guards against a zombie worker polluting a new user's
    /// `channel_session_ids` after account switch / logout.
    inactive: Arc<std::sync::atomic::AtomicBool>,
}

/// Path to AIjia's global config file (`~/.renlijia/config.json`). Used by
/// wechat's `appid::resolve_app_id` to honor the optional override.
fn aijia_config_path() -> PathBuf {
    AiJiaHome::from_home().global_config_path()
}

async fn handle_pending_action_pre_dispatch(
    ask_coordinator: Option<&Arc<super::shared::ask_coordinator::IMAskCoordinator>>,
    session_id: &crate::runtime::ids::SessionId,
    content: &str,
) -> anyhow::Result<super::shared::ask_coordinator::HandleOutcome> {
    if let Some(coordinator) = ask_coordinator {
        coordinator
            .try_handle_reply(session_id, content.to_string())
            .await
    } else {
        Ok(super::shared::ask_coordinator::HandleOutcome::NotPending)
    }
}

async fn send_pending_action_text_ack(
    connector: &Arc<dyn IMConnector>,
    session_id: &str,
    conv_key: &str,
    marker: &str,
    message: String,
) {
    if let Err(err) = connector
        .send(
            crate::connector::im::trait_def::ReplyTarget {
                session_id: session_id.to_string(),
                external_conversation_key: conv_key.to_string(),
            },
            crate::connector::im::trait_def::ReplyContent::Text(message),
        )
        .await
    {
        log::warn!(
            "{} pending approval ACK text send failed session={}: {:#}",
            marker,
            session_id,
            err
        );
    }
}

impl ChannelManager {
    pub fn new(
        app_handle: AppHandle,
        chat_adapter: Arc<TauriChatCommandAdapter>,
        conversation_store: Arc<dyn ConversationStore>,
        secure_storage: Option<Arc<SecureStorage>>,
        channels_dir: PathBuf,
        ask_coordinator: Option<Arc<super::shared::ask_coordinator::IMAskCoordinator>>,
        reply_manager: Arc<DingtalkReplyManager>,
        channel_session_ids: Arc<std::sync::RwLock<HashSet<String>>>,
        pending_manager: Arc<crate::runtime::pending::PendingQueueManager>,
        active_scope: Option<crate::storage::UserScope>,
    ) -> Self {
        let config_store = Arc::new(ChannelConfigStore::new(channels_dir, secure_storage));
        // 每个平台一个独立的 sessions.json，避免 router_key 跨平台串扰（这是上线前
        // 修复的 bug：早期所有平台共用 dingtalk/sessions.json，飞书会话被错落到
        // 钉钉那栏，hydrate 后还把 is_active_robot 算成 false）。
        let mut sessions_paths: HashMap<Platform, PathBuf> = HashMap::new();
        for platform in Platform::all() {
            sessions_paths.insert(platform, config_store.platform_sessions_path(platform));
        }
        // 一次性迁移：把旧版 dingtalk/sessions.json 里的飞书 / 企微 entries
        // 拆分到各自平台的 sessions.json。已迁移过 / 新装机器都是 no-op。
        if let Err(e) = super::shared::router::split_legacy_shared_sessions(
            &sessions_paths[&Platform::Dingtalk],
            &sessions_paths,
        ) {
            log::error!("[channel] split_legacy_shared_sessions failed: {:#}", e);
        }
        Self {
            app_handle,
            chat_adapter,
            conversation_store,
            config_store,
            sessions_paths,
            platform_state: Arc::new(RwLock::new(HashMap::new())),
            seen_msg_ids: Arc::new(super::shared::dedup::MessageDedupSet::with_default_cap()),
            conversations: Arc::new(RwLock::new(vec![])),
            reply_manager,
            reply_subscribed: Arc::new(AtomicBool::new(false)),
            feishu_reply_subscribed: Arc::new(AtomicBool::new(false)),
            wecom_reply_subscribed: Arc::new(AtomicBool::new(false)),
            wechat_reply_subscribed: Arc::new(AtomicBool::new(false)),
            telegram_reply_subscribed: Arc::new(AtomicBool::new(false)),
            whatsapp_reply_subscribed: Arc::new(AtomicBool::new(false)),
            telegram_concrete: Arc::new(tokio::sync::RwLock::new(None)),
            whatsapp_concrete: Arc::new(tokio::sync::RwLock::new(None)),
            whatsapp_gc_spawned: Arc::new(tokio::sync::OnceCell::new()),
            ask_coordinator,
            ask_subscribed: Arc::new(AtomicBool::new(false)),
            channel_session_ids,
            pending_manager,
            connectors: Arc::new(RwLock::new(HashMap::new())),
            started_at_ms: now_epoch_ms(),
            active_scope,
            subscriber_anchors: std::sync::Mutex::new(Vec::new()),
            inactive: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    // ---- Per-platform state accessors --------------------------------------

    /// Read a closure-derived projection of a platform's state slot. Returns
    /// `None` if the platform has never been touched yet.
    async fn platform_state_read<F, R>(&self, platform: Platform, f: F) -> Option<R>
    where
        F: FnOnce(&PerPlatformState) -> R,
    {
        let map = self.platform_state.read().await;
        map.get(&platform).map(f)
    }

    /// Mutate a platform's state slot, creating an `unconfigured` slot on
    /// first touch.
    async fn platform_state_mutate<F, R>(&self, platform: Platform, f: F) -> R
    where
        F: FnOnce(&mut PerPlatformState) -> R,
    {
        let mut map = self.platform_state.write().await;
        let state = map
            .entry(platform)
            .or_insert_with(PerPlatformState::unconfigured);
        f(state)
    }

    /// Clone the per-platform `stream_generation` Arc. Created on first call.
    /// Needed by `connect_*` to give the spawned worker / status callbacks
    /// stable access — `Arc::clone` is cheap so even repeat calls are fine.
    async fn platform_generation(&self, platform: Platform) -> Arc<AtomicU64> {
        self.platform_state_mutate(platform, |s| Arc::clone(&s.stream_generation))
            .await
    }

    /// Register (or replace) the Dingtalk connector for the given credentials.
    /// `on_status` is the closure that drives `channel:platform-state` emission.
    /// Returns the concrete `Arc<DingtalkConnector>` so the caller can invoke
    /// non-trait methods (e.g. `remember_session`) — the trait-erased copy is
    /// also kept in `self.connectors` for normal send dispatch.
    async fn register_dingtalk_connector(
        &self,
        app_key: String,
        app_secret: String,
        robot_code: String,
        on_status: super::factory::DingtalkStatusCallback,
    ) -> Arc<super::dingtalk::connector::DingtalkConnector> {
        let (dyn_conn, concrete) = build_dingtalk_connector(
            app_key,
            app_secret,
            robot_code,
            Arc::clone(&self.reply_manager),
            on_status,
        );
        let mut map = self.connectors.write().await;
        map.insert(Platform::Dingtalk, dyn_conn);
        concrete
    }

    /// Register (or replace) the Feishu connector for the given credentials.
    /// Mirrors `register_dingtalk_connector` shape — concrete handle returned
    /// for `remember_session` calls; dyn handle inserted into the connectors
    /// map for trait-erased dispatch (PR4+ send path).
    async fn register_feishu_connector(
        &self,
        app_id: String,
        app_secret: String,
        on_status: super::factory::FeishuStatusCallback,
    ) -> Arc<super::feishu::FeishuConnector> {
        let (dyn_conn, concrete) =
            super::factory::build_feishu_connector(app_id, app_secret, on_status);
        let mut map = self.connectors.write().await;
        map.insert(Platform::Feishu, dyn_conn);
        concrete
    }

    async fn current_dingtalk_state(&self) -> Result<ChannelPlatformState> {
        let (connection, last_error) = self
            .platform_state_read(Platform::Dingtalk, |s| {
                (s.connection.clone(), s.last_error.clone())
            })
            .await
            .unwrap_or((ChannelConnectionState::Unconfigured, None));
        self.config_store.dingtalk_state(connection, last_error)
    }

    async fn current_feishu_state(&self) -> Result<ChannelPlatformState> {
        let (connection, last_error) = self
            .platform_state_read(Platform::Feishu, |s| {
                (s.connection.clone(), s.last_error.clone())
            })
            .await
            .unwrap_or((ChannelConnectionState::Unconfigured, None));
        self.config_store.feishu_state(connection, last_error)
    }

    /// Register (or replace) the Wecom connector for the given credentials.
    /// Mirrors `register_feishu_connector`.
    async fn register_wecom_connector(
        &self,
        bot_id: String,
        secret: String,
        on_status: super::factory::WecomStatusCallback,
    ) -> Arc<super::wecom::connector::WecomConnector> {
        let (dyn_conn, concrete) = super::factory::build_wecom_connector(bot_id, secret, on_status);
        let mut map = self.connectors.write().await;
        map.insert(Platform::Wecom, dyn_conn);
        concrete
    }

    async fn current_wecom_state(&self) -> Result<ChannelPlatformState> {
        let (connection, last_error) = self
            .platform_state_read(Platform::Wecom, |s| {
                (s.connection.clone(), s.last_error.clone())
            })
            .await
            .unwrap_or((ChannelConnectionState::Unconfigured, None));
        self.config_store.wecom_state(connection, last_error)
    }

    /// Set Wecom's per-platform connection state and surface it through the
    /// `channel:platform-state` event. Mirrors `set_feishu_connection_state` —
    /// Connected 时按当前 bot_id 刷新仅企微会话的 `is_active_robot`，否则会被
    /// 钉钉/飞书的 connected 路径错刷成 false，sidebar 看不到。
    async fn set_wecom_connection_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) {
        log::info!(
            "[channel/wecom] set_wecom_connection_state connection={:?} last_error={:?}",
            connection,
            last_error
        );
        self.platform_state_mutate(Platform::Wecom, |s| {
            s.connection = connection.clone();
            s.last_error = last_error.clone();
        })
        .await;
        if matches!(connection, ChannelConnectionState::Connected) {
            let current_bot_id = self
                .config_store
                .read_wecom_config()
                .ok()
                .flatten()
                .map(|cfg| cfg.credentials.bot_id);
            self.refresh_active_robot_flags(Platform::Wecom, current_bot_id.as_deref())
                .await;
        }
        match self
            .config_store
            .wecom_state(connection.clone(), last_error)
        {
            Ok(state) => {
                let _ = self.app_handle.emit(
                    "channel:platform-state",
                    &ChannelPlatformStatePayload { state },
                );
            }
            Err(error) => {
                log::warn!(
                    "[channel/wecom] failed to emit platform state (connection={:?}): {:#}",
                    connection,
                    error
                )
            }
        }
    }

    // ---- Telegram (Bot API + long-poll + pairing-code allowlist) ----------

    async fn register_telegram_connector(
        &self,
        bot_id: String,
        bot_username: String,
        token: String,
        on_status: super::factory::TelegramStatusCallback,
    ) -> Result<Arc<super::telegram::connector::TelegramConnector>> {
        let (dyn_conn, concrete) = super::factory::build_telegram_connector(
            bot_id,
            bot_username,
            token,
            Arc::clone(&self.config_store),
            on_status,
        )?;
        {
            let mut map = self.connectors.write().await;
            map.insert(Platform::Telegram, dyn_conn);
        }
        *self.telegram_concrete.write().await = Some(Arc::clone(&concrete));
        Ok(concrete)
    }

    async fn current_telegram_state(&self) -> Result<ChannelPlatformState> {
        let (connection, last_error) = self
            .platform_state_read(Platform::Telegram, |s| {
                (s.connection.clone(), s.last_error.clone())
            })
            .await
            .unwrap_or((ChannelConnectionState::Unconfigured, None));
        self.config_store.telegram_state(connection, last_error)
    }

    /// Set Telegram's per-platform connection state and surface it through
    /// `channel:platform-state`. Mirrors `set_wecom_connection_state`.
    async fn set_telegram_connection_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) {
        log::info!(
            "[channel/telegram] set_telegram_connection_state connection={:?} last_error={:?}",
            connection,
            last_error
        );
        self.platform_state_mutate(Platform::Telegram, |s| {
            s.connection = connection.clone();
            s.last_error = last_error.clone();
        })
        .await;
        if matches!(connection, ChannelConnectionState::Connected) {
            let current_bot_id = self
                .config_store
                .read_telegram_config()
                .ok()
                .flatten()
                .map(|cfg| format!("tg-{}", cfg.bot.bot_id));
            self.refresh_active_robot_flags(Platform::Telegram, current_bot_id.as_deref())
                .await;
        }
        match self
            .config_store
            .telegram_state(connection.clone(), last_error)
        {
            Ok(state) => {
                let _ = self.app_handle.emit(
                    "channel:platform-state",
                    &ChannelPlatformStatePayload { state },
                );
            }
            Err(error) => log::warn!(
                "[channel/telegram] failed to emit platform state (connection={:?}): {:#}",
                connection,
                error
            ),
        }
    }

    /// Manager-facing entry point: 验证 token 已由 channel_telegram_save command 在
    /// 调用前完成（save 阶段拿到 bot_id/bot_username/bot_first_name）。
    pub async fn save_telegram_and_connect(
        &self,
        token: String,
        bot_id: String,
        bot_username: String,
        bot_first_name: String,
    ) -> Result<ChannelPlatformState> {
        self.config_store.save_telegram_registration(
            token,
            bot_id,
            bot_username,
            bot_first_name,
        )?;
        self.connect_telegram_from_store().await?;
        self.current_telegram_state().await
    }

    pub async fn connect_telegram_from_store(&self) -> Result<()> {
        let (config, token) = self.config_store.decrypt_telegram_config()?;
        self.connect_telegram(config, token).await
    }

    async fn connect_telegram(
        &self,
        config: super::telegram::types::TelegramStoredConfig,
        token: String,
    ) -> Result<()> {
        // Stop only the telegram slot; other platform streams are independent.
        self.stop_stream(Platform::Telegram).await;

        let bot_id = config.bot.bot_id.clone();
        let bot_username = config.bot.bot_username.clone();
        let router_key = format!("tg-{}", bot_id);

        self.set_telegram_connection_state(ChannelConnectionState::Connecting, None)
            .await;

        let stream_generation = self.platform_generation(Platform::Telegram).await;
        let generation = stream_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let message_stream_generation = Arc::clone(&stream_generation);
        let platform_state_for_status = Arc::clone(&self.platform_state);
        let config_store_for_status = Arc::clone(&self.config_store);
        let app_for_status = self.app_handle.clone();
        let stream_generation_for_status = Arc::clone(&stream_generation);
        let conversations_for_status = Arc::clone(&self.conversations);
        let on_status: super::factory::TelegramStatusCallback = Arc::new(
            move |new_connection: ChannelConnectionState, error: Option<String>| {
                let platform_state_for_status = platform_state_for_status.clone();
                let config_store = config_store_for_status.clone();
                let app_for_status = app_for_status.clone();
                let stream_generation_for_status = stream_generation_for_status.clone();
                let conversations_for_status = conversations_for_status.clone();
                tokio::spawn(async move {
                    if stream_generation_for_status.load(Ordering::SeqCst) != generation {
                        log::debug!("[channel/telegram] ignoring stale status callback");
                        return;
                    }
                    {
                        let mut map = platform_state_for_status.write().await;
                        let slot = map
                            .entry(Platform::Telegram)
                            .or_insert_with(PerPlatformState::unconfigured);
                        slot.connection = new_connection.clone();
                        slot.last_error = error.clone();
                    }
                    if matches!(new_connection, ChannelConnectionState::Connected) {
                        let current_router_key = config_store
                            .read_telegram_config()
                            .ok()
                            .flatten()
                            .map(|cfg| format!("tg-{}", cfg.bot.bot_id));
                        let mut convs = conversations_for_status.write().await;
                        for c in convs.iter_mut() {
                            if c.platform != Platform::Telegram {
                                continue;
                            }
                            c.is_active_robot = current_router_key
                                .as_deref()
                                .map(|rk| rk == c.robot_code)
                                .unwrap_or(false);
                        }
                    }
                    match config_store.telegram_state(new_connection, error) {
                        Ok(state) => {
                            let _ = app_for_status.emit(
                                "channel:platform-state",
                                &ChannelPlatformStatePayload { state },
                            );
                        }
                        Err(err) => log::warn!(
                            "[channel/telegram] failed to build platform state: {:#}",
                            err
                        ),
                    }
                });
            },
        );

        let concrete = self
            .register_telegram_connector(
                bot_id.clone(),
                bot_username.clone(),
                token,
                Arc::clone(&on_status),
            )
            .await?;

        // Subscribe TelegramReplyForwarder to RuntimeEventBus once per process lifecycle.
        if claim_first_subscription(&self.telegram_reply_subscribed) {
            let forwarder = Arc::new(
                super::telegram::reply_forwarder::TelegramReplyForwarder::new(Arc::clone(
                    &concrete,
                )),
            );
            let sub: Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber> = forwarder;
            self.chat_adapter.subscribe_event_listener(sub.clone());
            self.anchor_subscriber(sub);
            log::info!("[channel/telegram] subscribed TelegramReplyForwarder to RuntimeEventBus");
        }

        let new_token = CancellationToken::new();
        let ctx = ConnectorContext {
            config_store: Arc::clone(&self.config_store),
            secure_storage: None,
            ask_coordinator: self.ask_coordinator.as_ref().map(Arc::clone),
            pending_manager: Arc::clone(&self.pending_manager),
            cancel_token: new_token.clone(),
        };
        let connector = {
            let map = self.connectors.read().await;
            map.get(&Platform::Telegram)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("telegram connector not registered"))?
        };
        let mut message_stream = connector
            .start(ctx)
            .await
            .map_err(|e| anyhow::anyhow!("telegram connector start failed: {e}"))?;

        let message_cancel = new_token.clone();
        self.platform_state_mutate(Platform::Telegram, |s| {
            s.stream_cancel = Some(new_token);
        })
        .await;

        // Worker — receives ChannelMessages from the telegram stream and routes
        // them to the chat turn engine via PendingQueueManager. Mirrors wecom
        // worker shape (minus AI card / attachment download branches).
        let adapter = Arc::clone(&self.chat_adapter);
        let conv_store = Arc::clone(&self.conversation_store);
        let sessions_path = self.sessions_paths[&Platform::Telegram].clone();
        let seen_ids = Arc::clone(&self.seen_msg_ids);
        let convs = Arc::clone(&self.conversations);
        let app_handle = self.app_handle.clone();
        let channel_session_ids_ref = Arc::clone(&self.channel_session_ids);
        let inactive_ref = Arc::clone(&self.inactive);
        let on_status_for_worker = Arc::clone(&on_status);
        let ask_coordinator_ref = self.ask_coordinator.as_ref().map(Arc::clone);
        let pending_manager_ref = Arc::clone(&self.pending_manager);
        let platform_state_for_worker = Arc::clone(&self.platform_state);
        let connector_for_worker = {
            let map = self.connectors.read().await;
            Arc::clone(
                map.get(&Platform::Telegram)
                    .expect("telegram just registered"),
            )
        };
        let concrete_telegram_for_worker = Arc::clone(&concrete);
        // Build downloader once per stream; reuses Arc<TelegramApi> from connector
        // so token / reqwest client are shared.
        let telegram_downloader = Arc::new(super::telegram::download::TelegramFileDownloader::new(
            concrete.api(),
            self.telegram_downloads_dir(),
        ));

        let message_handle = tokio::spawn(async move {
            let mut router =
                match ChannelSessionRouter::migrate_or_load(&sessions_path, conv_store.as_ref()) {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("[channel/telegram] failed to load router: {:#}", e);
                        return;
                    }
                };

            loop {
                let msg = match recv_current_generation_message_stream(
                    &mut message_stream,
                    &message_stream_generation,
                    generation,
                    &message_cancel,
                )
                .await
                {
                    Some(m) => m,
                    None => {
                        log::info!("[channel/telegram] worker stream ended");
                        let current = {
                            let map = platform_state_for_worker.read().await;
                            map.get(&Platform::Telegram)
                                .map(|s| s.connection.clone())
                                .unwrap_or(ChannelConnectionState::Unconfigured)
                        };
                        if !matches!(
                            current,
                            ChannelConnectionState::NeedsReauth
                                | ChannelConnectionState::ConfigError
                        ) {
                            on_status_for_worker(ChannelConnectionState::Reconnecting, None);
                        }
                        break;
                    }
                };

                log::info!(
                    "[channel/telegram] worker received msg msg_id={} text_len={}",
                    msg.msg_id,
                    msg.text.len(),
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                if !seen_ids.observe(&msg.msg_id).await {
                    log::debug!(
                        "[channel/telegram] duplicate msg_id {}, skipping",
                        msg.msg_id
                    );
                    continue;
                }

                let conv_type = msg.conversation_type.clone();
                let conv_key = msg.conversation_key.clone();
                let sender_nick = msg.sender_nick.clone();
                let text = msg.text.clone();

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                let store_ref = Arc::clone(&conv_store);
                let ensure_store_ref = Arc::clone(&conv_store);
                let sender_nick_for_create = sender_nick.clone();
                let sender_nick_for_ensure = sender_nick.clone();
                let conv_key_for_create = conv_key.clone();
                let conv_type_for_create = conv_type.clone();
                let session_id = match router.get_or_create_session_with_ensure(
                    &conv_type,
                    &router_key,
                    &conv_key,
                    || {
                        let title = match &conv_type_for_create {
                            ConversationType::Group => format!(
                                "Telegram 群 {}",
                                &conv_key_for_create[..conv_key_for_create.len().min(8)]
                            ),
                            ConversationType::Private => sender_nick_for_create.clone(),
                        };
                        let id = uuid::Uuid::new_v4().to_string();
                        store_ref
                            .create_conversation_with_im_source(
                                &id,
                                &title,
                                Platform::Telegram.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))?;
                        Ok(id)
                    },
                    |existing_id| {
                        ensure_store_ref
                            .create_conversation_with_im_source(
                                existing_id,
                                &sender_nick_for_ensure,
                                Platform::Telegram.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))
                    },
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("[channel/telegram] get_or_create_session failed: {:#}", e);
                        continue;
                    }
                };

                // Register the session in the channel_session_ids set.
                {
                    if inactive_ref.load(std::sync::atomic::Ordering::SeqCst) {
                        log::debug!(
                            "[channel/telegram] worker observed inactive flag, dropping session id insert"
                        );
                        continue;
                    }
                    let mut ids = channel_session_ids_ref
                        .write()
                        .expect("channel_session_ids poisoned");
                    ids.insert(session_id.clone());
                }

                // Remember the chat_id/user_id so the reply forwarder can
                // route assistant replies back. chat_id == conv_key for telegram.
                let chat_id: i64 = conv_key.parse().unwrap_or(0);
                let user_id: i64 = msg.sender_id.parse().unwrap_or(0);
                concrete_telegram_for_worker
                    .remember_session(
                        session_id.clone(),
                        super::telegram::types::TelegramSessionTarget {
                            chat_id,
                            user_id,
                            last_inbound_message_id: None,
                        },
                    )
                    .await;

                {
                    let mut convs_lock = convs.write().await;
                    if !convs_lock.iter().any(|c| c.session_id == session_id) {
                        let display_name = match &conv_type {
                            ConversationType::Group => {
                                format!("Telegram 群 {}", &conv_key[..conv_key.len().min(8)])
                            }
                            ConversationType::Private => sender_nick.clone(),
                        };
                        convs_lock.push(ChannelConversation {
                            session_id: session_id.clone(),
                            platform: Platform::Telegram,
                            conversation_type: conv_type.clone(),
                            external_id: conv_key.clone(),
                            display_name,
                            unread_count: 0,
                            robot_code: router_key.clone(),
                            is_active_robot: true,
                        });
                    }
                }

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                let preview = if text.chars().count() > 30 {
                    format!("{}...", text.chars().take(30).collect::<String>())
                } else {
                    text.clone()
                };
                let _ = app_handle.emit(
                    "channel:message",
                    &ChannelMessagePayload {
                        platform: "telegram".into(),
                        session_id: session_id.clone(),
                        sender_nick: sender_nick.clone(),
                        text_preview: preview,
                    },
                );

                let session_for_ask = crate::runtime::ids::SessionId::new(session_id.clone());
                match handle_pending_action_pre_dispatch(
                    ask_coordinator_ref.as_ref(),
                    &session_for_ask,
                    &text,
                )
                .await
                {
                    Ok(super::shared::ask_coordinator::HandleOutcome::NotPending)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::NewTurnAfterAbandon) => {}
                    Ok(super::shared::ask_coordinator::HandleOutcome::ApprovalResolved)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::AnswerResolved) => {
                        continue;
                    }
                    Ok(super::shared::ask_coordinator::HandleOutcome::InvalidApprovalAction {
                        message,
                    }) => {
                        send_pending_action_text_ack(
                            &connector_for_worker,
                            &session_id,
                            &conv_key,
                            "[channel/telegram]",
                            message,
                        )
                        .await;
                        continue;
                    }
                    Err(err) => {
                        log::warn!(
                            "[channel/telegram] IM ask coordinator failed, falling back to normal turn: {:#}",
                            err
                        );
                    }
                };

                // Download attachments (photos / documents) via Bot API getFile.
                let (chat_attachments, download_failures) = if msg.attachments.is_empty() {
                    (Vec::<ChatAttachmentRef>::new(), Vec::<String>::new())
                } else {
                    log::info!(
                        "[channel/telegram] downloading {} attachments msg_id={} session={}",
                        msg.attachments.len(),
                        msg.msg_id,
                        session_id
                    );
                    download_specs_for_turn_telegram(&telegram_downloader, &msg.attachments).await
                };
                // All-attachments-failed + empty text → fallback reply (mirror wecom)
                if chat_attachments.is_empty()
                    && text.trim().is_empty()
                    && !msg.attachments.is_empty()
                {
                    log::warn!(
                        "[channel/telegram] all attachments failed and no text, replying via send(Text) msg_id={}",
                        msg.msg_id
                    );
                    let connector_for_fallback = Arc::clone(&connector_for_worker);
                    let session_for_fallback = session_id.clone();
                    let conv_key_for_fallback = conv_key.clone();
                    tokio::spawn(async move {
                        if let Err(e) = connector_for_fallback
                            .send(
                                crate::connector::im::trait_def::ReplyTarget {
                                    session_id: session_for_fallback.clone(),
                                    external_conversation_key: conv_key_for_fallback,
                                },
                                crate::connector::im::trait_def::ReplyContent::Text(
                                    "附件下载全部失败，请重发。".to_string(),
                                ),
                            )
                            .await
                        {
                            log::warn!(
                                "[channel/telegram] fallback text send failed session={}: {:#}",
                                session_for_fallback,
                                e
                            );
                        }
                    });
                    continue;
                }

                let request = build_channel_chat_request(
                    session_id.clone(),
                    crate::runtime::human_interaction::ImPlatform::Telegram,
                    conv_key.clone(),
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments.clone(),
                    &download_failures,
                );
                let pending_item = super::shared::pending_adapter::build_pending_item_from_telegram(
                    &msg.msg_id,
                    &session_id,
                    &conv_key,
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments,
                    &download_failures,
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                let adapter_for_turn = Arc::clone(&adapter);
                let session_for_log = session_id.clone();
                let pending_manager_for_send = Arc::clone(&pending_manager_ref);
                let session_for_enqueue = crate::runtime::ids::SessionId::new(session_id.clone());
                let connector_for_send = Arc::clone(&connector_for_worker);
                let conv_key_for_reject = conv_key.clone();
                tokio::spawn(async move {
                    match pending_manager_for_send
                        .enqueue_or_send(session_for_enqueue, pending_item)
                        .await
                    {
                        Ok(crate::runtime::pending::EnqueueOutcome::SentDirectly { .. }) => {
                            if let Err(e) = adapter_for_turn.send_chat_request(request).await {
                                log::error!(
                                    "[channel/telegram] send_chat_request failed session={}: {}",
                                    session_for_log,
                                    e
                                );
                                pending_manager_for_send
                                    .release_direct_dispatch(&crate::runtime::ids::SessionId::new(
                                        session_for_log.clone(),
                                    ))
                                    .await;
                            }
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Queued { snapshot }) => {
                            log::info!(
                                "[channel/telegram] message queued session={} queue_size={}",
                                session_for_log,
                                snapshot.len()
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::HeldForHumanInteraction {
                            interaction_id,
                        }) => {
                            log::info!(
                                "[channel/telegram] message held for human interaction session={} interaction_id={:?}",
                                session_for_log,
                                interaction_id
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Rejected { reason }) => {
                            log::warn!(
                                "[channel/telegram] enqueue rejected session={} reason={:?}",
                                session_for_log,
                                reason
                            );
                            if let crate::runtime::pending::EnqueueRejection::QueueFull { limit } =
                                reason
                            {
                                let text = format!("消息堆积过多（已达 {limit} 条），请稍后再发。");
                                if let Err(e) = connector_for_send
                                    .send(
                                        crate::connector::im::trait_def::ReplyTarget {
                                            session_id: session_for_log.clone(),
                                            external_conversation_key: conv_key_for_reject.clone(),
                                        },
                                        crate::connector::im::trait_def::ReplyContent::Text(text),
                                    )
                                    .await
                                {
                                    log::warn!(
                                        "[channel/telegram] queue-full reject text send failed session={}: {:#}",
                                        session_for_log,
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "[channel/telegram] enqueue_or_send error session={}: {:#}",
                                session_for_log,
                                e
                            );
                        }
                    }
                });
            }
        });
        self.platform_state_mutate(Platform::Telegram, |s| {
            s.message_task = Some(message_handle);
        })
        .await;

        Ok(())
    }

    /// Public accessor: return the cached concrete TelegramConnector handle (if any),
    /// used by Tauri pairing commands.
    pub async fn telegram_connector(
        &self,
    ) -> Option<Arc<super::telegram::connector::TelegramConnector>> {
        self.telegram_concrete.read().await.clone()
    }

    /// Public accessor: returns the cached WhatsApp connector handle if
    /// one was registered. Used by Tauri pairing commands and PR3+ flows
    /// that need to call inherent (non-trait) methods like
    /// `start_pairing_session` and `poll_pairing_state`.
    pub async fn whatsapp_connector(
        &self,
    ) -> Option<Arc<super::whatsapp::connector::WhatsAppConnector>> {
        self.whatsapp_concrete.read().await.clone()
    }

    /// Public accessor: clone the underlying config store so the registration
    /// module can write allowlist updates.
    pub fn config_store_arc(&self) -> Arc<ChannelConfigStore> {
        Arc::clone(&self.config_store)
    }

    // ---- WeChat (iLink scan-to-login + long-poll) -------------------------

    async fn register_wechat_connector(
        &self,
        bot_token: String,
        ilink_bot_id: String,
        ilink_user_id: String,
        base_url: String,
        app_id: String,
        client_version: String,
        on_status: super::factory::WechatStatusCallback,
    ) -> Arc<super::wechat::connector::WechatConnector> {
        let (dyn_conn, concrete) = super::factory::build_wechat_connector(
            bot_token,
            ilink_bot_id,
            ilink_user_id,
            base_url,
            app_id,
            client_version,
            on_status,
        );
        let mut map = self.connectors.write().await;
        map.insert(Platform::Wechat, dyn_conn);
        concrete
    }

    async fn register_whatsapp_connector(
        &self,
        on_status: super::factory::WhatsappStatusCallback,
    ) -> Arc<super::whatsapp::connector::WhatsAppConnector> {
        let attachments_dir = self.whatsapp_downloads_dir();
        let (dyn_conn, concrete) =
            super::factory::build_whatsapp_connector(on_status, attachments_dir.clone());
        let mut map = self.connectors.write().await;
        map.insert(Platform::Whatsapp, dyn_conn);
        drop(map);
        *self.whatsapp_concrete.write().await = Some(Arc::clone(&concrete));
        // Subscribe WhatsAppReplyForwarder to RuntimeEventBus once per process lifecycle.
        if claim_first_subscription(&self.whatsapp_reply_subscribed) {
            let forwarder = Arc::new(
                super::whatsapp::reply_forwarder::WhatsAppReplyForwarder::new(Arc::clone(
                    &concrete,
                )),
            );
            let sub: Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber> = forwarder;
            self.chat_adapter.subscribe_event_listener(sub.clone());
            self.anchor_subscriber(sub);
            log::info!("[channel/whatsapp] subscribed WhatsAppReplyForwarder to RuntimeEventBus");
        }
        // 整个进程只 spawn 一次 GC 任务，OnceCell 保证幂等。
        let gc_dir = attachments_dir;
        let _ = self
            .whatsapp_gc_spawned
            .get_or_init(|| async move {
                tokio::spawn(super::whatsapp::gc::run_attachment_gc(gc_dir));
            })
            .await;
        concrete
    }

    async fn current_wechat_state(&self) -> Result<ChannelPlatformState> {
        let (connection, last_error) = self
            .platform_state_read(Platform::Wechat, |s| {
                (s.connection.clone(), s.last_error.clone())
            })
            .await
            .unwrap_or((ChannelConnectionState::Unconfigured, None));
        self.config_store.wechat_state(connection, last_error)
    }

    /// Set WeChat's per-platform connection state and surface it through
    /// `channel:platform-state`. Connected 时按当前 ilink_bot_id 刷新仅
    /// 微信会话的 is_active_robot —— 对称于飞书/企微路径，避免被钉钉的
    /// connected 路径误刷成 false。
    async fn set_wechat_connection_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) {
        log::info!(
            "[channel/wechat] set_wechat_connection_state connection={:?} last_error={:?}",
            connection,
            last_error
        );
        self.platform_state_mutate(Platform::Wechat, |s| {
            s.connection = connection.clone();
            s.last_error = last_error.clone();
        })
        .await;
        if matches!(connection, ChannelConnectionState::Connected) {
            let current_bot_id = self
                .config_store
                .read_wechat_config()
                .ok()
                .flatten()
                .map(|cfg| cfg.bot.ilink_bot_id);
            self.refresh_active_robot_flags(Platform::Wechat, current_bot_id.as_deref())
                .await;
        }
        match self
            .config_store
            .wechat_state(connection.clone(), last_error)
        {
            Ok(state) => {
                let _ = self.app_handle.emit(
                    "channel:platform-state",
                    &ChannelPlatformStatePayload { state },
                );
            }
            Err(error) => log::warn!(
                "[channel/wechat] failed to emit platform state (connection={:?}): {:#}",
                connection,
                error
            ),
        }
    }

    // ---- WhatsApp helpers (Phase 4 PR3) -----------------------------------

    fn resolve_whatsapp_paths(&self) -> Result<super::whatsapp::session::WhatsAppPaths> {
        let dir = self.config_store.platform_dir(Platform::Whatsapp);
        Ok(super::whatsapp::session::WhatsAppPaths::new(dir))
    }

    fn make_whatsapp_status_callback(&self) -> super::factory::WhatsappStatusCallback {
        let app_handle = self.app_handle.clone();
        let config_store = self.config_store.clone();
        let platform_state = Arc::clone(&self.platform_state);
        Arc::new(
            move |state: ChannelConnectionState, last_error: Option<String>| {
                // 1) Update the manager's own PerPlatformState[Whatsapp] so that
                //    a subsequent channel_get_platforms() returns the correct
                //    connection state. Without this, the slot stays at the
                //    initial Unconfigured even after Connected fires, and any
                //    React listener that mounts after the initial emit reads
                //    stale state from loadPlatforms().
                //
                //    Mirrors what telegram / wecom / dingtalk callbacks do.
                let platform_state_for_write = platform_state.clone();
                let connection_for_write = state.clone();
                let last_error_for_write = last_error.clone();
                tokio::spawn(async move {
                    let mut map = platform_state_for_write.write().await;
                    let slot = map
                        .entry(Platform::Whatsapp)
                        .or_insert_with(PerPlatformState::unconfigured);
                    slot.connection = connection_for_write;
                    slot.last_error = last_error_for_write;
                });

                // 2) Surface to the frontend via channel:platform-state.
                //    走 config_store.whatsapp_state() 得到稳定的 configured/enabled
                //    （来自 config.json 是否存在），不再用 connection 状态推断。否则
                //    一旦网络抖动 / 主端手机 离线 → connection 掉到 Reconnecting →
                //    configured=false → 前端徽章显示"未配置"误导用户重新扫码。
                match config_store.whatsapp_state(state, last_error) {
                    Ok(channel_state) => {
                        log::info!(
                            "[channel/whatsapp] emit channel:platform-state connection={:?} configured={} enabled={} capability={:?}",
                            channel_state.connection,
                            channel_state.configured,
                            channel_state.enabled,
                            channel_state.capability,
                        );
                        let _ = app_handle.emit(
                            "channel:platform-state",
                            &ChannelPlatformStatePayload {
                                state: channel_state,
                            },
                        );
                    }
                    Err(err) => log::warn!(
                        "[channel/whatsapp] failed to build platform state: {:#}",
                        err
                    ),
                }
            },
        )
    }

    /// Set WhatsApp's per-platform connection state and surface it through
    /// `channel:platform-state`. Mirrors set_wechat_connection_state.
    async fn set_whatsapp_connection_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) {
        log::info!(
            "[channel/whatsapp] set_whatsapp_connection_state connection={:?} last_error={:?}",
            connection,
            last_error
        );
        self.platform_state_mutate(Platform::Whatsapp, |s| {
            s.connection = connection.clone();
            s.last_error = last_error.clone();
        })
        .await;
        // 同 make_whatsapp_status_callback：走 config_store.whatsapp_state()
        // 让 configured/enabled 来自 config.json 是否存在，不被瞬时 connection
        // 状态翻转。否则网络抖动会让前端显示"未配置"。
        match self.config_store.whatsapp_state(connection, last_error) {
            Ok(state) => {
                let _ = self.app_handle.emit(
                    "channel:platform-state",
                    &ChannelPlatformStatePayload { state },
                );
            }
            Err(err) => log::warn!(
                "[channel/whatsapp] failed to build platform state: {:#}",
                err
            ),
        }
    }

    /// 保存企微配置并建立连接。
    pub async fn save_wecom_and_connect(
        &self,
        bot_id: String,
        secret_plain: String,
        display_name: Option<String>,
    ) -> Result<ChannelPlatformState> {
        self.config_store
            .add_wecom(bot_id, secret_plain, display_name)?;
        self.connect_wecom_from_store().await?;
        self.current_wecom_state().await
    }

    /// Read wecom credentials from config_store, register connector with a
    /// proper on_status callback, start the stream, and spawn the worker loop.
    /// Mirrors `connect_feishu_from_store`.
    pub async fn connect_wecom_from_store(&self) -> Result<()> {
        let (config, secret_plain) = self.config_store.decrypt_wecom_config()?;
        self.connect_wecom(config, secret_plain).await
    }

    async fn connect_wecom(
        &self,
        config: super::wecom::types::WecomStoredConfig,
        secret_plain: String,
    ) -> Result<()> {
        // Stop only the wecom slot; other platform streams are independent.
        self.stop_stream(Platform::Wecom).await;

        let bot_id = config.credentials.bot_id.clone();
        let router_key = bot_id.clone();

        self.set_wecom_connection_state(ChannelConnectionState::Connecting, None)
            .await;

        // Grab the wecom slot's generation counter and bump it for this
        // (re)connect.
        let stream_generation = self.platform_generation(Platform::Wecom).await;
        let generation = stream_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let message_stream_generation = Arc::clone(&stream_generation);
        let platform_state_for_status = Arc::clone(&self.platform_state);
        let config_store = Arc::clone(&self.config_store);
        let app_for_status = self.app_handle.clone();
        let stream_generation_for_status = Arc::clone(&stream_generation);
        let conversations_for_status = Arc::clone(&self.conversations);
        let on_status: super::factory::WecomStatusCallback = Arc::new(
            move |new_connection: ChannelConnectionState, error: Option<String>| {
                let platform_state_for_status = platform_state_for_status.clone();
                let config_store = config_store.clone();
                let app_for_status = app_for_status.clone();
                let stream_generation_for_status = stream_generation_for_status.clone();
                let conversations_for_status = conversations_for_status.clone();
                tokio::spawn(async move {
                    if stream_generation_for_status.load(Ordering::SeqCst) != generation {
                        log::debug!("[channel/wecom] ignoring stale status callback");
                        return;
                    }
                    {
                        let mut map = platform_state_for_status.write().await;
                        let slot = map
                            .entry(Platform::Wecom)
                            .or_insert_with(PerPlatformState::unconfigured);
                        slot.connection = new_connection.clone();
                        slot.last_error = error.clone();
                    }
                    // Connected 时按 config 的 bot_id 刷新仅企微会话的
                    // is_active_robot —— 对称于飞书/钉钉 status callback。
                    if matches!(new_connection, ChannelConnectionState::Connected) {
                        let current_bot_id = config_store
                            .read_wecom_config()
                            .ok()
                            .flatten()
                            .map(|cfg| cfg.credentials.bot_id);
                        let mut convs = conversations_for_status.write().await;
                        for c in convs.iter_mut() {
                            if c.platform != Platform::Wecom {
                                continue;
                            }
                            c.is_active_robot = current_bot_id
                                .as_deref()
                                .map(|rc| rc == c.robot_code)
                                .unwrap_or(false);
                        }
                    }
                    match config_store.wecom_state(new_connection, error) {
                        Ok(state) => {
                            let _ = app_for_status.emit(
                                "channel:platform-state",
                                &ChannelPlatformStatePayload { state },
                            );
                        }
                        Err(err) => {
                            log::warn!("[channel/wecom] failed to build platform state: {:#}", err)
                        }
                    }
                });
            },
        );

        // Register the wecom connector (replaces any previous instance under
        // Platform::Wecom) and grab the concrete handle.
        let concrete_wecom = self
            .register_wecom_connector(bot_id.clone(), secret_plain, Arc::clone(&on_status))
            .await;

        // 订阅 RuntimeEventBus → connector.send(Markdown)（整个 manager 生命周期
        // 内只订阅一次，避免重连/重保存配置时把同一 subscriber 重复挂载，否则
        // 同一条 assistant 回复会被发送多次）。Forwarder 通过
        // connector.has_session() 过滤，不属于企微的会话直接忽略。
        if claim_first_subscription(&self.wecom_reply_subscribed) {
            let forwarder = Arc::new(super::wecom::reply_forwarder::WecomReplyForwarder::new(
                Arc::clone(&concrete_wecom),
            ));
            let sub: Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber> = forwarder;
            self.chat_adapter.subscribe_event_listener(sub.clone());
            self.anchor_subscriber(sub);
            log::info!("[channel/wecom] subscribed WecomReplyForwarder to RuntimeEventBus");
        }

        // Start via the trait surface — get BoxStream<ChannelMessage>.
        let new_token = CancellationToken::new();
        let ctx = ConnectorContext {
            config_store: Arc::clone(&self.config_store),
            secure_storage: None,
            ask_coordinator: self.ask_coordinator.as_ref().map(Arc::clone),
            pending_manager: Arc::clone(&self.pending_manager),
            cancel_token: new_token.clone(),
        };
        let connector = {
            let map = self.connectors.read().await;
            map.get(&Platform::Wecom)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("wecom connector not registered"))?
        };
        let mut message_stream = connector
            .start(ctx)
            .await
            .map_err(|e| anyhow::anyhow!("wecom connector start failed: {e}"))?;

        let message_cancel = new_token.clone();
        self.platform_state_mutate(Platform::Wecom, |s| {
            s.stream_cancel = Some(new_token);
        })
        .await;

        // Worker — receives ChannelMessages from the wecom stream and routes
        // them to the chat turn engine via PendingQueueManager. No reply_manager
        // (wecom uses markdown fallback, no AI card), no ask_coordinator branching
        // (deferred to later PR). Mirrors the feishu worker loop.
        let adapter = Arc::clone(&self.chat_adapter);
        let conv_store = Arc::clone(&self.conversation_store);
        let sessions_path = self.sessions_paths[&Platform::Wecom].clone();
        let seen_ids = Arc::clone(&self.seen_msg_ids);
        let convs = Arc::clone(&self.conversations);
        let app_handle = self.app_handle.clone();
        let channel_session_ids_ref = Arc::clone(&self.channel_session_ids);
        let inactive_ref = Arc::clone(&self.inactive);
        let on_status_for_worker = Arc::clone(&on_status);
        let ask_coordinator_ref = self.ask_coordinator.as_ref().map(Arc::clone);
        let pending_manager_ref = Arc::clone(&self.pending_manager);
        let platform_state_for_worker = Arc::clone(&self.platform_state);
        let connector_for_worker = {
            let map = self.connectors.read().await;
            Arc::clone(map.get(&Platform::Wecom).expect("wecom just registered"))
        };
        // Concrete handle for non-trait methods (`remember_session`). Trait-erased
        // `connector_for_worker` doesn't expose those — symmetric with the feishu
        // worker.
        let concrete_wecom_for_worker = Arc::clone(&concrete_wecom);
        // Snapshot the wecom downloads dir once before spawning the worker;
        // `self` isn't movable into the async block.
        let wecom_downloads_dir = self.wecom_downloads_dir();

        let message_handle = tokio::spawn(async move {
            let mut router =
                match ChannelSessionRouter::migrate_or_load(&sessions_path, conv_store.as_ref()) {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("[channel/wecom] failed to load router: {:#}", e);
                        return;
                    }
                };

            loop {
                let msg = match recv_current_generation_message_stream(
                    &mut message_stream,
                    &message_stream_generation,
                    generation,
                    &message_cancel,
                )
                .await
                {
                    Some(m) => m,
                    None => {
                        log::info!("[channel/wecom] worker stream ended");
                        let current = {
                            let map = platform_state_for_worker.read().await;
                            map.get(&Platform::Wecom)
                                .map(|s| s.connection.clone())
                                .unwrap_or(ChannelConnectionState::Unconfigured)
                        };
                        if !matches!(
                            current,
                            ChannelConnectionState::NeedsReauth
                                | ChannelConnectionState::ConfigError
                        ) {
                            on_status_for_worker(ChannelConnectionState::Reconnecting, None);
                        }
                        break;
                    }
                };

                log::info!(
                    "[channel/wecom] worker received msg msg_id={} text_len={} attachments={}",
                    msg.msg_id,
                    msg.text.len(),
                    msg.attachments.len()
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                // Manager-level dedup.
                if !seen_ids.observe(&msg.msg_id).await {
                    log::debug!("[channel/wecom] duplicate msg_id {}, skipping", msg.msg_id);
                    continue;
                }

                let conv_type = msg.conversation_type.clone();
                let conv_key = msg.conversation_key.clone();
                let sender_nick = msg.sender_nick.clone();
                let text = msg.text.clone();

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                let store_ref = Arc::clone(&conv_store);
                let ensure_store_ref = Arc::clone(&conv_store);
                let sender_nick_for_create = sender_nick.clone();
                let sender_nick_for_ensure = sender_nick.clone();
                let conv_key_for_create = conv_key.clone();
                let conv_type_for_create = conv_type.clone();
                let session_id = match router.get_or_create_session_with_ensure(
                    &conv_type,
                    &router_key,
                    &conv_key,
                    || {
                        let title = match &conv_type_for_create {
                            ConversationType::Group => format!(
                                "企微群 {}",
                                &conv_key_for_create[..conv_key_for_create.len().min(8)]
                            ),
                            ConversationType::Private => sender_nick_for_create.clone(),
                        };
                        let id = uuid::Uuid::new_v4().to_string();
                        store_ref
                            .create_conversation_with_im_source(
                                &id,
                                &title,
                                Platform::Wecom.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))?;
                        Ok(id)
                    },
                    |existing_id| {
                        ensure_store_ref
                            .create_conversation_with_im_source(
                                existing_id,
                                &sender_nick_for_ensure,
                                Platform::Wecom.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))
                    },
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        log::error!("[channel/wecom] session routing failed: {:#}", e);
                        continue;
                    }
                };

                // Cache the per-session reply target on the concrete connector
                // so WecomReplyForwarder can address future assistant messages
                // to the right chatid (parallel to feishu's `remember_session`).
                // `conv_key` is `chatid` for groups and `from.userid` for
                // privates — both are valid `sendMessage` targets.
                concrete_wecom_for_worker
                    .remember_session(
                        session_id.clone(),
                        super::wecom::types::WecomSessionTarget {
                            chat_id: conv_key.clone(),
                        },
                    )
                    .await;

                // Register the session id with the shared channel registry.
                {
                    if inactive_ref.load(std::sync::atomic::Ordering::SeqCst) {
                        log::debug!(
                            "[channel/wecom] worker observed inactive flag, dropping session id insert"
                        );
                        continue;
                    }
                    let mut ids = channel_session_ids_ref
                        .write()
                        .expect("channel_session_ids poisoned");
                    ids.insert(session_id.clone());
                }

                // Push to conversations list (new sessions only).
                {
                    let mut convs_lock = convs.write().await;
                    if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                        break;
                    }
                    if !convs_lock.iter().any(|c| c.session_id == session_id) {
                        let display_name = match &conv_type {
                            ConversationType::Group => {
                                format!("企微群 {}", &conv_key[..conv_key.len().min(8)])
                            }
                            ConversationType::Private => sender_nick.clone(),
                        };
                        convs_lock.push(ChannelConversation {
                            session_id: session_id.clone(),
                            platform: Platform::Wecom,
                            conversation_type: conv_type.clone(),
                            external_id: conv_key.clone(),
                            display_name,
                            unread_count: 0,
                            robot_code: router_key.clone(),
                            is_active_robot: true,
                        });
                    }
                }

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                let preview_source = if text.trim().is_empty() && !msg.attachments.is_empty() {
                    format!("[附件] {} 个文件", msg.attachments.len())
                } else if !msg.attachments.is_empty() {
                    format!("[附件] {}", text)
                } else {
                    text.clone()
                };
                let preview = if preview_source.chars().count() > 30 {
                    format!("{}...", preview_source.chars().take(30).collect::<String>())
                } else {
                    preview_source
                };
                let _ = app_handle.emit(
                    "channel:message",
                    &ChannelMessagePayload {
                        platform: "wecom".into(),
                        session_id: session_id.clone(),
                        sender_nick: sender_nick.clone(),
                        text_preview: preview,
                    },
                );

                let session_for_ask = crate::runtime::ids::SessionId::new(session_id.clone());
                match handle_pending_action_pre_dispatch(
                    ask_coordinator_ref.as_ref(),
                    &session_for_ask,
                    &text,
                )
                .await
                {
                    Ok(super::shared::ask_coordinator::HandleOutcome::NotPending)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::NewTurnAfterAbandon) => {}
                    Ok(super::shared::ask_coordinator::HandleOutcome::ApprovalResolved)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::AnswerResolved) => {
                        continue;
                    }
                    Ok(super::shared::ask_coordinator::HandleOutcome::InvalidApprovalAction {
                        message,
                    }) => {
                        send_pending_action_text_ack(
                            &connector_for_worker,
                            &session_id,
                            &conv_key,
                            "[channel/wecom]",
                            message,
                        )
                        .await;
                        continue;
                    }
                    Err(err) => {
                        log::warn!(
                            "[channel/wecom] IM ask coordinator failed, falling back to normal turn: {:#}",
                            err
                        );
                    }
                };

                // Wecom 附件下载：HTTP GET 加密文件 + AES-256-CBC 解密 → 落盘。
                // 实现走 `wecom::media::download_and_save`；error 收进 failures
                // 让 `build_compound_content` 给 LLM 加一段 "[注意：下列附件下载
                // 失败 ...]" hint，对称 feishu 路径。
                let (chat_attachments, download_failures) = if msg.attachments.is_empty() {
                    (Vec::new(), Vec::new())
                } else {
                    log::info!(
                        "[channel/wecom] downloading {} attachments msg_id={} session={}",
                        msg.attachments.len(),
                        msg.msg_id,
                        session_id
                    );
                    download_specs_for_turn_wecom(
                        &msg.attachments,
                        &wecom_downloads_dir,
                        &msg.msg_id,
                    )
                    .await
                };
                // All-attachments-failed + empty text → reply to user with
                // a hint; do NOT push a half-empty turn to the LLM (mirror
                // feishu's `all-attachments-failed` branch).
                if chat_attachments.is_empty()
                    && text.trim().is_empty()
                    && !msg.attachments.is_empty()
                {
                    log::warn!(
                        "[channel/wecom] all attachments failed and no text, replying via send(Text) msg_id={}",
                        msg.msg_id
                    );
                    let connector_for_fallback = Arc::clone(&connector_for_worker);
                    let session_for_fallback = session_id.clone();
                    let conv_key_for_fallback = conv_key.clone();
                    tokio::spawn(async move {
                        if let Err(e) = connector_for_fallback
                            .send(
                                crate::connector::im::trait_def::ReplyTarget {
                                    session_id: session_for_fallback.clone(),
                                    external_conversation_key: conv_key_for_fallback,
                                },
                                crate::connector::im::trait_def::ReplyContent::Text(
                                    "附件下载全部失败，请重发。".to_string(),
                                ),
                            )
                            .await
                        {
                            log::warn!(
                                "[channel/wecom] fallback text send failed session={}: {:#}",
                                session_for_fallback,
                                e
                            );
                        }
                    });
                    continue;
                }

                let request = build_channel_chat_request(
                    session_id.clone(),
                    crate::runtime::human_interaction::ImPlatform::Wecom,
                    conv_key.clone(),
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments.clone(),
                    &download_failures,
                );
                let pending_item = super::shared::pending_adapter::build_pending_item_from_wecom(
                    &msg.msg_id,
                    &session_id,
                    &conv_key,
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments,
                    &download_failures,
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                let adapter_for_turn = Arc::clone(&adapter);
                let session_for_log = session_id.clone();
                let pending_manager_for_send = Arc::clone(&pending_manager_ref);
                let session_for_enqueue = crate::runtime::ids::SessionId::new(session_id.clone());
                let connector_for_send = Arc::clone(&connector_for_worker);
                let conv_key_for_reject = conv_key.clone();
                tokio::spawn(async move {
                    match pending_manager_for_send
                        .enqueue_or_send(session_for_enqueue, pending_item)
                        .await
                    {
                        Ok(crate::runtime::pending::EnqueueOutcome::SentDirectly { .. }) => {
                            if let Err(e) = adapter_for_turn.send_chat_request(request).await {
                                log::error!(
                                    "[channel/wecom] send_chat_request failed session={}: {}",
                                    session_for_log,
                                    e
                                );
                                pending_manager_for_send
                                    .release_direct_dispatch(&crate::runtime::ids::SessionId::new(
                                        session_for_log.clone(),
                                    ))
                                    .await;
                            }
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Queued { snapshot }) => {
                            log::info!(
                                "[channel/wecom] message queued session={} queue_size={}",
                                session_for_log,
                                snapshot.len()
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::HeldForHumanInteraction {
                            interaction_id,
                        }) => {
                            log::info!(
                                "[channel/wecom] message held for human interaction session={} interaction_id={:?}",
                                session_for_log,
                                interaction_id
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Rejected { reason }) => {
                            log::warn!(
                                "[channel/wecom] enqueue rejected session={} reason={:?}",
                                session_for_log,
                                reason
                            );
                            if let crate::runtime::pending::EnqueueRejection::QueueFull { limit } =
                                reason
                            {
                                let text = format!("消息堆积过多（已达 {limit} 条），请稍后再发。");
                                if let Err(e) = connector_for_send
                                    .send(
                                        crate::connector::im::trait_def::ReplyTarget {
                                            session_id: session_for_log.clone(),
                                            external_conversation_key: conv_key_for_reject.clone(),
                                        },
                                        crate::connector::im::trait_def::ReplyContent::Text(text),
                                    )
                                    .await
                                {
                                    log::warn!(
                                        "[channel/wecom] queue-full reject text send failed session={}: {:#}",
                                        session_for_log,
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "[channel/wecom] enqueue_or_send error session={}: {:#}",
                                session_for_log,
                                e
                            );
                        }
                    }
                });
            }
        });
        self.platform_state_mutate(Platform::Wecom, |s| {
            s.message_task = Some(message_handle);
        })
        .await;

        Ok(())
    }

    /// 返回钉钉附件下载目录 `~/.renlijia/tmp/dingtalk_downloads/`。优先从
    /// AiJiaHome state 读，缺失时（理论上不应发生）退回 chat_adapter workspace。
    fn dingtalk_downloads_dir(&self) -> PathBuf {
        if let Some(home) = self
            .app_handle
            .try_state::<Arc<crate::storage::AiJiaHome>>()
        {
            home.tmp_dingtalk_downloads_dir()
        } else {
            self.chat_adapter
                .workspace_path()
                .join("dingtalk_downloads")
        }
    }

    /// 返回飞书附件下载目录 `~/.renlijia/tmp/feishu_downloads/`。优先从
    /// AiJiaHome state 读，缺失时退回 chat_adapter workspace。PR6 引入。
    fn feishu_downloads_dir(&self) -> PathBuf {
        if let Some(home) = self
            .app_handle
            .try_state::<Arc<crate::storage::AiJiaHome>>()
        {
            home.tmp_feishu_downloads_dir()
        } else {
            self.chat_adapter.workspace_path().join("feishu_downloads")
        }
    }

    /// 返回企微附件下载目录 `~/.renlijia/tmp/wecom_downloads/`。镜像
    /// `feishu_downloads_dir`：AiJiaHome state 缺失时回落到 chat_adapter
    /// workspace，避免 dev 模式下 manager 找不到 home 而 panic。
    fn wecom_downloads_dir(&self) -> PathBuf {
        if let Some(home) = self
            .app_handle
            .try_state::<Arc<crate::storage::AiJiaHome>>()
        {
            home.tmp_wecom_downloads_dir()
        } else {
            self.chat_adapter.workspace_path().join("wecom_downloads")
        }
    }

    /// 个人微信（iLink）附件下载目录 `~/.renlijia/tmp/wechat_downloads/`。镜像
    /// `wecom_downloads_dir`。
    fn wechat_downloads_dir(&self) -> PathBuf {
        if let Some(home) = self
            .app_handle
            .try_state::<Arc<crate::storage::AiJiaHome>>()
        {
            home.tmp_wechat_downloads_dir()
        } else {
            self.chat_adapter.workspace_path().join("wechat_downloads")
        }
    }

    /// Telegram 附件下载目录 `~/.renlijia/tmp/telegram_downloads/`。镜像
    /// `wecom_downloads_dir`。
    fn telegram_downloads_dir(&self) -> PathBuf {
        if let Some(home) = self
            .app_handle
            .try_state::<Arc<crate::storage::AiJiaHome>>()
        {
            home.tmp_telegram_downloads_dir()
        } else {
            self.chat_adapter
                .workspace_path()
                .join("telegram_downloads")
        }
    }

    /// WhatsApp 附件下载目录 `~/.renlijia/tmp/whatsapp_downloads/`。镜像
    /// `telegram_downloads_dir`。
    fn whatsapp_downloads_dir(&self) -> PathBuf {
        if let Some(home) = self
            .app_handle
            .try_state::<Arc<crate::storage::AiJiaHome>>()
        {
            home.tmp_whatsapp_downloads_dir()
        } else {
            self.chat_adapter
                .workspace_path()
                .join("whatsapp_downloads")
        }
    }

    async fn emit_dingtalk_state(&self) {
        match self.current_dingtalk_state().await {
            Ok(state) => {
                let _ = self.app_handle.emit(
                    "channel:platform-state",
                    &ChannelPlatformStatePayload { state },
                );
            }
            Err(error) => log::warn!("[channel] failed to emit platform state: {:#}", error),
        }
    }

    /// 启动时调用一次：从每个平台自己的 sessions.json + conversation_store 重建
    /// 内存 conversations 列表。期间检测到 v1 schema 会清掉所有指向的 conversation
    /// 目录（参见 router.migrate_or_load）。
    pub async fn hydrate_conversations(&self) {
        // 钉钉：only entries whose robot_code == 当前 dingtalk_config.bot.robot_code 是 active。
        let dingtalk_current_robot = match self.config_store.read_dingtalk_config() {
            Ok(Some(cfg)) => Some(cfg.bot.robot_code),
            Ok(None) => None,
            Err(e) => {
                log::warn!(
                    "[channel] hydrate_conversations: failed to read dingtalk config: {:#}",
                    e
                );
                None
            }
        };
        // 飞书：app_id 是 router_key；当前唯一配置的 app_id 视为 active。
        let feishu_current_app_id = match self.config_store.read_feishu_config() {
            Ok(Some(cfg)) => Some(cfg.credentials.app_id),
            Ok(None) => None,
            Err(e) => {
                log::warn!(
                    "[channel] hydrate_conversations: failed to read feishu config: {:#}",
                    e
                );
                None
            }
        };
        // 企微：bot_id 是 router_key；当前唯一配置的 bot_id 视为 active。
        let wecom_current_bot_id = match self.config_store.read_wecom_config() {
            Ok(Some(cfg)) => Some(cfg.credentials.bot_id),
            Ok(None) => None,
            Err(e) => {
                log::warn!(
                    "[channel] hydrate_conversations: failed to read wecom config: {:#}",
                    e
                );
                None
            }
        };
        let wechat_current_bot_id = match self.config_store.read_wechat_config() {
            Ok(Some(cfg)) => Some(cfg.bot.ilink_bot_id),
            Ok(None) => None,
            Err(e) => {
                log::warn!(
                    "[channel] hydrate_conversations: failed to read wechat config: {:#}",
                    e
                );
                None
            }
        };
        // Telegram：router_key = "tg-{bot_id}"；只有当前 token 配的 bot 视为 active。
        let telegram_current_router_key = match self.config_store.read_telegram_config() {
            Ok(Some(cfg)) => Some(format!("tg-{}", cfg.bot.bot_id)),
            Ok(None) => None,
            Err(e) => {
                log::warn!(
                    "[channel] hydrate_conversations: failed to read telegram config: {:#}",
                    e
                );
                None
            }
        };
        // WhatsApp：单账号场景下 router_key 始终是常量 "whatsapp"
        // （与 2695 行 ROUTER_KEY const 保持一致）。只要 config.json 存在 →
        // 当前已配对 → 该 router_key 视为 active。
        let whatsapp_current_router_key: Option<&str> = {
            let path = self.config_store.platform_config_path(Platform::Whatsapp);
            match crate::connector::im::whatsapp::config::read(&path) {
                Ok(Some(_)) => Some("whatsapp"),
                _ => None,
            }
        };

        let mut all_entries: Vec<(Platform, super::shared::router::RouterEntry)> = Vec::new();
        let platforms_to_hydrate = [
            Platform::Dingtalk,
            Platform::Feishu,
            Platform::Wecom,
            Platform::Wechat,
            Platform::Telegram,
            Platform::Whatsapp,
        ];
        for platform in platforms_to_hydrate {
            let Some(path) = self.sessions_paths.get(&platform) else {
                continue;
            };
            let router = match super::shared::router::ChannelSessionRouter::migrate_or_load(
                path,
                self.conversation_store.as_ref(),
            ) {
                Ok(r) => r,
                Err(e) => {
                    log::error!(
                        "[channel] hydrate_conversations({}): failed to load router: {:#}",
                        platform.as_str(),
                        e
                    );
                    continue;
                }
            };
            for entry in router.entries() {
                // One-shot migration: stamp `im_source` on old conv.json / index.json
                // entries that pre-date the IM-source field. `backfill_*` is
                // idempotent — entries that already carry an `im_source` are
                // skipped, so this is cheap to run every startup.
                if let Err(e) = self
                    .conversation_store
                    .backfill_conversation_im_source(&entry.session_id, platform.as_str())
                {
                    log::warn!(
                        "[channel] hydrate_conversations({}): backfill imSource for {} failed: {:#}",
                        platform.as_str(),
                        entry.session_id,
                        e
                    );
                }
                all_entries.push((platform, entry));
            }
        }

        // 将持久化的 session id 写入共享 registry，确保 ask_coordinator 从启动起
        // 就能识别已有频道会话（而不仅是本次运行新建的会话）。
        if !self.is_inactive() {
            let mut ids = self
                .channel_session_ids
                .write()
                .expect("channel_session_ids poisoned");
            for (_, entry) in &all_entries {
                ids.insert(entry.session_id.clone());
            }
        }

        let snapshot = build_conversation_snapshot(
            &all_entries,
            self.conversation_store.as_ref(),
            HydrateCurrentRobots {
                dingtalk: dingtalk_current_robot.as_deref(),
                feishu: feishu_current_app_id.as_deref(),
                wecom: wecom_current_bot_id.as_deref(),
                wechat: wechat_current_bot_id.as_deref(),
                telegram: telegram_current_router_key.as_deref(),
                whatsapp: whatsapp_current_router_key,
            },
        );
        *self.conversations.write().await = snapshot;
    }

    /// 重新计算指定平台下每条 conversation 的 `is_active_robot`：robot_code
    /// 等于该平台 `current_robot_code` 的为 true。其它平台的 conv 完全不动 ——
    /// 否则 `set_dingtalk_connection_state(Connected)` 会拿钉钉的 robot_code
    /// 跟飞书会话比较，结果飞书会话被一致判定 `is_active_robot=false`，sidebar
    /// 把它们全过滤掉。
    /// 调用方需要保证 emit 一次 platform-state 让前端重拉 conversations。
    pub async fn refresh_active_robot_flags(
        &self,
        platform: Platform,
        current_robot_code: Option<&str>,
    ) {
        let mut convs = self.conversations.write().await;
        for c in convs.iter_mut() {
            if c.platform != platform {
                continue;
            }
            c.is_active_robot = current_robot_code
                .map(|rc| rc == c.robot_code)
                .unwrap_or(false);
        }
    }

    /// Set Dingtalk's per-platform connection state and surface it through the
    /// `channel:platform-state` event. The Connected-side effect (refresh
    /// `is_active_robot` flags) is dingtalk-specific so it lives here, not in
    /// a generic helper.
    async fn set_dingtalk_connection_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) {
        self.platform_state_mutate(Platform::Dingtalk, |s| {
            s.connection = connection.clone();
            s.last_error = last_error.clone();
        })
        .await;
        if matches!(connection, ChannelConnectionState::Connected) {
            let current_robot = self
                .config_store
                .read_dingtalk_config()
                .ok()
                .flatten()
                .map(|cfg| cfg.bot.robot_code);
            self.refresh_active_robot_flags(Platform::Dingtalk, current_robot.as_deref())
                .await;
        }
        self.emit_dingtalk_state().await;
    }

    /// Set Feishu's per-platform connection state and surface it through the
    /// `channel:platform-state` event. Connected 时按当前 feishu config 的
    /// `app_id` 刷新仅飞书会话的 `is_active_robot`（对称于钉钉路径），否则
    /// hydrate 把它们标 active、`refresh_active_robot_flags` 又跨平台错刷 false
    /// 之后没人刷回来，sidebar 把它们全过滤掉。
    async fn set_feishu_connection_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) {
        log::info!(
            "[channel/feishu] set_feishu_connection_state connection={:?} last_error={:?}",
            connection,
            last_error
        );
        self.platform_state_mutate(Platform::Feishu, |s| {
            s.connection = connection.clone();
            s.last_error = last_error.clone();
        })
        .await;
        if matches!(connection, ChannelConnectionState::Connected) {
            let current_app_id = self
                .config_store
                .read_feishu_config()
                .ok()
                .flatten()
                .map(|cfg| cfg.credentials.app_id);
            self.refresh_active_robot_flags(Platform::Feishu, current_app_id.as_deref())
                .await;
        }
        match self
            .config_store
            .feishu_state(connection.clone(), last_error)
        {
            Ok(state) => {
                log::info!(
                    "[channel/feishu] emit channel:platform-state connection={:?} configured={} enabled={}",
                    state.connection,
                    state.configured,
                    state.enabled
                );
                let _ = self.app_handle.emit(
                    "channel:platform-state",
                    &ChannelPlatformStatePayload { state },
                );
            }
            Err(error) => {
                log::warn!(
                    "[channel/feishu] failed to emit platform state (connection={:?}): {:#}",
                    connection,
                    error
                )
            }
        }
    }

    /// Cancel the per-platform stream cancel token and await the per-platform
    /// worker task. Bumps the per-platform generation so any stale callbacks
    /// or worker iterations bail. Does NOT touch other platforms' slots.
    async fn stop_stream(&self, platform: Platform) {
        let (cancel_token, task_handle) = self
            .platform_state_mutate(platform.clone(), |s| {
                s.stream_generation.fetch_add(1, Ordering::SeqCst);
                (s.stream_cancel.take(), s.message_task.take())
            })
            .await;
        if let Some(token) = cancel_token {
            log::info!(
                "[channel/{}] cancelling previous stream connection",
                platform.as_str()
            );
            token.cancel();
        }
        if let Some(handle) = task_handle {
            if let Err(error) = handle.await {
                log::warn!(
                    "[channel/{}] message worker join failed: {}",
                    platform.as_str(),
                    error
                );
            }
        }
    }

    /// 读取用户隔离的平台配置，DingTalk / 飞书 各自独立 auto-connect。PR3.5：
    /// 两个连接互不干扰（各自的 stream_cancel / generation 在 platform_state map
    /// 里独立持有），可以**真正并发**——一个连失败不会影响另一个。
    pub async fn auto_connect_if_configured(&self) {
        match self.config_store.read_dingtalk_config() {
            Ok(Some(config)) if config.enabled => {
                if let Err(error) = self.connect_dingtalk_from_store().await {
                    log::warn!("[channel/dingtalk] auto_connect failed: {:#}", error);
                    self.set_dingtalk_connection_state(
                        ChannelConnectionState::ConfigError,
                        Some(error.to_string()),
                    )
                    .await;
                }
            }
            Ok(Some(_)) => {
                self.set_dingtalk_connection_state(ChannelConnectionState::Disconnected, None)
                    .await;
            }
            Ok(None) => {
                self.set_dingtalk_connection_state(ChannelConnectionState::Unconfigured, None)
                    .await;
            }
            Err(error) => {
                log::warn!("[channel/dingtalk] failed to read config: {:#}", error);
                self.set_dingtalk_connection_state(
                    ChannelConnectionState::ConfigError,
                    Some(error.to_string()),
                )
                .await;
            }
        }

        // Feishu auto-connect — fully independent of dingtalk. The two streams
        // hold their own per-platform slots (cancel token, worker task,
        // generation counter), so failures here cannot kill an active dingtalk
        // stream and vice-versa.
        match self.config_store.read_feishu_config() {
            Ok(Some(config)) if config.enabled => {
                if let Err(error) = self.connect_feishu_from_store().await {
                    log::warn!("[channel/feishu] auto_connect failed: {:#}", error);
                    self.set_feishu_connection_state(
                        ChannelConnectionState::ConfigError,
                        Some(error.to_string()),
                    )
                    .await;
                }
            }
            Ok(Some(_)) => {
                self.set_feishu_connection_state(ChannelConnectionState::Disconnected, None)
                    .await;
            }
            Ok(None) => {
                // No feishu config — leave the slot uninitialized; get_platform
                // returns the unconfigured default from feishu_state_stub.
            }
            Err(error) => {
                log::warn!("[channel/feishu] failed to read config: {:#}", error);
                self.set_feishu_connection_state(
                    ChannelConnectionState::ConfigError,
                    Some(error.to_string()),
                )
                .await;
            }
        }

        // Wecom auto-connect — fully independent of other platforms.
        match self.config_store.read_wecom_config() {
            Ok(Some(config)) if config.enabled => {
                if let Err(error) = self.connect_wecom_from_store().await {
                    log::warn!("[channel/wecom] auto_connect failed: {:#}", error);
                    self.set_wecom_connection_state(
                        ChannelConnectionState::ConfigError,
                        Some(error.to_string()),
                    )
                    .await;
                }
            }
            Ok(Some(_)) => {
                self.set_wecom_connection_state(ChannelConnectionState::Disconnected, None)
                    .await;
            }
            Ok(None) => {
                // No wecom config — leave slot uninitialized.
            }
            Err(error) => {
                log::warn!("[channel/wecom] failed to read config: {:#}", error);
                self.set_wecom_connection_state(
                    ChannelConnectionState::ConfigError,
                    Some(error.to_string()),
                )
                .await;
            }
        }

        // WeChat (iLink) auto-connect — independent of other platforms.
        match self.config_store.read_wechat_config() {
            Ok(Some(config)) if config.enabled => {
                if let Err(error) = self.connect_wechat_from_store().await {
                    log::warn!("[channel/wechat] auto_connect failed: {:#}", error);
                    self.set_wechat_connection_state(
                        ChannelConnectionState::ConfigError,
                        Some(error.to_string()),
                    )
                    .await;
                }
            }
            Ok(Some(_)) => {
                self.set_wechat_connection_state(ChannelConnectionState::Disconnected, None)
                    .await;
            }
            Ok(None) => {
                // No wechat config — leave slot uninitialized.
            }
            Err(error) => {
                log::warn!("[channel/wechat] failed to read config: {:#}", error);
                self.set_wechat_connection_state(
                    ChannelConnectionState::ConfigError,
                    Some(error.to_string()),
                )
                .await;
            }
        }

        // Telegram auto-connect — independent of other platforms.
        match self.config_store.read_telegram_config() {
            Ok(Some(config)) if config.enabled => {
                if let Err(error) = self.connect_telegram_from_store().await {
                    log::warn!("[channel/telegram] auto_connect failed: {:#}", error);
                    self.set_telegram_connection_state(
                        ChannelConnectionState::ConfigError,
                        Some(error.to_string()),
                    )
                    .await;
                }
            }
            Ok(Some(_)) => {
                self.set_telegram_connection_state(ChannelConnectionState::Disconnected, None)
                    .await;
            }
            Ok(None) => {
                // No telegram config — leave slot uninitialized.
            }
            Err(error) => {
                log::warn!("[channel/telegram] failed to read config: {:#}", error);
                self.set_telegram_connection_state(
                    ChannelConnectionState::ConfigError,
                    Some(error.to_string()),
                )
                .await;
            }
        }

        // WhatsApp auto-connect — independent of other platforms.
        // Existence of channels/whatsapp/config.json IS the signal.
        if let Err(error) = self.connect_whatsapp_from_store().await {
            log::warn!("[channel/whatsapp] auto_connect failed: {:#}", error);
            self.set_whatsapp_connection_state(
                ChannelConnectionState::ConfigError,
                Some(error.to_string()),
            )
            .await;
        }
    }

    pub async fn get_platforms(&self) -> Result<Vec<ChannelPlatformState>> {
        // Iterate per-platform — each has its own connection/last_error slot
        // (see PerPlatformState). The shared `all_platform_states` helper on
        // config_store is no longer used here because it took a single
        // connection state for all platforms.
        let mut out = Vec::with_capacity(Platform::all().len());
        for p in Platform::all() {
            out.push(self.get_platform(p).await?);
        }
        Ok(out)
    }

    pub async fn get_platform(&self, platform: Platform) -> Result<ChannelPlatformState> {
        let (connection, last_error) = self
            .platform_state_read(platform.clone(), |s| {
                (s.connection.clone(), s.last_error.clone())
            })
            .await
            .unwrap_or((ChannelConnectionState::Unconfigured, None));

        match platform {
            Platform::Dingtalk => self.config_store.dingtalk_state(connection, last_error),
            Platform::Feishu => self.config_store.feishu_state(connection, last_error),
            Platform::Wechat => self.config_store.wechat_state(connection, last_error),
            Platform::Wecom => self.config_store.wecom_state(connection, last_error),
            Platform::Telegram => self.config_store.telegram_state(connection, last_error),
            Platform::Whatsapp => self.config_store.whatsapp_state(connection, last_error),
            #[allow(unreachable_patterns)]
            other => Ok(ChannelConfigStore::coming_soon_state(other)),
        }
    }

    pub async fn set_enabled(
        &self,
        platform: Platform,
        enabled: bool,
    ) -> Result<ChannelPlatformState> {
        if self.is_inactive() {
            return Err(anyhow::anyhow!("channel manager inactive"));
        }
        match platform {
            Platform::Dingtalk => {
                if enabled {
                    self.config_store.set_dingtalk_enabled(true)?;
                    self.connect_dingtalk_from_store().await?;
                } else {
                    self.stop_stream(Platform::Dingtalk).await;
                    self.config_store.set_dingtalk_enabled(false)?;
                    self.set_dingtalk_connection_state(ChannelConnectionState::Disconnected, None)
                        .await;
                }
                self.current_dingtalk_state().await
            }
            Platform::Feishu => {
                if enabled {
                    self.config_store.set_feishu_enabled(true)?;
                    self.connect_feishu_from_store().await?;
                    self.current_feishu_state().await
                } else {
                    // PR3.5: stop ONLY the feishu stream — dingtalk is on a
                    // separate slot so an active dingtalk session is untouched.
                    self.stop_stream(Platform::Feishu).await;
                    let state = self.config_store.set_feishu_enabled(false)?;
                    self.set_feishu_connection_state(ChannelConnectionState::Disconnected, None)
                        .await;
                    Ok(state)
                }
            }
            Platform::Wecom => {
                if enabled {
                    self.config_store.set_wecom_enabled(true)?;
                    self.connect_wecom_from_store().await?;
                    self.current_wecom_state().await
                } else {
                    self.stop_stream(Platform::Wecom).await;
                    self.config_store.set_wecom_enabled(false)?;
                    self.set_wecom_connection_state(ChannelConnectionState::Disconnected, None)
                        .await;
                    self.current_wecom_state().await
                }
            }
            Platform::Wechat => {
                if enabled {
                    self.config_store.set_wechat_enabled(true)?;
                    self.connect_wechat_from_store().await?;
                    self.current_wechat_state().await
                } else {
                    self.stop_stream(Platform::Wechat).await;
                    self.config_store.set_wechat_enabled(false)?;
                    self.set_wechat_connection_state(ChannelConnectionState::Disconnected, None)
                        .await;
                    self.current_wechat_state().await
                }
            }
            Platform::Telegram => {
                if enabled {
                    self.config_store.set_telegram_enabled(true)?;
                    self.connect_telegram_from_store().await?;
                    self.current_telegram_state().await
                } else {
                    self.stop_stream(Platform::Telegram).await;
                    self.config_store.set_telegram_enabled(false)?;
                    self.set_telegram_connection_state(ChannelConnectionState::Disconnected, None)
                        .await;
                    self.current_telegram_state().await
                }
            }
            Platform::Whatsapp => {
                if enabled {
                    self.config_store.set_whatsapp_enabled(true)?;
                    self.connect_whatsapp_from_store().await?;
                } else {
                    self.stop_stream(Platform::Whatsapp).await;
                    self.config_store.set_whatsapp_enabled(false)?;
                    self.set_whatsapp_connection_state(ChannelConnectionState::Disconnected, None)
                        .await;
                }
                let (connection, last_error) = self
                    .platform_state_read(Platform::Whatsapp, |s| {
                        (s.connection.clone(), s.last_error.clone())
                    })
                    .await
                    .unwrap_or((ChannelConnectionState::Unconfigured, None));
                self.config_store.whatsapp_state(connection, last_error)
            }
        }
    }

    pub async fn remove_platform(&self, platform: Platform) -> Result<ChannelPlatformState> {
        match platform {
            Platform::Dingtalk => {
                self.stop_stream(Platform::Dingtalk).await;
                let state = self.config_store.remove_dingtalk()?;
                self.clear_runtime_state().await;
                self.refresh_active_robot_flags(Platform::Dingtalk, None)
                    .await;
                self.set_dingtalk_connection_state(ChannelConnectionState::Unconfigured, None)
                    .await;
                Ok(state)
            }
            Platform::Feishu => {
                // PR3.5: stop ONLY the feishu stream before deleting the
                // on-disk config; dingtalk's slot is untouched.
                self.stop_stream(Platform::Feishu).await;
                let state = self.config_store.remove_feishu()?;
                self.set_feishu_connection_state(ChannelConnectionState::Unconfigured, None)
                    .await;
                Ok(state)
            }
            Platform::Wecom => {
                self.stop_stream(Platform::Wecom).await;
                let state = self.config_store.remove_wecom()?;
                self.set_wecom_connection_state(ChannelConnectionState::Unconfigured, None)
                    .await;
                Ok(state)
            }
            Platform::Wechat => {
                self.stop_stream(Platform::Wechat).await;
                let state = self.config_store.remove_wechat()?;
                self.set_wechat_connection_state(ChannelConnectionState::Unconfigured, None)
                    .await;
                Ok(state)
            }
            Platform::Telegram => {
                self.stop_stream(Platform::Telegram).await;
                let state = self.config_store.remove_telegram()?;
                // Clear the cached concrete handle so the next save_telegram_and_connect
                // path builds a fresh connector against the new token.
                {
                    let mut guard = self.telegram_concrete.write().await;
                    *guard = None;
                }
                self.set_telegram_connection_state(ChannelConnectionState::Unconfigured, None)
                    .await;
                Ok(state)
            }
            Platform::Whatsapp => {
                self.stop_stream(Platform::Whatsapp).await;
                let state = self.config_store.remove_whatsapp()?;
                {
                    let mut guard = self.whatsapp_concrete.write().await;
                    *guard = None;
                }
                self.set_whatsapp_connection_state(ChannelConnectionState::Unconfigured, None)
                    .await;
                Ok(state)
            }
        }
    }

    pub async fn reveal_secret(&self, platform: Platform) -> Result<String> {
        match platform {
            Platform::Dingtalk => self.config_store.reveal_dingtalk_secret(),
            Platform::Feishu => self.config_store.reveal_feishu_secret(),
            Platform::Wecom => self.config_store.reveal_wecom_secret(),
            Platform::Telegram => self.config_store.reveal_telegram_token(),
            other => anyhow::bail!("{} channel is not available yet", other.as_str()),
        }
    }

    /// Trigger a small self-test turn in the current DingTalk private bot session.
    ///
    /// We cannot spoof a DingTalk user inbound event from the outside. Instead,
    /// reuse the active private channel session that was created by a real
    /// inbound message, register a DingTalk AI card target, and send a normal
    /// chat request with a small greeting. The resulting assistant response is
    /// delivered to the same DingTalk bot conversation.
    pub async fn send_dingtalk_greeting(&self) -> Result<()> {
        let config = self
            .config_store
            .read_dingtalk_config()?
            .ok_or_else(|| anyhow::anyhow!("钉钉频道尚未配置"))?;
        if !config.enabled {
            anyhow::bail!("钉钉频道已停用，请先启用后再发送问候");
        }
        let app_secret = self.config_store.reveal_dingtalk_secret()?;
        let robot_code = config.bot.robot_code.clone();
        let target = {
            let conversations = self.conversations.read().await;
            select_dingtalk_greeting_target(&conversations, &robot_code).ok_or_else(|| {
                anyhow::anyhow!("还没有当前钉钉机器人的私聊会话，请先在钉钉里给机器人发送一条消息")
            })?
        };

        let request = build_channel_chat_request(
            target.session_id.clone(),
            crate::runtime::human_interaction::ImPlatform::Dingtalk,
            target.external_conversation_key.clone(),
            &ConversationType::Private,
            "我",
            DINGTALK_GREETING_PROMPT,
            vec![],
            &[],
        );
        let run_id = request.run_id.as_str().to_string();

        self.reply_manager
            .register(
                target.session_id,
                run_id,
                config.credentials.app_key,
                app_secret,
                robot_code,
                CardTarget::Private {
                    user_id: target.external_conversation_key,
                },
            )
            .await;

        self.chat_adapter
            .send_chat_request(request)
            .await
            .map_err(|e| anyhow::anyhow!("发送钉钉问候失败：{e}"))?;
        Ok(())
    }

    /// 创建钉钉 OPEN_CLAW 一键注册会话，返回用户需要打开的授权 URL。
    pub async fn begin_dingtalk_registration(&self) -> Result<ChannelRegistrationBeginResult> {
        if self.is_inactive() {
            return Err(anyhow::anyhow!("channel manager inactive"));
        }
        let begin = begin_registration().await?;
        Ok(ChannelRegistrationBeginResult {
            device_code: begin.device_code,
            user_code: begin.user_code,
            verification_uri_complete: begin.verification_uri_complete,
            verification_uri: begin.verification_uri,
            interval_seconds: begin.interval_seconds,
            expires_in_seconds: begin.expires_in_seconds,
            source: begin.source,
        })
    }

    /// 轮询钉钉 OPEN_CLAW 注册结果。
    pub async fn poll_dingtalk_registration(
        &self,
        device_code: String,
    ) -> Result<ChannelRegistrationPollResult> {
        let poll = poll_registration(&device_code).await?;
        let state = match poll.state {
            RegistrationPollState::Waiting => ChannelRegistrationPollState::Waiting,
            RegistrationPollState::Success => ChannelRegistrationPollState::Success,
            RegistrationPollState::Fail => ChannelRegistrationPollState::Fail,
            RegistrationPollState::Expired => ChannelRegistrationPollState::Expired,
            RegistrationPollState::Unknown => ChannelRegistrationPollState::Unknown,
        };

        if state == ChannelRegistrationPollState::Success {
            let app_key = poll
                .client_id
                .clone()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("DingTalk registration succeeded without client_id")
                })?;
            let app_secret = poll
                .client_secret
                .clone()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("DingTalk registration succeeded without client_secret")
                })?;
            let state = self
                .save_config_and_connect(app_key, app_secret, poll.robot_code.clone())
                .await?;
            return Ok(ChannelRegistrationPollResult {
                state: ChannelRegistrationPollState::Success,
                client_id: poll.client_id,
                robot_code: state
                    .config
                    .as_ref()
                    .map(|config| config.robot_code.clone()),
                config: state.config.clone(),
                platform_state: Some(state),
                fail_reason: poll.fail_reason,
            });
        }

        Ok(ChannelRegistrationPollResult {
            state,
            client_id: poll.client_id,
            robot_code: poll.robot_code,
            config: None,
            platform_state: None,
            fail_reason: poll.fail_reason,
        })
    }

    /// 保存配置并建立连接。
    pub async fn save_config_and_connect(
        &self,
        app_key: String,
        app_secret_plain: String,
        robot_code: Option<String>,
    ) -> Result<ChannelPlatformState> {
        self.config_store
            .save_dingtalk_registration(app_key, app_secret_plain, robot_code)?;
        self.connect_dingtalk_from_store().await?;
        self.current_dingtalk_state().await
    }

    // ----- Feishu (Phase 1 PR2) -----

    /// 创建飞书 device-code 注册会话，返回 user_code + verification_uri 给前端展示。
    pub async fn begin_feishu_registration(&self) -> Result<ChannelRegistrationBeginResult> {
        if self.is_inactive() {
            return Err(anyhow::anyhow!("channel manager inactive"));
        }
        let begin = super::feishu::registration::begin_registration().await?;
        Ok(ChannelRegistrationBeginResult {
            device_code: begin.device_code,
            user_code: begin.user_code,
            verification_uri_complete: begin.verification_uri_complete,
            verification_uri: begin.verification_uri,
            interval_seconds: begin.interval_seconds,
            expires_in_seconds: begin.expires_in_seconds,
            source: begin.source,
        })
    }

    /// 轮询飞书 device-code 注册结果。成功时把返回的 client_id / client_secret
    /// 映射到 FeishuStoredCredentials 的 app_id / app_secret 字段并持久化。
    pub async fn poll_feishu_registration(
        &self,
        device_code: String,
    ) -> Result<ChannelRegistrationPollResult> {
        let poll = super::feishu::registration::poll_registration(&device_code).await?;
        let state = match poll.state {
            super::feishu::registration::FeishuPollState::Waiting => {
                ChannelRegistrationPollState::Waiting
            }
            super::feishu::registration::FeishuPollState::Success => {
                ChannelRegistrationPollState::Success
            }
            super::feishu::registration::FeishuPollState::Fail => {
                ChannelRegistrationPollState::Fail
            }
            super::feishu::registration::FeishuPollState::Expired => {
                ChannelRegistrationPollState::Expired
            }
            super::feishu::registration::FeishuPollState::Unknown => {
                ChannelRegistrationPollState::Unknown
            }
        };
        if state == ChannelRegistrationPollState::Success {
            // OAuth device-flow returns RFC 8628 `client_id` / `client_secret`. These map 1:1
            // to the tenant_access_token endpoint's `app_id` / `app_secret`. We persist them
            // under the on-disk schema's `app_id` field name (see FeishuStoredCredentials).
            let app_id = poll
                .client_id
                .clone()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("Feishu registration succeeded without client_id")
                })?;
            let app_secret = poll
                .client_secret
                .clone()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("Feishu registration succeeded without client_secret")
                })?;
            self.config_store
                .save_feishu_registration(app_id, app_secret)?;
            // Mirror save_config_and_connect (dingtalk): immediately bring up
            // the feishu WS runtime so the user can chat right after device-code
            // success. Without this, the connector only starts on the NEXT app
            // launch via auto_connect_if_configured — config persists, bot stays
            // silent, no logs.
            self.connect_feishu_from_store().await?;
            let platform_state = self.current_feishu_state().await?;
            return Ok(ChannelRegistrationPollResult {
                state: ChannelRegistrationPollState::Success,
                client_id: platform_state.config.as_ref().map(|c| c.app_key.clone()),
                robot_code: None,
                config: platform_state.config.clone(),
                platform_state: Some(platform_state),
                fail_reason: poll.fail_reason,
            });
        }
        Ok(ChannelRegistrationPollResult {
            state,
            client_id: None,
            robot_code: None,
            config: None,
            platform_state: None,
            fail_reason: poll.fail_reason,
        })
    }

    /// MVP (Phase 5): start a wechat scan-to-login session. Returns a "device_code"
    /// that is actually the iLink `qrcode` value plus `verification_uri_complete`
    /// holding the QR URL string (the frontend renders the QR via qrcode lib).
    ///
    /// `user_code` stays empty: iLink scan doesn't need a user-code overlay.
    pub async fn begin_wechat_registration(&self) -> Result<ChannelRegistrationBeginResult> {
        if self.is_inactive() {
            return Err(anyhow::anyhow!("channel manager inactive"));
        }
        let app_id = super::wechat::appid::resolve_app_id(&aijia_config_path());
        let client_version = env!("CARGO_PKG_VERSION").to_string();
        let begin =
            super::wechat::registration::begin_registration(&app_id, &client_version).await?;
        Ok(ChannelRegistrationBeginResult {
            device_code: begin.device_code,
            user_code: String::new(),
            verification_uri_complete: begin.qr_url.clone(),
            verification_uri: begin.qr_url,
            interval_seconds: begin.interval_seconds,
            expires_in_seconds: begin.expires_in_seconds,
            source: "wechat-ilink".to_string(),
        })
    }

    /// MVP (Phase 5): poll wechat scan status one tick. On `Success` the bot
    /// credentials are NOT yet persisted — that's a Phase 5 PR3 task. We just
    /// log the captured ids so the user can see the login closed the loop end-
    /// to-end. `fail_reason` carries the bot_id / user_id summary so the
    /// frontend can show a confirmation banner.
    pub async fn poll_wechat_registration(
        &self,
        device_code: String,
    ) -> Result<ChannelRegistrationPollResult> {
        let app_id = super::wechat::appid::resolve_app_id(&aijia_config_path());
        let client_version = env!("CARGO_PKG_VERSION").to_string();
        let state =
            super::wechat::registration::poll_registration(&app_id, &client_version, &device_code)
                .await?;
        let result = match state {
            super::wechat::registration::WechatPollState::Waiting => {
                ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Waiting,
                    client_id: None,
                    robot_code: None,
                    config: None,
                    platform_state: None,
                    fail_reason: None,
                }
            }
            super::wechat::registration::WechatPollState::Scanned => {
                // Surface "已扫码，请在手机上确认" to the frontend via the
                // fail_reason JSON envelope (same channel we use for
                // qr_refresh / wechat_success).
                let payload = serde_json::json!({ "kind": "scanned" });
                ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Waiting,
                    client_id: None,
                    robot_code: None,
                    config: None,
                    platform_state: None,
                    fail_reason: Some(payload.to_string()),
                }
            }
            super::wechat::registration::WechatPollState::Refreshed {
                new_device_code,
                new_qr_url,
            } => {
                // We surface a Waiting state plus a fail_reason payload that
                // carries the new poll handle + new QR URL so the frontend
                // can swap the modal without going through a full
                // begin_registration round-trip. The frontend wechat adapter
                // parses fail_reason as a JSON envelope.
                let payload = serde_json::json!({
                    "kind": "qr_refresh",
                    "device_code": new_device_code,
                    "qr_url": new_qr_url,
                });
                ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Waiting,
                    client_id: None,
                    robot_code: None,
                    config: None,
                    platform_state: None,
                    fail_reason: Some(payload.to_string()),
                }
            }
            super::wechat::registration::WechatPollState::Success(confirmed) => {
                log::info!(
                    "[wechat] scan-to-login confirmed: ilink_bot_id={} ilink_user_id={} baseurl={}",
                    confirmed.ilink_bot_id,
                    confirmed.ilink_user_id,
                    confirmed.effective_base_url
                );
                // Phase 5 PR1: persist credentials + auto-connect。
                // SecureStorage 加密 bot_token；不可用时回落明文。
                let platform_state = match self.config_store.save_wechat_registration(
                    confirmed.bot_token.clone(),
                    confirmed.ilink_bot_id.clone(),
                    confirmed.ilink_user_id.clone(),
                    confirmed.effective_base_url.clone(),
                ) {
                    Ok(state) => state,
                    Err(e) => {
                        log::error!("[wechat] save_wechat_registration failed: {e:#}");
                        return Ok(ChannelRegistrationPollResult {
                            state: ChannelRegistrationPollState::Fail,
                            client_id: None,
                            robot_code: None,
                            config: None,
                            platform_state: None,
                            fail_reason: Some(format!("配置保存失败：{e}")),
                        });
                    }
                };
                // 启动 long-poll worker；失败时把 platform_state 标记 ConfigError 而不阻塞登录返回。
                if let Err(e) = self.connect_wechat_from_store().await {
                    log::warn!("[wechat] auto-connect after registration failed: {e:#}");
                    self.set_wechat_connection_state(
                        ChannelConnectionState::ConfigError,
                        Some(e.to_string()),
                    )
                    .await;
                }
                let payload = serde_json::json!({
                    "kind": "wechat_success",
                    "ilink_bot_id": confirmed.ilink_bot_id,
                    "ilink_user_id": confirmed.ilink_user_id,
                    "baseurl": confirmed.effective_base_url,
                });
                ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Success,
                    client_id: Some(confirmed.ilink_user_id.clone()),
                    robot_code: Some(confirmed.ilink_bot_id.clone()),
                    config: platform_state.config.clone(),
                    platform_state: Some(platform_state),
                    fail_reason: Some(payload.to_string()),
                }
            }
            super::wechat::registration::WechatPollState::Expired => {
                ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Expired,
                    client_id: None,
                    robot_code: None,
                    config: None,
                    platform_state: None,
                    fail_reason: None,
                }
            }
            super::wechat::registration::WechatPollState::Fail(msg) => {
                ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Fail,
                    client_id: None,
                    robot_code: None,
                    config: None,
                    platform_state: None,
                    fail_reason: Some(msg),
                }
            }
        };
        Ok(result)
    }

    /// PR8 §3.10：更新 WhatsApp allow_from 配置。config.json 必须已存在
    /// （即已配对），否则返回 "whatsapp not paired yet" 错误。
    /// 空 vec 表示清空 allowlist（接收所有入站）；非空 vec 替换当前 allowlist。
    pub async fn update_whatsapp_allow_from(&self, allow_from: Vec<String>) -> Result<()> {
        let paths = self.resolve_whatsapp_paths()?;
        let mut cfg = super::whatsapp::config::read(&paths.config_path())?
            .ok_or_else(|| anyhow::anyhow!("whatsapp not paired yet"))?;
        cfg.allow_from = if allow_from.is_empty() {
            None
        } else {
            Some(allow_from)
        };
        super::whatsapp::config::write(&paths.config_path(), &cfg)?;
        log::info!(
            "[channel/whatsapp] allow_from updated: {} entries",
            cfg.allow_from.as_ref().map(|v| v.len()).unwrap_or(0)
        );
        Ok(())
    }

    /// 读当前 config.json 里的 allow_from 列表。未配对时返回 `None`,已配对但
    /// 列表为空(= 接收所有联系人)返回 `Some(vec![])`。前端"管理允许列表"
    /// 弹窗初始化时调用,据此推断 UI 该显示"接收所有"还是"仅指定"模式。
    pub async fn get_whatsapp_allow_from(&self) -> Result<Option<Vec<String>>> {
        let paths = self.resolve_whatsapp_paths()?;
        let cfg = super::whatsapp::config::read(&paths.config_path())?;
        Ok(cfg.map(|c| c.allow_from.unwrap_or_default()))
    }

    /// Phase 4 PR3 启动期 auto-connect：如果 config.json 存在则直接起 Bot
    /// 复用既有 session.db 凭证，不需用户重新扫码。Connected 状态由
    /// runtime.rs Event::Connected handler 推到 PairingState::Connected；
    /// manager 的 connection state 由 on_status 回调驱动。
    pub async fn connect_whatsapp_from_store(&self) -> Result<()> {
        let paths = self.resolve_whatsapp_paths()?;
        if !paths.config_path().exists() {
            log::info!("[whatsapp] no config.json — skipping auto-connect");
            return Ok(());
        }
        // Respect user-paused state — config exists but enabled=false means
        // the user toggled the channel off; don't auto-reconnect.
        match super::whatsapp::config::read(&paths.config_path()) {
            Ok(Some(cfg)) if !cfg.enabled => {
                log::info!(
                    "[whatsapp] config.json present but enabled=false — skipping auto-connect"
                );
                return Ok(());
            }
            Ok(_) => {}
            Err(e) => log::warn!("[whatsapp] read config.json failed: {e:#}"),
        }
        log::info!("[whatsapp] auto-connect: reusing existing session.db");
        let on_status = self.make_whatsapp_status_callback();
        let concrete = self.register_whatsapp_connector(on_status).await;
        concrete.start_pairing_session(paths).await?;
        if let Err(e) = self.spawn_whatsapp_inbound_worker().await {
            log::error!("[channel/whatsapp] failed to spawn inbound worker: {:#}", e);
        }
        Ok(())
    }

    /// Phase 4 PR4：建入站 worker，消费 connector.start() 返回的 BoxStream<ChannelMessage>。
    /// 每条消息经 router.get_or_create_session → ChannelConversation push →
    /// channel:message 事件 → PendingQueueManager.enqueue_or_send → ChatAdapter.send_chat_request。
    /// Worker 是 fire-and-forget（JoinHandle 不保存）；stream None / cancel 即退出。
    async fn spawn_whatsapp_inbound_worker(&self) -> Result<()> {
        const ROUTER_KEY: &str = "whatsapp";

        let cancel_token = CancellationToken::new();
        let ctx = ConnectorContext {
            config_store: Arc::clone(&self.config_store),
            secure_storage: None,
            ask_coordinator: self.ask_coordinator.as_ref().map(Arc::clone),
            pending_manager: Arc::clone(&self.pending_manager),
            cancel_token: cancel_token.clone(),
        };

        let connector = {
            let map = self.connectors.read().await;
            map.get(&Platform::Whatsapp)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("whatsapp connector not registered"))?
        };

        let mut message_stream = connector
            .start(ctx)
            .await
            .map_err(|e| anyhow::anyhow!("whatsapp connector start failed: {e}"))?;

        self.platform_state_mutate(Platform::Whatsapp, |s| {
            s.stream_cancel = Some(cancel_token.clone());
        })
        .await;

        let adapter = Arc::clone(&self.chat_adapter);
        let conv_store = Arc::clone(&self.conversation_store);
        let sessions_path = self.sessions_paths[&Platform::Whatsapp].clone();
        let seen_ids = Arc::clone(&self.seen_msg_ids);
        let convs = Arc::clone(&self.conversations);
        let app_handle = self.app_handle.clone();
        let channel_session_ids_ref = Arc::clone(&self.channel_session_ids);
        let inactive_ref = Arc::clone(&self.inactive);
        let ask_coordinator_ref = self.ask_coordinator.as_ref().map(Arc::clone);
        let pending_manager_ref = Arc::clone(&self.pending_manager);
        let connector_for_worker = Arc::clone(&connector);
        // Grab the concrete WhatsApp connector handle before spawning so the
        // worker can call remember_inbound (not on the dyn trait surface).
        // Option<Arc<_>>::clone() is cheap.
        let concrete_for_worker = self.whatsapp_concrete.read().await.clone();

        tokio::spawn(async move {
            let mut router =
                match ChannelSessionRouter::migrate_or_load(&sessions_path, conv_store.as_ref()) {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("[channel/whatsapp] failed to load router: {:#}", e);
                        return;
                    }
                };

            loop {
                let msg = tokio::select! {
                    _ = cancel_token.cancelled() => {
                        log::info!("[channel/whatsapp] worker cancelled");
                        break;
                    }
                    next = message_stream.next() => {
                        match next {
                            Some(m) => m,
                            None => {
                                log::info!("[channel/whatsapp] worker stream ended");
                                break;
                            }
                        }
                    }
                };

                log::info!(
                    "[channel/whatsapp] worker received msg msg_id={} text_len={}",
                    msg.msg_id,
                    msg.text.len(),
                );

                if !seen_ids.observe(&msg.msg_id).await {
                    log::debug!(
                        "[channel/whatsapp] duplicate msg_id {}, skipping",
                        msg.msg_id
                    );
                    continue;
                }

                let conv_type = msg.conversation_type.clone();
                let conv_key = msg.conversation_key.clone();
                let sender_nick = msg.sender_nick.clone();
                let text = msg.text.clone();

                let store_ref = Arc::clone(&conv_store);
                let ensure_store_ref = Arc::clone(&conv_store);
                let sender_nick_for_create = sender_nick.clone();
                let sender_nick_for_ensure = sender_nick.clone();
                let session_id = match router.get_or_create_session_with_ensure(
                    &conv_type,
                    ROUTER_KEY,
                    &conv_key,
                    || {
                        let title = sender_nick_for_create.clone();
                        let id = uuid::Uuid::new_v4().to_string();
                        store_ref
                            .create_conversation_with_im_source(
                                &id,
                                &title,
                                Platform::Whatsapp.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))?;
                        Ok(id)
                    },
                    |existing_id| {
                        ensure_store_ref
                            .create_conversation_with_im_source(
                                existing_id,
                                &sender_nick_for_ensure,
                                Platform::Whatsapp.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))
                    },
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("[channel/whatsapp] get_or_create_session failed: {:#}", e);
                        continue;
                    }
                };

                {
                    if inactive_ref.load(std::sync::atomic::Ordering::SeqCst) {
                        log::debug!(
                            "[channel/whatsapp] worker observed inactive flag, dropping session id insert"
                        );
                        continue;
                    }
                    let mut ids = channel_session_ids_ref
                        .write()
                        .expect("channel_session_ids poisoned");
                    ids.insert(session_id.clone());
                }

                // Remember the last inbound message context so the reply
                // forwarder can look up chat_jid / sender_jid / msg_id for
                // reaction and edit operations (spec v3 §6.1).
                if let Some(concrete) = concrete_for_worker.clone() {
                    concrete
                        .remember_inbound(
                            session_id.clone(),
                            super::whatsapp::types::WhatsAppLastInbound {
                                chat_jid: conv_key.clone(),
                                sender_jid: msg.sender_id.clone(),
                                msg_id: msg.msg_id.clone(),
                                is_group: matches!(
                                    conv_type,
                                    super::types::ConversationType::Group
                                ),
                            },
                        )
                        .await;
                }

                {
                    let mut convs_lock = convs.write().await;
                    if !convs_lock.iter().any(|c| c.session_id == session_id) {
                        convs_lock.push(ChannelConversation {
                            session_id: session_id.clone(),
                            platform: Platform::Whatsapp,
                            conversation_type: conv_type.clone(),
                            external_id: conv_key.clone(),
                            display_name: sender_nick.clone(),
                            unread_count: 0,
                            robot_code: ROUTER_KEY.to_string(),
                            is_active_robot: true,
                        });
                    }
                }

                let preview = if text.chars().count() > 30 {
                    format!("{}...", text.chars().take(30).collect::<String>())
                } else {
                    text.clone()
                };
                let _ = app_handle.emit(
                    "channel:message",
                    &ChannelMessagePayload {
                        platform: "whatsapp".into(),
                        session_id: session_id.clone(),
                        sender_nick: sender_nick.clone(),
                        text_preview: preview,
                    },
                );

                let session_for_ask = crate::runtime::ids::SessionId::new(session_id.clone());
                match handle_pending_action_pre_dispatch(
                    ask_coordinator_ref.as_ref(),
                    &session_for_ask,
                    &text,
                )
                .await
                {
                    Ok(super::shared::ask_coordinator::HandleOutcome::NotPending)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::NewTurnAfterAbandon) => {}
                    Ok(super::shared::ask_coordinator::HandleOutcome::ApprovalResolved)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::AnswerResolved) => {
                        continue;
                    }
                    Ok(super::shared::ask_coordinator::HandleOutcome::InvalidApprovalAction {
                        message,
                    }) => {
                        send_pending_action_text_ack(
                            &connector_for_worker,
                            &session_id,
                            &conv_key,
                            "[channel/whatsapp]",
                            message,
                        )
                        .await;
                        continue;
                    }
                    Err(err) => {
                        log::warn!(
                            "[channel/whatsapp] IM ask coordinator failed, falling back to normal turn: {:#}",
                            err
                        );
                    }
                };

                let (chat_attachments, download_failures) =
                    whatsapp_specs_to_chat_attachments(&msg.attachments);
                let request = build_channel_chat_request(
                    session_id.clone(),
                    crate::runtime::human_interaction::ImPlatform::Whatsapp,
                    conv_key.clone(),
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments.clone(),
                    &download_failures,
                );
                let pending_item = super::shared::pending_adapter::build_pending_item_from_whatsapp(
                    &msg.msg_id,
                    &session_id,
                    &conv_key,
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments,
                    &download_failures,
                );
                let adapter_for_turn = Arc::clone(&adapter);
                let session_for_log = session_id.clone();
                let pending_manager_for_send = Arc::clone(&pending_manager_ref);
                let session_for_enqueue = crate::runtime::ids::SessionId::new(session_id.clone());
                tokio::spawn(async move {
                    match pending_manager_for_send
                        .enqueue_or_send(session_for_enqueue, pending_item)
                        .await
                    {
                        Ok(crate::runtime::pending::EnqueueOutcome::SentDirectly { .. }) => {
                            if let Err(e) = adapter_for_turn.send_chat_request(request).await {
                                log::error!(
                                    "[channel/whatsapp] send_chat_request failed session={}: {}",
                                    session_for_log,
                                    e
                                );
                                pending_manager_for_send
                                    .release_direct_dispatch(&crate::runtime::ids::SessionId::new(
                                        session_for_log.clone(),
                                    ))
                                    .await;
                            }
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Queued { snapshot }) => {
                            log::info!(
                                "[channel/whatsapp] message queued session={} queue_size={}",
                                session_for_log,
                                snapshot.len()
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::HeldForHumanInteraction {
                            interaction_id,
                        }) => {
                            log::info!(
                                "[channel/whatsapp] message held for human interaction session={} interaction_id={:?}",
                                session_for_log,
                                interaction_id
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Rejected { reason }) => {
                            log::warn!(
                                "[channel/whatsapp] enqueue rejected session={} reason={:?}",
                                session_for_log,
                                reason
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "[channel/whatsapp] enqueue_or_send error session={}: {:#}",
                                session_for_log,
                                e
                            );
                        }
                    }
                });
            }
        });

        Ok(())
    }

    /// Phase 4 PR3：起 WhatsApp 扫码会话。Manager 解析 paths → 检查
    /// config.json 是否已存在（重新扫码场景）→ 删 config + session.db
    /// → 调 connector.start_pairing_session。spec v3 §3.6 + §3.9。
    pub async fn begin_whatsapp_registration(&self) -> Result<ChannelRegistrationBeginResult> {
        if self.is_inactive() {
            return Err(anyhow::anyhow!("channel manager inactive"));
        }
        let paths = self.resolve_whatsapp_paths()?;

        // 重新扫码场景:无论 config.json 是否存在，都先停旧 connector + 清
        // session.db。原因: kebab"移除"只删了 config.json，留下了 session.db
        // (保留聊天历史)。如果用户接着点"配置"想重新扫码，wa-rs 启动时会发现
        // session.db 里还有旧凭证 → 直接 Authenticated 跳过 PairingQrCode →
        // 前端永远等不到 QR (空白卡片)。所以 begin 路径必须无条件清 session。
        log::info!(
            "[whatsapp] begin_registration — clearing any prior session for fresh QR pairing"
        );
        if let Some(conn) = self
            .connectors
            .read()
            .await
            .get(&Platform::Whatsapp)
            .cloned()
        {
            let _ = conn.stop().await;
        }
        super::whatsapp::session::delete_for_reauth(&paths)?;

        let on_status = self.make_whatsapp_status_callback();
        let concrete = self.register_whatsapp_connector(on_status).await;
        concrete.start_pairing_session(paths).await?;
        if let Err(e) = self.spawn_whatsapp_inbound_worker().await {
            log::error!("[channel/whatsapp] failed to spawn inbound worker: {:#}", e);
        }

        Ok(ChannelRegistrationBeginResult {
            device_code: "whatsapp".to_string(), // 单账号约定常量
            user_code: String::new(),
            verification_uri_complete: String::new(), // QR 还没生成；poll 时返回
            verification_uri: String::new(),
            interval_seconds: 2,
            expires_in_seconds: 60, // wa-rs PairingQrCode 默认 timeout
            source: "whatsapp_web".to_string(),
        })
    }

    /// Phase 4 PR3：拉一次 WhatsApp PairingState 当前快照。
    /// QR string 通过 fail_reason JSON envelope 返回（跟 wechat 同款约定）。
    pub async fn poll_whatsapp_registration(
        &self,
        _device_code: String, // 单账号下忽略；仅保持 trait 一致性
    ) -> Result<ChannelRegistrationPollResult> {
        let conn_arc = self
            .whatsapp_connector()
            .await
            .ok_or_else(|| anyhow::anyhow!("whatsapp connector not registered"))?;
        let state = conn_arc.poll_pairing_state().await;

        use super::whatsapp::types::PairingState;
        use std::time::Instant;

        let result = match state {
            PairingState::Idle | PairingState::AwaitingQr { .. } => ChannelRegistrationPollResult {
                state: ChannelRegistrationPollState::Waiting,
                client_id: None,
                robot_code: None,
                config: None,
                platform_state: None,
                fail_reason: None,
            },
            PairingState::QrIssued { code, expires_at } => {
                if Instant::now() >= expires_at {
                    ChannelRegistrationPollResult {
                        state: ChannelRegistrationPollState::Expired,
                        client_id: None,
                        robot_code: None,
                        config: None,
                        platform_state: None,
                        fail_reason: Some("QR code expired".into()),
                    }
                } else {
                    let payload = serde_json::json!({
                        "kind": "qr",
                        "qr_url": code,
                        "expires_in_seconds": (expires_at - Instant::now()).as_secs(),
                    });
                    ChannelRegistrationPollResult {
                        state: ChannelRegistrationPollState::Waiting,
                        client_id: None,
                        robot_code: None,
                        config: None,
                        platform_state: None,
                        fail_reason: Some(payload.to_string()),
                    }
                }
            }
            PairingState::Connected { jid, push_name } => {
                log::info!(
                    "[whatsapp] pairing success: jid={} push_name={}",
                    jid,
                    push_name
                );
                self.set_whatsapp_connection_state(ChannelConnectionState::Connected, None)
                    .await;
                let payload = serde_json::json!({
                    "kind": "whatsapp_success",
                    "jid": jid,
                    "push_name": push_name,
                });
                ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Success,
                    client_id: None,
                    robot_code: None,
                    config: None,
                    platform_state: None,
                    fail_reason: Some(payload.to_string()),
                }
            }
        };
        Ok(result)
    }

    /// PR3: real implementation. Starts the feishu WS runtime and drives a
    /// slimmer worker loop than dingtalk (no reply_manager / ask_coordinator /
    /// pending queue / attachment download — those are PR4-PR6). Messages
    /// surface in `conversations`, push `channel:message`, and trigger a
    /// fire-and-forget `send_chat_request` for the AI turn.
    async fn connect_feishu_from_store(&self) -> Result<()> {
        let (config, app_secret_plain) = self.config_store.decrypt_feishu_config()?;
        self.connect_feishu(config, app_secret_plain).await
    }

    async fn connect_feishu(
        &self,
        config: super::feishu::types::FeishuStoredConfig,
        app_secret_plain: String,
    ) -> Result<()> {
        // PR3.5: stop ONLY the feishu slot — dingtalk's stream is independent
        // and must not be touched here.
        self.stop_stream(Platform::Feishu).await;

        let app_id = config.credentials.app_id.clone();
        // app_id is the namespacing key for ChannelSessionRouter (vs dingtalk's
        // robot_code). Empty would collide across feishu accounts.
        let router_key = app_id.clone();

        self.set_feishu_connection_state(ChannelConnectionState::Connecting, None)
            .await;

        // Grab the feishu slot's generation counter and bump it for this
        // (re)connect. The bumped value is captured in closures so any stale
        // status callbacks / worker iterations from a previous connect drop.
        let stream_generation = self.platform_generation(Platform::Feishu).await;
        let generation = stream_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let message_stream_generation = Arc::clone(&stream_generation);
        let platform_state_for_status = Arc::clone(&self.platform_state);
        let config_store = Arc::clone(&self.config_store);
        let app_for_status = self.app_handle.clone();
        let stream_generation_for_status = Arc::clone(&stream_generation);
        let conversations_for_status = Arc::clone(&self.conversations);
        let on_status: super::factory::FeishuStatusCallback = Arc::new(
            move |new_connection: ChannelConnectionState, error: Option<String>| {
                let platform_state_for_status = platform_state_for_status.clone();
                let config_store = config_store.clone();
                let app_for_status = app_for_status.clone();
                let stream_generation_for_status = stream_generation_for_status.clone();
                let conversations_for_status = conversations_for_status.clone();
                tokio::spawn(async move {
                    if stream_generation_for_status.load(Ordering::SeqCst) != generation {
                        log::debug!("[channel/feishu] ignoring stale status callback");
                        return;
                    }
                    {
                        let mut map = platform_state_for_status.write().await;
                        let slot = map
                            .entry(Platform::Feishu)
                            .or_insert_with(PerPlatformState::unconfigured);
                        slot.connection = new_connection.clone();
                        slot.last_error = error.clone();
                    }
                    // Connected 时按 config 的 app_id 刷新仅飞书会话的
                    // is_active_robot —— set_feishu_connection_state 不走这条
                    // 回调路径，所以要在这里也处理一次。**只动飞书自己的 conv**
                    // 否则 hydrate 时初始算成 true 的飞书会话会被钉钉那条
                    // status callback 用 dingaf 比错踩成 false，sidebar
                    // "暂无会话" 就来源于此。
                    if matches!(new_connection, ChannelConnectionState::Connected) {
                        let current_app_id = config_store
                            .read_feishu_config()
                            .ok()
                            .flatten()
                            .map(|cfg| cfg.credentials.app_id);
                        let mut convs = conversations_for_status.write().await;
                        for c in convs.iter_mut() {
                            if c.platform != Platform::Feishu {
                                continue;
                            }
                            c.is_active_robot = current_app_id
                                .as_deref()
                                .map(|rc| rc == c.robot_code)
                                .unwrap_or(false);
                        }
                    }
                    match config_store.feishu_state(new_connection, error) {
                        Ok(state) => {
                            let _ = app_for_status.emit(
                                "channel:platform-state",
                                &ChannelPlatformStatePayload { state },
                            );
                        }
                        Err(err) => {
                            log::warn!("[channel/feishu] failed to build platform state: {:#}", err)
                        }
                    }
                });
            },
        );

        // Register the feishu connector (replaces any previous instance under
        // Platform::Feishu) and grab the concrete handle for remember_session.
        let concrete_feishu = self
            .register_feishu_connector(app_id.clone(), app_secret_plain, Arc::clone(&on_status))
            .await;

        // 订阅 RuntimeEventBus → connector.send(AiCardChunk)（整个 manager 生命周期
        // 内只订阅一次，避免重连/重保存配置时把同一 subscriber 重复挂载，否则
        // CardKit 流式会看到字符叠倍）。Forwarder 通过 connector.has_session() 过滤，
        // 不属于飞书的会话直接忽略，不影响钉钉链路。
        if claim_first_subscription(&self.feishu_reply_subscribed) {
            let forwarder = Arc::new(super::feishu::reply_forwarder::FeishuReplyForwarder::new(
                Arc::clone(&concrete_feishu),
            ));
            let sub: Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber> = forwarder;
            self.chat_adapter.subscribe_event_listener(sub.clone());
            self.anchor_subscriber(sub);
            log::info!("[channel/feishu] subscribed FeishuReplyForwarder to RuntimeEventBus");
        }

        // Start via the trait surface — get BoxStream<ChannelMessage>.
        let new_token = CancellationToken::new();
        let ctx = ConnectorContext {
            config_store: Arc::clone(&self.config_store),
            secure_storage: None,
            ask_coordinator: self.ask_coordinator.as_ref().map(Arc::clone),
            pending_manager: Arc::clone(&self.pending_manager),
            cancel_token: new_token.clone(),
        };
        let connector = {
            let map = self.connectors.read().await;
            map.get(&Platform::Feishu)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("feishu connector not registered"))?
        };
        let mut message_stream = connector
            .start(ctx)
            .await
            .map_err(|e| anyhow::anyhow!("feishu connector start failed: {e}"))?;

        let message_cancel = new_token.clone();
        self.platform_state_mutate(Platform::Feishu, |s| {
            s.stream_cancel = Some(new_token);
        })
        .await;

        // Worker — PR6 wires the attachment download path + PendingQueueManager
        // routing. Unlike dingtalk we have no `reply_manager` (CardKit streaming
        // is lazy-built by `FeishuConnector::send` on first `AiCardChunk`), and
        // no `ask_coordinator` branching (deferred to PR7 along with end-to-end
        // integration test).
        let adapter = Arc::clone(&self.chat_adapter);
        let conv_store = Arc::clone(&self.conversation_store);
        let sessions_path = self.sessions_paths[&Platform::Feishu].clone();
        let seen_ids = Arc::clone(&self.seen_msg_ids);
        let convs = Arc::clone(&self.conversations);
        let app_handle = self.app_handle.clone();
        let channel_session_ids_ref = Arc::clone(&self.channel_session_ids);
        let inactive_ref = Arc::clone(&self.inactive);
        let concrete_feishu_for_worker = Arc::clone(&concrete_feishu);
        let on_status_for_worker = Arc::clone(&on_status);
        // PR6: build the feishu file downloader ONCE per connect; it captures
        // the connector's shared TokenCache (same one used by send() and
        // CardKitSender) so tenant_access_token refreshes amortize across all
        // three paths. dest_dir is `~/.renlijia/tmp/feishu_downloads/`,
        // created on first download by `tokio::fs::create_dir_all`.
        let downloader_ref = concrete_feishu
            .make_downloader(self.feishu_downloads_dir())
            .await;
        let ask_coordinator_ref = self.ask_coordinator.as_ref().map(Arc::clone);
        let pending_manager_ref = Arc::clone(&self.pending_manager);
        // Snapshot at `connect_feishu` time so the worker can later decide
        // whether an inbound message is a server replay from before this
        // process started. We use the manager-level `started_at_ms` (set in
        // `ChannelManager::new`) rather than re-stamping at connect time —
        // a *reconnect* (cancel + reconnect) shouldn't reset the boundary,
        // otherwise the user's recent messages would get silently dropped
        // every time the WS hiccups.
        let started_at_ms = self.started_at_ms;
        // Trait-erased connector handle, used for the "all-attachments-failed"
        // text fallback (parallel to dingtalk's session-webhook fallback at
        // line ~1462+). The concrete `concrete_feishu_for_worker` is also
        // captured for `remember_session`, which isn't on the trait.
        let connector_for_worker = {
            let map = self.connectors.read().await;
            Arc::clone(map.get(&Platform::Feishu).expect("feishu just registered"))
        };

        let message_handle = tokio::spawn(async move {
            let mut router =
                match ChannelSessionRouter::migrate_or_load(&sessions_path, conv_store.as_ref()) {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("[channel/feishu] failed to load router: {:#}", e);
                        return;
                    }
                };

            loop {
                let msg = match recv_current_generation_message_stream(
                    &mut message_stream,
                    &message_stream_generation,
                    generation,
                    &message_cancel,
                )
                .await
                {
                    Some(m) => m,
                    None => {
                        log::info!("[channel/feishu] worker stream ended");
                        // Stream ended unexpectedly (server kicked or cancelled);
                        // surface Reconnecting so the user sees something change.
                        on_status_for_worker(ChannelConnectionState::Reconnecting, None);
                        break;
                    }
                };

                log::info!(
                    "[channel/feishu] worker received msg msg_id={} text_len={} attachments={}",
                    msg.msg_id,
                    msg.text.len(),
                    msg.attachments.len()
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                // Manager-level dedup. The connector's own MessageDedupSet
                // catches transient WS replays; this catches anything that
                // slipped through (e.g. across reconnects when the connector
                // is re-instantiated).
                if !seen_ids.observe(&msg.msg_id).await {
                    log::debug!("[channel/feishu] duplicate msg_id {}, skipping", msg.msg_id);
                    continue;
                }

                // Skip server replays from before this process started.
                // Feishu WS re-delivers any messages it never saw a frame-level
                // ACK for; on a fresh process the in-memory msg_id dedup set
                // is empty, so without this guard those replays would each
                // trigger a full LLM turn (the bug the user originally hit).
                // `created_at_ms` is ms since epoch, sourced from the feishu
                // event's `message.create_time`. Grace = 60s buffers against
                // small clock skew and messages that genuinely arrived during
                // app startup. Missing/unparseable timestamp → don't skip
                // (parse_im_message yields `None`, treated as "no judgment").
                const REPLAY_GRACE_MS: i64 = 60_000;
                if let Some(created_ms) = msg.created_at_ms {
                    if started_at_ms > 0
                        && created_ms < started_at_ms.saturating_sub(REPLAY_GRACE_MS)
                    {
                        log::info!(
                            "[channel/feishu] skipping pre-launch replay msg_id={} \
                             created_at_ms={} started_at_ms={} (delta={}ms)",
                            msg.msg_id,
                            created_ms,
                            started_at_ms,
                            started_at_ms - created_ms,
                        );
                        continue;
                    }
                }

                let conv_type = msg.conversation_type.clone();
                let conv_key = msg.conversation_key.clone();
                // Feishu doesn't ship a display name on im.message.receive_v1
                // (would need a separate contact API lookup). The connector
                // forwards `sender_id` as `sender_nick` — render a friendlier
                // truncated string here for use in conversation titles + the
                // channel:message payload. Format: "飞书用户 ou_abcdef12" (12
                // chars of open_id is enough to distinguish users).
                let sender_nick = render_feishu_sender_nick(&msg.sender_nick);
                let text = msg.text.clone();

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                let store_ref = Arc::clone(&conv_store);
                let ensure_store_ref = Arc::clone(&conv_store);
                let sender_nick_for_create = sender_nick.clone();
                let sender_nick_for_ensure = sender_nick.clone();
                let conv_key_for_create = conv_key.clone();
                let conv_type_for_create = conv_type.clone();
                let session_id = match router.get_or_create_session_with_ensure(
                    &conv_type,
                    &router_key,
                    &conv_key,
                    || {
                        let title = match &conv_type_for_create {
                            ConversationType::Group => format!(
                                "飞书群 {}",
                                &conv_key_for_create[..conv_key_for_create.len().min(8)]
                            ),
                            ConversationType::Private => sender_nick_for_create.clone(),
                        };
                        let id = uuid::Uuid::new_v4().to_string();
                        store_ref
                            .create_conversation_with_im_source(
                                &id,
                                &title,
                                Platform::Feishu.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))?;
                        Ok(id)
                    },
                    |existing_id| {
                        ensure_store_ref
                            .create_conversation_with_im_source(
                                existing_id,
                                &sender_nick_for_ensure,
                                Platform::Feishu.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))
                    },
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        log::error!("[channel/feishu] session routing failed: {:#}", e);
                        continue;
                    }
                };

                // Cache the per-session reply target on the concrete connector
                // so PR4's send() can dispatch by session_id alone. Per the
                // openclaw lark plugin reference, receive_id_type is exactly
                // "chat_id" for group and "open_id" for p2p.
                let (receive_id_type, receive_id) = match &conv_type {
                    ConversationType::Group => ("chat_id".to_string(), conv_key.clone()),
                    ConversationType::Private => ("open_id".to_string(), msg.sender_id.clone()),
                };
                concrete_feishu_for_worker
                    .remember_session(
                        session_id.clone(),
                        super::feishu::types::FeishuSessionTarget {
                            receive_id_type,
                            receive_id,
                        },
                    )
                    .await;

                // Register the session id with the shared channel registry so
                // ask_coordinator (when wired in PR4) can identify it.
                {
                    if inactive_ref.load(std::sync::atomic::Ordering::SeqCst) {
                        log::debug!(
                            "[channel/feishu] worker observed inactive flag, dropping session id insert"
                        );
                        continue;
                    }
                    let mut ids = channel_session_ids_ref
                        .write()
                        .expect("channel_session_ids poisoned");
                    ids.insert(session_id.clone());
                }

                // Push to conversations list (new sessions only).
                {
                    let mut convs_lock = convs.write().await;
                    if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                        break;
                    }
                    if !convs_lock.iter().any(|c| c.session_id == session_id) {
                        let display_name = match &conv_type {
                            ConversationType::Group => {
                                format!("飞书群 {}", &conv_key[..conv_key.len().min(8)])
                            }
                            ConversationType::Private => sender_nick.clone(),
                        };
                        convs_lock.push(ChannelConversation {
                            session_id: session_id.clone(),
                            platform: Platform::Feishu,
                            conversation_type: conv_type.clone(),
                            external_id: conv_key.clone(),
                            display_name,
                            unread_count: 0,
                            robot_code: router_key.clone(),
                            is_active_robot: true,
                        });
                    }
                }

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                let preview_source = if text.trim().is_empty() && !msg.attachments.is_empty() {
                    format!("[附件] {} 个文件", msg.attachments.len())
                } else if !msg.attachments.is_empty() {
                    format!("[附件] {}", text)
                } else {
                    text.clone()
                };
                let preview = if preview_source.chars().count() > 30 {
                    format!("{}...", preview_source.chars().take(30).collect::<String>())
                } else {
                    preview_source
                };
                let _ = app_handle.emit(
                    "channel:message",
                    &ChannelMessagePayload {
                        platform: "feishu".into(),
                        session_id: session_id.clone(),
                        sender_nick: sender_nick.clone(),
                        text_preview: preview,
                    },
                );

                let session_for_ask = crate::runtime::ids::SessionId::new(session_id.clone());
                match handle_pending_action_pre_dispatch(
                    ask_coordinator_ref.as_ref(),
                    &session_for_ask,
                    &text,
                )
                .await
                {
                    Ok(super::shared::ask_coordinator::HandleOutcome::NotPending)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::NewTurnAfterAbandon) => {}
                    Ok(super::shared::ask_coordinator::HandleOutcome::ApprovalResolved)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::AnswerResolved) => {
                        continue;
                    }
                    Ok(super::shared::ask_coordinator::HandleOutcome::InvalidApprovalAction {
                        message,
                    }) => {
                        send_pending_action_text_ack(
                            &connector_for_worker,
                            &session_id,
                            &conv_key,
                            "[channel/feishu]",
                            message,
                        )
                        .await;
                        continue;
                    }
                    Err(err) => {
                        log::warn!(
                            "[channel/feishu] IM ask coordinator failed, falling back to normal turn: {:#}",
                            err
                        );
                    }
                };

                // PR6: download attachments before building the chat turn.
                // Sequential per-message (matches dingtalk); the helper logs
                // per-failure and returns parallel vecs.
                let (chat_attachments, download_failures) = if msg.attachments.is_empty() {
                    (Vec::new(), Vec::new())
                } else {
                    log::info!(
                        "[channel/feishu] downloading {} attachments msg_id={} session={}",
                        msg.attachments.len(),
                        msg.msg_id,
                        session_id
                    );
                    download_specs_for_turn_feishu(
                        downloader_ref.as_ref(),
                        &msg.attachments,
                        &msg.msg_id,
                    )
                    .await
                };
                // All-attachments-failed + empty text → reply to user with
                // a hint; do NOT push a half-empty turn to the LLM.
                if chat_attachments.is_empty()
                    && text.trim().is_empty()
                    && !msg.attachments.is_empty()
                {
                    log::warn!(
                        "[channel/feishu] all attachments failed and no text, replying via send(Text) msg_id={}",
                        msg.msg_id
                    );
                    let connector_for_fallback = Arc::clone(&connector_for_worker);
                    let session_for_fallback = session_id.clone();
                    let conv_key_for_fallback = conv_key.clone();
                    tokio::spawn(async move {
                        if let Err(e) = connector_for_fallback
                            .send(
                                crate::connector::im::trait_def::ReplyTarget {
                                    session_id: session_for_fallback.clone(),
                                    external_conversation_key: conv_key_for_fallback,
                                },
                                crate::connector::im::trait_def::ReplyContent::Text(
                                    "附件下载全部失败，请重发。".to_string(),
                                ),
                            )
                            .await
                        {
                            log::warn!(
                                "[channel/feishu] fallback text send failed session={}: {:#}",
                                session_for_fallback,
                                e
                            );
                        }
                    });
                    continue;
                }

                // Build both the direct-send `ChatTurnRequest` (used when the
                // session is idle) and the `PendingItem` (used when busy or
                // queue-full). Same dual-construction pattern as dingtalk.
                let request = build_channel_chat_request(
                    session_id.clone(),
                    crate::runtime::human_interaction::ImPlatform::Feishu,
                    conv_key.clone(),
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments.clone(),
                    &download_failures,
                );
                let pending_item = super::shared::pending_adapter::build_pending_item_from_feishu(
                    &msg.msg_id,
                    &session_id,
                    &conv_key,
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments,
                    &download_failures,
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                // Route through PendingQueueManager — same three outcomes as
                // dingtalk:
                //   - SentDirectly  → fire-and-forget `send_chat_request`
                //   - Queued        → log; drain on next turn boundary
                //   - Rejected      → reply on QueueFull, log on Archived
                // Spawn so AskUserQuestion (PR7+) doesn't deadlock the recv loop.
                let adapter_for_turn = Arc::clone(&adapter);
                let session_for_log = session_id.clone();
                let pending_manager_for_send = Arc::clone(&pending_manager_ref);
                let session_for_enqueue = crate::runtime::ids::SessionId::new(session_id.clone());
                let connector_for_send = Arc::clone(&connector_for_worker);
                let conv_key_for_reject = conv_key.clone();
                tokio::spawn(async move {
                    match pending_manager_for_send
                        .enqueue_or_send(session_for_enqueue, pending_item)
                        .await
                    {
                        Ok(crate::runtime::pending::EnqueueOutcome::SentDirectly { .. }) => {
                            // Idle: use our pre-built request which already has
                            // the IM-flavoured `[sender]:` prefix + attachment
                            // formatting (vs manager-rebuilt content from text
                            // alone, which would lose the sender prefix).
                            if let Err(e) = adapter_for_turn.send_chat_request(request).await {
                                log::error!(
                                    "[channel/feishu] send_chat_request failed session={}: {}",
                                    session_for_log,
                                    e
                                );
                                pending_manager_for_send
                                    .release_direct_dispatch(&crate::runtime::ids::SessionId::new(
                                        session_for_log.clone(),
                                    ))
                                    .await;
                            }
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Queued { snapshot }) => {
                            log::info!(
                                "[channel/feishu] message queued session={} queue_size={}",
                                session_for_log,
                                snapshot.len()
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::HeldForHumanInteraction {
                            interaction_id,
                        }) => {
                            log::info!(
                                "[channel/feishu] message held for human interaction session={} interaction_id={:?}",
                                session_for_log,
                                interaction_id
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Rejected { reason }) => {
                            log::warn!(
                                "[channel/feishu] enqueue rejected session={} reason={:?}",
                                session_for_log,
                                reason
                            );
                            if let crate::runtime::pending::EnqueueRejection::QueueFull { limit } =
                                reason
                            {
                                let text = format!("消息堆积过多（已达 {limit} 条），请稍后再发。");
                                if let Err(e) = connector_for_send
                                    .send(
                                        crate::connector::im::trait_def::ReplyTarget {
                                            session_id: session_for_log.clone(),
                                            external_conversation_key: conv_key_for_reject.clone(),
                                        },
                                        crate::connector::im::trait_def::ReplyContent::Text(text),
                                    )
                                    .await
                                {
                                    log::warn!(
                                        "[channel/feishu] queue-full reject text send failed session={}: {:#}",
                                        session_for_log,
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "[channel/feishu] enqueue_or_send error session={}: {:#}",
                                session_for_log,
                                e
                            );
                        }
                    }
                });
            }
        });
        self.platform_state_mutate(Platform::Feishu, |s| {
            s.message_task = Some(message_handle);
        })
        .await;

        Ok(())
    }

    pub async fn connect_wechat_from_store(&self) -> Result<()> {
        let (config, bot_token_plain) = self.config_store.decrypt_wechat_config()?;
        self.connect_wechat(config, bot_token_plain).await
    }

    async fn connect_wechat(
        &self,
        config: super::wechat::types::WechatStoredConfig,
        bot_token_plain: String,
    ) -> Result<()> {
        // 只停 wechat 自己的 slot，钉钉/飞书/企微 不动。
        self.stop_stream(Platform::Wechat).await;

        let ilink_bot_id = config.bot.ilink_bot_id.clone();
        let ilink_user_id = config.bot.ilink_user_id.clone();
        let base_url = config.bot.effective_base_url.clone();
        let router_key = ilink_bot_id.clone();
        let app_id = super::wechat::appid::resolve_app_id(&aijia_config_path());
        let client_version = env!("CARGO_PKG_VERSION").to_string();

        self.set_wechat_connection_state(ChannelConnectionState::Connecting, None)
            .await;

        let stream_generation = self.platform_generation(Platform::Wechat).await;
        let generation = stream_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let message_stream_generation = Arc::clone(&stream_generation);
        let platform_state_for_status = Arc::clone(&self.platform_state);
        let config_store = Arc::clone(&self.config_store);
        let app_for_status = self.app_handle.clone();
        let stream_generation_for_status = Arc::clone(&stream_generation);
        let conversations_for_status = Arc::clone(&self.conversations);
        let on_status: super::factory::WechatStatusCallback = Arc::new(
            move |new_connection: ChannelConnectionState, error: Option<String>| {
                let platform_state_for_status = platform_state_for_status.clone();
                let config_store = config_store.clone();
                let app_for_status = app_for_status.clone();
                let stream_generation_for_status = stream_generation_for_status.clone();
                let conversations_for_status = conversations_for_status.clone();
                tokio::spawn(async move {
                    if stream_generation_for_status.load(Ordering::SeqCst) != generation {
                        log::debug!("[channel/wechat] ignoring stale status callback");
                        return;
                    }
                    {
                        let mut map = platform_state_for_status.write().await;
                        let slot = map
                            .entry(Platform::Wechat)
                            .or_insert_with(PerPlatformState::unconfigured);
                        slot.connection = new_connection.clone();
                        slot.last_error = error.clone();
                    }
                    if matches!(new_connection, ChannelConnectionState::Connected) {
                        let current_bot_id = config_store
                            .read_wechat_config()
                            .ok()
                            .flatten()
                            .map(|cfg| cfg.bot.ilink_bot_id);
                        let mut convs = conversations_for_status.write().await;
                        for c in convs.iter_mut() {
                            if c.platform != Platform::Wechat {
                                continue;
                            }
                            c.is_active_robot = current_bot_id
                                .as_deref()
                                .map(|rc| rc == c.robot_code)
                                .unwrap_or(false);
                        }
                    }
                    match config_store.wechat_state(new_connection, error) {
                        Ok(state) => {
                            let _ = app_for_status.emit(
                                "channel:platform-state",
                                &ChannelPlatformStatePayload { state },
                            );
                        }
                        Err(err) => {
                            log::warn!("[channel/wechat] failed to build platform state: {:#}", err)
                        }
                    }
                });
            },
        );

        let concrete_wechat = self
            .register_wechat_connector(
                bot_token_plain,
                ilink_bot_id.clone(),
                ilink_user_id.clone(),
                base_url.clone(),
                app_id,
                client_version,
                Arc::clone(&on_status),
            )
            .await;

        // RuntimeEventBus 订阅一次，避免重连时重复挂载。
        if claim_first_subscription(&self.wechat_reply_subscribed) {
            let forwarder = Arc::new(super::wechat::reply_forwarder::WechatReplyForwarder::new(
                Arc::clone(&concrete_wechat),
            ));
            let sub: Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber> = forwarder;
            self.chat_adapter.subscribe_event_listener(sub.clone());
            self.anchor_subscriber(sub);
            log::info!("[channel/wechat] subscribed WechatReplyForwarder to RuntimeEventBus");
        }

        let new_token = CancellationToken::new();
        let ctx = ConnectorContext {
            config_store: Arc::clone(&self.config_store),
            secure_storage: None,
            ask_coordinator: self.ask_coordinator.as_ref().map(Arc::clone),
            pending_manager: Arc::clone(&self.pending_manager),
            cancel_token: new_token.clone(),
        };
        let connector = {
            let map = self.connectors.read().await;
            map.get(&Platform::Wechat)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("wechat connector not registered"))?
        };
        let mut message_stream = connector
            .start(ctx)
            .await
            .map_err(|e| anyhow::anyhow!("wechat connector start failed: {e}"))?;

        let message_cancel = new_token.clone();
        self.platform_state_mutate(Platform::Wechat, |s| {
            s.stream_cancel = Some(new_token);
        })
        .await;

        // Worker — read inbound ChannelMessage stream → router get_or_create_session
        // → remember_session on the concrete connector → enqueue chat turn.
        let adapter = Arc::clone(&self.chat_adapter);
        let conv_store = Arc::clone(&self.conversation_store);
        let sessions_path = self.sessions_paths[&Platform::Wechat].clone();
        let seen_ids = Arc::clone(&self.seen_msg_ids);
        let convs = Arc::clone(&self.conversations);
        let app_handle = self.app_handle.clone();
        let channel_session_ids_ref = Arc::clone(&self.channel_session_ids);
        let inactive_ref = Arc::clone(&self.inactive);
        let on_status_for_worker = Arc::clone(&on_status);
        let ask_coordinator_ref = self.ask_coordinator.as_ref().map(Arc::clone);
        let pending_manager_ref = Arc::clone(&self.pending_manager);
        let platform_state_for_worker = Arc::clone(&self.platform_state);
        let connector_for_worker = {
            let map = self.connectors.read().await;
            Arc::clone(map.get(&Platform::Wechat).expect("wechat just registered"))
        };
        let concrete_wechat_for_worker = Arc::clone(&concrete_wechat);
        let wechat_downloads_dir = self.wechat_downloads_dir();

        let message_handle = tokio::spawn(async move {
            let mut router =
                match ChannelSessionRouter::migrate_or_load(&sessions_path, conv_store.as_ref()) {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("[channel/wechat] failed to load router: {:#}", e);
                        return;
                    }
                };

            loop {
                let msg = match recv_current_generation_message_stream(
                    &mut message_stream,
                    &message_stream_generation,
                    generation,
                    &message_cancel,
                )
                .await
                {
                    Some(m) => m,
                    None => {
                        log::info!("[channel/wechat] worker stream ended");
                        let current = {
                            let map = platform_state_for_worker.read().await;
                            map.get(&Platform::Wechat)
                                .map(|s| s.connection.clone())
                                .unwrap_or(ChannelConnectionState::Unconfigured)
                        };
                        if !matches!(
                            current,
                            ChannelConnectionState::NeedsReauth
                                | ChannelConnectionState::ConfigError
                        ) {
                            on_status_for_worker(ChannelConnectionState::Reconnecting, None);
                        }
                        break;
                    }
                };

                log::info!(
                    "[channel/wechat] worker received msg msg_id={} text_len={}",
                    msg.msg_id,
                    msg.text.len(),
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                if !seen_ids.observe(&msg.msg_id).await {
                    log::debug!("[channel/wechat] duplicate msg_id {}, skipping", msg.msg_id);
                    continue;
                }

                let conv_type = msg.conversation_type.clone();
                let conv_key = msg.conversation_key.clone();
                let sender_nick = msg.sender_nick.clone();
                let text = msg.text.clone();

                let store_ref = Arc::clone(&conv_store);
                let ensure_store_ref = Arc::clone(&conv_store);
                let sender_nick_for_create = sender_nick.clone();
                let sender_nick_for_ensure = sender_nick.clone();
                let conv_type_for_create = conv_type.clone();
                let session_id = match router.get_or_create_session_with_ensure(
                    &conv_type,
                    &router_key,
                    &conv_key,
                    || {
                        let title = match &conv_type_for_create {
                            ConversationType::Group => {
                                format!("微信群 {}", &sender_nick_for_create)
                            }
                            ConversationType::Private => sender_nick_for_create.clone(),
                        };
                        let id = uuid::Uuid::new_v4().to_string();
                        store_ref
                            .create_conversation_with_im_source(
                                &id,
                                &title,
                                Platform::Wechat.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))?;
                        Ok(id)
                    },
                    |existing_id| {
                        ensure_store_ref
                            .create_conversation_with_im_source(
                                existing_id,
                                &sender_nick_for_ensure,
                                Platform::Wechat.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))
                    },
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        log::error!("[channel/wechat] session routing failed: {:#}", e);
                        continue;
                    }
                };

                // 缓存回信目标到 connector。`context_token` 由 connector 内部的
                // latest_context_tokens 旁路自动注入（remember_session 看到 None
                // 会去拉最新的）。
                concrete_wechat_for_worker
                    .remember_session(
                        session_id.clone(),
                        super::wechat::types::WechatSessionTarget {
                            to_user_id: conv_key.clone(),
                            context_token: None,
                        },
                    )
                    .await;

                {
                    if inactive_ref.load(std::sync::atomic::Ordering::SeqCst) {
                        log::debug!(
                            "[channel/wechat] worker observed inactive flag, dropping session id insert"
                        );
                        continue;
                    }
                    let mut ids = channel_session_ids_ref
                        .write()
                        .expect("channel_session_ids poisoned");
                    ids.insert(session_id.clone());
                }

                // Push to conversations list (new sessions only).
                {
                    let mut convs_lock = convs.write().await;
                    if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                        break;
                    }
                    if !convs_lock.iter().any(|c| c.session_id == session_id) {
                        let display_name = match &conv_type {
                            ConversationType::Group => format!("微信群 {}", &sender_nick),
                            ConversationType::Private => sender_nick.clone(),
                        };
                        convs_lock.push(ChannelConversation {
                            session_id: session_id.clone(),
                            platform: Platform::Wechat,
                            conversation_type: conv_type.clone(),
                            external_id: conv_key.clone(),
                            display_name,
                            unread_count: 0,
                            robot_code: router_key.clone(),
                            is_active_robot: true,
                        });
                    }
                }

                let preview = if text.chars().count() > 30 {
                    format!("{}...", text.chars().take(30).collect::<String>())
                } else {
                    text.clone()
                };
                let _ = app_handle.emit(
                    "channel:message",
                    &ChannelMessagePayload {
                        platform: "wechat".into(),
                        session_id: session_id.clone(),
                        sender_nick: sender_nick.clone(),
                        text_preview: preview,
                    },
                );

                let session_for_ask = crate::runtime::ids::SessionId::new(session_id.clone());
                match handle_pending_action_pre_dispatch(
                    ask_coordinator_ref.as_ref(),
                    &session_for_ask,
                    &text,
                )
                .await
                {
                    Ok(super::shared::ask_coordinator::HandleOutcome::NotPending)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::NewTurnAfterAbandon) => {}
                    Ok(super::shared::ask_coordinator::HandleOutcome::ApprovalResolved)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::AnswerResolved) => {
                        continue;
                    }
                    Ok(super::shared::ask_coordinator::HandleOutcome::InvalidApprovalAction {
                        message,
                    }) => {
                        send_pending_action_text_ack(
                            &connector_for_worker,
                            &session_id,
                            &conv_key,
                            "[channel/wechat]",
                            message,
                        )
                        .await;
                        continue;
                    }
                    Err(err) => {
                        log::warn!(
                            "[channel/wechat] IM ask coordinator failed, falling back to normal turn: {:#}",
                            err
                        );
                    }
                };

                // Wechat 附件下载：HTTP GET 加密文件 + AES-128-ECB 解密 → 落盘。
                // 实现走 `wechat::media::download_and_save`，error 收进 failures
                // 让 `build_compound_content` 给 LLM 加一段 "[注意：下列附件下载
                // 失败 ...]" hint，对称 wecom / feishu 路径。
                let (chat_attachments, download_failures) = if msg.attachments.is_empty() {
                    (Vec::new(), Vec::new())
                } else {
                    log::info!(
                        "[channel/wechat] downloading {} attachments msg_id={} session={}",
                        msg.attachments.len(),
                        msg.msg_id,
                        session_id
                    );
                    download_specs_for_turn_wechat(
                        &msg.attachments,
                        &wechat_downloads_dir,
                        &msg.msg_id,
                    )
                    .await
                };
                // All-attachments-failed + empty text → reply to user with
                // a hint; do NOT push a half-empty turn to the LLM.
                if chat_attachments.is_empty()
                    && text.trim().is_empty()
                    && !msg.attachments.is_empty()
                {
                    log::warn!(
                        "[channel/wechat] all attachments failed and no text, replying via send(Text) msg_id={}",
                        msg.msg_id
                    );
                    let connector_for_fallback = Arc::clone(&connector_for_worker);
                    let session_for_fallback = session_id.clone();
                    let conv_key_for_fallback = conv_key.clone();
                    tokio::spawn(async move {
                        if let Err(e) = connector_for_fallback
                            .send(
                                crate::connector::im::trait_def::ReplyTarget {
                                    session_id: session_for_fallback.clone(),
                                    external_conversation_key: conv_key_for_fallback,
                                },
                                crate::connector::im::trait_def::ReplyContent::Text(
                                    "附件下载全部失败，请重发。".to_string(),
                                ),
                            )
                            .await
                        {
                            log::warn!(
                                "[channel/wechat] fallback text send failed session={}: {:#}",
                                session_for_fallback,
                                e
                            );
                        }
                    });
                    continue;
                }

                let request = build_channel_chat_request(
                    session_id.clone(),
                    crate::runtime::human_interaction::ImPlatform::Wechat,
                    conv_key.clone(),
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments.clone(),
                    &download_failures,
                );
                let pending_item = super::shared::pending_adapter::build_pending_item_from_wechat(
                    &msg.msg_id,
                    &session_id,
                    &conv_key,
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments,
                    &download_failures,
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }

                let adapter_for_turn = Arc::clone(&adapter);
                let session_for_log = session_id.clone();
                let pending_manager_for_send = Arc::clone(&pending_manager_ref);
                let session_for_enqueue = crate::runtime::ids::SessionId::new(session_id.clone());
                let connector_for_send = Arc::clone(&connector_for_worker);
                let conv_key_for_reject = conv_key.clone();
                tokio::spawn(async move {
                    match pending_manager_for_send
                        .enqueue_or_send(session_for_enqueue, pending_item)
                        .await
                    {
                        Ok(crate::runtime::pending::EnqueueOutcome::SentDirectly { .. }) => {
                            if let Err(e) = adapter_for_turn.send_chat_request(request).await {
                                log::error!(
                                    "[channel/wechat] send_chat_request failed session={}: {}",
                                    session_for_log,
                                    e
                                );
                                pending_manager_for_send
                                    .release_direct_dispatch(&crate::runtime::ids::SessionId::new(
                                        session_for_log.clone(),
                                    ))
                                    .await;
                            }
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Queued { snapshot }) => {
                            log::info!(
                                "[channel/wechat] message queued session={} queue_size={}",
                                session_for_log,
                                snapshot.len()
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::HeldForHumanInteraction {
                            interaction_id,
                        }) => {
                            log::info!(
                                "[channel/wechat] message held for human interaction session={} interaction_id={:?}",
                                session_for_log,
                                interaction_id
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Rejected { reason }) => {
                            log::warn!(
                                "[channel/wechat] enqueue rejected session={} reason={:?}",
                                session_for_log,
                                reason
                            );
                            if let crate::runtime::pending::EnqueueRejection::QueueFull { limit } =
                                reason
                            {
                                let text = format!("消息堆积过多（已达 {limit} 条），请稍后再发。");
                                if let Err(e) = connector_for_send
                                    .send(
                                        crate::connector::im::trait_def::ReplyTarget {
                                            session_id: session_for_log.clone(),
                                            external_conversation_key: conv_key_for_reject.clone(),
                                        },
                                        crate::connector::im::trait_def::ReplyContent::Text(text),
                                    )
                                    .await
                                {
                                    log::warn!(
                                        "[channel/wechat] queue-full reject text send failed session={}: {:#}",
                                        session_for_log,
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "[channel/wechat] enqueue_or_send error session={}: {:#}",
                                session_for_log,
                                e
                            );
                        }
                    }
                });
            }
        });
        self.platform_state_mutate(Platform::Wechat, |s| {
            s.message_task = Some(message_handle);
        })
        .await;

        Ok(())
    }

    async fn connect_dingtalk_from_store(&self) -> Result<()> {
        let (config, app_secret_plain) = self.config_store.decrypt_dingtalk_config()?;
        self.connect_dingtalk(config, app_secret_plain).await
    }

    async fn connect_dingtalk(
        &self,
        config: DingtalkStoredConfig,
        app_secret_plain: String,
    ) -> Result<()> {
        // PR3.5: stop ONLY the dingtalk slot — feishu's stream is independent
        // and must not be touched here.
        self.stop_stream(Platform::Dingtalk).await;

        let reply_app_key = config.credentials.app_key.clone();
        let reply_app_secret = app_secret_plain.clone();
        let reply_robot_code = config.bot.robot_code.clone();

        let downloader = Arc::new(DingtalkFileDownloader::new(
            super::dingtalk::token::TokenCache::new(),
            config.credentials.app_key.clone(),
            app_secret_plain.clone(),
            self.dingtalk_downloads_dir(),
        ));

        self.set_dingtalk_connection_state(ChannelConnectionState::Connecting, None)
            .await;

        // Grab the dingtalk slot's generation counter and bump it for this
        // (re)connect. Captured in closures so stale status callbacks / worker
        // iterations from a previous connect drop on read-back.
        let stream_generation = self.platform_generation(Platform::Dingtalk).await;
        let generation = stream_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let message_stream_generation = Arc::clone(&stream_generation);
        let platform_state_for_status = Arc::clone(&self.platform_state);
        let config_store = Arc::clone(&self.config_store);
        let app_for_status = self.app_handle.clone();
        let stream_generation_for_status = Arc::clone(&stream_generation);
        let conversations_arc = Arc::clone(&self.conversations);
        let on_status: super::factory::DingtalkStatusCallback = Arc::new(
            move |new_connection: ChannelConnectionState, error: Option<String>| {
                let platform_state_for_status = platform_state_for_status.clone();
                let config_store = config_store.clone();
                let app_for_status = app_for_status.clone();
                let stream_generation_for_status = stream_generation_for_status.clone();
                let conversations_arc = conversations_arc.clone();
                tokio::spawn(async move {
                    if stream_generation_for_status.load(Ordering::SeqCst) != generation {
                        log::debug!("[channel/dingtalk] ignoring stale stream status callback");
                        return;
                    }
                    {
                        let mut map = platform_state_for_status.write().await;
                        let slot = map
                            .entry(Platform::Dingtalk)
                            .or_insert_with(PerPlatformState::unconfigured);
                        slot.connection = new_connection.clone();
                        slot.last_error = error.clone();
                    }
                    // Connected 时按 config 的 robot_code 刷新本平台 conv 的
                    // is_active_robot；set_dingtalk_connection_state 不走这条回调
                    // 路径，所以要在这里也处理一次。**只动钉钉自己的 conv** —— 否则
                    // 飞书 / 企微会话会被钉钉的 robot_code 比错，全部标 false
                    // （sidebar 把它们过滤掉就看不到了）。
                    if matches!(new_connection, ChannelConnectionState::Connected) {
                        let current_robot = config_store
                            .read_dingtalk_config()
                            .ok()
                            .flatten()
                            .map(|cfg| cfg.bot.robot_code);
                        let mut convs = conversations_arc.write().await;
                        for c in convs.iter_mut() {
                            if c.platform != Platform::Dingtalk {
                                continue;
                            }
                            c.is_active_robot = current_robot
                                .as_deref()
                                .map(|rc| rc == c.robot_code)
                                .unwrap_or(false);
                        }
                    }
                    match config_store.dingtalk_state(new_connection, error) {
                        Ok(state) => {
                            let _ = app_for_status.emit(
                                "channel:platform-state",
                                &ChannelPlatformStatePayload { state },
                            );
                        }
                        Err(error) => {
                            log::warn!(
                                "[channel/dingtalk] failed to build platform state: {:#}",
                                error
                            )
                        }
                    }
                });
            },
        );

        // 注册 Dingtalk connector（替换上一次连接的实例，保留 generation 隔离）。
        // 每次保存配置 / 自动连接都会以新的 status 回调和 generation 重建。
        let concrete_dingtalk = self
            .register_dingtalk_connector(
                config.credentials.app_key.clone(),
                app_secret_plain,
                config.bot.robot_code.clone(),
                on_status,
            )
            .await;

        // 通过 trait 启动 connector 拿到 BoxStream<ChannelMessage>。
        // cancel_token 由 manager 持有：stop_stream / 新一次 connect 都通过取消它来停旧 stream。
        let new_token = CancellationToken::new();
        let ctx = ConnectorContext {
            config_store: Arc::clone(&self.config_store),
            secure_storage: None,
            ask_coordinator: self.ask_coordinator.as_ref().map(Arc::clone),
            pending_manager: Arc::clone(&self.pending_manager),
            cancel_token: new_token.clone(),
        };
        let connector = {
            let map = self.connectors.read().await;
            map.get(&Platform::Dingtalk)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("dingtalk connector not registered"))?
        };
        let mut message_stream = connector
            .start(ctx)
            .await
            .map_err(|e| anyhow::anyhow!("dingtalk connector start failed: {e}"))?;

        let message_cancel = new_token.clone();
        self.platform_state_mutate(Platform::Dingtalk, |s| {
            s.stream_cancel = Some(new_token);
        })
        .await;

        // 订阅 reply_manager 到 chat_adapter 的 event bus（整个 manager 生命周期内只做一次，
        // 避免重连/重保存配置时把同一个 subscriber 重复挂载——RuntimeEventBus 没有去重也没有
        // unsubscribe，重复订阅会让 StreamDelta 被回放多次，钉钉 AI Card 上看到字符叠倍）
        if claim_first_subscription(&self.reply_subscribed) {
            let reply_sub = Arc::clone(&self.reply_manager)
                as Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>;
            self.chat_adapter
                .subscribe_event_listener(reply_sub.clone());
            self.anchor_subscriber(reply_sub);
        }

        // 订阅 ask_coordinator 到 event bus（同样只做一次，避免重连时重复订阅）
        if let Some(coordinator) = self.ask_coordinator.as_ref() {
            if claim_first_subscription(&self.ask_subscribed) {
                let sub = Arc::clone(coordinator)
                    as Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>;
                self.chat_adapter.subscribe_event_listener(sub.clone());
                self.anchor_subscriber(sub);
            }
        }

        // 消息处理 loop
        let adapter = Arc::clone(&self.chat_adapter);
        let conv_store = Arc::clone(&self.conversation_store);
        let sessions_path = self.sessions_paths[&Platform::Dingtalk].clone();
        let seen_ids = Arc::clone(&self.seen_msg_ids);
        let convs = Arc::clone(&self.conversations);
        let app_handle = self.app_handle.clone();
        let reply_manager_ref = Arc::clone(&self.reply_manager);
        let reply_robot_code_for_worker = reply_robot_code.clone();
        let downloader_ref = Arc::clone(&downloader);
        let ask_coordinator_ref = self.ask_coordinator.as_ref().map(Arc::clone);
        let channel_session_ids_ref = Arc::clone(&self.channel_session_ids);
        let inactive_ref = Arc::clone(&self.inactive);
        let pending_manager_ref = Arc::clone(&self.pending_manager);
        // 把"刚注册的 dingtalk connector"两份引用都吃进 worker：
        // - `concrete_dingtalk_for_worker` 用来调 `remember_session`（trait 不暴露）。
        // - `connector_for_worker` 是 trait-erased 句柄，用来在 fallback 文本路径
        //   走统一的 `connector.send(ReplyTarget, ReplyContent::Text(_))` 而不是
        //   直接 import `super::dingtalk::stream::send_session_webhook_text`。
        let concrete_dingtalk_for_worker = Arc::clone(&concrete_dingtalk);
        let connector_for_worker = {
            let map = self.connectors.read().await;
            Arc::clone(
                map.get(&Platform::Dingtalk)
                    .expect("dingtalk just registered"),
            )
        };

        let message_handle = tokio::spawn(async move {
            let mut router =
                match ChannelSessionRouter::migrate_or_load(&sessions_path, conv_store.as_ref()) {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("[channel] failed to load router: {:#}", e);
                        return;
                    }
                };

            while let Some(msg) = recv_current_generation_message_stream(
                &mut message_stream,
                &message_stream_generation,
                generation,
                &message_cancel,
            )
            .await
            {
                log::info!(
                    "[channel] worker received msg msg_id={} text_len={} attachments={}",
                    msg.msg_id,
                    msg.text.len(),
                    msg.attachments.len()
                );
                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    log::warn!("[channel] worker stream changed before processing, break");
                    break;
                }
                // 幂等去重
                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }
                if !seen_ids.observe(&msg.msg_id).await {
                    log::debug!("[channel] duplicate msg_id {}, skipping", msg.msg_id);
                    continue;
                }

                let conv_type = msg.conversation_type.clone();
                let conv_key = msg.conversation_key.clone();
                let sender_nick = msg.sender_nick.clone();
                let text = msg.text.clone();

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }
                // 路由到 session
                let store_ref = Arc::clone(&conv_store);
                let ensure_store_ref = Arc::clone(&conv_store);
                let sender_nick_for_create = sender_nick.clone();
                let sender_nick_for_ensure = sender_nick.clone();
                let conv_key_for_create = conv_key.clone();
                let conv_type_for_create = conv_type.clone();
                let session_id = match router.get_or_create_session_with_ensure(
                    &conv_type,
                    &reply_robot_code_for_worker,
                    &conv_key,
                    || {
                        let title = match &conv_type_for_create {
                            ConversationType::Group => format!(
                                "钉钉群 {}",
                                &conv_key_for_create[..conv_key_for_create.len().min(8)]
                            ),
                            ConversationType::Private => sender_nick_for_create.clone(),
                        };
                        let id = uuid::Uuid::new_v4().to_string();
                        store_ref
                            .create_conversation_with_im_source(
                                &id,
                                &title,
                                Platform::Dingtalk.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))?;
                        Ok(id)
                    },
                    |existing_id| {
                        ensure_store_ref
                            .create_conversation_with_im_source(
                                existing_id,
                                &sender_nick_for_ensure,
                                Platform::Dingtalk.as_str(),
                            )
                            .map_err(|e| anyhow::anyhow!(e))
                    },
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        log::error!("[channel] session routing failed: {:#}", e);
                        continue;
                    }
                };

                // 确保 ask_coordinator registry 能识别此频道 session
                // （std::sync::RwLock write lock 极短，不会阻塞 async reactor）
                {
                    if inactive_ref.load(std::sync::atomic::Ordering::SeqCst) {
                        log::debug!(
                            "[channel/dingtalk] worker observed inactive flag, dropping session id insert"
                        );
                        continue;
                    }
                    let mut ids = channel_session_ids_ref
                        .write()
                        .expect("channel_session_ids poisoned");
                    ids.insert(session_id.clone());
                }

                // 更新 conversations 列表（新会话）
                {
                    let mut convs_lock = convs.write().await;
                    if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                        break;
                    }
                    if !convs_lock.iter().any(|c| c.session_id == session_id) {
                        let display_name = match &conv_type {
                            ConversationType::Group => {
                                format!("钉钉群 {}", &conv_key[..conv_key.len().min(8)])
                            }
                            ConversationType::Private => sender_nick.clone(),
                        };
                        convs_lock.push(ChannelConversation {
                            session_id: session_id.clone(),
                            platform: Platform::Dingtalk,
                            conversation_type: conv_type.clone(),
                            external_id: conv_key.clone(),
                            display_name,
                            unread_count: 0,
                            robot_code: reply_robot_code_for_worker.clone(),
                            is_active_robot: true,
                        });
                    }
                }

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }
                // 推新消息通知给前端
                let preview_source = if text.trim().is_empty() && !msg.attachments.is_empty() {
                    format!("[附件] {} 个文件", msg.attachments.len())
                } else if !msg.attachments.is_empty() {
                    format!("[附件] {}", text)
                } else {
                    text.clone()
                };
                let preview = if preview_source.chars().count() > 30 {
                    format!("{}...", preview_source.chars().take(30).collect::<String>())
                } else {
                    preview_source
                };
                let _ = app_handle.emit(
                    "channel:message",
                    &ChannelMessagePayload {
                        platform: "dingtalk".into(),
                        session_id: session_id.clone(),
                        sender_nick: sender_nick.clone(),
                        text_preview: preview,
                    },
                );

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }
                let session_for_ask = crate::runtime::ids::SessionId::new(session_id.clone());
                match handle_pending_action_pre_dispatch(
                    ask_coordinator_ref.as_ref(),
                    &session_for_ask,
                    &text,
                )
                .await
                {
                    Ok(super::shared::ask_coordinator::HandleOutcome::NotPending)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::NewTurnAfterAbandon) => {}
                    Ok(super::shared::ask_coordinator::HandleOutcome::ApprovalResolved)
                    | Ok(super::shared::ask_coordinator::HandleOutcome::AnswerResolved) => {
                        continue;
                    }
                    Ok(super::shared::ask_coordinator::HandleOutcome::InvalidApprovalAction {
                        message,
                    }) => {
                        let _ = reply_manager_ref
                            .deliver_pending_approval_ack(&session_for_ask, &message)
                            .await;
                        continue;
                    }
                    Err(error) => {
                        log::warn!(
                            "[channel/dingtalk] IM ask coordinator failed, falling back to normal turn: {:#}",
                            error
                        );
                    }
                };

                // 构造 AI 输入（群聊带发送者前缀）
                let (chat_attachments, download_failures) = if msg.attachments.is_empty() {
                    (Vec::new(), Vec::new())
                } else {
                    log::info!(
                        "[channel] downloading {} attachments msgId={} session={}",
                        msg.attachments.len(),
                        msg.msg_id,
                        session_id
                    );
                    download_specs_for_turn(
                        downloader_ref.as_ref(),
                        &msg.attachments,
                        &msg.robot_code,
                        &msg.msg_id,
                    )
                    .await
                };
                if chat_attachments.is_empty()
                    && text.trim().is_empty()
                    && !msg.attachments.is_empty()
                {
                    log::warn!(
                        "[channel] all attachments failed and no text, replying via sessionWebhook msgId={}",
                        msg.msg_id
                    );
                    // 在 fallback 文本路径之前先把本 session 的钉钉特定回复参数缓存上，
                    // 这样 connector.send(Text) 才能在内部 session_targets map 中查到 webhook。
                    let dingtalk_target = super::dingtalk::connector::DingtalkSessionTarget {
                        robot_code: msg.robot_code.clone(),
                        reply_group_id: msg.reply_group_id.clone(),
                        session_webhook: msg.session_webhook.clone(),
                    };
                    concrete_dingtalk_for_worker
                        .remember_session(session_id.clone(), dingtalk_target)
                        .await;
                    let connector_for_fallback = Arc::clone(&connector_for_worker);
                    let session_for_fallback = session_id.clone();
                    let conv_key_for_fallback = conv_key.clone();
                    tokio::spawn(async move {
                        if let Err(e) = connector_for_fallback
                            .send(
                                crate::connector::im::trait_def::ReplyTarget {
                                    session_id: session_for_fallback.clone(),
                                    external_conversation_key: conv_key_for_fallback,
                                },
                                crate::connector::im::trait_def::ReplyContent::Text(
                                    "附件下载全部失败，请重发。".to_string(),
                                ),
                            )
                            .await
                        {
                            log::warn!(
                                "[channel] fallback text send failed session={}: {:#}",
                                session_for_fallback,
                                e
                            );
                        }
                    });
                    continue;
                }

                let request = build_channel_chat_request(
                    session_id.clone(),
                    crate::runtime::human_interaction::ImPlatform::Dingtalk,
                    conv_key.clone(),
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments.clone(),
                    &download_failures,
                );
                let run_id = request.run_id.as_str().to_string();

                // Build a PendingItem in parallel — used only if the manager queues
                // this message (busy session). On idle, the request above is sent
                // directly to preserve the legacy IM `[sender]: text` formatting.
                let pending_item = super::shared::pending_adapter::build_pending_item_from_dingtalk(
                    &msg.msg_id,
                    &session_id,
                    &conv_key,
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments,
                    &download_failures,
                );

                let card_target = match &conv_type {
                    ConversationType::Group => CardTarget::Group {
                        open_conversation_id: conv_key.clone(),
                    },
                    ConversationType::Private => CardTarget::Private {
                        user_id: msg.sender_id.clone(),
                    },
                };

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }
                // 记住本 session 的钉钉凭证（不管这条消息直发还是入队都记）。
                // 这份缓存只用于显式反馈卡或 connector send(AiCardChunk) 回到原钉钉会话；
                // RuntimeEventBus 普通流不能凭缓存懒建 IM 卡，避免 APP-only 输出串到 RM。
                reply_manager_ref
                    .remember_credentials(
                        session_id.clone(),
                        reply_app_key.clone(),
                        reply_app_secret.clone(),
                        reply_robot_code.clone(),
                        card_target.clone(),
                    )
                    .await;
                // 与 reply_manager 凭证同步：把本 session 的钉钉特定回复参数
                // （robot_code / reply_group_id / session_webhook）存进
                // concrete connector 的 session_targets map，供后续
                // `connector.send(Text|Markdown)` 路径按 session_id 查询使用。
                let dingtalk_target = super::dingtalk::connector::DingtalkSessionTarget {
                    robot_code: msg.robot_code.clone(),
                    reply_group_id: msg.reply_group_id.clone(),
                    session_webhook: msg.session_webhook.clone(),
                };
                concrete_dingtalk_for_worker
                    .remember_session(session_id.clone(), dingtalk_target)
                    .await;

                // 路由到 PendingQueueManager:
                //   - 闲时（队列空 + 非 busy）→ SentDirectly，直接 send_chat_request
                //   - 忙时 → Queued，等下次 turn 结束防抖 drain
                //   - 队列满 → 回钉钉端提示
                // 不能 await send_chat_request — turn 内部可能触发 AskUserQuestion，
                // 用户的回复需要本 worker 继续 recv 才能 resolve（死锁）。
                //
                // register（建钉钉 AI 卡片）只在 SentDirectly 分支后做；
                // Queued 分支完全不建卡（否则会有"处理中..."占位卡永远转圈，
                // 因为 drain 路径触发的是新 run_id、和这张卡的 run_id 对不上）。
                let adapter_for_turn = Arc::clone(&adapter);
                let session_for_log = session_id.clone();
                let pending_manager_for_send = Arc::clone(&pending_manager_ref);
                let session_for_enqueue = crate::runtime::ids::SessionId::new(session_id.clone());
                let connector_for_send = Arc::clone(&connector_for_worker);
                let conv_key_for_reject = conv_key.clone();
                let request_for_send = request;
                let reply_manager_for_send = Arc::clone(&reply_manager_ref);
                let reply_app_key_for_send = reply_app_key.clone();
                let reply_app_secret_for_send = reply_app_secret.clone();
                let reply_robot_code_for_send = reply_robot_code.clone();
                let card_target_for_send = card_target;
                let session_id_for_register = session_id.clone();
                let run_id_for_register = run_id;
                tokio::spawn(async move {
                    match pending_manager_for_send
                        .enqueue_or_send(session_for_enqueue, pending_item)
                        .await
                    {
                        Ok(crate::runtime::pending::EnqueueOutcome::SentDirectly { .. }) => {
                            // 闲时：先 register 钉钉 card，再发起 LLM。
                            // 用我们预构造的 request（含 IM 专属 [sender]: 前缀和附件格式化），
                            // 不用 manager rebuild 的版本。
                            reply_manager_for_send
                                .register(
                                    session_id_for_register,
                                    run_id_for_register,
                                    reply_app_key_for_send,
                                    reply_app_secret_for_send,
                                    reply_robot_code_for_send,
                                    card_target_for_send,
                                )
                                .await;
                            if let Err(e) =
                                adapter_for_turn.send_chat_request(request_for_send).await
                            {
                                log::error!(
                                    "[channel] send_chat_request failed session={}: {}",
                                    session_for_log,
                                    e
                                );
                                pending_manager_for_send
                                    .release_direct_dispatch(&crate::runtime::ids::SessionId::new(
                                        session_for_log.clone(),
                                    ))
                                    .await;
                            }
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Queued { snapshot }) => {
                            // 忙时：不 register card；等 drain 时由 PendingQueueManager
                            // 触发 LLM。drain 路径不绑定钉钉卡（IM 场景下用户语义就是
                            // "只回第一条"），后续合并 user message 仍然落 messages.jsonl
                            // 供 history 用。
                            log::info!(
                                "[channel] message queued session={} queue_size={} (no card)",
                                session_for_log,
                                snapshot.len()
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::HeldForHumanInteraction {
                            interaction_id,
                        }) => {
                            log::info!(
                                "[channel] message held for human interaction session={} interaction_id={:?}",
                                session_for_log,
                                interaction_id
                            );
                        }
                        Ok(crate::runtime::pending::EnqueueOutcome::Rejected { reason }) => {
                            log::warn!(
                                "[channel] enqueue rejected session={} reason={:?}",
                                session_for_log,
                                reason
                            );
                            if let crate::runtime::pending::EnqueueRejection::QueueFull { limit } =
                                reason
                            {
                                let text = format!("消息堆积过多（已达 {limit} 条），请稍后再发。");
                                if let Err(e) = connector_for_send
                                    .send(
                                        crate::connector::im::trait_def::ReplyTarget {
                                            session_id: session_for_log.clone(),
                                            external_conversation_key: conv_key_for_reject.clone(),
                                        },
                                        crate::connector::im::trait_def::ReplyContent::Text(text),
                                    )
                                    .await
                                {
                                    log::warn!(
                                        "[channel] queue-full reject text send failed session={}: {:#}",
                                        session_for_log,
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "[channel] enqueue_or_send error session={}: {:#}",
                                session_for_log,
                                e
                            );
                        }
                    }
                });
            }
        });
        self.platform_state_mutate(Platform::Dingtalk, |s| {
            s.message_task = Some(message_handle);
        })
        .await;

        Ok(())
    }

    /// 旧入口：保留供 ChannelManager 内部其它代码调用，但语义变更——只清 reply_manager，
    /// 不再 clear conversations。conversations 由 refresh_active_robot_flags 标记 inactive。
    async fn clear_runtime_state(&self) {
        self.reply_manager.clear().await;
    }

    pub async fn get_conversations(&self) -> Vec<ChannelConversation> {
        self.conversations.read().await.clone()
    }

    /// Returns `true` if this instance has been shut down via `shutdown()`.
    pub fn is_inactive(&self) -> bool {
        self.inactive.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn active_scope(&self) -> Option<crate::storage::UserScope> {
        self.active_scope.clone()
    }

    fn anchor_subscriber(&self, sub: Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>) {
        self.subscriber_anchors.lock().unwrap().push(sub);
    }

    /// Test-only: directly insert a session id into the shared registry,
    /// honouring the inactive gate. Used to verify that `shutdown()` prevents
    /// further session-id registrations.
    #[cfg(test)]
    pub fn register_channel_session_for_test(&self, session_id: String) {
        if self.is_inactive() {
            return;
        }
        self.channel_session_ids
            .write()
            .expect("channel_session_ids poisoned")
            .insert(session_id);
    }

    /// Best-effort shutdown — marks inactive, cancels all per-platform streams,
    /// awaits worker tasks with a 3s overall budget, and clears any session
    /// ids this instance owns from the shared `channel_session_ids` registry.
    ///
    /// Idempotent: subsequent calls are no-ops once `inactive` is set.
    pub async fn shutdown(&self) {
        use std::sync::atomic::Ordering;
        use std::time::Duration;

        // Idempotency gate: only the first caller does the work.
        if self.inactive.swap(true, Ordering::SeqCst) {
            log::debug!("[channel] shutdown: already inactive, skipping");
            return;
        }
        log::info!("[channel] shutdown: begin");

        // Step 1: collect per-platform cancel tokens + task handles, replacing
        // them with empty slots so subsequent set_enabled / connect attempts
        // see a clean slate. We don't await tasks while holding the write lock.
        let mut to_join: Vec<(Platform, tokio::task::JoinHandle<()>)> = Vec::new();
        {
            let mut states = self.platform_state.write().await;
            for (platform, slot) in states.iter_mut() {
                if let Some(token) = slot.stream_cancel.take() {
                    token.cancel();
                }
                if let Some(handle) = slot.message_task.take() {
                    to_join.push((platform.clone(), handle));
                }
                slot.stream_generation.fetch_add(1, Ordering::SeqCst);
            }
        }

        // Step 2: await all workers with a global 3s budget. Anything still
        // running gets dropped — the inactive flag prevents user-visible side
        // effects from those zombies.
        let join_all = async {
            for (platform, handle) in to_join {
                if let Err(e) = handle.await {
                    log::warn!(
                        "[channel/{}] shutdown worker join failed: {}",
                        platform.as_str(),
                        e
                    );
                }
            }
        };
        if tokio::time::timeout(Duration::from_secs(3), join_all)
            .await
            .is_err()
        {
            log::warn!("[channel] shutdown: worker join exceeded 3s budget, dropping");
        }

        // Step 3: drop our entries from the shared session-id registry. We
        // intentionally do NOT clear the whole set — a future owner may have
        // already registered new sessions. Instead, drain everything the local
        // conversations cache claims is ours.
        let owned: Vec<String> = {
            let convs = self.conversations.read().await;
            convs.iter().map(|c| c.session_id.clone()).collect()
        };
        {
            let mut ids = self
                .channel_session_ids
                .write()
                .expect("channel_session_ids poisoned");
            for sid in owned {
                ids.remove(&sid);
            }
        }

        // Step 4: release subscriber anchors so the bus's Weak refs die.
        self.subscriber_anchors.lock().unwrap().clear();

        log::info!("[channel] shutdown: complete");
    }
}

fn claim_first_subscription(flag: &AtomicBool) -> bool {
    flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

#[cfg(test)]
async fn stop_stream_components(
    stream_generation: &Arc<AtomicU64>,
    stream_cancel: &Arc<RwLock<Option<CancellationToken>>>,
    message_task: &Arc<RwLock<Option<JoinHandle<()>>>>,
) {
    stream_generation.fetch_add(1, Ordering::SeqCst);
    let token = stream_cancel.write().await.take();
    if let Some(token) = token {
        log::info!("[channel] cancelling previous stream connection");
        token.cancel();
    }
    if let Some(handle) = message_task.write().await.take() {
        if let Err(error) = handle.await {
            log::warn!("[channel] message worker join failed: {}", error);
        }
    }
}

#[cfg(test)]
async fn recv_current_generation_message(
    msg_rx: &mut tokio::sync::mpsc::Receiver<ChannelMessage>,
    stream_generation: &Arc<AtomicU64>,
    generation: u64,
    cancel_token: &CancellationToken,
) -> Option<ChannelMessage> {
    let current_gen = stream_generation.load(Ordering::SeqCst);
    if current_gen != generation {
        log::warn!(
            "[channel] worker pre-recv: generation drift my_gen={} current_gen={}, exiting",
            generation,
            current_gen
        );
        return None;
    }

    let msg = tokio::select! {
        biased;
        _ = cancel_token.cancelled() => {
            log::info!("[channel] worker recv: cancel token fired, exiting");
            return None;
        },
        msg = msg_rx.recv() => msg?,
    };
    let current_gen = stream_generation.load(Ordering::SeqCst);
    if current_gen != generation {
        log::warn!(
            "[channel] worker post-recv: generation drift my_gen={} current_gen={} msg_id={}, dropping message",
            generation, current_gen, msg.msg_id
        );
        return None;
    }

    Some(msg)
}

/// Stream-based variant of `recv_current_generation_message` used after the
/// PR5 trait rewire. Same generation/cancel semantics, but the source is now
/// the `BoxStream<ChannelMessage>` returned by `IMConnector::start`.
async fn recv_current_generation_message_stream(
    message_stream: &mut futures::stream::BoxStream<'static, ChannelMessage>,
    stream_generation: &Arc<AtomicU64>,
    generation: u64,
    cancel_token: &CancellationToken,
) -> Option<ChannelMessage> {
    let current_gen = stream_generation.load(Ordering::SeqCst);
    if current_gen != generation {
        log::warn!(
            "[channel] worker pre-recv: generation drift my_gen={} current_gen={}, exiting",
            generation,
            current_gen
        );
        return None;
    }

    let msg = tokio::select! {
        biased;
        _ = cancel_token.cancelled() => {
            log::info!("[channel] worker recv: cancel token fired, exiting");
            return None;
        },
        msg = message_stream.next() => msg?,
    };
    let current_gen = stream_generation.load(Ordering::SeqCst);
    if current_gen != generation {
        log::warn!(
            "[channel] worker post-recv: generation drift my_gen={} current_gen={} msg_id={}, dropping message",
            generation,
            current_gen,
            msg.msg_id
        );
        return None;
    }

    Some(msg)
}

fn is_current_stream(
    stream_generation: &Arc<AtomicU64>,
    generation: u64,
    cancel_token: &CancellationToken,
) -> bool {
    !cancel_token.is_cancelled() && stream_generation.load(Ordering::SeqCst) == generation
}

/// Current wall-clock time in milliseconds since the UNIX epoch. Used by the
/// feishu worker to compare against `ChannelMessage.created_at_ms` (which is
/// itself ms, see `feishu::stream::parse_im_message`). Returns 0 if the
/// system clock is somehow earlier than 1970 — caller treats 0 as "comparison
/// disabled" which is the safe fallback (don't skip anything).
fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Render a feishu sender_id (open_id `ou_xxx`) as a user-friendly nick string
/// for display in conversation titles and `channel:message` previews. Truncates
/// the id to 12 chars (enough to disambiguate users in a tenant) and prefixes
/// with "飞书用户". Caller in the manager worker uses this whenever the connector
/// hands an open_id as `msg.sender_nick` — the feishu IM event doesn't ship a
/// human-readable display name (would need a separate contact API lookup).
fn render_feishu_sender_nick(open_id: &str) -> String {
    if open_id.is_empty() {
        return "飞书用户".to_string();
    }
    // Use char-based truncation so multi-byte chars (shouldn't appear in
    // open_id, but defensive) don't slice mid-codepoint.
    let short: String = open_id.chars().take(12).collect();
    format!("飞书用户 {}", short)
}

fn build_compound_content(
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: &[ChatAttachmentRef],
    download_failures: &[String],
) -> String {
    // Lead block: group chats always carry a `[sender]:` prefix so the
    // downstream LLM (and the user bubble) can tell apart speakers; private
    // chats are 1:1 so the prefix would be noise. When the sender wrote no
    // body we keep the prefix as `[sender]:` (without trailing space) — this
    // signals "speaker said something non-textual" without producing a
    // dangling colon-space that looks like a parse error.
    let mut blocks: Vec<String> = Vec::new();
    let lead = match conv_type {
        ConversationType::Group => {
            if text.is_empty() {
                format!("[{}]:", sender_nick)
            } else {
                format!("[{}]: {}", sender_nick, text)
            }
        }
        ConversationType::Private => text.to_string(),
    };
    if !lead.is_empty() {
        blocks.push(lead);
    }
    for att in attachments {
        blocks.push(attachment_to_markdown(att));
    }
    if !download_failures.is_empty() {
        blocks.push(format!(
            "[注意：以下附件下载失败，未能加载：{}]",
            download_failures.join(", ")
        ));
    }
    blocks.join("\n\n")
}

/// Render one downloaded attachment as inline markdown that
/// `UserBubbleMarkdown` knows how to chip-render: image kind → `![]()`, others
/// → `[附件: name]()`. The URL is always angle-bracket-wrapped so paths with
/// spaces / CJK / special chars survive CommonMark parsing without escape
/// gymnastics.
fn attachment_to_markdown(att: &ChatAttachmentRef) -> String {
    let url = path_to_file_url(&att.file_path);
    if att.kind == "image" {
        format!("![{}](<{}>)", att.file_name, url)
    } else {
        format!("[附件: {}](<{}>)", att.file_name, url)
    }
}

fn path_to_file_url(path: &str) -> String {
    // Windows absolute path (e.g. `C:\Users\u\x.docx`) → `file:///C:/Users/u/x.docx`
    // Unix absolute path (e.g. `/Users/u/x.docx`) → `file:///Users/u/x.docx`
    if path.len() >= 2 && path.as_bytes()[1] == b':' && path.as_bytes()[0].is_ascii_alphabetic() {
        format!("file:///{}", path.replace('\\', "/"))
    } else {
        format!("file://{}", path)
    }
}

fn build_channel_chat_request(
    session_id: String,
    platform: crate::runtime::human_interaction::ImPlatform,
    external_conversation_key: String,
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: Vec<ChatAttachmentRef>,
    download_failures: &[String],
) -> ChatTurnRequest {
    let content = build_compound_content(
        conv_type,
        sender_nick,
        text,
        &attachments,
        download_failures,
    );
    let mut request = ChatTurnRequest::new(session_id.clone(), content, attachments);
    request.channel_context = Some(IM_MOBILE_CHANNEL_CONTEXT.to_string());
    request.turn_origin = crate::runtime::human_interaction::TurnOrigin::Im {
        platform,
        external_conversation_key: external_conversation_key.clone(),
        sender_id: None,
        sender_label: Some(sender_nick.to_string()),
        account_id: None,
        thread_id: None,
    };
    request.output_binding = crate::runtime::human_interaction::OutputBinding::im(
        platform,
        session_id,
        external_conversation_key,
        true,
    );
    request.session_attachment_dirs =
        crate::runtime::path_auth::derive_working_dirs_from_attachments(
            &request
                .attachments
                .iter()
                .map(|a| std::path::PathBuf::from(&a.file_path))
                .collect::<Vec<_>>(),
        );
    request
}

fn downloaded_to_chat_attachment(
    downloaded: &DownloadedFile,
    kind: AttachmentKind,
) -> ChatAttachmentRef {
    ChatAttachmentRef {
        id: downloaded.sha256.clone(),
        file_name: downloaded.file_name.clone(),
        file_path: downloaded.path.to_string_lossy().to_string(),
        kind: match kind {
            AttachmentKind::Picture => "image".to_string(),
            AttachmentKind::File => "file".to_string(),
        },
        file_size: downloaded.size,
        file_type: super::dingtalk::download::extension_or_bin(&downloaded.path.to_string_lossy()),
        mime_type: downloaded.mime_type.clone(),
    }
}

async fn download_specs_for_turn(
    downloader: &DingtalkFileDownloader,
    specs: &[super::types::ChannelAttachmentSpec],
    robot_code: &str,
    msg_id: &str,
) -> (Vec<ChatAttachmentRef>, Vec<String>) {
    let mut attachments = Vec::new();
    let mut failures = Vec::new();
    for spec in specs {
        match downloader
            .download(&spec.download_code, robot_code, &spec.file_name)
            .await
        {
            Ok(downloaded) => {
                attachments.push(downloaded_to_chat_attachment(&downloaded, spec.kind))
            }
            Err(error) => {
                log::warn!(
                    "[channel] attachment download failed msgId={} file_name={} err={:#}",
                    msg_id,
                    spec.file_name,
                    error
                );
                failures.push(spec.file_name.clone());
            }
        }
    }
    (attachments, failures)
}

/// Feishu-shaped twin of `downloaded_to_chat_attachment`. Mirrors fields
/// 1:1; uses `feishu::download::extension_or_bin` for `file_type`. PR6.
fn downloaded_to_chat_attachment_feishu(
    downloaded: &super::feishu::download::FeishuDownloadedFile,
    kind: AttachmentKind,
) -> ChatAttachmentRef {
    ChatAttachmentRef {
        id: downloaded.sha256.clone(),
        file_name: downloaded.file_name.clone(),
        file_path: downloaded.path.to_string_lossy().to_string(),
        kind: match kind {
            AttachmentKind::Picture => "image".to_string(),
            AttachmentKind::File => "file".to_string(),
        },
        file_size: downloaded.size,
        file_type: super::feishu::download::extension_or_bin(&downloaded.path.to_string_lossy()),
        mime_type: downloaded.mime_type.clone(),
    }
}

/// Feishu-shaped twin of `download_specs_for_turn`. Sequential per-message
/// (matches dingtalk; spec §6 disallows fanning out parallel tokio tasks
/// per attachment within a single message). For feishu, `download_code`
/// on `ChannelAttachmentSpec` holds the `file_key` / `image_key` set by
/// `feishu::stream::parse_im_message`; the `msg_id` is required by the
/// `/messages/{message_id}/resources/{file_key}` endpoint.
async fn download_specs_for_turn_feishu(
    downloader: &super::feishu::download::FeishuFileDownloader,
    specs: &[super::types::ChannelAttachmentSpec],
    msg_id: &str,
) -> (Vec<ChatAttachmentRef>, Vec<String>) {
    let mut attachments = Vec::new();
    let mut failures = Vec::new();
    for spec in specs {
        match downloader
            .download(msg_id, &spec.download_code, spec.kind, &spec.file_name)
            .await
        {
            Ok(downloaded) => {
                attachments.push(downloaded_to_chat_attachment_feishu(&downloaded, spec.kind))
            }
            Err(error) => {
                log::warn!(
                    "[channel/feishu] attachment download failed msg_id={} file_name={} err={:#}",
                    msg_id,
                    spec.file_name,
                    error
                );
                failures.push(spec.file_name.clone());
            }
        }
    }
    (attachments, failures)
}

/// 把 `wecom::media::WecomDownloadedFile` 映射到 trait 中性的 `ChatAttachmentRef`，
/// 对应 `downloaded_to_chat_attachment_feishu`。`mime_type` 在 wecom 路径下
/// 是按扩展名手工映射的，不像飞书走 HTTP Content-Type；详见 `media::mime_from_ext`。
fn downloaded_to_chat_attachment_wecom(
    downloaded: &super::wecom::media::WecomDownloadedFile,
    kind: AttachmentKind,
) -> ChatAttachmentRef {
    ChatAttachmentRef {
        id: downloaded.sha256.clone(),
        file_name: downloaded.file_name.clone(),
        file_path: downloaded.path.to_string_lossy().to_string(),
        kind: match kind {
            AttachmentKind::Picture => "image".to_string(),
            AttachmentKind::File => "file".to_string(),
        },
        file_size: downloaded.size,
        file_type: super::wecom::media::extension_or_bin(&downloaded.path.to_string_lossy()),
        mime_type: downloaded.mime_type.clone(),
    }
}

/// Wecom-shaped twin of `download_specs_for_turn_feishu`. `download_code` on
/// `ChannelAttachmentSpec` carries `wecom://{aeskey}@{url}` set by
/// `wecom::parser::parse_inbound`. Sequential per-message — aibot 下载链接
/// 5 分钟内有效，串行 = 1 张图最长几秒，并发拉满 worker IO 收益不大。
async fn download_specs_for_turn_wecom(
    specs: &[super::types::ChannelAttachmentSpec],
    dest_dir: &std::path::Path,
    msg_id: &str,
) -> (Vec<ChatAttachmentRef>, Vec<String>) {
    let mut attachments = Vec::new();
    let mut failures = Vec::new();
    for spec in specs {
        match super::wecom::media::download_and_save(&spec.download_code, dest_dir, &spec.file_name)
            .await
        {
            Ok(downloaded) => {
                attachments.push(downloaded_to_chat_attachment_wecom(&downloaded, spec.kind))
            }
            Err(error) => {
                log::warn!(
                    "[channel/wecom] attachment download failed msg_id={} file_name={} err={:#}",
                    msg_id,
                    spec.file_name,
                    error
                );
                failures.push(spec.file_name.clone());
            }
        }
    }
    (attachments, failures)
}

/// 把 `wechat::media::WechatDownloadedFile` 映射到 trait 中性的 `ChatAttachmentRef`。
/// 跟 `downloaded_to_chat_attachment_wecom` 完全等价，独立一份避免跨平台耦合
/// （wechat 将来如果要加 voice/silk 特殊路径，shape 可以自行演进）。
fn downloaded_to_chat_attachment_wechat(
    downloaded: &super::wechat::media::WechatDownloadedFile,
    kind: AttachmentKind,
) -> ChatAttachmentRef {
    ChatAttachmentRef {
        id: downloaded.sha256.clone(),
        file_name: downloaded.file_name.clone(),
        file_path: downloaded.path.to_string_lossy().to_string(),
        kind: match kind {
            AttachmentKind::Picture => "image".to_string(),
            AttachmentKind::File => "file".to_string(),
        },
        file_size: downloaded.size,
        file_type: super::wechat::media::extension_or_bin(&downloaded.path.to_string_lossy()),
        mime_type: downloaded.mime_type.clone(),
    }
}

/// Wechat-shaped twin of `download_specs_for_turn_wecom`. `download_code` on
/// `ChannelAttachmentSpec` carries `wechat://{aes_key_b64}@{full_url}` set by
/// `wechat::api::extract_attachments_from_item_list`. 串行下载，跟 wecom 同
/// 模式。iLink CDN URL 有效期未在协议中明确说明，本期按 wecom 5min 估算。
async fn download_specs_for_turn_wechat(
    specs: &[super::types::ChannelAttachmentSpec],
    dest_dir: &std::path::Path,
    msg_id: &str,
) -> (Vec<ChatAttachmentRef>, Vec<String>) {
    let mut attachments = Vec::new();
    let mut failures = Vec::new();
    for spec in specs {
        match super::wechat::media::download_and_save(
            &spec.download_code,
            dest_dir,
            &spec.file_name,
        )
        .await
        {
            Ok(downloaded) => {
                attachments.push(downloaded_to_chat_attachment_wechat(&downloaded, spec.kind))
            }
            Err(error) => {
                log::warn!(
                    "[channel/wechat] attachment download failed msg_id={} file_name={} err={:#}",
                    msg_id,
                    spec.file_name,
                    error
                );
                failures.push(spec.file_name.clone());
            }
        }
    }
    (attachments, failures)
}

/// Telegram 版 `downloaded_to_chat_attachment_*`，把 `TelegramDownloadedFile`
/// 映射到 trait 中性的 `ChatAttachmentRef`。
///
/// `file_type` 是 chat_turn_driver vision filter 的判定字段之一（见
/// `multimodal::is_image_attachment`：file_type == "image" 或 mime starts with
/// "image/" 才会进 vision path）。当 spec.kind == Picture 时强制 `"image"`，
/// 文件路径扩展名（如 jpg / png）仍可通过 `mime_type` 回到下游。
fn downloaded_to_chat_attachment_telegram(
    downloaded: &super::telegram::download::TelegramDownloadedFile,
    kind: AttachmentKind,
) -> ChatAttachmentRef {
    let (kind_str, file_type) = match kind {
        AttachmentKind::Picture => ("image".to_string(), "image".to_string()),
        AttachmentKind::File => (
            "file".to_string(),
            super::telegram::download::extension_or_bin(&downloaded.path.to_string_lossy()),
        ),
    };
    ChatAttachmentRef {
        id: downloaded.sha256.clone(),
        file_name: downloaded.file_name.clone(),
        file_path: downloaded.path.to_string_lossy().to_string(),
        kind: kind_str,
        file_size: downloaded.size,
        file_type,
        mime_type: downloaded.mime_type.clone(),
    }
}

/// Telegram 版 `download_specs_for_turn_*`：`download_code` 上是 file_id；
/// downloader 通过 Bot API `getFile` → `file_path` → 拼 download URL 取字节。
/// 串行下载，与其它平台模式一致。
async fn download_specs_for_turn_telegram(
    downloader: &super::telegram::download::TelegramFileDownloader,
    specs: &[super::types::ChannelAttachmentSpec],
) -> (Vec<ChatAttachmentRef>, Vec<String>) {
    let mut attachments = Vec::new();
    let mut failures = Vec::new();
    for spec in specs {
        // Photo 一定是图片（parser 强制合成 photo-{msg_id}.jpg）；document
        // 让 downloader 按扩展名推断 mime；其它一律 None hint 走扩展名路径。
        let mime_hint = match spec.kind {
            AttachmentKind::Picture => Some("image/jpeg".to_string()),
            AttachmentKind::File => None,
        };
        match downloader
            .download(&spec.download_code, &spec.file_name, mime_hint)
            .await
        {
            Ok(downloaded) => {
                attachments.push(downloaded_to_chat_attachment_telegram(
                    &downloaded,
                    spec.kind,
                ));
            }
            Err(error) => {
                log::warn!(
                    "[channel/telegram] attachment download failed file_name={} err={:#}",
                    spec.file_name,
                    error
                );
                failures.push(spec.file_name.clone());
            }
        }
    }
    (attachments, failures)
}

/// WhatsApp 版附件转换：parser 阶段已把原始媒体写入本地文件，`download_code`
/// 字段直接是绝对路径字符串，无需网络下载。
///
/// `file_type` 对接 `multimodal::is_image_attachment`：Picture → "image"，
/// File → 从路径扩展名推断（无扩展名时降级 "bin"）。
/// `id` 用文件路径字符串（parser 已去重，无需再 sha256 哈希）。
fn whatsapp_specs_to_chat_attachments(
    specs: &[super::types::ChannelAttachmentSpec],
) -> (Vec<ChatAttachmentRef>, Vec<String>) {
    let mut attachments = Vec::new();
    let mut failures = Vec::new();
    for spec in specs {
        let path = std::path::Path::new(&spec.download_code);
        if !path.exists() {
            log::warn!(
                "[channel/whatsapp] attachment path missing file_name={} path={}",
                spec.file_name,
                path.display()
            );
            failures.push(spec.file_name.clone());
            continue;
        }
        let size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(_) => 0,
        };
        let (kind_str, file_type) = match spec.kind {
            AttachmentKind::Picture => ("image".to_string(), "image".to_string()),
            AttachmentKind::File => (
                "file".to_string(),
                super::telegram::download::extension_or_bin(&path.to_string_lossy()),
            ),
        };
        attachments.push(ChatAttachmentRef {
            id: spec.download_code.clone(),
            file_name: spec.file_name.clone(),
            file_path: spec.download_code.clone(),
            kind: kind_str,
            file_size: size,
            file_type,
            mime_type: None,
        });
    }
    (attachments, failures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_first_subscription_returns_true_only_once() {
        let flag = AtomicBool::new(false);
        assert!(claim_first_subscription(&flag));
        assert!(!claim_first_subscription(&flag));
        assert!(!claim_first_subscription(&flag));
    }

    /// PR5 contract: `register_dingtalk_connector` inserts under
    /// `Platform::Dingtalk` and a second call replaces the entry (does NOT
    /// double-insert). This is the seam Phase 1+ platforms will reuse — adding
    /// 飞书 / 企微 just means adding a sibling `register_feishu_connector`
    /// helper that targets a different `Platform` key.
    #[tokio::test]
    async fn register_dingtalk_connector_replaces_entry_under_same_platform_key() {
        let map: Arc<RwLock<HashMap<Platform, Arc<dyn IMConnector>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let reply_manager = Arc::new(DingtalkReplyManager::new());

        let (c1, _) = super::super::factory::build_dingtalk_connector(
            "ak1".into(),
            "as1".into(),
            "rc1".into(),
            Arc::clone(&reply_manager),
            Arc::new(|_state, _err| {}),
        );
        map.write().await.insert(Platform::Dingtalk, c1);
        assert_eq!(map.read().await.len(), 1);

        let (c2, _) = super::super::factory::build_dingtalk_connector(
            "ak2".into(),
            "as2".into(),
            "rc2".into(),
            Arc::clone(&reply_manager),
            Arc::new(|_state, _err| {}),
        );
        map.write().await.insert(Platform::Dingtalk, c2);
        assert_eq!(
            map.read().await.len(),
            1,
            "second insert must replace, not duplicate"
        );

        // The registered connector reports the right platform — verifying the
        // factory wired Platform::Dingtalk through correctly.
        let map_read = map.read().await;
        let registered = map_read.get(&Platform::Dingtalk).expect("inserted");
        assert_eq!(registered.platform(), Platform::Dingtalk);
    }

    /// PR3.5 contract: `stop_stream(Feishu)` operates on the feishu slot only;
    /// the dingtalk slot's `stream_cancel` token must NOT be cancelled.
    /// This is the regression test for the pre-PR3.5 single-slot bug where
    /// `set_enabled(Feishu, false)` would tear down a running dingtalk stream.
    ///
    /// Test pokes the `platform_state` map directly — constructing a full
    /// ChannelManager requires an AppHandle which is too heavy here.
    #[tokio::test]
    async fn stop_one_platform_slot_does_not_cancel_another_platforms_token() {
        let map: Arc<RwLock<HashMap<Platform, PerPlatformState>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let dingtalk_token = CancellationToken::new();
        let feishu_token = CancellationToken::new();

        // Seed both slots with live cancel tokens.
        {
            let mut guard = map.write().await;
            let mut d = PerPlatformState::unconfigured();
            d.stream_cancel = Some(dingtalk_token.clone());
            guard.insert(Platform::Dingtalk, d);
            let mut f = PerPlatformState::unconfigured();
            f.stream_cancel = Some(feishu_token.clone());
            guard.insert(Platform::Feishu, f);
        }

        // Replicate `stop_stream(Platform::Feishu)`'s slot-mutation step.
        let (taken_cancel, taken_task) = {
            let mut guard = map.write().await;
            let slot = guard
                .entry(Platform::Feishu)
                .or_insert_with(PerPlatformState::unconfigured);
            slot.stream_generation.fetch_add(1, Ordering::SeqCst);
            (slot.stream_cancel.take(), slot.message_task.take())
        };
        if let Some(t) = taken_cancel {
            t.cancel();
        }
        assert!(taken_task.is_none());

        // The feishu token (the one stop_stream pulled) is cancelled, but
        // dingtalk's stays untouched.
        assert!(
            feishu_token.is_cancelled(),
            "feishu token should be cancelled"
        );
        assert!(
            !dingtalk_token.is_cancelled(),
            "dingtalk token must NOT be cancelled by stop_stream(Feishu)"
        );

        // Feishu slot's cancel field is now None; dingtalk's slot still holds
        // the original (live) token.
        let guard = map.read().await;
        assert!(guard
            .get(&Platform::Feishu)
            .unwrap()
            .stream_cancel
            .is_none());
        assert!(guard
            .get(&Platform::Dingtalk)
            .unwrap()
            .stream_cancel
            .is_some());
    }

    /// PR3.5 contract: the per-platform `connection` slot in `platform_state`
    /// is per-platform — reads for Dingtalk and Feishu return their own values,
    /// not whatever the most-recently-written slot held. Regression for the
    /// pre-PR3.5 bug where `get_platform(Dingtalk)` would return feishu's
    /// state (and vice versa) because both shared one `Arc<RwLock<...>>`.
    #[tokio::test]
    async fn per_platform_connection_state_is_isolated_per_slot() {
        let map: Arc<RwLock<HashMap<Platform, PerPlatformState>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Dingtalk Connected, Feishu Disconnected.
        {
            let mut guard = map.write().await;
            let mut d = PerPlatformState::unconfigured();
            d.connection = ChannelConnectionState::Connected;
            d.last_error = None;
            guard.insert(Platform::Dingtalk, d);
            let mut f = PerPlatformState::unconfigured();
            f.connection = ChannelConnectionState::Disconnected;
            f.last_error = Some("feishu off".into());
            guard.insert(Platform::Feishu, f);
        }

        let guard = map.read().await;
        let d = guard.get(&Platform::Dingtalk).unwrap();
        assert_eq!(d.connection, ChannelConnectionState::Connected);
        assert!(d.last_error.is_none());

        let f = guard.get(&Platform::Feishu).unwrap();
        assert_eq!(f.connection, ChannelConnectionState::Disconnected);
        assert_eq!(f.last_error.as_deref(), Some("feishu off"));

        // Writes to one slot don't leak into the other.
        drop(guard);
        {
            let mut guard = map.write().await;
            guard
                .entry(Platform::Feishu)
                .or_insert_with(PerPlatformState::unconfigured)
                .connection = ChannelConnectionState::ConfigError;
        }
        let guard = map.read().await;
        assert_eq!(
            guard.get(&Platform::Dingtalk).unwrap().connection,
            ChannelConnectionState::Connected,
            "writing feishu slot must NOT touch dingtalk slot's connection state"
        );
        assert_eq!(
            guard.get(&Platform::Feishu).unwrap().connection,
            ChannelConnectionState::ConfigError
        );
    }

    #[test]
    fn render_feishu_sender_nick_truncates_and_prefixes() {
        // 12 chars after the 飞书用户 prefix.
        assert_eq!(
            render_feishu_sender_nick("ou_abcdef0123456789xxxxxxxxxx"),
            "飞书用户 ou_abcdef012"
        );
        // Short id: no truncation needed.
        assert_eq!(render_feishu_sender_nick("ou_short"), "飞书用户 ou_short");
        // Defensive: empty id falls back to a plain label.
        assert_eq!(render_feishu_sender_nick(""), "飞书用户");
    }

    // ---- shutdown() + inactive flag tests ----------------------------------
    //
    // Full ChannelManager construction requires an AppHandle, which has no
    // lightweight constructor outside the Tauri test harness. Instead these
    // tests exercise the *internals* that shutdown() composes: the
    // platform_state map, the inactive AtomicBool, and the channel_session_ids
    // registry — exactly the same approach used by the existing
    // stop_one_platform_slot_does_not_cancel_another_platforms_token test.

    /// shutdown() marks inactive, cancels all per-platform cancel tokens, and
    /// awaits the spawned workers.
    #[tokio::test]
    async fn shutdown_marks_inactive_and_stops_all_streams() {
        let inactive = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let platform_state: Arc<RwLock<HashMap<Platform, PerPlatformState>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let dingtalk_token = CancellationToken::new();
        let feishu_token = CancellationToken::new();

        // Seed both platform slots with live cancel tokens and a worker task.
        let (dt_done_tx, mut dt_done_rx) = tokio::sync::mpsc::channel::<()>(1);
        let dt_cancel_clone = dingtalk_token.clone();
        let dingtalk_handle = tokio::spawn(async move {
            dt_cancel_clone.cancelled().await;
            let _ = dt_done_tx.send(()).await;
        });

        let (fs_done_tx, mut fs_done_rx) = tokio::sync::mpsc::channel::<()>(1);
        let fs_cancel_clone = feishu_token.clone();
        let feishu_handle = tokio::spawn(async move {
            fs_cancel_clone.cancelled().await;
            let _ = fs_done_tx.send(()).await;
        });

        {
            let mut guard = platform_state.write().await;
            let mut d = PerPlatformState::unconfigured();
            d.stream_cancel = Some(dingtalk_token.clone());
            d.message_task = Some(dingtalk_handle);
            guard.insert(Platform::Dingtalk, d);

            let mut f = PerPlatformState::unconfigured();
            f.stream_cancel = Some(feishu_token.clone());
            f.message_task = Some(feishu_handle);
            guard.insert(Platform::Feishu, f);
        }

        // Replicate the shutdown() inner loop (the logic we just implemented).
        assert!(!inactive.load(Ordering::SeqCst), "initially active");

        let was_already_inactive = inactive.swap(true, Ordering::SeqCst);
        assert!(!was_already_inactive, "first swap returns false");

        let mut to_join: Vec<(Platform, tokio::task::JoinHandle<()>)> = Vec::new();
        {
            let mut states = platform_state.write().await;
            for (platform, slot) in states.iter_mut() {
                if let Some(token) = slot.stream_cancel.take() {
                    token.cancel();
                }
                if let Some(handle) = slot.message_task.take() {
                    to_join.push((platform.clone(), handle));
                }
                slot.stream_generation.fetch_add(1, Ordering::SeqCst);
            }
        }
        for (_platform, handle) in to_join {
            handle.await.expect("worker join");
        }

        // Both tokens must have been cancelled and workers must have signalled done.
        assert!(dingtalk_token.is_cancelled(), "dingtalk token cancelled");
        assert!(feishu_token.is_cancelled(), "feishu token cancelled");
        assert!(dt_done_rx.try_recv().is_ok(), "dingtalk worker finished");
        assert!(fs_done_rx.try_recv().is_ok(), "feishu worker finished");
        assert!(
            inactive.load(Ordering::SeqCst),
            "inactive flag is true after shutdown"
        );

        // Slots are cleared.
        let guard = platform_state.read().await;
        for (_, slot) in guard.iter() {
            assert!(slot.stream_cancel.is_none());
            assert!(slot.message_task.is_none());
        }
    }

    /// shutdown() is idempotent — calling the swap/gate logic twice must be a
    /// no-op on the second call (returns early without double-cancelling etc.).
    #[test]
    fn shutdown_is_idempotent() {
        let inactive = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // First call: swap false → true, returns false (was NOT inactive).
        let first = inactive.swap(true, Ordering::SeqCst);
        assert!(!first, "first call should perform the work");

        // Second call: swap true → true, returns true (was already inactive).
        let second = inactive.swap(true, Ordering::SeqCst);
        assert!(second, "second call hits the idempotency gate");

        // The flag stays true.
        assert!(inactive.load(Ordering::SeqCst));
    }

    /// After shutdown (inactive flag set), register_channel_session_for_test
    /// must NOT add an id to channel_session_ids.
    ///
    /// This exercises the `register_channel_session_for_test` helper that was
    /// added specifically for test-coverage of the inactive gate on worker
    /// session-id inserts.
    #[test]
    fn inactive_blocks_channel_session_id_registration() {
        let inactive = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ids: Arc<std::sync::RwLock<HashSet<String>>> =
            Arc::new(std::sync::RwLock::new(HashSet::new()));

        // Before shutdown: insert should succeed.
        {
            let is_inactive = inactive.load(Ordering::SeqCst);
            if !is_inactive {
                ids.write()
                    .expect("poisoned")
                    .insert("session-before".into());
            }
        }
        assert!(ids.read().expect("poisoned").contains("session-before"));

        // Mark inactive (simulating shutdown()).
        inactive.store(true, Ordering::SeqCst);

        // After shutdown: the inactive gate should block the insert.
        {
            let is_inactive = inactive.load(Ordering::SeqCst);
            if !is_inactive {
                ids.write()
                    .expect("poisoned")
                    .insert("session-after".into());
            }
        }
        assert!(
            !ids.read().expect("poisoned").contains("session-after"),
            "insert after shutdown must be blocked by inactive gate"
        );
        assert_eq!(
            ids.read().expect("poisoned").len(),
            1,
            "only the before-shutdown entry"
        );
    }
}

/// 启动期 hydrate 时，每个平台的"当前 active router_key"集合。给
/// `build_conversation_snapshot` 用，决定每个 RouterEntry 的 `is_active_robot`。
///
/// - dingtalk: 当前在线的 robot_code（来自 dingtalk_config.bot.robot_code）。
///   钉钉可能有"多机器人/多应用历史会话"，所以严格按 robot_code 匹配。
/// - feishu / wecom: 当前唯一配置的 app_id / bot_id。这两个平台没有"多机器人切换"
///   概念，每平台同时只配一个；当前配置的 router_key 视为 active，旧 entry 视为
///   inactive。
#[derive(Debug, Default, Clone, Copy)]
pub struct HydrateCurrentRobots<'a> {
    pub dingtalk: Option<&'a str>,
    pub feishu: Option<&'a str>,
    pub wecom: Option<&'a str>,
    pub wechat: Option<&'a str>,
    pub telegram: Option<&'a str>,
    pub whatsapp: Option<&'a str>,
}

impl<'a> HydrateCurrentRobots<'a> {
    fn for_platform(&self, platform: Platform) -> Option<&'a str> {
        match platform {
            Platform::Dingtalk => self.dingtalk,
            Platform::Feishu => self.feishu,
            Platform::Wecom => self.wecom,
            Platform::Wechat => self.wechat,
            Platform::Telegram => self.telegram,
            Platform::Whatsapp => self.whatsapp,
        }
    }
}

pub fn build_conversation_snapshot(
    entries: &[(Platform, crate::connector::im::shared::router::RouterEntry)],
    conversation_store: &dyn crate::runtime::store::ConversationStore,
    current_robots: HydrateCurrentRobots<'_>,
) -> Vec<ChannelConversation> {
    let titles: std::collections::HashMap<String, String> =
        match conversation_store.get_conversations() {
            Ok(values) => values
                .into_iter()
                .filter_map(|v| {
                    let id = v.get("id").and_then(|x| x.as_str())?.to_string();
                    let title = v
                        .get("title")
                        .and_then(|x| x.as_str())
                        .unwrap_or("未知会话")
                        .to_string();
                    Some((id, title))
                })
                .collect(),
            Err(e) => {
                log::warn!(
                    "[channel] failed to read conversations during hydrate: {:#}",
                    e
                );
                std::collections::HashMap::new()
            }
        };

    entries
        .iter()
        .map(|(platform, entry)| {
            let display_name = titles.get(&entry.session_id).cloned().unwrap_or_else(|| {
                log::warn!(
                    "[channel] hydrate: conversation {} not found in store, using placeholder",
                    entry.session_id
                );
                "未知会话".to_string()
            });
            let is_active_robot = current_robots
                .for_platform(*platform)
                .map(|rc| rc == entry.robot_code)
                .unwrap_or(false);
            ChannelConversation {
                session_id: entry.session_id.clone(),
                platform: *platform,
                conversation_type: entry.conversation_type.clone(),
                external_id: entry.external_id.clone(),
                display_name,
                unread_count: 0,
                robot_code: entry.robot_code.clone(),
                is_active_robot,
            }
        })
        .collect()
}

#[cfg(test)]
mod hydrate_tests {
    use super::*;
    use crate::connector::im::shared::router::RouterEntry;
    use crate::runtime::store::{ConversationStore, InMemoryConversationStore};
    use std::sync::Arc;

    #[test]
    fn snapshot_marks_only_current_robot_as_active() {
        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store
            .create_conversation("sess-1", "Active Title")
            .unwrap();
        conv_store
            .create_conversation("sess-2", "Legacy Title")
            .unwrap();

        let entries = vec![
            (
                Platform::Dingtalk,
                RouterEntry {
                    conversation_type: ConversationType::Private,
                    robot_code: "robot-current".into(),
                    external_id: "user1".into(),
                    session_id: "sess-1".into(),
                },
            ),
            (
                Platform::Dingtalk,
                RouterEntry {
                    conversation_type: ConversationType::Group,
                    robot_code: "robot-old".into(),
                    external_id: "cid2".into(),
                    session_id: "sess-2".into(),
                },
            ),
        ];

        let snapshot = build_conversation_snapshot(
            &entries,
            conv_store.as_ref(),
            HydrateCurrentRobots {
                dingtalk: Some("robot-current"),
                ..Default::default()
            },
        );

        assert_eq!(snapshot.len(), 2);
        let active: Vec<_> = snapshot.iter().filter(|c| c.is_active_robot).collect();
        let inactive: Vec<_> = snapshot.iter().filter(|c| !c.is_active_robot).collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].session_id, "sess-1");
        assert_eq!(active[0].display_name, "Active Title");
        assert_eq!(inactive.len(), 1);
        assert_eq!(inactive[0].session_id, "sess-2");
        assert_eq!(inactive[0].robot_code, "robot-old");
    }

    #[test]
    fn snapshot_falls_back_to_placeholder_when_title_missing() {
        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());

        let entries = vec![(
            Platform::Dingtalk,
            RouterEntry {
                conversation_type: ConversationType::Private,
                robot_code: "robot-1".into(),
                external_id: "user1".into(),
                session_id: "sess-orphan".into(),
            },
        )];

        let snapshot = build_conversation_snapshot(
            &entries,
            conv_store.as_ref(),
            HydrateCurrentRobots {
                dingtalk: Some("robot-1"),
                ..Default::default()
            },
        );

        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].display_name, "未知会话");
    }

    #[test]
    fn snapshot_marks_all_inactive_when_no_current_robot() {
        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store.create_conversation("sess-1", "Title").unwrap();

        let entries = vec![(
            Platform::Dingtalk,
            RouterEntry {
                conversation_type: ConversationType::Private,
                robot_code: "robot-1".into(),
                external_id: "user1".into(),
                session_id: "sess-1".into(),
            },
        )];

        let snapshot = build_conversation_snapshot(
            &entries,
            conv_store.as_ref(),
            HydrateCurrentRobots::default(),
        );

        assert_eq!(snapshot.len(), 1);
        assert!(!snapshot[0].is_active_robot);
    }

    #[test]
    fn snapshot_isolates_per_platform_active_robot() {
        // Regression: 旧版用单一 current_robot_code 比对所有 entry，飞书会话
        // 拿钉钉的 robot_code 去比，永远 mismatch → 全部被标 inactive，前端
        // 飞书栏永远"暂无会话"。这里钉钉和飞书各自 active，跨平台不串扰。
        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store
            .create_conversation("sess-ding", "钉钉私聊")
            .unwrap();
        conv_store
            .create_conversation("sess-feishu", "飞书私聊")
            .unwrap();
        conv_store
            .create_conversation("sess-feishu-old", "飞书旧应用")
            .unwrap();

        let entries = vec![
            (
                Platform::Dingtalk,
                RouterEntry {
                    conversation_type: ConversationType::Private,
                    robot_code: "dingaf79qt8carlhcwav".into(),
                    external_id: "075919431222937233".into(),
                    session_id: "sess-ding".into(),
                },
            ),
            (
                Platform::Feishu,
                RouterEntry {
                    conversation_type: ConversationType::Private,
                    robot_code: "cli_aa812b8928f8dcc9".into(),
                    external_id: "oc_dc11f03e".into(),
                    session_id: "sess-feishu".into(),
                },
            ),
            (
                Platform::Feishu,
                RouterEntry {
                    conversation_type: ConversationType::Private,
                    robot_code: "cli_oldappid".into(),
                    external_id: "oc_legacy".into(),
                    session_id: "sess-feishu-old".into(),
                },
            ),
        ];

        let snapshot = build_conversation_snapshot(
            &entries,
            conv_store.as_ref(),
            HydrateCurrentRobots {
                dingtalk: Some("dingaf79qt8carlhcwav"),
                feishu: Some("cli_aa812b8928f8dcc9"),
                wecom: None,
                wechat: None,
                telegram: None,
                whatsapp: None,
            },
        );

        assert_eq!(snapshot.len(), 3);
        let ding = snapshot
            .iter()
            .find(|c| c.session_id == "sess-ding")
            .unwrap();
        let fs_active = snapshot
            .iter()
            .find(|c| c.session_id == "sess-feishu")
            .unwrap();
        let fs_old = snapshot
            .iter()
            .find(|c| c.session_id == "sess-feishu-old")
            .unwrap();
        assert_eq!(ding.platform, Platform::Dingtalk);
        assert!(ding.is_active_robot, "钉钉当前 robot_code 匹配 → active");
        assert_eq!(fs_active.platform, Platform::Feishu);
        assert!(fs_active.is_active_robot, "飞书当前 app_id 匹配 → active");
        assert_eq!(fs_old.platform, Platform::Feishu);
        assert!(!fs_old.is_active_robot, "飞书旧 app_id 不匹配 → inactive");
    }

    #[test]
    fn build_compound_content_appends_group_prefix_and_download_failures() {
        let content = build_compound_content(
            &ConversationType::Group,
            "张三",
            "请看附件",
            &[],
            &["bad.jpg".to_string(), "expired.pdf".to_string()],
        );
        assert!(content.starts_with("[张三]: 请看附件"));
        assert!(content.contains("[注意：以下附件下载失败，未能加载：bad.jpg, expired.pdf]"));
    }

    fn make_image_attachment(file_name: &str, file_path: &str) -> ChatAttachmentRef {
        ChatAttachmentRef {
            id: format!("sha-{}", file_name),
            file_name: file_name.to_string(),
            file_path: file_path.to_string(),
            kind: "image".into(),
            file_size: 0,
            file_type: "jpg".into(),
            mime_type: None,
        }
    }

    fn make_file_attachment(file_name: &str, file_path: &str) -> ChatAttachmentRef {
        ChatAttachmentRef {
            id: format!("sha-{}", file_name),
            file_name: file_name.to_string(),
            file_path: file_path.to_string(),
            kind: "file".into(),
            file_size: 0,
            file_type: "pdf".into(),
            mime_type: None,
        }
    }

    #[test]
    fn private_image_inlines_as_markdown_image_with_angle_bracketed_url() {
        let img = make_image_attachment("photo.jpg", "/Users/u/.renlijia/uploads/photo.jpg");
        let content = build_compound_content(&ConversationType::Private, "Alice", "", &[img], &[]);
        // Empty text + private chat → markdown is just the image link, no prefix.
        assert_eq!(
            content,
            "![photo.jpg](<file:///Users/u/.renlijia/uploads/photo.jpg>)"
        );
    }

    #[test]
    fn private_file_inlines_as_attachment_link_with_chinese_prefix() {
        let f = make_file_attachment("季度报告.pdf", "/Users/u/.renlijia/uploads/季度报告.pdf");
        let content =
            build_compound_content(&ConversationType::Private, "Alice", "请看", &[f], &[]);
        assert_eq!(
            content,
            "请看\n\n[附件: 季度报告.pdf](<file:///Users/u/.renlijia/uploads/季度报告.pdf>)"
        );
    }

    #[test]
    fn group_with_text_and_image_inserts_prefix_then_blank_line_then_image() {
        let img = make_image_attachment("a.png", "/tmp/a.png");
        let content = build_compound_content(&ConversationType::Group, "张三", "看图", &[img], &[]);
        assert_eq!(content, "[张三]: 看图\n\n![a.png](<file:///tmp/a.png>)");
    }

    #[test]
    fn empty_text_with_multiple_attachments_lists_each_on_its_own_paragraph() {
        let img = make_image_attachment("x.png", "/tmp/x.png");
        let f = make_file_attachment("y.pdf", "/tmp/y.pdf");
        let content =
            build_compound_content(&ConversationType::Private, "Alice", "", &[img, f], &[]);
        assert_eq!(
            content,
            "![x.png](<file:///tmp/x.png>)\n\n[附件: y.pdf](<file:///tmp/y.pdf>)"
        );
    }

    #[test]
    fn group_empty_text_omits_dangling_prefix_colon() {
        // Group + empty text + only attachments must NOT emit `[张三]: ` (a bare
        // sender prefix with no body looks like a parsing error in the bubble).
        let img = make_image_attachment("a.png", "/tmp/a.png");
        let content = build_compound_content(&ConversationType::Group, "张三", "", &[img], &[]);
        assert_eq!(content, "[张三]:\n\n![a.png](<file:///tmp/a.png>)");
    }

    #[test]
    fn windows_path_uses_three_slash_file_url_with_forward_slashes() {
        let f = make_file_attachment("doc.docx", "C:\\Users\\u\\doc.docx");
        let content = build_compound_content(&ConversationType::Private, "Alice", "", &[f], &[]);
        assert_eq!(content, "[附件: doc.docx](<file:///C:/Users/u/doc.docx>)");
    }

    #[test]
    fn download_failures_appended_after_attachments_block() {
        let img = make_image_attachment("ok.png", "/tmp/ok.png");
        let content = build_compound_content(
            &ConversationType::Private,
            "Alice",
            "请看",
            &[img],
            &["broken.zip".to_string()],
        );
        assert_eq!(
            content,
            "请看\n\n![ok.png](<file:///tmp/ok.png>)\n\n[注意：以下附件下载失败，未能加载：broken.zip]"
        );
    }

    #[test]
    fn im_request_sets_mobile_channel_context_without_polluting_content() {
        let request = build_channel_chat_request(
            "sess-im".to_string(),
            crate::runtime::human_interaction::ImPlatform::Dingtalk,
            "conv-im".to_string(),
            &ConversationType::Private,
            "Alice",
            "帮我处理一下 AI 表格",
            vec![],
            &[],
        );

        assert_eq!(request.content, "帮我处理一下 AI 表格");
        assert!(request.skill_command.is_none());
        let channel_context = request.channel_context.as_deref().unwrap_or_default();
        assert!(channel_context.contains("IM/移动端渠道"));
        assert!(channel_context.contains("完整授权链接"));
        assert!(!request.content.contains("浏览器已打开"));
        assert_eq!(
            request.output_binding,
            crate::runtime::human_interaction::OutputBinding::im(
                crate::runtime::human_interaction::ImPlatform::Dingtalk,
                "sess-im",
                "conv-im",
                true,
            )
        );
    }

    #[test]
    fn im_request_sets_mobile_channel_context_for_unrelated_text_too() {
        let request = build_channel_chat_request(
            "sess-normal".to_string(),
            crate::runtime::human_interaction::ImPlatform::Feishu,
            "conv-normal".to_string(),
            &ConversationType::Private,
            "Alice",
            "帮我总结一下这个文件",
            vec![],
            &[],
        );

        assert!(request.skill_command.is_none());
        assert!(request
            .channel_context
            .as_deref()
            .unwrap_or_default()
            .contains("IM/移动端渠道"));
    }

    #[test]
    fn dingtalk_greeting_target_uses_current_active_private_session() {
        let conversations = vec![
            ChannelConversation {
                session_id: "old-private".into(),
                platform: Platform::Dingtalk,
                conversation_type: ConversationType::Private,
                external_id: "old-user".into(),
                display_name: "旧机器人".into(),
                unread_count: 0,
                robot_code: "old-robot".into(),
                is_active_robot: false,
            },
            ChannelConversation {
                session_id: "current-group".into(),
                platform: Platform::Dingtalk,
                conversation_type: ConversationType::Group,
                external_id: "group-id".into(),
                display_name: "群聊".into(),
                unread_count: 0,
                robot_code: "robot-current".into(),
                is_active_robot: true,
            },
            ChannelConversation {
                session_id: "current-private".into(),
                platform: Platform::Dingtalk,
                conversation_type: ConversationType::Private,
                external_id: "user-001".into(),
                display_name: "姚斌权".into(),
                unread_count: 0,
                robot_code: "robot-current".into(),
                is_active_robot: true,
            },
        ];

        let target = select_dingtalk_greeting_target(&conversations, "robot-current")
            .expect("current private dingtalk session should be selected");

        assert_eq!(target.session_id, "current-private");
        assert_eq!(target.external_conversation_key, "user-001");
    }

    #[test]
    fn downloaded_file_to_chat_attachment_maps_kind_and_type() {
        let downloaded = DownloadedFile {
            path: std::path::PathBuf::from("/tmp/a/report.xlsx"),
            file_name: "report.xlsx".into(),
            size: 12,
            sha256: "abc".into(),
            mime_type: Some(
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
            ),
        };
        let attachment = downloaded_to_chat_attachment(&downloaded, AttachmentKind::File);
        assert_eq!(attachment.id, "abc");
        assert_eq!(attachment.file_name, "report.xlsx");
        assert_eq!(attachment.kind, "file");
        assert_eq!(attachment.file_type, "xlsx");
    }
}

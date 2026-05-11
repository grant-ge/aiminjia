//! ChannelManager — 管理 IM 频道连接生命周期

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::runtime::store::ConversationStore;
use crate::runtime::ChatTurnRequest;
use crate::storage::crypto::SecureStorage;
use crate::transport::tauri_commands::chat::TauriChatCommandAdapter;

use super::config_store::ChannelConfigStore;
use super::dingtalk_card::CardTarget;
use super::dingtalk_registration::{begin_registration, poll_registration, RegistrationPollState};
use super::dingtalk_stream::DingtalkStreamClient;
use super::reply_manager::DingtalkReplyManager;
use super::router::ChannelSessionRouter;
use super::types::{
    ChannelConnectionState, ChannelConversation, ChannelMessage, ChannelMessagePayload,
    ChannelPlatformState, ChannelPlatformStatePayload, ChannelRegistrationBeginResult,
    ChannelRegistrationPollResult, ChannelRegistrationPollState, ConversationType,
    DingtalkStoredConfig, Platform,
};

use crate::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use super::dingtalk_download::{DingtalkFileDownloader, DownloadedFile};
use super::types::AttachmentKind;

pub struct ChannelManager {
    app_handle: AppHandle,
    chat_adapter: Arc<TauriChatCommandAdapter>,
    conversation_store: Arc<dyn ConversationStore>,
    config_store: Arc<ChannelConfigStore>,
    sessions_path: PathBuf,
    connection: Arc<RwLock<ChannelConnectionState>>,
    last_error: Arc<RwLock<Option<String>>>,
    seen_msg_ids: Arc<RwLock<HashSet<String>>>,
    conversations: Arc<RwLock<Vec<ChannelConversation>>>,
    reply_manager: Arc<DingtalkReplyManager>,
    reply_subscribed: Arc<AtomicBool>,
    stream_cancel: Arc<RwLock<Option<CancellationToken>>>,
    message_task: Arc<RwLock<Option<JoinHandle<()>>>>,
    stream_generation: Arc<AtomicU64>,
    ask_coordinator: Option<Arc<super::ask_coordinator::IMAskCoordinator>>,
    ask_subscribed: Arc<AtomicBool>,
    /// 已建立的 IM 频道 session id 集合，与 ask_coordinator 的 registry 共享同一 Arc。
    /// 消息 worker 每创建一个新 session 时向此集合写入，确保 coordinator 能识别频道会话。
    channel_session_ids: Arc<std::sync::RwLock<HashSet<String>>>,
}

impl ChannelManager {
    pub fn new(
        app_handle: AppHandle,
        chat_adapter: Arc<TauriChatCommandAdapter>,
        conversation_store: Arc<dyn ConversationStore>,
        secure_storage: Option<Arc<SecureStorage>>,
        channels_dir: PathBuf,
        ask_coordinator: Option<Arc<super::ask_coordinator::IMAskCoordinator>>,
        reply_manager: Arc<DingtalkReplyManager>,
        channel_session_ids: Arc<std::sync::RwLock<HashSet<String>>>,
    ) -> Self {
        let config_store = Arc::new(ChannelConfigStore::new(channels_dir, secure_storage));
        let sessions_path = config_store.dingtalk_sessions_path();
        Self {
            app_handle,
            chat_adapter,
            conversation_store,
            config_store,
            sessions_path,
            connection: Arc::new(RwLock::new(ChannelConnectionState::Unconfigured)),
            last_error: Arc::new(RwLock::new(None)),
            seen_msg_ids: Arc::new(RwLock::new(HashSet::new())),
            conversations: Arc::new(RwLock::new(vec![])),
            reply_manager,
            reply_subscribed: Arc::new(AtomicBool::new(false)),
            stream_cancel: Arc::new(RwLock::new(None)),
            message_task: Arc::new(RwLock::new(None)),
            stream_generation: Arc::new(AtomicU64::new(0)),
            ask_coordinator,
            ask_subscribed: Arc::new(AtomicBool::new(false)),
            channel_session_ids,
        }
    }

    async fn current_dingtalk_state(&self) -> Result<ChannelPlatformState> {
        let connection = self.connection.read().await.clone();
        let last_error = self.last_error.read().await.clone();
        self.config_store.dingtalk_state(connection, last_error)
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

    /// 启动时调用一次：从 sessions.json + conversation_store 重建内存 conversations 列表。
    /// 期间检测到 v1 schema 会清掉所有指向的 conversation 目录（参见 router.migrate_or_load）。
    pub async fn hydrate_conversations(&self) {
        let router = match super::router::ChannelSessionRouter::migrate_or_load(
            &self.sessions_path,
            self.conversation_store.as_ref(),
        ) {
            Ok(r) => r,
            Err(e) => {
                log::error!("[channel] hydrate_conversations: failed to load router: {:#}", e);
                return;
            }
        };
        let entries = router.entries();

        // 将持久化的 session id 写入共享 registry，确保 ask_coordinator 从启动起
        // 就能识别已有频道会话（而不仅是本次运行新建的会话）。
        {
            let mut ids = self.channel_session_ids.write().expect("channel_session_ids poisoned");
            for entry in &entries {
                ids.insert(entry.session_id.clone());
            }
        }

        let current_robot = match self.config_store.read_dingtalk_config() {
            Ok(Some(cfg)) => Some(cfg.bot.robot_code),
            Ok(None) => None,
            Err(e) => {
                log::warn!(
                    "[channel] hydrate_conversations: failed to read config: {:#}",
                    e
                );
                None
            }
        };
        let snapshot = build_conversation_snapshot(
            &entries,
            self.conversation_store.as_ref(),
            current_robot.as_deref(),
        );
        *self.conversations.write().await = snapshot;
    }

    /// 重新计算每条 conversation 的 is_active_robot：等于 current_robot_code 的为 true。
    /// 调用方需要保证 emit 一次 platform-state 让前端重拉 conversations。
    pub async fn refresh_active_robot_flags(&self, current_robot_code: Option<&str>) {
        let mut convs = self.conversations.write().await;
        for c in convs.iter_mut() {
            c.is_active_robot = current_robot_code
                .map(|rc| rc == c.robot_code)
                .unwrap_or(false);
        }
    }

    async fn set_connection_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) {
        *self.connection.write().await = connection.clone();
        *self.last_error.write().await = last_error;
        if matches!(connection, ChannelConnectionState::Connected) {
            let current_robot = self
                .config_store
                .read_dingtalk_config()
                .ok()
                .flatten()
                .map(|cfg| cfg.bot.robot_code);
            self.refresh_active_robot_flags(current_robot.as_deref()).await;
        }
        self.emit_dingtalk_state().await;
    }

    async fn stop_stream(&self) {
        stop_stream_components(&self.stream_generation, &self.stream_cancel, &self.message_task)
            .await;
    }

    /// 读取用户隔离的平台配置，若 DingTalk 已启用则自动连接。
    pub async fn auto_connect_if_configured(&self) {
        match self.config_store.read_dingtalk_config() {
            Ok(Some(config)) if config.enabled => {
                if let Err(error) = self.connect_dingtalk_from_store().await {
                    log::warn!("[channel] auto_connect failed: {:#}", error);
                    self.set_connection_state(
                        ChannelConnectionState::ConfigError,
                        Some(error.to_string()),
                    )
                    .await;
                }
            }
            Ok(Some(_)) => {
                self.set_connection_state(ChannelConnectionState::Disconnected, None)
                    .await;
            }
            Ok(None) => {
                self.set_connection_state(ChannelConnectionState::Unconfigured, None)
                    .await;
            }
            Err(error) => {
                log::warn!("[channel] failed to read config: {:#}", error);
                self.set_connection_state(
                    ChannelConnectionState::ConfigError,
                    Some(error.to_string()),
                )
                .await;
            }
        }
    }

    pub async fn get_platforms(&self) -> Result<Vec<ChannelPlatformState>> {
        let connection = self.connection.read().await.clone();
        let last_error = self.last_error.read().await.clone();
        self.config_store
            .all_platform_states(connection, last_error)
    }

    pub async fn get_platform(&self, platform: Platform) -> Result<ChannelPlatformState> {
        match platform {
            Platform::Dingtalk => self.current_dingtalk_state().await,
            other => Ok(ChannelConfigStore::coming_soon_state(other)),
        }
    }

    pub async fn set_enabled(
        &self,
        platform: Platform,
        enabled: bool,
    ) -> Result<ChannelPlatformState> {
        match platform {
            Platform::Dingtalk => {
                if enabled {
                    self.config_store.set_dingtalk_enabled(true)?;
                    self.connect_dingtalk_from_store().await?;
                } else {
                    self.stop_stream().await;
                    self.config_store.set_dingtalk_enabled(false)?;
                    self.set_connection_state(ChannelConnectionState::Disconnected, None)
                        .await;
                }
                self.current_dingtalk_state().await
            }
            other => anyhow::bail!("{} channel is not available yet", other.as_str()),
        }
    }

    pub async fn remove_platform(&self, platform: Platform) -> Result<ChannelPlatformState> {
        match platform {
            Platform::Dingtalk => {
                self.stop_stream().await;
                let state = self.config_store.remove_dingtalk()?;
                self.clear_runtime_state().await;
                self.refresh_active_robot_flags(None).await;
                self.set_connection_state(ChannelConnectionState::Unconfigured, None)
                    .await;
                Ok(state)
            }
            other => anyhow::bail!("{} channel is not available yet", other.as_str()),
        }
    }

    pub async fn reveal_secret(&self, platform: Platform) -> Result<String> {
        match platform {
            Platform::Dingtalk => self.config_store.reveal_dingtalk_secret(),
            other => anyhow::bail!("{} channel is not available yet", other.as_str()),
        }
    }

    /// 创建钉钉 OPEN_CLAW 一键注册会话，返回用户需要打开的授权 URL。
    pub async fn begin_dingtalk_registration(&self) -> Result<ChannelRegistrationBeginResult> {
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

    async fn connect_dingtalk_from_store(&self) -> Result<()> {
        let (config, app_secret_plain) = self.config_store.decrypt_dingtalk_config()?;
        self.connect_dingtalk(config, app_secret_plain).await
    }

    async fn connect_dingtalk(
        &self,
        config: DingtalkStoredConfig,
        app_secret_plain: String,
    ) -> Result<()> {
        self.stop_stream().await;

        let (msg_tx, mut msg_rx) = mpsc::channel::<ChannelMessage>(64);
        let reply_app_key = config.credentials.app_key.clone();
        let reply_app_secret = app_secret_plain.clone();
        let reply_robot_code = config.bot.robot_code.clone();

        let downloader = Arc::new(DingtalkFileDownloader::new(
            super::dingtalk_token::TokenCache::new(),
            config.credentials.app_key.clone(),
            app_secret_plain.clone(),
            self.dingtalk_downloads_dir(),
        ));

        let stream_client = DingtalkStreamClient::new(
            config.credentials.app_key.clone(),
            app_secret_plain,
            config.bot.robot_code.clone(),
            msg_tx,
        );
        self.set_connection_state(ChannelConnectionState::Connecting, None)
            .await;

        let generation = self.stream_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let connection_arc = Arc::clone(&self.connection);
        let last_error_arc = Arc::clone(&self.last_error);
        let config_store = Arc::clone(&self.config_store);
        let app_for_status = self.app_handle.clone();
        let stream_generation = Arc::clone(&self.stream_generation);
        let message_stream_generation = Arc::clone(&self.stream_generation);
        let conversations_arc = Arc::clone(&self.conversations);
        let on_status = move |new_connection: ChannelConnectionState, error: Option<String>| {
            let connection_arc = connection_arc.clone();
            let last_error_arc = last_error_arc.clone();
            let config_store = config_store.clone();
            let app_for_status = app_for_status.clone();
            let stream_generation = stream_generation.clone();
            let conversations_arc = conversations_arc.clone();
            tokio::spawn(async move {
                if stream_generation.load(Ordering::SeqCst) != generation {
                    log::debug!("[channel] ignoring stale stream status callback");
                    return;
                }
                *connection_arc.write().await = new_connection.clone();
                *last_error_arc.write().await = error.clone();
                // Connected 时按 config 的 robot_code 刷新 is_active_robot；
                // set_connection_state 不走这条回调路径，所以要在这里也处理一次。
                if matches!(new_connection, ChannelConnectionState::Connected) {
                    let current_robot = config_store
                        .read_dingtalk_config()
                        .ok()
                        .flatten()
                        .map(|cfg| cfg.bot.robot_code);
                    let mut convs = conversations_arc.write().await;
                    for c in convs.iter_mut() {
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
                        log::warn!("[channel] failed to build platform state: {:#}", error)
                    }
                }
            });
        };

        let new_token = stream_client.start(on_status);
        let message_cancel = new_token.clone();
        *self.stream_cancel.write().await = Some(new_token);

        // 订阅 reply_manager 到 chat_adapter 的 event bus（整个 manager 生命周期内只做一次，
        // 避免重连/重保存配置时把同一个 subscriber 重复挂载——RuntimeEventBus 没有去重也没有
        // unsubscribe，重复订阅会让 StreamDelta 被回放多次，钉钉 AI Card 上看到字符叠倍）
        if claim_first_subscription(&self.reply_subscribed) {
            let reply_sub = Arc::clone(&self.reply_manager)
                as Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>;
            self.chat_adapter.subscribe_event_listener(reply_sub);
        }

        // 订阅 ask_coordinator 到 event bus（同样只做一次，避免重连时重复订阅）
        if let Some(coordinator) = self.ask_coordinator.as_ref() {
            if claim_first_subscription(&self.ask_subscribed) {
                let sub = Arc::clone(coordinator) as Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>;
                self.chat_adapter.subscribe_event_listener(sub);
            }
        }

        // 消息处理 loop
        let adapter = Arc::clone(&self.chat_adapter);
        let conv_store = Arc::clone(&self.conversation_store);
        let sessions_path = self.sessions_path.clone();
        let seen_ids = Arc::clone(&self.seen_msg_ids);
        let convs = Arc::clone(&self.conversations);
        let app_handle = self.app_handle.clone();
        let reply_manager_ref = Arc::clone(&self.reply_manager);
        let reply_robot_code_for_worker = reply_robot_code.clone();
        let downloader_ref = Arc::clone(&downloader);
        let ask_coordinator_ref = self.ask_coordinator.as_ref().map(Arc::clone);
        let channel_session_ids_ref = Arc::clone(&self.channel_session_ids);

        let message_handle = tokio::spawn(async move {
            let mut router = match ChannelSessionRouter::migrate_or_load(&sessions_path, conv_store.as_ref()) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("[channel] failed to load router: {:#}", e);
                    return;
                }
            };

            while let Some(msg) =
                recv_current_generation_message(
                    &mut msg_rx,
                    &message_stream_generation,
                    generation,
                    &message_cancel,
                )
                .await
            {
                log::info!(
                    "[channel] worker received msg msg_id={} text_len={} attachments={}",
                    msg.msg_id, msg.text.len(), msg.attachments.len()
                );
                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    log::warn!("[channel] worker stream changed before processing, break");
                    break;
                }
                // 幂等去重
                {
                    let mut ids = seen_ids.write().await;
                    if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                        break;
                    }
                    if !msg.msg_id.is_empty() && !ids.insert(msg.msg_id.clone()) {
                        log::debug!("[channel] duplicate msg_id {}, skipping", msg.msg_id);
                        continue;
                    }
                    // 防止无限增长：超过 5000 条时清空
                    if ids.len() > 5000 {
                        ids.clear();
                        log::debug!("[channel] seen_msg_ids cleared (exceeded 5000)");
                    }
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
                let sender_nick_for_create = sender_nick.clone();
                let conv_key_for_create = conv_key.clone();
                let conv_type_for_create = conv_type.clone();
                let session_id = match router.get_or_create_session(&conv_type, &reply_robot_code_for_worker, &conv_key, || {
                    let title = match &conv_type_for_create {
                        ConversationType::Group => format!(
                            "钉钉群 {}",
                            &conv_key_for_create[..conv_key_for_create.len().min(8)]
                        ),
                        ConversationType::Private => {
                            format!("钉钉私聊 {}", &sender_nick_for_create)
                        }
                    };
                    let id = uuid::Uuid::new_v4().to_string();
                    store_ref
                        .create_conversation(&id, &title)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    Ok(id)
                }) {
                    Ok(id) => id,
                    Err(e) => {
                        log::error!("[channel] session routing failed: {:#}", e);
                        continue;
                    }
                };

                // 确保 ask_coordinator registry 能识别此频道 session
                // （std::sync::RwLock write lock 极短，不会阻塞 async reactor）
                {
                    let mut ids = channel_session_ids_ref.write().expect("channel_session_ids poisoned");
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
                // 先让 ask_coordinator 尝试处理这条消息：
                // - NotPending → 继续正常流程
                // - Consumed   → 消息已被用来 resolve 某个 pending ask，跳过本次 turn
                // - Reroute    → 用户切换了话题，以新 turn 重新发起
                if let Some(coordinator) = ask_coordinator_ref.as_ref() {
                    log::info!(
                        "[channel] dispatch to im-ask coordinator session={} text_len={}",
                        session_id, text.len()
                    );
                    match coordinator
                        .try_handle_reply(&crate::runtime::ids::SessionId::new(session_id.clone()), text.clone())
                        .await
                    {
                        Ok(super::ask_coordinator::HandleOutcome::NotPending) => {}
                        Ok(super::ask_coordinator::HandleOutcome::Consumed) => continue,
                        Ok(super::ask_coordinator::HandleOutcome::Reroute { content }) => {
                            log::info!("[channel] IM ask abandoned, rerouting message session={}", session_id);
                            let text = content;
                            let content = match &conv_type {
                                ConversationType::Group => format!("[{}]: {}", sender_nick, text),
                                ConversationType::Private => text,
                            };
                            let request = ChatTurnRequest::new(session_id.clone(), content, vec![]);
                            let run_id = request.run_id.as_str().to_string();
                            let card_target = match &conv_type {
                                ConversationType::Group => CardTarget::Group { open_conversation_id: conv_key.clone() },
                                ConversationType::Private => CardTarget::Private { user_id: msg.sender_id.clone() },
                            };
                            reply_manager_ref.register(
                                session_id.clone(),
                                run_id,
                                reply_app_key.clone(),
                                reply_app_secret.clone(),
                                reply_robot_code.clone(),
                                card_target,
                            ).await;
                            // 同样不能 await — 见下方主路径的死锁说明
                            let adapter_for_reroute = Arc::clone(&adapter);
                            let session_for_log = session_id.clone();
                            tokio::spawn(async move {
                                if let Err(e) = adapter_for_reroute.send_chat_request(request).await {
                                    log::error!("[channel] rerouted send_chat_request failed session={}: {}", session_for_log, e);
                                }
                            });
                            continue;
                        }
                        Err(error) => {
                            log::warn!("[channel] IM ask coordinator failed, falling back to normal turn: {:#}", error);
                        }
                    }
                }

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

                if chat_attachments.is_empty() && text.trim().is_empty() && !msg.attachments.is_empty() {
                    log::warn!(
                        "[channel] all attachments failed and no text, replying via sessionWebhook msgId={}",
                        msg.msg_id
                    );
                    if let Some(webhook) = msg.session_webhook.clone() {
                        tokio::spawn(super::dingtalk_stream::send_session_webhook_text(
                            webhook,
                            "附件下载全部失败，请重发。".to_string(),
                        ));
                    }
                    continue;
                }

                let request = build_channel_chat_request(
                    session_id.clone(),
                    &conv_type,
                    &sender_nick,
                    &text,
                    chat_attachments,
                    &download_failures,
                );
                let run_id = request.run_id.as_str().to_string();

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
                let register_reply = reply_manager_ref.register(
                    session_id.clone(),
                    run_id,
                    reply_app_key.clone(),
                    reply_app_secret.clone(),
                    reply_robot_code.clone(),
                    card_target,
                );
                tokio::select! {
                    biased;
                    _ = message_cancel.cancelled() => break,
                    _ = register_reply => {}
                }

                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }
                // 不能 await send_chat_request — turn 内部可能触发 AskUserQuestion / 权限 ask
                // 等待用户在 IM 端回复，而用户的回复需要本 worker 继续 recv 才能 resolve。
                // 同步 await 会形成死锁：worker 卡在 send_chat_request → 永远收不到用户回复
                // → ask 永远不会 resolve → send_chat_request 永远不返回。
                // 把 turn 跑到独立 task 里，worker 立即返回继续 recv 下一条消息。
                let adapter_for_turn = Arc::clone(&adapter);
                let session_for_log = session_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = adapter_for_turn.send_chat_request(request).await {
                        log::error!("[channel] send_chat_request failed session={}: {}", session_for_log, e);
                    }
                });
            }
        });
        *self.message_task.write().await = Some(message_handle);

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
}

fn claim_first_subscription(flag: &AtomicBool) -> bool {
    flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

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

async fn recv_current_generation_message(
    msg_rx: &mut mpsc::Receiver<ChannelMessage>,
    stream_generation: &Arc<AtomicU64>,
    generation: u64,
    cancel_token: &CancellationToken,
) -> Option<ChannelMessage> {
    let current_gen = stream_generation.load(Ordering::SeqCst);
    if current_gen != generation {
        log::warn!(
            "[channel] worker pre-recv: generation drift my_gen={} current_gen={}, exiting",
            generation, current_gen
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

fn is_current_stream(
    stream_generation: &Arc<AtomicU64>,
    generation: u64,
    cancel_token: &CancellationToken,
) -> bool {
    !cancel_token.is_cancelled() && stream_generation.load(Ordering::SeqCst) == generation
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
    if path.len() >= 2
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[0].is_ascii_alphabetic()
    {
        format!("file:///{}", path.replace('\\', "/"))
    } else {
        format!("file://{}", path)
    }
}

fn build_channel_chat_request(
    session_id: String,
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: Vec<ChatAttachmentRef>,
    download_failures: &[String],
) -> ChatTurnRequest {
    let content = build_compound_content(conv_type, sender_nick, text, &attachments, download_failures);
    let mut request = ChatTurnRequest::new(session_id, content, attachments);
    request.session_attachment_dirs = crate::runtime::path_auth::derive_working_dirs_from_attachments(
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
        file_type: super::dingtalk_download::extension_or_bin(
            &downloaded.path.to_string_lossy(),
        ),
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
            Ok(downloaded) => attachments.push(downloaded_to_chat_attachment(&downloaded, spec.kind)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_message() -> ChannelMessage {
        ChannelMessage {
            msg_id: "msg-1".into(),
            conversation_type: ConversationType::Private,
            conversation_key: "user-1".into(),
            sender_id: "user-1".into(),
            sender_nick: "User 1".into(),
            text: "hello".into(),
            robot_code: "robot-1".into(),
            reply_group_id: "user-1".into(),
            attachments: Vec::new(),
            session_webhook: None,
        }
    }

    #[tokio::test]
    async fn queued_messages_from_stale_generation_are_dropped() {
        let stream_generation = Arc::new(AtomicU64::new(1));
        let cancel_token = CancellationToken::new();
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(test_message()).await.expect("queue message");
        stream_generation.fetch_add(1, Ordering::SeqCst);

        let msg =
            recv_current_generation_message(&mut rx, &stream_generation, 1, &cancel_token).await;

        assert!(msg.is_none());
    }

    #[tokio::test]
    async fn stop_stream_components_cancels_and_awaits_message_worker_before_returning() {
        let stream_generation = Arc::new(AtomicU64::new(0));
        let stream_cancel = Arc::new(RwLock::new(None));
        let message_task = Arc::new(RwLock::new(None));
        let cancel_token = CancellationToken::new();
        let (done_tx, mut done_rx) = mpsc::channel(1);

        *stream_cancel.write().await = Some(cancel_token.clone());
        *message_task.write().await = Some(tokio::spawn(async move {
            cancel_token.cancelled().await;
            done_tx.send(()).await.expect("send done");
        }));

        stop_stream_components(&stream_generation, &stream_cancel, &message_task).await;

        assert_eq!(stream_generation.load(Ordering::SeqCst), 1);
        assert!(stream_cancel.read().await.is_none());
        assert!(message_task.read().await.is_none());
        assert!(done_rx.try_recv().is_ok());
    }

    #[test]
    fn claim_first_subscription_returns_true_only_once() {
        let flag = AtomicBool::new(false);
        assert!(claim_first_subscription(&flag));
        assert!(!claim_first_subscription(&flag));
        assert!(!claim_first_subscription(&flag));
    }
}

pub fn build_conversation_snapshot(
    entries: &[crate::connector::channel::router::RouterEntry],
    conversation_store: &dyn crate::runtime::store::ConversationStore,
    current_robot_code: Option<&str>,
) -> Vec<ChannelConversation> {
    let titles: std::collections::HashMap<String, String> = match conversation_store
        .get_conversations()
    {
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
            log::warn!("[channel] failed to read conversations during hydrate: {:#}", e);
            std::collections::HashMap::new()
        }
    };

    entries
        .iter()
        .map(|entry| {
            let display_name = titles
                .get(&entry.session_id)
                .cloned()
                .unwrap_or_else(|| {
                    log::warn!(
                        "[channel] hydrate: conversation {} not found in store, using placeholder",
                        entry.session_id
                    );
                    "未知会话".to_string()
                });
            let is_active_robot =
                current_robot_code.map(|rc| rc == entry.robot_code).unwrap_or(false);
            ChannelConversation {
                session_id: entry.session_id.clone(),
                platform: Platform::Dingtalk,
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
    use crate::connector::channel::router::RouterEntry;
    use crate::runtime::store::{ConversationStore, InMemoryConversationStore};
    use std::sync::Arc;

    #[test]
    fn snapshot_marks_only_current_robot_as_active() {
        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store.create_conversation("sess-1", "Active Title").unwrap();
        conv_store.create_conversation("sess-2", "Legacy Title").unwrap();

        let entries = vec![
            RouterEntry {
                conversation_type: ConversationType::Private,
                robot_code: "robot-current".into(),
                external_id: "user1".into(),
                session_id: "sess-1".into(),
            },
            RouterEntry {
                conversation_type: ConversationType::Group,
                robot_code: "robot-old".into(),
                external_id: "cid2".into(),
                session_id: "sess-2".into(),
            },
        ];

        let snapshot = build_conversation_snapshot(
            &entries,
            conv_store.as_ref(),
            Some("robot-current"),
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

        let entries = vec![RouterEntry {
            conversation_type: ConversationType::Private,
            robot_code: "robot-1".into(),
            external_id: "user1".into(),
            session_id: "sess-orphan".into(),
        }];

        let snapshot = build_conversation_snapshot(
            &entries,
            conv_store.as_ref(),
            Some("robot-1"),
        );

        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].display_name, "未知会话");
    }

    #[test]
    fn snapshot_marks_all_inactive_when_no_current_robot() {
        let conv_store: Arc<dyn ConversationStore> = Arc::new(InMemoryConversationStore::new());
        conv_store.create_conversation("sess-1", "Title").unwrap();

        let entries = vec![RouterEntry {
            conversation_type: ConversationType::Private,
            robot_code: "robot-1".into(),
            external_id: "user1".into(),
            session_id: "sess-1".into(),
        }];

        let snapshot = build_conversation_snapshot(&entries, conv_store.as_ref(), None);

        assert_eq!(snapshot.len(), 1);
        assert!(!snapshot[0].is_active_robot);
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
        let content = build_compound_content(
            &ConversationType::Private,
            "Alice",
            "",
            &[img],
            &[],
        );
        // Empty text + private chat → markdown is just the image link, no prefix.
        assert_eq!(
            content,
            "![photo.jpg](<file:///Users/u/.renlijia/uploads/photo.jpg>)"
        );
    }

    #[test]
    fn private_file_inlines_as_attachment_link_with_chinese_prefix() {
        let f = make_file_attachment(
            "季度报告.pdf",
            "/Users/u/.renlijia/uploads/季度报告.pdf",
        );
        let content = build_compound_content(
            &ConversationType::Private,
            "Alice",
            "请看",
            &[f],
            &[],
        );
        assert_eq!(
            content,
            "请看\n\n[附件: 季度报告.pdf](<file:///Users/u/.renlijia/uploads/季度报告.pdf>)"
        );
    }

    #[test]
    fn group_with_text_and_image_inserts_prefix_then_blank_line_then_image() {
        let img = make_image_attachment("a.png", "/tmp/a.png");
        let content = build_compound_content(
            &ConversationType::Group,
            "张三",
            "看图",
            &[img],
            &[],
        );
        assert_eq!(
            content,
            "[张三]: 看图\n\n![a.png](<file:///tmp/a.png>)"
        );
    }

    #[test]
    fn empty_text_with_multiple_attachments_lists_each_on_its_own_paragraph() {
        let img = make_image_attachment("x.png", "/tmp/x.png");
        let f = make_file_attachment("y.pdf", "/tmp/y.pdf");
        let content = build_compound_content(
            &ConversationType::Private,
            "Alice",
            "",
            &[img, f],
            &[],
        );
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
        let content = build_compound_content(
            &ConversationType::Group,
            "张三",
            "",
            &[img],
            &[],
        );
        assert_eq!(
            content,
            "[张三]:\n\n![a.png](<file:///tmp/a.png>)"
        );
    }

    #[test]
    fn windows_path_uses_three_slash_file_url_with_forward_slashes() {
        let f = make_file_attachment("doc.docx", "C:\\Users\\u\\doc.docx");
        let content = build_compound_content(
            &ConversationType::Private,
            "Alice",
            "",
            &[f],
            &[],
        );
        assert_eq!(
            content,
            "[附件: doc.docx](<file:///C:/Users/u/doc.docx>)"
        );
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
    fn downloaded_file_to_chat_attachment_maps_kind_and_type() {
        let downloaded = DownloadedFile {
            path: std::path::PathBuf::from("/tmp/a/report.xlsx"),
            file_name: "report.xlsx".into(),
            size: 12,
            sha256: "abc".into(),
            mime_type: Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into()),
        };
        let attachment = downloaded_to_chat_attachment(&downloaded, AttachmentKind::File);
        assert_eq!(attachment.id, "abc");
        assert_eq!(attachment.file_name, "report.xlsx");
        assert_eq!(attachment.kind, "file");
        assert_eq!(attachment.file_type, "xlsx");
    }
}

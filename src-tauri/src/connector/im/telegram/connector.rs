//! `TelegramConnector` — implements `IMConnector`, bridging the long-poll
//! inbound task to a `ChannelMessage` stream and outbound `RuntimeEvent`
//! replies to `sendMessage`.
//!
//! Mirrors `wecom::connector::WecomConnector` in shape:
//! - `start()` spawns the long-poll task; it pumps `ChannelMessage`s through
//!   `msg_tx`, owns offset persistence, and emits status callbacks.
//! - `send()` routes Text/Markdown through `TelegramSender`. On 403 (user
//!   blocked the bot) we drop the user from the allowlist + session_targets
//!   and surface as a transient error so the run-loop survives.
//! - `remember_session()` / `has_session()` give `TelegramReplyForwarder` a
//!   way to filter foreign-platform events.
//!
//! `TelegramApi::new` is fallible (reqwest client build), so the constructor
//! returns `Result<Self>` — diverges from the plan, but the only way to
//! propagate the build error from the manager.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_stream::wrappers::ReceiverStream;

use super::api::TelegramApi;
use super::approval_callback::TelegramApprovalManager;
use super::draft_stream::{DraftAction, TelegramDraftState, RECENT_FINALIZED_TTL};
use super::pairing::PairingCodeStore;
use super::sender::{SenderError, TelegramSender};
use super::types::TelegramSessionTarget;
use super::typing::TelegramTypingHeartbeatManager;
use crate::connector::im::shared::config_store::ChannelConfigStore;
use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};

pub struct TelegramConnector {
    bot_id: String,
    bot_username: String,
    api: Arc<TelegramApi>,
    sender: TelegramSender,
    approval: Arc<TelegramApprovalManager>,
    typing: TelegramTypingHeartbeatManager,
    pairing: PairingCodeStore,
    session_targets: Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
    draft_sessions: Arc<Mutex<HashMap<String, TelegramDraftState>>>,
    recently_finalized: Arc<Mutex<HashMap<String, Instant>>>,
    config_store: Arc<ChannelConfigStore>,
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
}

impl TelegramConnector {
    pub fn new(
        bot_id: String,
        bot_username: String,
        token: String,
        config_store: Arc<ChannelConfigStore>,
        on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    ) -> Result<Self> {
        let api = Arc::new(TelegramApi::new(token)?);
        let sender = TelegramSender::new(api.clone());
        let session_targets = Arc::new(RwLock::new(HashMap::new()));
        let approval = Arc::new(TelegramApprovalManager::new(
            api.clone(),
            Arc::clone(&session_targets),
        ));
        let typing = TelegramTypingHeartbeatManager::new(api.clone());
        // PR4：启动时从磁盘读回 pending pairings（重启抗性）。
        // block_in_place 在 Tauri multi-thread runtime 下可安全使用。
        let pairing_path =
            super::pairing::pending_path_in(&config_store.platform_dir(Platform::Telegram));
        let pairing = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(
                super::pairing::PairingCodeStore::load_from_disk(&pairing_path),
            )
        })
        .with_save_path(pairing_path);
        Ok(Self {
            bot_id,
            bot_username,
            api,
            sender,
            approval,
            typing,
            pairing,
            session_targets,
            draft_sessions: Arc::new(Mutex::new(HashMap::new())),
            recently_finalized: Arc::new(Mutex::new(HashMap::new())),
            config_store,
            on_status,
        })
    }

    /// Test-only constructor that accepts a pre-built `TelegramApi` so
    /// integration tests can wire wiremock URIs through
    /// `TelegramApi::new_with_api_base_for_tests`.
    #[doc(hidden)]
    pub fn for_test(
        bot_id: String,
        bot_username: String,
        api: Arc<TelegramApi>,
        config_store: Arc<ChannelConfigStore>,
    ) -> Self {
        let sender = TelegramSender::new(api.clone());
        let session_targets = Arc::new(RwLock::new(HashMap::new()));
        let approval = Arc::new(TelegramApprovalManager::new(
            api.clone(),
            Arc::clone(&session_targets),
        ));
        let typing = TelegramTypingHeartbeatManager::new(api.clone());
        Self {
            bot_id,
            bot_username,
            api,
            sender,
            approval,
            typing,
            pairing: PairingCodeStore::new(),
            session_targets,
            draft_sessions: Arc::new(Mutex::new(HashMap::new())),
            recently_finalized: Arc::new(Mutex::new(HashMap::new())),
            config_store,
            on_status: Arc::new(|_, _| {}),
        }
    }

    pub fn bot_id(&self) -> &str {
        &self.bot_id
    }
    pub fn bot_username(&self) -> &str {
        &self.bot_username
    }
    pub fn pairing(&self) -> PairingCodeStore {
        self.pairing.clone()
    }
    pub fn api(&self) -> Arc<TelegramApi> {
        self.api.clone()
    }
    pub fn sender(&self) -> &TelegramSender {
        &self.sender
    }
    pub fn approval_sink(&self) -> Arc<TelegramApprovalManager> {
        Arc::clone(&self.approval)
    }
    pub fn typing_manager(&self) -> TelegramTypingHeartbeatManager {
        self.typing.clone()
    }

    /// 由 manager worker 在 `get_or_create_session` 之后调用，让 connector 记住
    /// `session_id → (chat_id, user_id)` 映射。`TelegramReplyForwarder` 通过
    /// `has_session` 过滤掉非 telegram 自己的会话；`send()` 在 ReplyTarget
    /// 缺 `external_conversation_key` 时也走这条 fallback。
    pub async fn remember_session(&self, session_id: String, target: TelegramSessionTarget) {
        self.session_targets
            .write()
            .await
            .insert(session_id, target);
    }

    pub async fn has_session(&self, session_id: &str) -> bool {
        self.session_targets.read().await.contains_key(session_id)
    }

    pub async fn has_active_or_recent_draft(&self, session_id: &str) -> bool {
        if self.draft_sessions.lock().await.contains_key(session_id) {
            return true;
        }
        let now = Instant::now();
        let mut recent = self.recently_finalized.lock().await;
        recent.retain(|_, seen_at| now.duration_since(*seen_at) <= RECENT_FINALIZED_TTL);
        recent.contains_key(session_id)
    }

    /// 给 long_poll 任务借用一份。`Arc<RwLock<HashMap<..>>>` 是 cheap clone。
    pub fn session_targets_handle(&self) -> Arc<RwLock<HashMap<String, TelegramSessionTarget>>> {
        self.session_targets.clone()
    }

    pub fn config_store(&self) -> Arc<ChannelConfigStore> {
        self.config_store.clone()
    }

    /// Best-effort status feedback for the latest inbound user message in a
    /// remembered Telegram session. `emoji=None` clears the reaction.
    pub async fn react_to_latest_inbound(
        &self,
        session_id: &str,
        emoji: Option<&str>,
    ) -> Result<()> {
        let target = {
            let guard = self.session_targets.read().await;
            guard.get(session_id).cloned()
        }
        .ok_or_else(|| anyhow::anyhow!("telegram session target missing"))?;
        let message_id = target
            .last_inbound_message_id
            .ok_or_else(|| anyhow::anyhow!("telegram inbound message_id missing"))?;
        self.api
            .set_message_reaction(target.chat_id, message_id, emoji)
            .await?;
        Ok(())
    }

    pub fn status_callback(
        &self,
    ) -> Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync> {
        self.on_status.clone()
    }

    pub async fn start_typing(&self, session_id: String, run_id: String, chat_id: i64) {
        self.typing.start_run(session_id, run_id, chat_id).await;
    }

    pub async fn stop_typing(&self, session_id: &str, run_id: &str) {
        self.typing.stop_run(session_id, run_id).await;
    }

    async fn resolve_chat_id(&self, target: &ReplyTarget) -> Option<i64> {
        // dispatch path 传完整 ReplyTarget，external_conversation_key = chat_id 字符串。
        // RuntimeEventBus 路径 external_conversation_key 为空 → 从 session_targets 取。
        if let Ok(parsed) = target.external_conversation_key.parse::<i64>() {
            return Some(parsed);
        }
        let guard = self.session_targets.read().await;
        guard.get(&target.session_id).map(|t| t.chat_id)
    }

    async fn resolve_reply_to(&self, chat_id: i64) -> Option<i64> {
        let guard = self.session_targets.read().await;
        guard
            .values()
            .find(|t| t.chat_id == chat_id)
            .and_then(|t| t.last_inbound_message_id)
    }

    async fn mark_recently_finalized(&self, session_id: &str) {
        self.recently_finalized
            .lock()
            .await
            .insert(session_id.to_string(), Instant::now());
    }

    async fn mark_preview_stopped(&self, session_id: &str) {
        if let Some(state) = self.draft_sessions.lock().await.get_mut(session_id) {
            state.stop_preview();
        }
    }

    async fn record_preview_sent(&self, session_id: &str, message_id: i64) {
        if let Some(state) = self.draft_sessions.lock().await.get_mut(session_id) {
            state.record_preview_sent(message_id, Instant::now());
        }
    }

    async fn record_preview_edit(&self, session_id: &str) {
        if let Some(state) = self.draft_sessions.lock().await.get_mut(session_id) {
            state.record_preview_edit(Instant::now());
        }
    }

    async fn clear_draft_after_final(&self, session_id: &str) {
        self.draft_sessions.lock().await.remove(session_id);
        self.mark_recently_finalized(session_id).await;
    }

    async fn map_sender_error(&self, chat_id: i64, err: SenderError) -> ConnectorError {
        match err {
            SenderError::Unauthorized(d) => ConnectorError::AuthExpired(d),
            SenderError::Forbidden(d) => {
                log::warn!(
                    "[telegram-{}] forbidden when sending to chat={}, removing from allowlist",
                    self.bot_id,
                    chat_id
                );
                let _ =
                    remove_user_by_chat(&self.config_store, chat_id, &self.session_targets).await;
                ConnectorError::Transient(format!("forbidden: {d}"))
            }
            SenderError::Transport(d) => ConnectorError::Transient(d),
        }
    }

    async fn send_stream_chunk(
        &self,
        target: ReplyTarget,
        delta: String,
        final_chunk: bool,
    ) -> Result<(), ConnectorError> {
        let chat_id = self
            .resolve_chat_id(&target)
            .await
            .ok_or_else(|| ConnectorError::Transient("telegram chat_id missing".into()))?;
        let reply_to = self.resolve_reply_to(chat_id).await;
        let session_id = target.session_id;
        let action = {
            let mut drafts = self.draft_sessions.lock().await;
            drafts.entry(session_id.clone()).or_default().observe_chunk(
                &delta,
                final_chunk,
                Instant::now(),
            )
        };

        match action {
            DraftAction::None => {
                if final_chunk {
                    log::info!(
                        "[telegram-draft] event=final_noop session={} reason=empty_or_stopped",
                        session_id
                    );
                    self.draft_sessions.lock().await.remove(&session_id);
                }
                Ok(())
            }
            DraftAction::SendPreview { text } => {
                log::info!(
                    "[telegram-draft] event=preview_send_start session={} text_bytes={}",
                    session_id,
                    text.len()
                );
                match self
                    .sender
                    .send_draft_preview_with_reply(chat_id, &text, reply_to)
                    .await
                {
                    Ok(Some(message_id)) => {
                        log::info!(
                            "[telegram-draft] event=preview_sent session={} message_id={} text_bytes={}",
                            session_id,
                            message_id,
                            text.len()
                        );
                        self.record_preview_sent(&session_id, message_id).await;
                    }
                    Ok(None) => {
                        log::info!(
                            "[telegram-draft] event=preview_empty session={}",
                            session_id
                        );
                    }
                    Err(err @ (SenderError::Unauthorized(_) | SenderError::Forbidden(_))) => {
                        return Err(self.map_sender_error(chat_id, err).await);
                    }
                    Err(SenderError::Transport(desc)) => {
                        log::warn!(
                            "[telegram-draft] event=preview_send_error session={} reason={}",
                            session_id,
                            desc
                        );
                        self.mark_preview_stopped(&session_id).await;
                    }
                }
                Ok(())
            }
            DraftAction::EditPreview { message_id, text } => {
                log::info!(
                    "[telegram-draft] event=preview_edit_start session={} message_id={} text_bytes={}",
                    session_id,
                    message_id,
                    text.len()
                );
                match self
                    .sender
                    .edit_draft_preview(chat_id, message_id, &text)
                    .await
                {
                    Ok(()) => {
                        log::info!(
                            "[telegram-draft] event=preview_edit_ok session={} message_id={} text_bytes={}",
                            session_id,
                            message_id,
                            text.len()
                        );
                        self.record_preview_edit(&session_id).await;
                    }
                    Err(err @ (SenderError::Unauthorized(_) | SenderError::Forbidden(_))) => {
                        return Err(self.map_sender_error(chat_id, err).await);
                    }
                    Err(SenderError::Transport(desc)) => {
                        log::warn!(
                            "[telegram-draft] event=preview_edit_error session={} message_id={} reason={}",
                            session_id,
                            message_id,
                            desc
                        );
                        self.mark_preview_stopped(&session_id).await;
                    }
                }
                Ok(())
            }
            DraftAction::SendFinal { text } => {
                log::info!(
                    "[telegram-draft] event=final_send_start session={} mode=no_preview text_bytes={}",
                    session_id,
                    text.len()
                );
                if let Err(err) = self
                    .sender
                    .finalize_draft_markdown_with_reply(chat_id, None, &text, reply_to)
                    .await
                {
                    return Err(self.map_sender_error(chat_id, err).await);
                }
                log::info!(
                    "[telegram-draft] event=final_sent session={} mode=no_preview text_bytes={}",
                    session_id,
                    text.len()
                );
                self.clear_draft_after_final(&session_id).await;
                Ok(())
            }
            DraftAction::EditFinal { message_id, text } => {
                log::info!(
                    "[telegram-draft] event=final_send_start session={} mode=edit_preview message_id={} text_bytes={}",
                    session_id,
                    message_id,
                    text.len()
                );
                if let Err(err) = self
                    .sender
                    .finalize_draft_markdown_with_reply(chat_id, Some(message_id), &text, reply_to)
                    .await
                {
                    return Err(self.map_sender_error(chat_id, err).await);
                }
                log::info!(
                    "[telegram-draft] event=final_sent session={} mode=edit_preview message_id={} text_bytes={}",
                    session_id,
                    message_id,
                    text.len()
                );
                self.clear_draft_after_final(&session_id).await;
                Ok(())
            }
            DraftAction::SendFail { text } => {
                log::warn!(
                    "[telegram-draft] event=fail_send_start session={} mode=no_preview",
                    session_id
                );
                if let Err(err) = self
                    .sender
                    .send_markdown_with_reply(chat_id, &text, reply_to)
                    .await
                {
                    return Err(self.map_sender_error(chat_id, err).await);
                }
                log::warn!(
                    "[telegram-draft] event=fail_sent session={} mode=no_preview",
                    session_id
                );
                self.clear_draft_after_final(&session_id).await;
                Ok(())
            }
            DraftAction::EditFail { message_id, text } => {
                log::warn!(
                    "[telegram-draft] event=fail_send_start session={} mode=edit_preview message_id={}",
                    session_id,
                    message_id
                );
                if let Err(err) = self
                    .sender
                    .finalize_draft_markdown_with_reply(chat_id, Some(message_id), &text, reply_to)
                    .await
                {
                    return Err(self.map_sender_error(chat_id, err).await);
                }
                log::warn!(
                    "[telegram-draft] event=fail_sent session={} mode=edit_preview message_id={}",
                    session_id,
                    message_id
                );
                self.clear_draft_after_final(&session_id).await;
                Ok(())
            }
        }
    }

    async fn send_stream_fail(&self, target: ReplyTarget) -> Result<(), ConnectorError> {
        let chat_id = self
            .resolve_chat_id(&target)
            .await
            .ok_or_else(|| ConnectorError::Transient("telegram chat_id missing".into()))?;
        let reply_to = self.resolve_reply_to(chat_id).await;
        let session_id = target.session_id;
        let action = {
            let mut drafts = self.draft_sessions.lock().await;
            drafts.entry(session_id.clone()).or_default().observe_fail()
        };
        match action {
            DraftAction::SendFail { text } => {
                log::warn!(
                    "[telegram-draft] event=stream_error_fail_start session={} mode=no_preview",
                    session_id
                );
                if let Err(err) = self
                    .sender
                    .send_markdown_with_reply(chat_id, &text, reply_to)
                    .await
                {
                    return Err(self.map_sender_error(chat_id, err).await);
                }
                log::warn!(
                    "[telegram-draft] event=stream_error_fail_sent session={} mode=no_preview",
                    session_id
                );
            }
            DraftAction::EditFail { message_id, text } => {
                log::warn!(
                    "[telegram-draft] event=stream_error_fail_start session={} mode=edit_preview message_id={}",
                    session_id,
                    message_id
                );
                if let Err(err) = self
                    .sender
                    .finalize_draft_markdown_with_reply(chat_id, Some(message_id), &text, reply_to)
                    .await
                {
                    return Err(self.map_sender_error(chat_id, err).await);
                }
                log::warn!(
                    "[telegram-draft] event=stream_error_fail_sent session={} mode=edit_preview message_id={}",
                    session_id,
                    message_id
                );
            }
            _ => {}
        }
        self.clear_draft_after_final(&session_id).await;
        Ok(())
    }
}

#[async_trait]
impl IMConnector for TelegramConnector {
    fn platform(&self) -> Platform {
        Platform::Telegram
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: false,
            outbound_text_streaming: true,
            outbound_markdown: true,
            supports_attachments: true,
            supports_group_chat: false,
            supports_private_chat: true,
            auth_flow: AuthFlow::ApiKey,
        }
    }

    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let (msg_tx, msg_rx) = mpsc::channel::<ChannelMessage>(256);

        // Surface "we're trying" — long_poll 拉到第一轮成功后会 emit Connected。
        (self.on_status)(ChannelConnectionState::Connecting, None);

        let api = self.api.clone();
        let bot_id = self.bot_id.clone();
        let pairing = self.pairing.clone();
        let sender_for_pump = self.sender.clone_inner();
        let session_targets = self.session_targets.clone();
        let config_store = self.config_store.clone();
        let on_status = self.on_status.clone();
        let cancel = ctx.cancel_token.clone();
        self.approval.start_cleanup(cancel.clone());
        let approval = Arc::clone(&self.approval);
        let ask_coordinator = ctx.ask_coordinator.clone();
        let last_get_updates_at = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));

        // Watchdog 独立 task，共享 cancel 和时间戳
        let watchdog_params = super::long_poll::WatchdogParams {
            api: api.clone(),
            bot_id: bot_id.clone(),
            on_status: on_status.clone(),
            last_get_updates_at: last_get_updates_at.clone(),
            cancel: cancel.clone(),
        };
        tokio::spawn(async move { super::long_poll::run_watchdog(watchdog_params).await });

        tokio::spawn(async move {
            super::long_poll::run(super::long_poll::Params {
                api,
                bot_id,
                pairing,
                sender: sender_for_pump,
                session_targets,
                config_store,
                msg_tx,
                on_status,
                ask_coordinator,
                approval,
                cancel,
                last_get_updates_at,
            })
            .await
        });

        Ok(ReceiverStream::new(msg_rx).boxed())
    }

    async fn stop(&self) -> Result<(), ConnectorError> {
        self.typing.stop_all().await;
        Ok(())
    }

    async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
        let text = match content {
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                return self.send_stream_chunk(target, delta, final_chunk).await;
            }
            ReplyContent::AiCardFail => {
                return self.send_stream_fail(target).await;
            }
            ReplyContent::Text(t) | ReplyContent::Markdown(t) => t,
        };
        let chat_id = self
            .resolve_chat_id(&target)
            .await
            .ok_or_else(|| ConnectorError::Transient("telegram chat_id missing".into()))?;
        // 取该 chat 最近一条入站 message_id 作 reply_to_message_id（让 Telegram 端
        // 显示「答复 → 用户问题」的视觉聚合）。私聊 1:1，按 chat_id 找匹配 session。
        let reply_to = {
            let guard = self.session_targets.read().await;
            guard
                .values()
                .find(|t| t.chat_id == chat_id)
                .and_then(|t| t.last_inbound_message_id)
        };
        match self
            .sender
            .send_markdown_with_reply(chat_id, &text, reply_to)
            .await
        {
            Ok(()) => Ok(()),
            Err(err) => Err(self.map_sender_error(chat_id, err).await),
        }
    }
}

async fn remove_user_by_chat(
    config_store: &Arc<ChannelConfigStore>,
    chat_id: i64,
    session_targets: &Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
) -> Result<()> {
    // Telegram 私聊约定：chat_id == user_id。
    let user_id = chat_id;
    // 把用户从持久化 allowlist 移除，迫使下次必须重新走 /start <code> 配对。
    config_store.telegram_remove_allowlist_user(user_id)?;
    let mut guard = session_targets.write().await;
    guard.retain(|_, t| t.user_id != user_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_connector() -> TelegramConnector {
        let dir = tempfile::TempDir::new().unwrap();
        let cs = Arc::new(ChannelConfigStore::new(dir.path().to_path_buf(), None));
        let api = Arc::new(
            TelegramApi::new_with_api_base_for_tests("TOKEN".into(), "http://127.0.0.1:1".into())
                .expect("test api builds"),
        );
        // Keep TempDir alive by leaking it — these constructors are only used
        // by lib-tests that synchronously assert shape, not by I/O paths.
        std::mem::forget(dir);
        TelegramConnector::for_test("8123".into(), "test_bot".into(), api, cs)
    }

    #[tokio::test]
    async fn platform_and_capabilities() {
        let c = build_test_connector();
        assert_eq!(c.platform(), Platform::Telegram);
        let caps = c.capabilities();
        assert!(matches!(caps.inbound, InboundModel::Stream));
        assert!(!caps.supports_group_chat);
        assert!(caps.supports_private_chat);
        assert!(matches!(caps.auth_flow, AuthFlow::ApiKey));
        assert!(caps.outbound_markdown);
        assert!(!caps.outbound_aicard);
        assert!(caps.outbound_text_streaming);
    }

    #[tokio::test]
    async fn remember_and_has_session() {
        let c = build_test_connector();
        c.remember_session(
            "sess-1".into(),
            TelegramSessionTarget {
                chat_id: 42,
                user_id: 42,
                last_inbound_message_id: None,
            },
        )
        .await;
        assert!(c.has_session("sess-1").await);
        assert!(!c.has_session("sess-2").await);
    }
}

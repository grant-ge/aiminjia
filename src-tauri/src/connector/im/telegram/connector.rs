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

use anyhow::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;

use super::api::TelegramApi;
use super::pairing::PairingCodeStore;
use super::sender::{SenderError, TelegramSender};
use super::types::TelegramSessionTarget;
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
    pairing: PairingCodeStore,
    session_targets: Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
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
        // PR4：启动时从磁盘读回 pending pairings（重启抗性）。
        // block_in_place 在 Tauri multi-thread runtime 下可安全使用。
        let pairing_path =
            super::pairing::pending_path_in(&config_store.platform_dir(Platform::Telegram));
        let pairing = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(super::pairing::PairingCodeStore::load_from_disk(&pairing_path))
        })
        .with_save_path(pairing_path);
        Ok(Self {
            bot_id,
            bot_username,
            api,
            sender,
            pairing,
            session_targets: Arc::new(RwLock::new(HashMap::new())),
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
        Self {
            bot_id,
            bot_username,
            api,
            sender,
            pairing: PairingCodeStore::new(),
            session_targets: Arc::new(RwLock::new(HashMap::new())),
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

    /// 给 long_poll 任务借用一份。`Arc<RwLock<HashMap<..>>>` 是 cheap clone。
    pub fn session_targets_handle(&self) -> Arc<RwLock<HashMap<String, TelegramSessionTarget>>> {
        self.session_targets.clone()
    }

    pub fn config_store(&self) -> Arc<ChannelConfigStore> {
        self.config_store.clone()
    }

    pub fn status_callback(
        &self,
    ) -> Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync> {
        self.on_status.clone()
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
            outbound_text_streaming: false,
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
        let last_get_updates_at =
            std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));

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
                cancel,
                last_get_updates_at,
            })
            .await
        });

        Ok(ReceiverStream::new(msg_rx).boxed())
    }

    async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
        let chat_id = self
            .resolve_chat_id(&target)
            .await
            .ok_or_else(|| ConnectorError::Transient("telegram chat_id missing".into()))?;
        let text = match content {
            ReplyContent::Text(t) | ReplyContent::Markdown(t) => t,
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                if final_chunk {
                    delta
                } else {
                    // 中间 chunk 丢弃；最终结果由 reply forwarder 的
                    // MessagePersisted 一次性下发。
                    return Ok(());
                }
            }
            ReplyContent::AiCardFail => "❌ 处理失败，请重试".to_string(),
        };
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
            Err(SenderError::Unauthorized(d)) => Err(ConnectorError::AuthExpired(d)),
            Err(SenderError::Forbidden(d)) => {
                // 用户从 Telegram 端 block 了 bot → 删 allowlist + session_targets，
                // 但 connector 继续运行（其它已配对用户不受影响）。
                log::warn!(
                    "[telegram-{}] forbidden when sending to chat={}, removing from allowlist",
                    self.bot_id,
                    chat_id
                );
                let _ =
                    remove_user_by_chat(&self.config_store, chat_id, &self.session_targets).await;
                Err(ConnectorError::Transient(format!("forbidden: {d}")))
            }
            Err(SenderError::Transport(d)) => Err(ConnectorError::Transient(d)),
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

//! `WhatsAppConnector` —— PR4 升级版（持 inbound_tx + dedup；start() 真实现）。
//!
//! 实施进度：
//! - PR1：stub，start/send 返 NotSupported
//! - PR2：加 bot_handle + pairing_state 字段；stop() 真做
//!   JoinHandle::abort()；start/send 仍 NotSupported
//! - PR3：begin/poll_registration 真做扫码（用 PairingState）
//! - **PR4（本 PR）**：加 inbound_tx + dedup 字段；start() 创 mpsc + 装 sink +
//!   返 ReceiverStream(rx).boxed()；stop() 先 drop tx 再 abort bot
//! - PR5：send() 真做出站
//! - PR6：AI Card edit 路径
//! - PR7：媒体下载
//! - PR8：集成测试 + UI banner

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;

use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};

use super::types::PairingState;

pub struct WhatsAppConnector {
    /// 状态回调。Connected / PairSuccess / PairError 时驱动 manager 更新
    /// connection state，让前端看到正确状态。
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>,

    /// bot.run() 起的 task join handle。spec v3 §3.4：wa-rs 无 graceful stop，
    /// 关闭靠 abort()。PR2 在 stop() 里调用；PR3 真起 Bot 后会 set 它。
    pub(crate) bot_handle: Arc<Mutex<Option<JoinHandle<()>>>>,

    /// 扫码状态机。PR3 begin_registration 改这里，poll_registration 读这里。
    /// PR2 只声明字段并默认 Idle，让 PR3 实施时无须改 struct 形状。
    /// `pub(crate)` 对齐 `bot_handle`，让 PR3 在 manager.rs 直接 set/read。
    pub(crate) pairing_state: Arc<Mutex<PairingState>>,

    /// PR4 入站消息 sink。`runtime::handle_event` 的 closure capture 这个 Arc；
    /// `start()` 装 mpsc::Sender 进去；`stop()` 取走 Sender 让 BoxStream 结束。
    /// closure build 一次，sink 可以多次切换——典型"运行时切换 sink"模式。
    pub(crate) inbound_tx:
        Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Sender<ChannelMessage>>>>,

    /// PR5 出站 client 句柄。`runtime::start_bot` 在 bot.run() 前存入；
    /// `stop()` 清空；`send()` lock 读取。同 inbound_tx 的"运行时切换"模式。
    pub(crate) bot_client: Arc<tokio::sync::Mutex<Option<Arc<wa_rs::client::Client>>>>,

    /// PR6 入站消息上下文表，manager worker 写、send() 读。spec §6.1。
    pub(crate) session_inbound: Arc<
        tokio::sync::RwLock<std::collections::HashMap<String, super::types::WhatsAppLastInbound>>,
    >,
    /// PR6 AI Card 状态机 per session。
    pub(crate) fallback_buffers: Arc<
        tokio::sync::Mutex<std::collections::HashMap<String, super::aicard::WhatsAppAiCardSession>>,
    >,

    /// PR4 入站去重。connector 内部 owns；runtime closure 通过 Arc::clone 使用。
    pub(crate) dedup: Arc<crate::connector::im::shared::dedup::MessageDedupSet>,

    /// PR7 媒体下载目标目录。factory 传入（来自 AiJiaHome::tmp_whatsapp_downloads_dir()）；
    /// start_pairing_session 用它构造 WhatsAppMediaDownloader 传给 runtime::start_bot。
    pub(crate) attachments_dir: std::path::PathBuf,
}

impl WhatsAppConnector {
    pub fn new() -> Self {
        Self::with_status_callback(
            Arc::new(|_state, _err| {}),
            std::path::PathBuf::from("/tmp/whatsapp_downloads"),
        )
    }

    pub fn with_status_callback(
        on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>,
        attachments_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            on_status,
            bot_handle: Arc::new(Mutex::new(None)),
            pairing_state: Arc::new(Mutex::new(PairingState::default())),
            inbound_tx: Arc::new(tokio::sync::Mutex::new(None)),
            bot_client: Arc::new(tokio::sync::Mutex::new(None)),
            session_inbound: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            fallback_buffers: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            dedup: Arc::new(
                crate::connector::im::shared::dedup::MessageDedupSet::with_default_cap(),
            ),
            attachments_dir,
        }
    }

    /// 起一次 pairing 会话。**Manager 入口**：manager.begin_whatsapp_registration
    /// 解析 scope → 路径 → 调本方法。spec v3 §3.6。
    pub async fn start_pairing_session(
        &self,
        paths: super::session::WhatsAppPaths,
    ) -> anyhow::Result<()> {
        // 启动备份兜底（spec v3 §3.3）
        let _backed_up = super::session::backup_session_db_if_present(&paths)?;
        paths.ensure_base_dir()?;

        // PR7：build downloader（复用 bot_client Arc，下载时 lock 拿 client）
        let downloader = Arc::new(super::download::WhatsAppMediaDownloader::new(
            Arc::clone(&self.bot_client),
            self.attachments_dir.clone(),
        ));

        // 起 Bot，注入 on_status / inbound_tx / dedup 回调以驱动 manager 的 connection state
        // 和入站消息管道。
        let handle = super::runtime::start_bot(
            paths,
            Arc::clone(&self.pairing_state),
            Arc::clone(&self.on_status),
            Arc::clone(&self.inbound_tx),
            Arc::clone(&self.dedup),
            Arc::clone(&self.bot_client),
            downloader,
        )
        .await?;

        // 存 join handle
        *self.bot_handle.lock().await = Some(handle);
        Ok(())
    }

    /// Manager 入口：拉一次 PairingState 当前快照，给 poll_whatsapp_registration 用。
    pub async fn poll_pairing_state(&self) -> super::types::PairingState {
        self.pairing_state.lock().await.clone()
    }

    /// manager worker 在 push pending 之前调，把入站消息上下文存下来，给 send() 反查用。
    pub async fn remember_inbound(
        &self,
        session_id: String,
        last: super::types::WhatsAppLastInbound,
    ) {
        self.session_inbound.write().await.insert(session_id, last);
    }

    /// 给 ReplyForwarder 用：检查 session_id 是否归 WhatsApp。
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.session_inbound.read().await.contains_key(session_id)
    }

    /// 给 ReplyForwarder 用：反查 chat_jid 用于构造 ReplyTarget。
    pub async fn lookup_chat_jid(&self, session_id: &str) -> Option<String> {
        self.session_inbound
            .read()
            .await
            .get(session_id)
            .map(|last| last.chat_jid.clone())
    }
}

impl Default for WhatsAppConnector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IMConnector for WhatsAppConnector {
    fn platform(&self) -> Platform {
        Platform::Whatsapp
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        // spec §1 capability 表逐字对齐。PR2 不动 capability 值。
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: false,
            outbound_text_streaming: true,
            outbound_markdown: false,
            supports_attachments: true,
            supports_group_chat: false,
            supports_private_chat: true,
            auth_flow: AuthFlow::QRCode,
        }
    }

    /// spec v3 §4.1。创 mpsc(64) channel，装 sender 进 inbound_tx sink，
    /// 返 ReceiverStream(rx).boxed()。
    ///
    /// 可被 manager 多次调用（rebuild stream）——旧 sender 被替换，旧 stream 自然 end；
    /// runtime closure capture 的 Arc 始终指向同一个 Mutex，无需重建 Bot。
    async fn start(
        &self,
        _ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let (tx, rx) = tokio::sync::mpsc::channel::<ChannelMessage>(64);
        // 装入 sink；如果已有旧 sink 则替换（旧 stream 自然 end）
        *self.inbound_tx.lock().await = Some(tx);
        log::info!("[whatsapp] inbound stream attached");
        Ok(ReceiverStream::new(rx).boxed())
    }

    async fn send(&self, target: ReplyTarget, content: ReplyContent) -> Result<(), ConnectorError> {
        use std::time::Instant;

        // 1. 先拿 client 句柄（lock-first 顺序，与 stop() 对称）
        let client = self.bot_client.lock().await.clone();
        let client = match client {
            Some(c) => c,
            None => {
                return Err(ConnectorError::Transient(
                    "whatsapp: bot not running, cannot send".into(),
                ))
            }
        };

        // 2. 按 ReplyContent 路由
        match content {
            ReplyContent::Text(t) => {
                super::sender::send_text(&client, &target.external_conversation_key, &t).await?;
            }
            ReplyContent::Markdown(m) => {
                let wa_text = super::markdown::strip_to_wa(&m);
                super::sender::send_text(&client, &target.external_conversation_key, &wa_text)
                    .await?;
            }
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                let session_id = target.session_id.clone();
                let chat_jid_str = target.external_conversation_key.clone();
                let now = Instant::now();

                // 1. 拿状态 + observe
                let action = {
                    let mut buffers = self.fallback_buffers.lock().await;
                    let session = buffers.entry(session_id.clone()).or_default();
                    session.observe_chunk(&delta, final_chunk, now)
                };

                // 2. 执行 action
                use super::aicard::AiCardAction;
                match action {
                    AiCardAction::Buffer
                    | AiCardAction::DropAfterFinalized
                    | AiCardAction::Noop => {}
                    AiCardAction::SendFinal { text } => {
                        // 1st chunk 就 final：直接发完整文本（跟 PR5 一致行为）
                        super::sender::send_text(&client, &chat_jid_str, &text).await?;
                        self.fallback_buffers.lock().await.remove(&session_id);
                    }
                    AiCardAction::StartPlaceholder { text } => {
                        // 先发 reaction ⏳ 到用户那条原消息（best-effort，失败不阻塞）
                        {
                            let already_sent = self
                                .fallback_buffers
                                .lock()
                                .await
                                .get(&session_id)
                                .map(|s| s.reaction_sent)
                                .unwrap_or(false);
                            if !already_sent {
                                if let Some(last) =
                                    self.session_inbound.read().await.get(&session_id).cloned()
                                {
                                    let _ = super::sender::send_reaction(
                                        &client,
                                        &last.chat_jid,
                                        &last.msg_id,
                                        &last.sender_jid,
                                        last.is_group,
                                        "⏳",
                                    )
                                    .await
                                    .inspect_err(|e| {
                                        log::debug!(
                                            "[whatsapp] reaction send failed (best-effort): {e}"
                                        )
                                    });
                                }
                            }
                        }
                        // 然后发 placeholder 文本
                        let msg_id =
                            super::sender::send_text(&client, &chat_jid_str, &text).await?;
                        // record_placeholder + mark reaction_sent
                        let mut buffers = self.fallback_buffers.lock().await;
                        if let Some(session) = buffers.get_mut(&session_id) {
                            session.record_placeholder(msg_id, now);
                            session.reaction_sent = true;
                        }
                    }
                    AiCardAction::EditPlaceholder { msg_id, text } => {
                        // edit 失败静默丢 + 下次 chunk 重试（不调 record_edit_success）
                        match super::sender::edit_text(&client, &chat_jid_str, &msg_id, &text).await
                        {
                            Ok(()) => {
                                let mut buffers = self.fallback_buffers.lock().await;
                                if let Some(session) = buffers.get_mut(&session_id) {
                                    session.record_edit_success(now);
                                }
                            }
                            Err(e) => {
                                log::debug!(
                                    "[whatsapp] edit_placeholder failed (silent retry): {e}"
                                )
                            }
                        }
                    }
                    AiCardAction::EditFinal { msg_id, text } => {
                        // final edit 失败时 fallback 发新的 send_text（不让用户丢内容）
                        if let Err(e) =
                            super::sender::edit_text(&client, &chat_jid_str, &msg_id, &text).await
                        {
                            log::warn!(
                                "[whatsapp] edit_final failed, falling back to send_text: {e}"
                            );
                            super::sender::send_text(&client, &chat_jid_str, &text).await?;
                        }
                        // 换 reaction ⏳ → ✅（best-effort）
                        if let Some(last) =
                            self.session_inbound.read().await.get(&session_id).cloned()
                        {
                            let _ = super::sender::send_reaction(
                                &client,
                                &last.chat_jid,
                                &last.msg_id,
                                &last.sender_jid,
                                last.is_group,
                                "✅",
                            )
                            .await
                            .inspect_err(|e| log::debug!("[whatsapp] final reaction failed: {e}"));
                        }
                        self.fallback_buffers.lock().await.remove(&session_id);
                    }
                    AiCardAction::EditFailMessage { .. } => {
                        unreachable!("EditFailMessage 只能从 observe_fail 出来")
                    }
                }
            }
            ReplyContent::AiCardFail => {
                let session_id = target.session_id.clone();
                let chat_jid_str = target.external_conversation_key.clone();
                let action = {
                    let mut buffers = self.fallback_buffers.lock().await;
                    let session = buffers.entry(session_id.clone()).or_default();
                    session.observe_fail()
                };
                use super::aicard::AiCardAction;
                match action {
                    AiCardAction::EditFailMessage { msg_id } => {
                        let _ = super::sender::edit_text(
                            &client,
                            &chat_jid_str,
                            &msg_id,
                            "_[生成失败]_",
                        )
                        .await
                        .inspect_err(|e| log::debug!("[whatsapp] edit_fail_message failed: {e}"));
                        // reaction ⏳ → ❌
                        if let Some(last) =
                            self.session_inbound.read().await.get(&session_id).cloned()
                        {
                            let _ = super::sender::send_reaction(
                                &client,
                                &last.chat_jid,
                                &last.msg_id,
                                &last.sender_jid,
                                last.is_group,
                                "❌",
                            )
                            .await
                            .inspect_err(|e| log::debug!("[whatsapp] fail reaction failed: {e}"));
                        }
                    }
                    AiCardAction::Noop => {
                        // 没 placeholder：还没发任何消息就 fail，发一条简单失败提示
                        super::sender::send_text(&client, &chat_jid_str, "❌ 处理失败，请重试")
                            .await?;
                    }
                    other => log::warn!("[whatsapp] unexpected aicard action on fail: {other:?}"),
                }
                self.fallback_buffers.lock().await.remove(&session_id);
            }
        }
        Ok(())
    }

    async fn begin_registration(
        &self,
        _req: &crate::connector::im::trait_def::RegistrationRequest,
    ) -> Result<crate::connector::im::trait_def::RegistrationBegin, ConnectorError> {
        // Manager 走 inherent `start_pairing_session(paths)` 方法，因为
        // 需要传 paths 参数（trait 签名没暴露）。本 trait method 仅保持
        // 形状一致；实际不会被调用。
        Err(ConnectorError::NotSupported(
            "whatsapp::begin_registration — manager 走 start_pairing_session(paths)",
        ))
    }

    async fn poll_registration(
        &self,
        _req: &crate::connector::im::trait_def::PollRequest,
    ) -> Result<crate::connector::im::trait_def::RegistrationPoll, ConnectorError> {
        Err(ConnectorError::NotSupported(
            "whatsapp::poll_registration — manager 走 poll_pairing_state()",
        ))
    }

    /// spec v3 §3.4 + §4.1：先 drop inbound sink 让 BoxStream 自然结束，
    /// 再 abort Bot task。
    ///
    /// wa-rs Bot 没 graceful shutdown，唯一手段 JoinHandle::abort()。
    /// 不 await handle —— abort 后 await 会返 `Err(JoinError::Cancelled)`，
    /// 这正是预期，不需等"任务完整跑完"。
    ///
    /// SqliteStore 的 Arc 在 connector 被 drop 时由 r2d2 自动回收 connection pool；
    /// in-flight 的 spawn_blocking 写入可能丢，由 session.db.bak 兜底
    /// （spec v3 §3.3）。
    async fn stop(&self) -> Result<(), ConnectorError> {
        // 0. 清空 PR6 状态表
        self.session_inbound.write().await.clear();
        self.fallback_buffers.lock().await.clear();
        // 1. drop bot_client → send() 调用返回 Transient
        *self.bot_client.lock().await = None;
        // 2. drop inbound sink → consumer 收到 None（stream 自然结束）
        *self.inbound_tx.lock().await = None;
        // 3. abort bot task
        if let Some(handle) = self.bot_handle.lock().await.take() {
            handle.abort();
            log::info!("[whatsapp] bot task aborted");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::connector::im::shared::config_store::ChannelConfigStore;
    use crate::runtime::event_bus::RuntimeEventBus;
    use crate::runtime::ids::SessionId;
    use crate::runtime::pending::queue_manager::{ConvDirResolver, PendingQueueManager};
    use crate::runtime::pending::types::PendingConfig;
    use crate::runtime::run_registry::RuntimeRunRegistry;
    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    /// Stub resolver that hands back the conversations root.
    struct StubConvResolver(std::path::PathBuf);

    impl ConvDirResolver for StubConvResolver {
        fn conversation_dir(&self, session_id: &SessionId) -> Option<std::path::PathBuf> {
            let dir = self.0.join(session_id.as_str());
            std::fs::create_dir_all(&dir).ok()?;
            Some(dir)
        }

        fn is_archived(&self, _session_id: &SessionId) -> bool {
            false
        }

        fn conversations_root(&self) -> std::path::PathBuf {
            self.0.clone()
        }
    }

    fn test_ctx() -> ConnectorContext {
        let dir = TempDir::new().unwrap();
        let cs = Arc::new(ChannelConfigStore::new(dir.path().to_path_buf(), None));
        let registry = Arc::new(RuntimeRunRegistry::new());
        let bus = Arc::new(RuntimeEventBus::new());
        let resolver = Arc::new(StubConvResolver(dir.path().to_path_buf()));
        let pending_manager =
            PendingQueueManager::new(registry, bus, resolver, PendingConfig::default());
        // Leak temp dir — sync test that doesn't need cleanup pressure.
        std::mem::forget(dir);
        ConnectorContext {
            config_store: cs,
            secure_storage: None,
            ask_coordinator: None,
            pending_manager,
            cancel_token: CancellationToken::new(),
        }
    }

    #[test]
    fn capabilities_match_phase4_spec() {
        let c = WhatsAppConnector::new();
        let caps = c.capabilities();
        assert_eq!(caps.inbound, InboundModel::Stream);
        assert!(!caps.outbound_aicard);
        assert!(caps.outbound_text_streaming);
        assert!(!caps.outbound_markdown);
        assert!(caps.supports_attachments);
        assert!(!caps.supports_group_chat);
        assert!(caps.supports_private_chat);
        assert_eq!(caps.auth_flow, AuthFlow::QRCode);
    }

    #[test]
    fn platform_is_whatsapp() {
        let c = WhatsAppConnector::new();
        assert_eq!(c.platform(), Platform::Whatsapp);
    }

    #[tokio::test]
    async fn send_returns_transient_when_bot_not_running() {
        // bot_client 未装入 → send 应返回 Transient("bot not running")
        let c = WhatsAppConnector::new();
        let err = c
            .send(
                ReplyTarget {
                    session_id: "sess".into(),
                    external_conversation_key: "8613800138000@s.whatsapp.net".into(),
                },
                ReplyContent::Text("hi".into()),
            )
            .await
            .unwrap_err();
        match err {
            ConnectorError::Transient(msg) => assert!(msg.contains("bot not running")),
            other => panic!("expected Transient, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_aicard_returns_transient_when_bot_not_running() {
        // AiCardChunk 路径在 bot_client 缺失时也返回 Transient（lock-first ordering）。
        let c = WhatsAppConnector::new();
        let err = c
            .send(
                ReplyTarget {
                    session_id: "sess".into(),
                    external_conversation_key: "8613800138000@s.whatsapp.net".into(),
                },
                ReplyContent::AiCardChunk {
                    delta: "partial".into(),
                    final_chunk: false,
                },
            )
            .await
            .unwrap_err();
        match err {
            ConnectorError::Transient(msg) => assert!(msg.contains("bot not running")),
            other => panic!("expected Transient, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stop_is_noop_when_no_bot_handle() {
        // PR2：没起过 Bot，stop() 应该 silently Ok。
        let c = WhatsAppConnector::new();
        c.stop()
            .await
            .expect("stop should be Ok when no bot handle");
    }

    #[tokio::test]
    async fn stop_aborts_the_bot_handle_when_present() {
        // 直接对 connector 的 bot_handle (pub(crate)) 注入一个 join handle，
        // 验证 stop 会 abort 它并清空 Option。
        let c = WhatsAppConnector::new();
        let task: JoinHandle<()> = tokio::spawn(async {
            // 跑一个永远不结束的 task，模拟 bot.run()。
            std::future::pending::<()>().await;
        });
        *c.bot_handle.lock().await = Some(task);

        c.stop().await.expect("stop should abort bot handle");

        // 验证 handle 被取走（再调一次 stop 是 noop）
        assert!(c.bot_handle.lock().await.is_none());
    }

    #[tokio::test]
    async fn start_attaches_inbound_sink_and_returns_box_stream() {
        let c = WhatsAppConnector::new();
        let ctx = test_ctx();
        let mut stream = c.start(ctx).await.expect("start ok");
        // tx 应已装进 inbound_tx
        assert!(c.inbound_tx.lock().await.is_some());
        // 没人 push tx，stream 应该 pending（用 timeout 验证）
        let res = tokio::time::timeout(std::time::Duration::from_millis(50), stream.next()).await;
        assert!(res.is_err(), "stream should pend with no senders posting");
    }

    #[tokio::test]
    async fn stop_drops_inbound_tx_so_stream_ends() {
        let c = WhatsAppConnector::new();
        let ctx = test_ctx();
        let mut stream = c.start(ctx).await.expect("start ok");
        c.stop().await.expect("stop ok");
        assert!(c.inbound_tx.lock().await.is_none());
        // tx 已 drop → next 应该返 None
        let res = tokio::time::timeout(std::time::Duration::from_millis(50), stream.next()).await;
        assert!(
            matches!(res.expect("stream not pending"), None),
            "stream should have ended"
        );
    }

    #[tokio::test]
    async fn pushing_message_through_inbound_tx_arrives_at_stream() {
        let c = WhatsAppConnector::new();
        let ctx = test_ctx();
        let mut stream = c.start(ctx).await.expect("start ok");
        // 模拟 runtime closure 直接 push
        let tx = c.inbound_tx.lock().await.clone().expect("tx installed");
        tokio::spawn(async move {
            let cm = ChannelMessage {
                msg_id: "M1".into(),
                conversation_type: crate::connector::im::types::ConversationType::Private,
                conversation_key: "k".into(),
                sender_id: "s".into(),
                sender_nick: "Alice".into(),
                text: "hi".into(),
                robot_code: String::new(),
                reply_group_id: String::new(),
                attachments: vec![],
                session_webhook: None,
                created_at_ms: Some(0),
            };
            let _ = tx.send(cm).await;
        });
        let got = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .expect("not pending")
            .expect("not closed");
        assert_eq!(got.msg_id, "M1");
    }
}

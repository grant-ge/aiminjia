//! DingtalkReplyManager — 订阅 RuntimeEventBus，将 AI 回复流式投放到钉钉 AI Card

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::human_interaction::{ImPlatform, OutputBinding, RunOutputBindingRegistry};
use crate::runtime::ids::{RunId, SessionId};

use super::super::dingtalk::card::{self as dingtalk_card, CardInstance, CardTarget};
use super::super::dingtalk::token::TokenCache;

const ASK_CARD_FALLBACK_TEXT: &str = "需要你补充信息后我才能继续。";
const CARD_UPDATE_DEBOUNCE: Duration = Duration::from_millis(400);
const PENDING_APPROVAL_ACK_SUPPRESS_FOR: Duration = Duration::from_secs(60);

fn non_empty_ask_content(markdown: String) -> String {
    if markdown.trim().is_empty() {
        ASK_CARD_FALLBACK_TEXT.to_string()
    } else {
        markdown
    }
}

/// 卡片的生命周期状态
#[derive(Debug)]
enum CardLifecycle {
    Streaming(CardInstance),
    Finished,
}

/// 一个正在进行的回复上下文，关联到一个 session 的一次 run
#[derive(Debug)]
struct ReplyContext {
    card_lifecycle: CardLifecycle,
    accumulated_text: String,
    app_key: String,
    app_secret: String,
    run_id: String,
}

/// 已知的 session 凭证缓存。每次该 session 来 IM 消息都会刷新一次，
/// 用于显式反馈卡以及 connector send(AiCardChunk) 的兜底投放。
#[derive(Debug, Clone)]
struct ReplyCredentials {
    app_key: String,
    app_secret: String,
    robot_code: String,
    target: CardTarget,
}

#[derive(Debug)]
struct ScheduledCardUpdate {
    final_chunk: bool,
    dirty: bool,
}

pub struct DingtalkReplyManager {
    /// session_id/run_id → ReplyContext
    contexts: Arc<Mutex<HashMap<String, ReplyContext>>>,
    /// session_id → 凭证（活的钉钉会话）。clear 时一并清空。
    session_credentials: Arc<Mutex<HashMap<String, ReplyCredentials>>>,
    /// session_id/run_id → 后台卡片刷新状态。事件总线只更新内存并调度，
    /// 网络 PUT 由后台任务合并执行，避免反压 app 侧 streaming。
    scheduled_card_updates: Arc<Mutex<HashMap<String, ScheduledCardUpdate>>>,
    /// session_id → 最近一次等待审批提示，用于避免 IM 普通消息连续刷屏。
    pending_approval_ack_cache: Arc<Mutex<HashMap<String, (String, Instant)>>>,
    output_binding_registry: Arc<RunOutputBindingRegistry>,
    token_cache: TokenCache,
}

impl DingtalkReplyManager {
    pub fn new() -> Self {
        Self::new_with_output_binding_registry(Arc::new(RunOutputBindingRegistry::new()))
    }

    pub fn new_with_output_binding_registry(
        output_binding_registry: Arc<RunOutputBindingRegistry>,
    ) -> Self {
        Self {
            contexts: Arc::new(Mutex::new(HashMap::new())),
            session_credentials: Arc::new(Mutex::new(HashMap::new())),
            scheduled_card_updates: Arc::new(Mutex::new(HashMap::new())),
            pending_approval_ack_cache: Arc::new(Mutex::new(HashMap::new())),
            output_binding_registry,
            token_cache: TokenCache::new(),
        }
    }

    pub async fn clear(&self) {
        self.contexts.lock().await.clear();
        self.session_credentials.lock().await.clear();
        self.scheduled_card_updates.lock().await.clear();
        self.pending_approval_ack_cache.lock().await.clear();
    }

    /// 记住一个 IM session 的钉钉凭证。worker 收到任何一条消息都该调一次，
    /// 后续显式反馈卡或 connector send(AiCardChunk) 可据此回到原钉钉会话。
    pub async fn remember_credentials(
        &self,
        session_id: String,
        app_key: String,
        app_secret: String,
        robot_code: String,
        target: CardTarget,
    ) {
        self.session_credentials.lock().await.insert(
            session_id,
            ReplyCredentials {
                app_key,
                app_secret,
                robot_code,
                target,
            },
        );
    }

    /// 在 AI 处理开始前调用，创建 AI Card 并注册回复上下文。
    /// Card 创建失败时不注册（消息继续处理，但钉钉不会收到回复）。
    pub async fn register(
        &self,
        session_id: String,
        run_id: String,
        app_key: String,
        app_secret: String,
        robot_code: String,
        target: CardTarget,
    ) {
        self.pending_approval_ack_cache
            .lock()
            .await
            .remove(&session_id);
        let card = dingtalk_card::create_and_deliver_card(
            &self.token_cache,
            &app_key,
            &app_secret,
            &robot_code,
            &target,
        )
        .await;

        if let Some(card) = card {
            let mut contexts = self.contexts.lock().await;
            let context_key = card_context_key(&session_id, &run_id);
            contexts.insert(
                context_key,
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(card),
                    accumulated_text: String::new(),
                    app_key,
                    app_secret,
                    run_id,
                },
            );
            log::info!(
                "[reply-manager] registered context for session {}",
                session_id
            );
        } else {
            log::warn!(
                "[reply-manager] card creation failed for session {}, reply will not be sent to DingTalk",
                session_id
            );
        }
    }

    /// Connector send(AiCardChunk) 的执行体：按 session_id 找上下文，攒 delta，按需 lazy-create / stream / finish。
    /// 调用方（DingtalkConnector::send）只负责把 target.session_id 喂进来，不感知卡片生命周期。
    pub async fn dispatch_chunk(
        &self,
        session_id: &str,
        delta: &str,
        final_chunk: bool,
    ) -> anyhow::Result<()> {
        let mut contexts = self.contexts.lock().await;
        let Some(key) = find_context_key_for_session(&contexts, session_id) else {
            return Ok(());
        };
        let Some(ctx) = contexts.get_mut(&key) else {
            return Ok(());
        };

        ctx.accumulated_text.push_str(delta);
        let text = ctx.accumulated_text.clone();
        let app_key = ctx.app_key.clone();
        let app_secret = ctx.app_secret.clone();
        let cache = self.token_cache.clone();

        if final_chunk {
            if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
                if let Err(e) =
                    dingtalk_card::finish_card(&cache, &app_key, &app_secret, card, &text).await
                {
                    log::warn!("[reply-manager] finish_card via dispatch failed: {:#}", e);
                }
            }
            contexts.remove(&key);
        } else if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
            if let Err(e) =
                dingtalk_card::stream_card(&cache, &app_key, &app_secret, card, &text, false).await
            {
                log::warn!("[reply-manager] stream_card via dispatch failed: {:#}", e);
            }
        }
        Ok(())
    }

    /// Connector send(AiCardFail) 的执行体：把当前 card 标记为 fail 并清理上下文。
    pub async fn dispatch_fail(&self, session_id: &str) -> anyhow::Result<()> {
        let mut contexts = self.contexts.lock().await;
        let Some(key) = find_context_key_for_session(&contexts, session_id) else {
            return Ok(());
        };
        let Some(ctx) = contexts.remove(&key) else {
            return Ok(());
        };
        if let CardLifecycle::Streaming(card) = &ctx.card_lifecycle {
            if let Err(e) =
                dingtalk_card::fail_card(&self.token_cache, &ctx.app_key, &ctx.app_secret, card)
                    .await
            {
                log::warn!("[reply-manager] fail_card via dispatch failed: {:#}", e);
            }
        }
        Ok(())
    }

    pub async fn deliver_pending_approval_ack(
        &self,
        session_id: &crate::runtime::ids::SessionId,
        message: &str,
    ) -> anyhow::Result<()> {
        if !self
            .should_deliver_pending_approval_ack(session_id.as_str(), message)
            .await
        {
            log::info!(
                "[reply-manager] suppressed duplicate pending approval ACK session={}",
                session_id.as_str()
            );
            return Ok(());
        }
        self.deliver_session_feedback_card(session_id, message.to_string())
            .await
    }

    pub async fn deliver_app_feedback(
        &self,
        session_id: &crate::runtime::ids::SessionId,
        message: &str,
    ) -> anyhow::Result<()> {
        self.deliver_session_feedback_card(session_id, message.to_string())
            .await
    }

    async fn deliver_session_feedback_card(
        &self,
        session_id: &crate::runtime::ids::SessionId,
        message: String,
    ) -> anyhow::Result<()> {
        let creds = self
            .session_credentials
            .lock()
            .await
            .get(session_id.as_str())
            .cloned();
        let Some(creds) = creds else {
            log::warn!(
                "[reply-manager] no cached credentials for feedback card session={}",
                session_id.as_str()
            );
            return Ok(());
        };
        if let Some(mut card) = dingtalk_card::create_and_deliver_card(
            &self.token_cache,
            &creds.app_key,
            &creds.app_secret,
            &creds.robot_code,
            &creds.target,
        )
        .await
        {
            let _ = dingtalk_card::finish_card(
                &self.token_cache,
                &creds.app_key,
                &creds.app_secret,
                &mut card,
                &message,
            )
            .await;
        }
        Ok(())
    }

    async fn should_deliver_pending_approval_ack(&self, session_id: &str, message: &str) -> bool {
        let now = Instant::now();
        let mut cache = self.pending_approval_ack_cache.lock().await;
        if let Some((last_message, last_sent_at)) = cache.get(session_id) {
            if last_message == message
                && now.duration_since(*last_sent_at) < PENDING_APPROVAL_ACK_SUPPRESS_FOR
            {
                return false;
            }
        }
        cache.insert(session_id.to_string(), (message.to_string(), now));
        true
    }

    async fn has_matching_context_for_event(&self, session_id: &str, run_id: &str) -> bool {
        let contexts = self.contexts.lock().await;
        let key = card_context_key(session_id, run_id);
        contexts
            .get(&key)
            .map(|ctx| ctx.run_id == run_id)
            .unwrap_or(false)
    }

    fn binding_allows_dingtalk_delivery(&self, session_id: &SessionId, run_id: &RunId) -> bool {
        matches!(
            self.output_binding_registry.get(session_id, run_id),
            Some(OutputBinding::Im {
                platform: ImPlatform::Dingtalk,
                allow_streaming_reply: true,
                ..
            })
        )
    }

    async fn ensure_context_for_event(&self, session_id: &SessionId, run_id: &RunId) -> bool {
        if self
            .has_matching_context_for_event(session_id.as_str(), run_id.as_str())
            .await
        {
            return true;
        }
        if !self.binding_allows_dingtalk_delivery(session_id, run_id) {
            log::debug!(
                "[reply-manager] no DingTalk output binding for session={} run={}; skip IM delivery",
                session_id.as_str(),
                run_id.as_str()
            );
            return false;
        }

        let creds = self
            .session_credentials
            .lock()
            .await
            .get(session_id.as_str())
            .cloned();
        let Some(creds) = creds else {
            return false;
        };

        let Some(card) = dingtalk_card::create_and_deliver_card(
            &self.token_cache,
            &creds.app_key,
            &creds.app_secret,
            &creds.robot_code,
            &creds.target,
        )
        .await
        else {
            log::warn!(
                "[reply-manager] bound lazy card creation failed for session={} run={}",
                session_id.as_str(),
                run_id.as_str()
            );
            return false;
        };

        let mut contexts = self.contexts.lock().await;
        let key = card_context_key(session_id.as_str(), run_id.as_str());
        contexts.entry(key).or_insert(ReplyContext {
            card_lifecycle: CardLifecycle::Streaming(card),
            accumulated_text: String::new(),
            app_key: creds.app_key,
            app_secret: creds.app_secret,
            run_id: run_id.as_str().to_string(),
        });
        true
    }

    #[cfg(test)]
    async fn dispatch_chunk_for_test(
        &self,
        session_id: SessionId,
        run_id: RunId,
        _delta: &str,
        _final_chunk: bool,
    ) -> bool {
        self.binding_allows_dingtalk_delivery(&session_id, &run_id)
    }

    async fn enqueue_card_update(&self, session_id: String, run_id: String, final_chunk: bool) {
        let key = scheduled_card_update_key(&session_id, &run_id);
        if final_chunk {
            self.scheduled_card_updates.lock().await.remove(&key);
            let contexts = Arc::clone(&self.contexts);
            let scheduled = Arc::clone(&self.scheduled_card_updates);
            let token_cache = self.token_cache.clone();
            tokio::spawn(async move {
                flush_scheduled_card_update(contexts, token_cache, session_id, run_id, true).await;
                scheduled.lock().await.remove(&key);
            });
            return;
        }

        let mut should_spawn = false;
        {
            let mut scheduled = self.scheduled_card_updates.lock().await;
            if let Some(update) = scheduled.get_mut(&key) {
                update.dirty = true;
                update.final_chunk |= final_chunk;
            } else {
                scheduled.insert(
                    key.clone(),
                    ScheduledCardUpdate {
                        final_chunk,
                        dirty: true,
                    },
                );
                should_spawn = true;
            }
        }

        if !should_spawn {
            return;
        }

        let contexts = Arc::clone(&self.contexts);
        let scheduled = Arc::clone(&self.scheduled_card_updates);
        let token_cache = self.token_cache.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(CARD_UPDATE_DEBOUNCE).await;

                let should_finish = {
                    let mut scheduled = scheduled.lock().await;
                    let Some(update) = scheduled.get_mut(&key) else {
                        return;
                    };
                    update.dirty = false;
                    update.final_chunk
                };

                flush_scheduled_card_update(
                    Arc::clone(&contexts),
                    token_cache.clone(),
                    session_id.clone(),
                    run_id.clone(),
                    should_finish,
                )
                .await;

                let should_continue = {
                    let mut scheduled = scheduled.lock().await;
                    match scheduled.get(&key) {
                        Some(update) if update.final_chunk || !update.dirty => {
                            scheduled.remove(&key);
                            false
                        }
                        Some(_) => true,
                        None => false,
                    }
                };

                if !should_continue {
                    break;
                }
            }
        });
    }
}

fn scheduled_card_update_key(session_id: &str, run_id: &str) -> String {
    format!("{session_id}\n{run_id}")
}

fn card_context_key(session_id: &str, run_id: &str) -> String {
    format!("{session_id}\n{run_id}")
}

fn find_context_key_for_session(
    contexts: &HashMap<String, ReplyContext>,
    session_id: &str,
) -> Option<String> {
    let prefix = format!("{session_id}\n");
    contexts
        .keys()
        .find(|key| key.starts_with(&prefix))
        .cloned()
}

async fn flush_scheduled_card_update(
    contexts: Arc<Mutex<HashMap<String, ReplyContext>>>,
    token_cache: TokenCache,
    session_id: String,
    run_id: String,
    final_chunk: bool,
) {
    let snapshot = {
        let contexts_guard = contexts.lock().await;
        let key = card_context_key(&session_id, &run_id);
        let Some(ctx) = contexts_guard.get(&key) else {
            return;
        };
        if ctx.run_id != run_id {
            return;
        }
        let card = match &ctx.card_lifecycle {
            CardLifecycle::Streaming(card) => card.clone(),
            CardLifecycle::Finished => {
                drop(contexts_guard);
                if final_chunk {
                    let key = card_context_key(&session_id, &run_id);
                    contexts.lock().await.remove(&key);
                }
                return;
            }
        };
        (
            card,
            ctx.accumulated_text.clone(),
            ctx.app_key.clone(),
            ctx.app_secret.clone(),
        )
    };

    let (mut card, text, app_key, app_secret) = snapshot;

    if final_chunk {
        if let Err(e) =
            dingtalk_card::finish_card(&token_cache, &app_key, &app_secret, &mut card, &text).await
        {
            log::warn!("[reply-manager] finish_card failed: {:#}", e);
        }
        let key = card_context_key(&session_id, &run_id);
        contexts.lock().await.remove(&key);
        log::info!("[reply-manager] finished reply for session {}", session_id);
        return;
    }

    if let Err(e) =
        dingtalk_card::stream_card(&token_cache, &app_key, &app_secret, &mut card, &text, false)
            .await
    {
        log::warn!("[reply-manager] stream_card failed: {:#}", e);
    }

    let mut contexts = contexts.lock().await;
    let key = card_context_key(&session_id, &run_id);
    if let Some(ctx) = contexts.get_mut(&key) {
        if ctx.run_id == run_id {
            ctx.card_lifecycle = CardLifecycle::Streaming(card);
        }
    }
}

#[async_trait]
impl RuntimeEventSubscriber for DingtalkReplyManager {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        let session_id_ref = event.session_id.clone();
        let run_id_ref = event.run_id.clone();
        let session_id = session_id_ref.as_str().to_string();
        let run_id = run_id_ref.as_str().to_string();

        match &event.kind {
            RuntimeEventKind::StreamDelta { content } => {
                if !self
                    .ensure_context_for_event(&session_id_ref, &run_id_ref)
                    .await
                {
                    return Ok(());
                }

                let key = card_context_key(&session_id, &run_id);
                let mut contexts = self.contexts.lock().await;
                let Some(ctx) = contexts.get_mut(&key) else {
                    return Ok(());
                };
                if ctx.run_id != run_id {
                    return Ok(());
                }
                ctx.accumulated_text.push_str(content);
                drop(contexts);
                self.enqueue_card_update(session_id, run_id, false).await;
            }
            RuntimeEventKind::StreamDone => {
                let should_finish = {
                    let contexts = self.contexts.lock().await;
                    let key = card_context_key(&session_id, &run_id);
                    let Some(ctx) = contexts.get(&key) else {
                        return Ok(());
                    };
                    if ctx.run_id != run_id {
                        return Ok(());
                    }
                    true
                };
                if should_finish {
                    self.enqueue_card_update(session_id, run_id, true).await;
                }
            }
            RuntimeEventKind::StreamError { error, .. } => {
                let ctx = {
                    let mut contexts = self.contexts.lock().await;
                    let key = card_context_key(&session_id, &run_id);
                    if contexts
                        .get(&key)
                        .map(|ctx| ctx.run_id.as_str() == run_id.as_str())
                        .unwrap_or(false)
                    {
                        contexts.remove(&key)
                    } else {
                        None
                    }
                };
                self.scheduled_card_updates
                    .lock()
                    .await
                    .remove(&scheduled_card_update_key(&session_id, &run_id));
                if let Some(ctx) = ctx {
                    log::warn!(
                        "[reply-manager] stream error for session {}: {}",
                        session_id,
                        error
                    );
                    if let CardLifecycle::Streaming(card) = ctx.card_lifecycle {
                        let cache = self.token_cache.clone();
                        tokio::spawn(async move {
                            if let Err(e) = dingtalk_card::fail_card(
                                &cache,
                                &ctx.app_key,
                                &ctx.app_secret,
                                &card,
                            )
                            .await
                            {
                                log::warn!("[reply-manager] fail_card error: {:#}", e);
                            }
                        });
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[async_trait]
impl super::ask_coordinator::ImAskSink for DingtalkReplyManager {
    async fn deliver_ask(
        &self,
        payload: &super::ask_coordinator::AskDeliveryPayload,
    ) -> Result<()> {
        if payload.followup {
            let ask_content = non_empty_ask_content(payload.markdown.clone());
            log::info!(
                "[reply-manager] delivering follow-up ask card session={}",
                payload.session_id.as_str()
            );
            return self
                .deliver_session_feedback_card(&payload.session_id, ask_content)
                .await;
        }

        let mut contexts = self.contexts.lock().await;
        let key = card_context_key(payload.session_id.as_str(), payload.run_id.as_str());
        let Some(ctx) = contexts.get_mut(&key) else {
            return Ok(());
        };
        if ctx.run_id != payload.run_id.as_str() {
            return Ok(());
        }
        let ask_content = non_empty_ask_content(payload.markdown.clone());
        if !ctx.accumulated_text.trim().is_empty() {
            ctx.accumulated_text.push_str("\n\n");
        }
        ctx.accumulated_text.push_str(&ask_content);
        let text = ctx.accumulated_text.clone();

        if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
            let _ = dingtalk_card::finish_card(
                &self.token_cache,
                &ctx.app_key,
                &ctx.app_secret,
                card,
                &text,
            )
            .await;
        }
        contexts.remove(&key);
        drop(contexts);
        self.scheduled_card_updates
            .lock()
            .await
            .remove(&scheduled_card_update_key(
                payload.session_id.as_str(),
                payload.run_id.as_str(),
            ));
        Ok(())
    }

    async fn force_finish_current_card(
        &self,
        session_id: &crate::runtime::ids::SessionId,
        reason_for_log: &str,
    ) -> Result<()> {
        let mut contexts = self.contexts.lock().await;
        let Some(key) = find_context_key_for_session(&contexts, session_id.as_str()) else {
            return Ok(());
        };
        let Some(ctx) = contexts.get_mut(&key) else {
            return Ok(());
        };
        if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
            let text = ctx.accumulated_text.clone();
            let _ = dingtalk_card::finish_card(
                &self.token_cache,
                &ctx.app_key,
                &ctx.app_secret,
                card,
                &text,
            )
            .await;
        }
        ctx.card_lifecycle = CardLifecycle::Finished;
        log::info!(
            "[reply-manager] force finished card session={} reason={}",
            session_id.as_str(),
            reason_for_log
        );
        Ok(())
    }
}

#[async_trait]
impl super::app_feedback::AppFeedbackSink for DingtalkReplyManager {
    async fn deliver_app_feedback(
        &self,
        session_id: &crate::runtime::ids::SessionId,
        message: &str,
    ) -> anyhow::Result<()> {
        self.deliver_session_feedback_card(session_id, message.to_string())
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::events::RuntimeEventKind;
    use crate::runtime::ids::{RunId, SessionId};

    fn make_event(session_id: &str, run_id: &str, kind: RuntimeEventKind) -> RuntimeEvent {
        RuntimeEvent {
            session_id: SessionId::new(session_id),
            run_id: RunId::new(run_id),
            agent_id: None,
            tool_call_id: None,
            kind,
        }
    }

    fn make_context(card_instance_id: &str, run_id: &str) -> ReplyContext {
        ReplyContext {
            card_lifecycle: CardLifecycle::Streaming(CardInstance {
                card_instance_id: card_instance_id.into(),
                inputing_started: false,
            }),
            accumulated_text: String::new(),
            app_key: "key".into(),
            app_secret: "secret".into(),
            run_id: run_id.into(),
        }
    }

    #[tokio::test]
    async fn ignores_events_without_registered_context() {
        let mgr = DingtalkReplyManager::new();
        let event = make_event("no-such-session", "run1", RuntimeEventKind::StreamDone);
        assert!(mgr.on_event(&event).await.is_ok());
    }

    #[tokio::test]
    async fn accumulates_delta_text() {
        let mgr = DingtalkReplyManager::new();
        let key = card_context_key("sess1", "run1");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(key.clone(), make_context("card1", "run1"));
        }

        let delta_event = make_event(
            "sess1",
            "run1",
            RuntimeEventKind::StreamDelta {
                content: "hello ".into(),
            },
        );
        // 忽略网络错误（测试环境无法访问钉钉 API）
        let _ = mgr.on_event(&delta_event).await;

        let ctx = mgr.contexts.lock().await;
        assert_eq!(ctx[&key].accumulated_text, "hello ");
    }

    #[tokio::test]
    async fn stream_delta_enqueues_card_update_without_inline_put() {
        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-scheduled", "run1");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(context_key.clone(), make_context("card-scheduled", "run1"));
        }

        let delta_event = make_event(
            "sess-scheduled",
            "run1",
            RuntimeEventKind::StreamDelta {
                content: "hello".into(),
            },
        );
        let _ = mgr.on_event(&delta_event).await;

        let ctx = mgr.contexts.lock().await;
        assert_eq!(ctx[&context_key].accumulated_text, "hello");
        drop(ctx);

        let key = scheduled_card_update_key("sess-scheduled", "run1");
        let scheduled = mgr.scheduled_card_updates.lock().await;
        let update = scheduled
            .get(&key)
            .expect("stream delta should schedule a background card update");
        assert!(!update.final_chunk);
    }

    #[tokio::test]
    async fn stream_done_flushes_final_even_when_delta_update_was_scheduled() {
        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-final", "run1");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(context_key.clone(), make_context("card-final", "run1"));
        }

        let _ = mgr
            .on_event(&make_event(
                "sess-final",
                "run1",
                RuntimeEventKind::StreamDelta {
                    content: "hello".into(),
                },
            ))
            .await;
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.get_mut(&context_key).unwrap().card_lifecycle = CardLifecycle::Finished;
        }
        let _ = mgr
            .on_event(&make_event(
                "sess-final",
                "run1",
                RuntimeEventKind::StreamDone,
            ))
            .await;
        tokio::task::yield_now().await;

        let key = scheduled_card_update_key("sess-final", "run1");
        let scheduled = mgr.scheduled_card_updates.lock().await;
        assert!(
            !scheduled.contains_key(&key),
            "stream done should cancel the debounced update after immediate final flush"
        );
        drop(scheduled);
        let ctx = mgr.contexts.lock().await;
        assert!(
            !ctx.contains_key(&context_key),
            "final flush should remove the card context"
        );
    }

    #[tokio::test]
    async fn stream_delta_without_registered_run_does_not_lazy_create_context() {
        let mgr = DingtalkReplyManager::new();
        mgr.remember_credentials(
            "sess-app-only".into(),
            "key".into(),
            "secret".into(),
            "robot".into(),
            CardTarget::Private {
                user_id: "user".into(),
            },
        )
        .await;

        let _ = mgr
            .on_event(&make_event(
                "sess-app-only",
                "app-run",
                RuntimeEventKind::StreamDelta {
                    content: "APP 里普通回复".into(),
                },
            ))
            .await;

        let ctx = mgr.contexts.lock().await;
        assert!(
            !ctx.keys().any(|key| key.starts_with("sess-app-only\n")),
            "APP-only stream must not create IM card context from cached credentials"
        );
    }

    #[tokio::test]
    async fn app_only_run_does_not_lazy_create_im_card_from_session_credentials() {
        let manager = DingtalkReplyManager::new();
        let session = SessionId::new("sess");
        let run = RunId::new("run-app");

        manager
            .remember_credentials(
                session.as_str().to_string(),
                "app-key".into(),
                "app-secret".into(),
                "robot".into(),
                CardTarget::Private {
                    user_id: "user".into(),
                },
            )
            .await;

        let delivered = manager
            .dispatch_chunk_for_test(session.clone(), run.clone(), "hello", true)
            .await;

        assert!(
            !delivered,
            "app-only run must not deliver to IM from cached credentials"
        );
    }

    #[tokio::test]
    async fn dingtalk_output_binding_allows_im_delivery_for_origin_run() {
        let registry = Arc::new(RunOutputBindingRegistry::new());
        let manager = DingtalkReplyManager::new_with_output_binding_registry(Arc::clone(&registry));
        let session = SessionId::new("sess-bound");
        let run = RunId::new("run-im");
        registry.register(
            &session,
            &run,
            OutputBinding::im(ImPlatform::Dingtalk, "sess-bound", "conv-1", true),
        );

        let delivered = manager
            .dispatch_chunk_for_test(session, run, "hello", true)
            .await;

        assert!(
            delivered,
            "DingTalk-origin run should be eligible for DingTalk IM delivery"
        );
    }

    #[tokio::test]
    async fn disabled_or_other_platform_output_binding_does_not_deliver_to_dingtalk() {
        let registry = Arc::new(RunOutputBindingRegistry::new());
        let manager = DingtalkReplyManager::new_with_output_binding_registry(Arc::clone(&registry));
        let disabled_session = SessionId::new("sess-disabled");
        let disabled_run = RunId::new("run-disabled");
        let feishu_session = SessionId::new("sess-feishu");
        let feishu_run = RunId::new("run-feishu");
        registry.register(
            &disabled_session,
            &disabled_run,
            OutputBinding::im(ImPlatform::Dingtalk, "sess-disabled", "conv-1", false),
        );
        registry.register(
            &feishu_session,
            &feishu_run,
            OutputBinding::im(ImPlatform::Feishu, "sess-feishu", "conv-2", true),
        );

        assert!(
            !manager
                .dispatch_chunk_for_test(disabled_session, disabled_run, "hello", true)
                .await,
            "DingTalk binding with streaming disabled must not deliver to IM"
        );
        assert!(
            !manager
                .dispatch_chunk_for_test(feishu_session, feishu_run, "hello", true)
                .await,
            "Feishu-origin run must not be delivered by DingTalk reply manager"
        );
    }

    #[tokio::test]
    async fn registered_im_run_still_accumulates_stream_delta() {
        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-im", "run-im");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(context_key.clone(), make_context("card-im", "run-im"));
        }

        let _ = mgr
            .on_event(&make_event(
                "sess-im",
                "run-im",
                RuntimeEventKind::StreamDelta {
                    content: "IM 回复".into(),
                },
            ))
            .await;

        let ctx = mgr.contexts.lock().await;
        assert_eq!(ctx[&context_key].accumulated_text, "IM 回复");
    }

    #[tokio::test]
    async fn skips_event_when_run_id_mismatch() {
        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess2", "run-A");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(context_key.clone(), make_context("card2", "run-A"));
        }
        let delta_event = make_event(
            "sess2",
            "run-B", // 不匹配
            RuntimeEventKind::StreamDelta {
                content: "should not appear".into(),
            },
        );
        let _ = mgr.on_event(&delta_event).await;

        let ctx = mgr.contexts.lock().await;
        assert_eq!(ctx[&context_key].accumulated_text, ""); // 不变
    }

    #[tokio::test]
    async fn clear_removes_registered_contexts() {
        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                card_context_key("sess3", "run1"),
                make_context("card3", "run1"),
            );
        }

        mgr.clear().await;

        assert!(mgr.contexts.lock().await.is_empty());
    }

    /// 回归测试：复现"钉钉 AI Card 字符叠倍" bug 的根本机制——
    /// 把同一个 Arc<DingtalkReplyManager> 当作 subscriber 注册到 RuntimeEventBus 两次后，
    /// 单次 emit StreamDelta 会被处理两次，accumulated_text 被叠倍。
    /// ChannelManager 必须用 `claim_first_subscription` 守卫保证只 subscribe 一次。
    #[tokio::test]
    async fn double_subscription_doubles_accumulated_text() {
        use crate::runtime::event_bus::RuntimeEventBus;

        let mgr = Arc::new(DingtalkReplyManager::new());
        let context_key = card_context_key("sess-double", "run-double");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                context_key.clone(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-double".into(),
                        inputing_started: true, // 跳过 INPUTING PUT，少一次必然失败的网络调用
                    }),
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    run_id: "run-double".into(),
                },
            );
        }

        let bus = RuntimeEventBus::new();
        bus.subscribe(Arc::clone(&mgr) as Arc<dyn RuntimeEventSubscriber>);
        bus.subscribe(Arc::clone(&mgr) as Arc<dyn RuntimeEventSubscriber>);

        // emit 一次 delta，两个 subscriber 都会跑 push_str；忽略 stream_card 的网络错误
        let _ = bus
            .emit(make_event(
                "sess-double",
                "run-double",
                RuntimeEventKind::StreamDelta {
                    content: "你好".into(),
                },
            ))
            .await;

        let ctx = mgr.contexts.lock().await;
        // bug 现场：被 push 了两次 → "你好你好"
        assert_eq!(ctx[&context_key].accumulated_text, "你好你好");
    }

    /// AskOutputSink: force_finish_current_card 对 Streaming 状态的卡片标记为 Finished
    #[tokio::test]
    async fn force_finish_marks_lifecycle_finished() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-ask", "run1");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                context_key.clone(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-ask".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: "partial".into(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    run_id: "run1".into(),
                },
            );
        }

        // Network will fail but we only care about the lifecycle transition
        let _ = mgr
            .force_finish_current_card(&SessionId::new("sess-ask"), "test")
            .await;

        let ctx = mgr.contexts.lock().await;
        assert!(matches!(
            ctx[&context_key].card_lifecycle,
            CardLifecycle::Finished
        ));
    }

    /// AskOutputSink: force_finish_current_card 对不存在的 session 静默返回 Ok
    #[tokio::test]
    async fn force_finish_no_context_is_ok() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        let result = mgr
            .force_finish_current_card(&SessionId::new("nonexistent"), "reason")
            .await;
        assert!(result.is_ok());
    }

    /// AskOutputSink: 空 streaming 卡片收到 AskUserQuestion 时应复用原卡并保留提问内容。
    #[tokio::test]
    async fn deliver_ask_card_reuses_empty_streaming_card() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-empty-ask", "run1");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                context_key.clone(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-empty-ask".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    run_id: "run1".into(),
                },
            );
        }

        let _ = mgr
            .deliver_ask_card(
                &SessionId::new("sess-empty-ask"),
                &RunId::new("run1"),
                "question markdown".into(),
            )
            .await;

        let ctx = mgr.contexts.lock().await;
        assert!(
            !ctx.contains_key(&context_key),
            "finished ask card should release context so resumed stream can create a fresh card"
        );
    }

    /// AskOutputSink: 非空 streaming 卡片收到审批卡后合并到同一张 run 卡片。
    #[tokio::test]
    async fn deliver_ask_card_merges_with_same_run_preface() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-non-empty-ask", "run1");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                context_key.clone(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-non-empty-ask".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: "好的，我来读取这个文件。".into(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    run_id: "run1".into(),
                },
            );
        }

        let _ = mgr
            .deliver_ask_card(
                &SessionId::new("sess-non-empty-ask"),
                &RunId::new("run1"),
                "permission markdown".into(),
            )
            .await;

        let ctx = mgr.contexts.lock().await;
        assert!(
            !ctx.contains_key(&context_key),
            "finished ask card should release context so resumed stream is not blocked"
        );
    }

    /// AskOutputSink: pending ask 接管并完成同一张卡后，不能留下后台 StreamDone 刷新任务。
    #[tokio::test]
    async fn deliver_ask_card_clears_scheduled_update_for_same_run() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-scheduled-ask", "run1");
        let scheduled_key = scheduled_card_update_key("sess-scheduled-ask", "run1");
        {
            let mut ctx = mgr.contexts.lock().await;
            let mut context = make_context("card-scheduled-ask", "run1");
            context.accumulated_text = "好的，我来读取这个文件。".into();
            ctx.insert(context_key.clone(), context);
        }
        {
            let mut scheduled = mgr.scheduled_card_updates.lock().await;
            scheduled.insert(
                scheduled_key.clone(),
                ScheduledCardUpdate {
                    final_chunk: true,
                    dirty: true,
                },
            );
        }

        let _ = mgr
            .deliver_ask_card(
                &SessionId::new("sess-scheduled-ask"),
                &RunId::new("run1"),
                "permission markdown".into(),
            )
            .await;

        let scheduled = mgr.scheduled_card_updates.lock().await;
        assert!(
            !scheduled.contains_key(&scheduled_key),
            "ask card finish must cancel the pending StreamDone flush for the same run"
        );
    }

    /// StreamDone 代表最终内容已经齐了，不能再等普通 delta 的 debounce。
    #[tokio::test]
    async fn final_card_update_flushes_without_waiting_for_debounce() {
        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-final-now", "run1");
        {
            let mut ctx = mgr.contexts.lock().await;
            let mut context = make_context("card-final-now", "run1");
            context.accumulated_text = "final content".into();
            context.card_lifecycle = CardLifecycle::Finished;
            ctx.insert(context_key.clone(), context);
        }

        mgr.enqueue_card_update("sess-final-now".into(), "run1".into(), true)
            .await;
        tokio::task::yield_now().await;

        let ctx = mgr.contexts.lock().await;
        assert!(
            !ctx.contains_key(&context_key),
            "final update should finish and remove the context immediately"
        );
    }

    /// AskOutputSink: 同 session 的旧 run 不能被新 run 的 pending ask 改写。
    #[tokio::test]
    async fn deliver_ask_card_does_not_modify_other_run_context() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        let old_key = card_context_key("sess-run-scope", "run-old");
        let new_key = card_context_key("sess-run-scope", "run-new");
        {
            let mut ctx = mgr.contexts.lock().await;
            let mut old_context = make_context("card-old", "run-old");
            old_context.accumulated_text = "old preface".into();
            ctx.insert(old_key.clone(), old_context);
        }

        let _ = mgr
            .deliver_ask_card(
                &SessionId::new("sess-run-scope"),
                &RunId::new("run-new"),
                "new ask".into(),
            )
            .await;

        let ctx = mgr.contexts.lock().await;
        assert_eq!(ctx[&old_key].accumulated_text, "old preface");
        assert!(
            !ctx.contains_key(&new_key),
            "ask card should not create or mutate another run context"
        );
    }

    /// AskOutputSink: 即使 AskUserQuestion markdown 为空，也不能完成一张空白卡片。
    #[tokio::test]
    async fn deliver_ask_card_uses_fallback_when_markdown_is_empty() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-empty-markdown", "run1");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                context_key.clone(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-empty-markdown".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    run_id: "run1".into(),
                },
            );
        }

        let _ = mgr
            .deliver_ask_card(
                &SessionId::new("sess-empty-markdown"),
                &RunId::new("run1"),
                "   ".into(),
            )
            .await;

        let ctx = mgr.contexts.lock().await;
        assert!(
            !ctx.contains_key(&context_key),
            "finished fallback ask card should release context for resumed stream"
        );
    }

    #[test]
    fn non_empty_ask_content_falls_back_for_blank_markdown() {
        assert_eq!(non_empty_ask_content("   ".into()), ASK_CARD_FALLBACK_TEXT);
        assert_eq!(non_empty_ask_content("question".into()), "question");
    }

    #[tokio::test]
    async fn duplicate_pending_approval_ack_is_suppressed_briefly() {
        let mgr = DingtalkReplyManager::new();

        assert!(
            mgr.should_deliver_pending_approval_ack("sess-ack", "等待审批")
                .await
        );
        assert!(
            !mgr.should_deliver_pending_approval_ack("sess-ack", "等待审批")
                .await
        );
        assert!(
            mgr.should_deliver_pending_approval_ack("sess-ack", "等待审批：新内容")
                .await
        );
    }

    #[tokio::test]
    async fn clear_removes_pending_approval_ack_cache() {
        let mgr = DingtalkReplyManager::new();

        assert!(
            mgr.should_deliver_pending_approval_ack("sess-ack", "等待审批")
                .await
        );
        mgr.clear().await;
        assert!(
            mgr.should_deliver_pending_approval_ack("sess-ack", "等待审批")
                .await
        );
    }

    /// dispatch_chunk 应当 push delta、final_chunk=true 时清掉 context（即便 finish_card 网络失败也清）。
    #[tokio::test]
    async fn dispatch_chunk_pushes_text_and_finalizes_on_final_chunk() {
        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-d1", "run-d1");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                context_key.clone(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-d1".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    run_id: "run-d1".into(),
                },
            );
        }

        let _ = mgr.dispatch_chunk("sess-d1", "hello", false).await;
        {
            let ctx = mgr.contexts.lock().await;
            assert_eq!(ctx[&context_key].accumulated_text, "hello");
        }

        let _ = mgr.dispatch_chunk("sess-d1", " world", true).await;
        {
            let ctx = mgr.contexts.lock().await;
            assert!(
                ctx.get(&context_key).is_none(),
                "final_chunk must clear context"
            );
        }
    }

    /// dispatch_fail 清掉 context，无 context 时静默 Ok。
    #[tokio::test]
    async fn dispatch_fail_clears_context_and_noop_when_absent() {
        let mgr = DingtalkReplyManager::new();
        let context_key = card_context_key("sess-d2", "run-d2");
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                context_key.clone(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-d2".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: "partial".into(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    run_id: "run-d2".into(),
                },
            );
        }

        let _ = mgr.dispatch_fail("sess-d2").await;
        assert!(mgr.contexts.lock().await.get(&context_key).is_none());

        // Absent session is a no-op.
        assert!(mgr.dispatch_fail("nonexistent").await.is_ok());
    }

    /// dispatch_chunk 在没有 context、没有 credentials 时是 no-op（不会 panic）。
    #[tokio::test]
    async fn dispatch_chunk_without_context_or_credentials_is_noop() {
        let mgr = DingtalkReplyManager::new();
        let r = mgr.dispatch_chunk("ghost-session", "ignored", false).await;
        assert!(r.is_ok());
        assert!(mgr.contexts.lock().await.is_empty());
    }
}

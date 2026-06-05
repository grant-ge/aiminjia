//! DingtalkReplyManager — 订阅 RuntimeEventBus，将 AI 回复流式投放到钉钉 AI Card

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};

use super::super::dingtalk::card::{self as dingtalk_card, CardInstance, CardTarget};
use super::super::dingtalk::token::TokenCache;

const ASK_CARD_FALLBACK_TEXT: &str = "需要你补充信息后我才能继续。";

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
    robot_code: String,
    target: CardTarget,
    run_id: String,
}

/// 已知的 session 凭证缓存。每次该 session 来 IM 消息都会刷新一次，
/// 用于 drain 触发的 LLM turn 在 StreamDelta 到达时懒建 card（drain 路径
/// 无 worker context，没法预先 register）。
#[derive(Debug, Clone)]
struct ReplyCredentials {
    app_key: String,
    app_secret: String,
    robot_code: String,
    target: CardTarget,
}

pub struct DingtalkReplyManager {
    /// session_id → ReplyContext
    contexts: Arc<Mutex<HashMap<String, ReplyContext>>>,
    /// session_id → 凭证（活的钉钉会话）。clear 时一并清空。
    session_credentials: Arc<Mutex<HashMap<String, ReplyCredentials>>>,
    token_cache: TokenCache,
}

impl DingtalkReplyManager {
    pub fn new() -> Self {
        Self {
            contexts: Arc::new(Mutex::new(HashMap::new())),
            session_credentials: Arc::new(Mutex::new(HashMap::new())),
            token_cache: TokenCache::new(),
        }
    }

    pub async fn clear(&self) {
        self.contexts.lock().await.clear();
        self.session_credentials.lock().await.clear();
    }

    /// 记住一个 IM session 的钉钉凭证。worker 收到任何一条消息都该调一次，
    /// 这样后续 drain 触发的 LLM turn 也能懒建 card 把 AI 回复发回钉钉。
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
            contexts.insert(
                session_id.clone(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(card),
                    accumulated_text: String::new(),
                    app_key,
                    app_secret,
                    robot_code,
                    target,
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
        // Lazy-register: 如果没有 active context 但有缓存凭证，懒建一张卡。
        let needs_lazy_register = {
            let contexts = self.contexts.lock().await;
            !contexts.contains_key(session_id)
        };
        if needs_lazy_register {
            let creds = self
                .session_credentials
                .lock()
                .await
                .get(session_id)
                .cloned();
            if let Some(creds) = creds {
                if let Some(card) = dingtalk_card::create_and_deliver_card(
                    &self.token_cache,
                    &creds.app_key,
                    &creds.app_secret,
                    &creds.robot_code,
                    &creds.target,
                )
                .await
                {
                    let mut contexts = self.contexts.lock().await;
                    contexts
                        .entry(session_id.to_string())
                        .or_insert(ReplyContext {
                            card_lifecycle: CardLifecycle::Streaming(card),
                            accumulated_text: String::new(),
                            app_key: creds.app_key,
                            app_secret: creds.app_secret,
                            robot_code: creds.robot_code,
                            target: creds.target,
                            run_id: String::new(), // dispatch_chunk 路径不绑定 run_id
                        });
                }
            }
        }

        let mut contexts = self.contexts.lock().await;
        let Some(ctx) = contexts.get_mut(session_id) else {
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
            contexts.remove(session_id);
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
        let Some(ctx) = contexts.remove(session_id) else {
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
}

#[async_trait]
impl RuntimeEventSubscriber for DingtalkReplyManager {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        let session_id = event.session_id.as_str().to_string();
        let run_id = event.run_id.as_str().to_string();

        match &event.kind {
            RuntimeEventKind::StreamDelta { content } => {
                // Lazy-register: drain 触发的 LLM turn 没经过 ChannelManager.register，
                // 第一个 StreamDelta 到达时若没有 active context 但有缓存凭证，
                // 就用凭证懒建一张新 card 绑定当前 run_id。
                let needs_lazy_register = {
                    let contexts = self.contexts.lock().await;
                    !contexts.contains_key(&session_id)
                };
                if needs_lazy_register {
                    let creds = self
                        .session_credentials
                        .lock()
                        .await
                        .get(&session_id)
                        .cloned();
                    if let Some(creds) = creds {
                        if let Some(card) = dingtalk_card::create_and_deliver_card(
                            &self.token_cache,
                            &creds.app_key,
                            &creds.app_secret,
                            &creds.robot_code,
                            &creds.target,
                        )
                        .await
                        {
                            let mut contexts = self.contexts.lock().await;
                            contexts.entry(session_id.clone()).or_insert(ReplyContext {
                                card_lifecycle: CardLifecycle::Streaming(card),
                                accumulated_text: String::new(),
                                app_key: creds.app_key,
                                app_secret: creds.app_secret,
                                robot_code: creds.robot_code,
                                target: creds.target,
                                run_id: run_id.clone(),
                            });
                            log::info!(
                                "[reply-manager] lazy-registered context for session {} run {} (drain path)",
                                session_id,
                                run_id
                            );
                        } else {
                            log::warn!(
                                "[reply-manager] lazy card creation failed for session {}",
                                session_id
                            );
                        }
                    }
                }

                let mut contexts = self.contexts.lock().await;
                if let Some(ctx) = contexts.get_mut(&session_id) {
                    if ctx.run_id != run_id {
                        return Ok(());
                    }
                    // Step 2: 按需开卡 — 如果之前已 Finished，重新创建卡片
                    if matches!(ctx.card_lifecycle, CardLifecycle::Finished) {
                        if let Some(card) = dingtalk_card::create_and_deliver_card(
                            &self.token_cache,
                            &ctx.app_key,
                            &ctx.app_secret,
                            &ctx.robot_code,
                            &ctx.target,
                        )
                        .await
                        {
                            ctx.card_lifecycle = CardLifecycle::Streaming(card);
                        }
                    }
                    ctx.accumulated_text.push_str(content);
                    // Clone fields needed for the API call before borrowing card mutably
                    let text = ctx.accumulated_text.clone();
                    let app_key = ctx.app_key.clone();
                    let app_secret = ctx.app_secret.clone();
                    let cache = self.token_cache.clone();
                    if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
                        if let Err(e) = dingtalk_card::stream_card(
                            &cache,
                            &app_key,
                            &app_secret,
                            card,
                            &text,
                            false,
                        )
                        .await
                        {
                            log::warn!("[reply-manager] stream_card failed: {:#}", e);
                        }
                    }
                }
            }
            RuntimeEventKind::StreamDone => {
                let mut contexts = self.contexts.lock().await;
                if let Some(ctx) = contexts.get_mut(&session_id) {
                    if ctx.run_id != run_id {
                        return Ok(());
                    }
                    let text = ctx.accumulated_text.clone();
                    let app_key = ctx.app_key.clone();
                    let app_secret = ctx.app_secret.clone();
                    let cache = self.token_cache.clone();
                    if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
                        if let Err(e) =
                            dingtalk_card::finish_card(&cache, &app_key, &app_secret, card, &text)
                                .await
                        {
                            log::warn!("[reply-manager] finish_card failed: {:#}", e);
                        }
                    }
                    contexts.remove(&session_id);
                    log::info!("[reply-manager] finished reply for session {}", session_id);
                }
            }
            RuntimeEventKind::StreamError { error, .. } => {
                let mut contexts = self.contexts.lock().await;
                if let Some(ctx) = contexts.remove(&session_id) {
                    if ctx.run_id != run_id {
                        return Ok(());
                    }
                    let cache = self.token_cache.clone();
                    if let CardLifecycle::Streaming(card) = &ctx.card_lifecycle {
                        if let Err(e) =
                            dingtalk_card::fail_card(&cache, &ctx.app_key, &ctx.app_secret, card)
                                .await
                        {
                            log::warn!("[reply-manager] fail_card error: {:#}", e);
                        }
                    }
                    log::warn!(
                        "[reply-manager] stream error for session {}: {}",
                        session_id,
                        error
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[async_trait]
impl super::ask_coordinator::AskOutputSink for DingtalkReplyManager {
    async fn deliver_ask_card(
        &self,
        session_id: &crate::runtime::ids::SessionId,
        markdown: String,
    ) -> Result<()> {
        let mut contexts = self.contexts.lock().await;
        let Some(ctx) = contexts.get_mut(session_id.as_str()) else {
            return Ok(());
        };
        let ask_content = non_empty_ask_content(markdown);
        if ctx.accumulated_text.trim().is_empty()
            && matches!(ctx.card_lifecycle, CardLifecycle::Streaming(_))
        {
            if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
                let _ = dingtalk_card::finish_card(
                    &self.token_cache,
                    &ctx.app_key,
                    &ctx.app_secret,
                    card,
                    &ask_content,
                )
                .await;
            }
            ctx.card_lifecycle = CardLifecycle::Finished;
            return Ok(());
        }
        // Finish the current streaming card (if any) before delivering the ask card
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
        // Create a fresh card for the ask and immediately finish it with the ask markdown
        if let Some(mut ask_card) = dingtalk_card::create_and_deliver_card(
            &self.token_cache,
            &ctx.app_key,
            &ctx.app_secret,
            &ctx.robot_code,
            &ctx.target,
        )
        .await
        {
            let _ = dingtalk_card::finish_card(
                &self.token_cache,
                &ctx.app_key,
                &ctx.app_secret,
                &mut ask_card,
                &ask_content,
            )
            .await;
        }
        ctx.card_lifecycle = CardLifecycle::Finished;
        Ok(())
    }

    async fn force_finish_current_card(
        &self,
        session_id: &crate::runtime::ids::SessionId,
        reason_for_log: &str,
    ) -> Result<()> {
        let mut contexts = self.contexts.lock().await;
        let Some(ctx) = contexts.get_mut(session_id.as_str()) else {
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
            robot_code: "robot".into(),
            target: CardTarget::Private {
                user_id: "user".into(),
            },
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
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert("sess1".into(), make_context("card1", "run1"));
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
        assert_eq!(ctx["sess1"].accumulated_text, "hello ");
    }

    #[tokio::test]
    async fn skips_event_when_run_id_mismatch() {
        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert("sess2".into(), make_context("card2", "run-A"));
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
        assert_eq!(ctx["sess2"].accumulated_text, ""); // 不变
    }

    #[tokio::test]
    async fn clear_removes_registered_contexts() {
        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert("sess3".into(), make_context("card3", "run1"));
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
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess-double".into(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-double".into(),
                        inputing_started: true, // 跳过 INPUTING PUT，少一次必然失败的网络调用
                    }),
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    robot_code: "robot".into(),
                    target: CardTarget::Private {
                        user_id: "user".into(),
                    },
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
        assert_eq!(ctx["sess-double"].accumulated_text, "你好你好");
    }

    /// AskOutputSink: force_finish_current_card 对 Streaming 状态的卡片标记为 Finished
    #[tokio::test]
    async fn force_finish_marks_lifecycle_finished() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess-ask".into(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-ask".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: "partial".into(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    robot_code: "robot".into(),
                    target: CardTarget::Private {
                        user_id: "user".into(),
                    },
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
            ctx["sess-ask"].card_lifecycle,
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

    /// AskOutputSink: 空 streaming 卡片收到 AskUserQuestion 时应复用原卡，
    /// 但不能把提问内容污染到后续回答缓冲里。
    #[tokio::test]
    async fn deliver_ask_card_reuses_empty_streaming_card_without_polluting_followup() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess-empty-ask".into(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-empty-ask".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    robot_code: "robot".into(),
                    target: CardTarget::Private {
                        user_id: "user".into(),
                    },
                    run_id: "run1".into(),
                },
            );
        }

        let _ = mgr
            .deliver_ask_card(
                &SessionId::new("sess-empty-ask"),
                "question markdown".into(),
            )
            .await;

        {
            let ctx = mgr.contexts.lock().await;
            assert_eq!(ctx["sess-empty-ask"].accumulated_text, "");
            assert!(matches!(
                ctx["sess-empty-ask"].card_lifecycle,
                CardLifecycle::Finished
            ));
        }

        let _ = mgr
            .on_event(&make_event(
                "sess-empty-ask",
                "run1",
                RuntimeEventKind::StreamDelta {
                    content: "follow-up answer".into(),
                },
            ))
            .await;

        let ctx = mgr.contexts.lock().await;
        assert_eq!(ctx["sess-empty-ask"].accumulated_text, "follow-up answer");
    }

    /// AskOutputSink: 即使 AskUserQuestion markdown 为空，也不能完成一张空白卡片。
    #[tokio::test]
    async fn deliver_ask_card_uses_fallback_when_markdown_is_empty() {
        use super::super::ask_coordinator::AskOutputSink;

        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess-empty-markdown".into(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-empty-markdown".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    robot_code: "robot".into(),
                    target: CardTarget::Private {
                        user_id: "user".into(),
                    },
                    run_id: "run1".into(),
                },
            );
        }

        let _ = mgr
            .deliver_ask_card(&SessionId::new("sess-empty-markdown"), "   ".into())
            .await;

        let ctx = mgr.contexts.lock().await;
        assert_eq!(ctx["sess-empty-markdown"].accumulated_text, "");
        assert!(matches!(
            ctx["sess-empty-markdown"].card_lifecycle,
            CardLifecycle::Finished
        ));
    }

    #[test]
    fn non_empty_ask_content_falls_back_for_blank_markdown() {
        assert_eq!(non_empty_ask_content("   ".into()), ASK_CARD_FALLBACK_TEXT);
        assert_eq!(non_empty_ask_content("question".into()), "question");
    }

    /// dispatch_chunk 应当 push delta、final_chunk=true 时清掉 context（即便 finish_card 网络失败也清）。
    #[tokio::test]
    async fn dispatch_chunk_pushes_text_and_finalizes_on_final_chunk() {
        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess-d1".into(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-d1".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    robot_code: "robot".into(),
                    target: CardTarget::Private {
                        user_id: "user".into(),
                    },
                    run_id: "run-d1".into(),
                },
            );
        }

        let _ = mgr.dispatch_chunk("sess-d1", "hello", false).await;
        {
            let ctx = mgr.contexts.lock().await;
            assert_eq!(ctx["sess-d1"].accumulated_text, "hello");
        }

        let _ = mgr.dispatch_chunk("sess-d1", " world", true).await;
        {
            let ctx = mgr.contexts.lock().await;
            assert!(
                ctx.get("sess-d1").is_none(),
                "final_chunk must clear context"
            );
        }
    }

    /// dispatch_fail 清掉 context，无 context 时静默 Ok。
    #[tokio::test]
    async fn dispatch_fail_clears_context_and_noop_when_absent() {
        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess-d2".into(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-d2".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: "partial".into(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    robot_code: "robot".into(),
                    target: CardTarget::Private {
                        user_id: "user".into(),
                    },
                    run_id: "run-d2".into(),
                },
            );
        }

        let _ = mgr.dispatch_fail("sess-d2").await;
        assert!(mgr.contexts.lock().await.get("sess-d2").is_none());

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

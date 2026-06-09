use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::ids::{SessionId, ToolCallId};
use crate::runtime::interaction::{
    InteractionId, InteractionResolution, PendingInteractionControlPlane,
};
use crate::runtime::store::{PendingPermissionControlPlane, PendingPermissionResolution};

const ASK_DEADLINE: Duration = Duration::from_secs(10 * 60);

#[async_trait]
pub trait AskOutputSink: Send + Sync {
    async fn deliver_ask_card(&self, session_id: &SessionId, markdown: String) -> Result<()>;
    async fn force_finish_current_card(
        &self,
        session_id: &SessionId,
        reason_for_log: &str,
    ) -> Result<()>;
}

pub trait ChannelSessionRegistry: Send + Sync {
    fn is_channel_session(&self, session_id: &SessionId) -> bool;
}

#[async_trait]
pub trait AskReplyJudge: Send + Sync {
    async fn judge_permission(
        &self,
        model: &str,
        tool_name: &str,
        ask_message: &str,
        suggestions: &[String],
        user_reply: &str,
    ) -> JudgeResult;

    async fn judge_user_question(
        &self,
        model: &str,
        questions: &serde_json::Value,
        user_reply: &str,
    ) -> JudgeResult;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JudgeResult {
    PermissionAnswered {
        allow: bool,
        reason: String,
    },
    UserQuestionAnswered {
        value: serde_json::Value,
        reason: String,
    },
    Abandoned {
        reason: String,
    },
    Ambiguous {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleOutcome {
    NotPending,
    Consumed,
    Reroute { content: String },
}

#[derive(Debug, Clone)]
pub enum PendingAskKind {
    Permission {
        tool_call_id: ToolCallId,
        tool_name: String,
        message: String,
        suggestions: Vec<String>,
    },
    UserQuestion {
        interaction_id: InteractionId,
        tool_call_id: ToolCallId,
        questions: serde_json::Value,
    },
}

#[derive(Debug)]
struct PendingAsk {
    kind: PendingAskKind,
    cancel: CancellationToken,
    primary_model: String,
}

pub struct IMAskCoordinator {
    pending: Arc<Mutex<HashMap<String, PendingAsk>>>,
    registry: Arc<dyn ChannelSessionRegistry>,
    sink: Arc<dyn AskOutputSink>,
    permission_cp: Arc<dyn PendingPermissionControlPlane>,
    interaction_cp: Arc<dyn PendingInteractionControlPlane>,
    judge: Arc<dyn AskReplyJudge>,
}

impl IMAskCoordinator {
    pub fn new(
        registry: Arc<dyn ChannelSessionRegistry>,
        sink: Arc<dyn AskOutputSink>,
        permission_cp: Arc<dyn PendingPermissionControlPlane>,
        interaction_cp: Arc<dyn PendingInteractionControlPlane>,
        judge: Arc<dyn AskReplyJudge>,
    ) -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            registry,
            sink,
            permission_cp,
            interaction_cp,
            judge,
        }
    }

    fn clone_handle(&self) -> IMAskCoordinatorHandle {
        IMAskCoordinatorHandle {
            pending: Arc::clone(&self.pending),
            sink: Arc::clone(&self.sink),
            permission_cp: Arc::clone(&self.permission_cp),
            interaction_cp: Arc::clone(&self.interaction_cp),
        }
    }

    pub async fn try_handle_reply(
        &self,
        session_id: &SessionId,
        content: String,
    ) -> Result<HandleOutcome> {
        let pending = self.pending.lock().await.remove(session_id.as_str());
        let Some(pending) = pending else {
            log::info!(
                "[im-ask] try_handle_reply session={} no pending ask, fallthrough",
                session_id.as_str()
            );
            return Ok(HandleOutcome::NotPending);
        };
        log::info!(
            "[im-ask] try_handle_reply session={} found pending kind={} model={}",
            session_id.as_str(),
            match &pending.kind {
                PendingAskKind::Permission { .. } => "permission",
                PendingAskKind::UserQuestion { .. } => "user_question",
            },
            pending.primary_model
        );
        pending.cancel.cancel();

        if content.trim().is_empty() {
            self.resolve_ambiguous(&pending, &content, "empty reply".to_string())
                .await?;
            return Ok(HandleOutcome::Consumed);
        }
        let judgement = match &pending.kind {
            PendingAskKind::Permission {
                tool_name,
                message,
                suggestions,
                ..
            } => {
                self.judge
                    .judge_permission(
                        &pending.primary_model,
                        tool_name,
                        message,
                        suggestions,
                        &content,
                    )
                    .await
            }
            PendingAskKind::UserQuestion { questions, .. } => {
                self.judge
                    .judge_user_question(&pending.primary_model, questions, &content)
                    .await
            }
        };
        log::info!(
            "[im-ask] try_handle_reply session={} judge result={}",
            session_id.as_str(),
            match &judgement {
                JudgeResult::PermissionAnswered { allow, .. } =>
                    if *allow {
                        "permission_allow"
                    } else {
                        "permission_deny"
                    },
                JudgeResult::UserQuestionAnswered { .. } => "user_question_answered",
                JudgeResult::Abandoned { .. } => "abandoned",
                JudgeResult::Ambiguous { .. } => "ambiguous",
            }
        );
        match judgement {
            JudgeResult::PermissionAnswered { allow, reason } => {
                self.resolve_permission_answer(&pending, allow, reason)?;
                Ok(HandleOutcome::Consumed)
            }
            JudgeResult::UserQuestionAnswered { value, .. } => {
                self.resolve_user_question_answer(&pending, value)?;
                Ok(HandleOutcome::Consumed)
            }
            JudgeResult::Abandoned { reason } => {
                self.resolve_abandoned(&pending, reason)?;
                self.sink
                    .force_finish_current_card(session_id, "abandoned")
                    .await?;
                Ok(HandleOutcome::Reroute { content })
            }
            JudgeResult::Ambiguous { reason } => {
                self.resolve_ambiguous(&pending, &content, reason).await?;
                Ok(HandleOutcome::Consumed)
            }
        }
    }

    fn resolve_permission_answer(
        &self,
        pending: &PendingAsk,
        allow: bool,
        reason: String,
    ) -> Result<()> {
        if let PendingAskKind::Permission { tool_call_id, .. } = &pending.kind {
            if self.permission_cp.is_pending(tool_call_id) {
                if allow {
                    self.permission_cp.resolve_pending_request(
                        tool_call_id,
                        PendingPermissionResolution::Allow {
                            updated_input: None,
                            remember: false,
                            destination: None,
                        },
                    )?;
                } else {
                    self.permission_cp.resolve_pending_request(
                        tool_call_id,
                        PendingPermissionResolution::Deny {
                            message: reason,
                            remember: false,
                            destination: None,
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    fn resolve_user_question_answer(
        &self,
        pending: &PendingAsk,
        value: serde_json::Value,
    ) -> Result<()> {
        if let PendingAskKind::UserQuestion { interaction_id, .. } = &pending.kind {
            if self.interaction_cp.is_pending(interaction_id) {
                self.interaction_cp
                    .resolve(interaction_id, InteractionResolution::Submit { value })?;
            }
        }
        Ok(())
    }

    fn resolve_abandoned(&self, pending: &PendingAsk, reason: String) -> Result<()> {
        match &pending.kind {
            PendingAskKind::Permission { tool_call_id, .. } => {
                if self.permission_cp.is_pending(tool_call_id) {
                    self.permission_cp.resolve_pending_request(
                        tool_call_id,
                        PendingPermissionResolution::Deny {
                            message: format!("User changed topic in IM channel: {}", reason),
                            remember: false,
                            destination: None,
                        },
                    )?;
                }
            }
            PendingAskKind::UserQuestion { interaction_id, .. } => {
                if self.interaction_cp.is_pending(interaction_id) {
                    self.interaction_cp.resolve(
                        interaction_id,
                        InteractionResolution::Cancel {
                            message: format!("User changed topic in IM channel: {}", reason),
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    async fn resolve_ambiguous(
        &self,
        pending: &PendingAsk,
        user_reply: &str,
        reason: String,
    ) -> Result<()> {
        match &pending.kind {
            PendingAskKind::Permission { tool_call_id, .. } => {
                if self.permission_cp.is_pending(tool_call_id) {
                    self.permission_cp.resolve_pending_request(
                        tool_call_id,
                        PendingPermissionResolution::Deny {
                            message: format!("IM reply did not clearly grant permission. User said: {}. Judge reason: {}", user_reply, reason),
                            remember: false,
                            destination: None,
                        },
                    )?;
                }
            }
            PendingAskKind::UserQuestion { interaction_id, .. } => {
                if self.interaction_cp.is_pending(interaction_id) {
                    self.interaction_cp.resolve(
                        interaction_id,
                        InteractionResolution::Submit {
                            value: serde_json::json!({
                                "kind": "user_did_not_answer",
                                "user_said": user_reply,
                                "guidance": reason,
                            }),
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    async fn register_pending(
        &self,
        event: &RuntimeEvent,
        kind: PendingAskKind,
        primary_model: String,
    ) -> Result<()> {
        if !self.registry.is_channel_session(&event.session_id) {
            log::trace!(
                "[im-ask] ignore non-channel session {}",
                event.session_id.as_str()
            );
            return Ok(());
        }
        log::info!(
            "[im-ask] register_pending session={} kind={} model={}",
            event.session_id.as_str(),
            match &kind {
                PendingAskKind::Permission { tool_name, .. } => format!("permission/{}", tool_name),
                PendingAskKind::UserQuestion { .. } => "user_question".to_string(),
            },
            primary_model
        );
        let markdown = format_pending_ask_markdown(&kind);
        self.sink
            .deliver_ask_card(&event.session_id, markdown)
            .await?;
        let cancel = CancellationToken::new();
        let pending = PendingAsk {
            kind,
            cancel: cancel.clone(),
            primary_model,
        };
        self.pending
            .lock()
            .await
            .insert(event.session_id.as_str().to_string(), pending);

        let session_id = event.session_id.clone();
        let coordinator_handle = self.clone_handle();
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(ASK_DEADLINE) => {
                    coordinator_handle.resolve_deadline(session_id).await;
                }
                _ = cancel.cancelled() => {}
            }
        });

        Ok(())
    }
}

#[derive(Clone)]
struct IMAskCoordinatorHandle {
    pending: Arc<Mutex<HashMap<String, PendingAsk>>>,
    sink: Arc<dyn AskOutputSink>,
    permission_cp: Arc<dyn PendingPermissionControlPlane>,
    interaction_cp: Arc<dyn PendingInteractionControlPlane>,
}

impl IMAskCoordinatorHandle {
    async fn resolve_deadline(&self, session_id: SessionId) {
        let pending = self.pending.lock().await.remove(session_id.as_str());
        if let Some(pending) = pending {
            resolve_pending_as_timeout(
                self.permission_cp.as_ref(),
                self.interaction_cp.as_ref(),
                &pending.kind,
            );
            let _ = self
                .sink
                .force_finish_current_card(&session_id, "deadline")
                .await;
        }
    }
}

fn resolve_pending_as_timeout(
    permission_cp: &dyn PendingPermissionControlPlane,
    interaction_cp: &dyn PendingInteractionControlPlane,
    kind: &PendingAskKind,
) {
    match kind {
        PendingAskKind::Permission { tool_call_id, .. } => {
            if permission_cp.is_pending(tool_call_id) {
                let _ = permission_cp.resolve_pending_request(
                    tool_call_id,
                    PendingPermissionResolution::Deny {
                        message: "IM permission request timed out without user response."
                            .to_string(),
                        remember: false,
                        destination: None,
                    },
                );
            }
        }
        PendingAskKind::UserQuestion { interaction_id, .. } => {
            if interaction_cp.is_pending(interaction_id) {
                let _ = interaction_cp.resolve(
                    interaction_id,
                    InteractionResolution::Cancel {
                        message: "IM user question timed out without user response.".to_string(),
                    },
                );
            }
        }
    }
}

#[async_trait]
impl RuntimeEventSubscriber for IMAskCoordinator {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        match &event.kind {
            RuntimeEventKind::PermissionAskRequired {
                tool_call_id,
                tool_name,
                message,
                suggestions,
                primary_model,
                ..
            } => {
                self.register_pending(
                    event,
                    PendingAskKind::Permission {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        message: message.clone(),
                        suggestions: suggestions.clone(),
                    },
                    primary_model.clone(),
                )
                .await
            }
            RuntimeEventKind::UserInteractionRequired {
                interaction_id,
                tool_call_id,
                payload,
                primary_model,
                ..
            } => {
                self.register_pending(
                    event,
                    PendingAskKind::UserQuestion {
                        interaction_id: interaction_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        questions: payload.clone(),
                    },
                    primary_model.clone(),
                )
                .await
            }
            _ => Ok(()),
        }
    }
}

pub fn format_pending_ask_markdown(kind: &PendingAskKind) -> String {
    match kind {
        PendingAskKind::Permission {
            tool_name,
            message,
            suggestions,
            ..
        } => {
            let mut text = format!(
                "🔒 我需要你的确认才能继续\n\n打算执行：**{}**\n\n> {}\n\n是否允许？请直接回复，例如\u{201c}可以\u{201d}或\u{201c}不要\u{201d}。",
                tool_name, message
            );
            if !suggestions.is_empty() {
                text.push_str("\n\n建议参数：\n");
                for suggestion in suggestions {
                    text.push_str("- ");
                    text.push_str(suggestion);
                    text.push('\n');
                }
            }
            text
        }
        PendingAskKind::UserQuestion { questions, .. } => {
            let questions_array = questions
                .get("questions")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mut text = "❓ 我有几个问题想问你\n".to_string();
            for (idx, question) in questions_array.iter().enumerate() {
                let title = question
                    .get("question")
                    .and_then(|v| v.as_str())
                    .unwrap_or("请选择一个选项");
                text.push_str(&format!("\n**{}. {}**\n", idx + 1, title));
                if question
                    .get("multiSelect")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    text.push_str("（可多选）\n");
                }
                if let Some(options) = question.get("options").and_then(|v| v.as_array()) {
                    for option in options {
                        if let Some(label) = option.get("label").and_then(|v| v.as_str()) {
                            text.push_str("- ");
                            text.push_str(label);
                            text.push('\n');
                        }
                    }
                }
            }
            text.push_str("\n请直接回复你的选择，自然语言即可。");
            text
        }
    }
}

// ---------------------------------------------------------------------------
// GatewayAskReplyJudge — production impl backed by LlmGateway
// ---------------------------------------------------------------------------

pub struct GatewayAskReplyJudge {
    gateway: Arc<crate::llm::gateway::LlmGateway>,
    settings: crate::models::settings::AppSettings,
}

impl GatewayAskReplyJudge {
    pub fn new(
        gateway: Arc<crate::llm::gateway::LlmGateway>,
        settings: crate::models::settings::AppSettings,
    ) -> Self {
        Self { gateway, settings }
    }
}

#[derive(serde::Deserialize)]
struct PermissionJudgeJson {
    verdict: String,
    decision: Option<String>,
    reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct UserQuestionJudgeJson {
    verdict: String,
    answers: Option<serde_json::Value>,
    reason: Option<String>,
}

fn strip_json_fence(input: &str) -> &str {
    input
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

#[async_trait]
impl AskReplyJudge for GatewayAskReplyJudge {
    async fn judge_permission(
        &self,
        model: &str,
        tool_name: &str,
        ask_message: &str,
        suggestions: &[String],
        user_reply: &str,
    ) -> JudgeResult {
        let mut settings = self.settings.clone();
        settings.primary_model = model.to_string();
        let prompt = format!(
            "你是一个分诊器。AI 助手刚向用户请求高风险操作授权。只输出 JSON。\n\nAI 想做的操作：\n{}: {}\n建议参数：{}\n\n用户回复：\n\"\"\"{}\"\"\"\n\n输出 JSON：{{\"verdict\":\"answered|abandoned|ambiguous\",\"decision\":\"allow|deny\",\"reason\":\"一句话\"}}",
            tool_name,
            ask_message,
            suggestions.join("\n"),
            user_reply
        );
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            self.gateway.send_message(
                &settings,
                vec![crate::llm::streaming::ChatMessage::text("user", prompt)],
                crate::llm::masking::MaskingLevel::Relaxed,
                None,
                None,
                Some(Vec::new()),
            ),
        )
        .await;
        let Ok(Ok(response)) = response else {
            log::warn!(
                "[im-ask] judge_permission gateway call failed user_reply={:?}",
                user_reply
            );
            return JudgeResult::Ambiguous {
                reason: "judge call failed".into(),
            };
        };
        log::info!(
            "[im-ask] judge_permission raw response user_reply={:?} content={:?}",
            user_reply,
            response.content
        );
        let parsed: Result<PermissionJudgeJson, _> =
            serde_json::from_str(strip_json_fence(&response.content));
        match parsed {
            Ok(v) if v.verdict == "answered" => JudgeResult::PermissionAnswered {
                allow: v.decision.as_deref() == Some("allow"),
                reason: v
                    .reason
                    .unwrap_or_else(|| "permission answered by IM user".into()),
            },
            Ok(v) if v.verdict == "abandoned" => JudgeResult::Abandoned {
                reason: v.reason.unwrap_or_else(|| "user changed topic".into()),
            },
            Ok(v) => JudgeResult::Ambiguous {
                reason: v
                    .reason
                    .unwrap_or_else(|| "unclear permission reply".into()),
            },
            Err(_) => JudgeResult::Ambiguous {
                reason: "judge JSON parse failed".into(),
            },
        }
    }

    async fn judge_user_question(
        &self,
        model: &str,
        questions: &serde_json::Value,
        user_reply: &str,
    ) -> JudgeResult {
        let mut settings = self.settings.clone();
        settings.primary_model = model.to_string();
        let prompt = format!(
            "你是一个分诊器。AI 助手刚通过 AskUserQuestion 工具向用户问了一组问题。只输出 JSON。\n\nAI 提的问题：\n{}\n\n用户回复：\n\"\"\"{}\"\"\"\n\n输出 JSON：\n- verdict 是 \"answered\"|\"abandoned\"|\"ambiguous\" 之一\n- 当 verdict=answered 时：answers 是一个对象，key 是问题原文（与 AI 提问中的 question 字段完全一致），value 是用户的答案文本（如果是从 options 选了一个或多个，把选的 label 用逗号拼起来；如果是自由回答，用用户原话）\n- reason 是一句话解释判断\n\n例：{{\"verdict\":\"answered\",\"answers\":{{\"用哪个数据源？\":\"A, B\"}},\"reason\":\"用户明确选了 A 和 B\"}}",
            questions, user_reply
        );
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            self.gateway.send_message(
                &settings,
                vec![crate::llm::streaming::ChatMessage::text("user", prompt)],
                crate::llm::masking::MaskingLevel::Relaxed,
                None,
                None,
                Some(Vec::new()),
            ),
        )
        .await;
        let Ok(Ok(response)) = response else {
            log::warn!(
                "[im-ask] judge_user_question gateway call failed user_reply={:?}",
                user_reply
            );
            return JudgeResult::Ambiguous {
                reason: "judge call failed".into(),
            };
        };
        log::info!(
            "[im-ask] judge_user_question raw response user_reply={:?} content={:?}",
            user_reply,
            response.content
        );
        let parsed: Result<UserQuestionJudgeJson, _> =
            serde_json::from_str(strip_json_fence(&response.content));
        match parsed {
            Ok(v) if v.verdict == "answered" => match v.answers {
                Some(answers) => JudgeResult::UserQuestionAnswered {
                    value: serde_json::json!({ "answers": answers }),
                    reason: v
                        .reason
                        .unwrap_or_else(|| "question answered by IM user".into()),
                },
                None => JudgeResult::Ambiguous {
                    reason: "answered without answers".into(),
                },
            },
            Ok(v) if v.verdict == "abandoned" => JudgeResult::Abandoned {
                reason: v.reason.unwrap_or_else(|| "user changed topic".into()),
            },
            Ok(v) => JudgeResult::Ambiguous {
                reason: v.reason.unwrap_or_else(|| "unclear question reply".into()),
            },
            Err(_) => JudgeResult::Ambiguous {
                reason: "judge JSON parse failed".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use crate::runtime::events::RuntimeEventKind;
    use crate::runtime::ids::{RunId, SessionId, ToolCallId};
    use crate::runtime::tools::permission::PermissionMode;

    struct Registry(bool);
    impl ChannelSessionRegistry for Registry {
        fn is_channel_session(&self, _session_id: &SessionId) -> bool {
            self.0
        }
    }

    struct RecordingSink {
        calls: StdMutex<Vec<String>>,
    }
    #[async_trait]
    impl AskOutputSink for RecordingSink {
        async fn deliver_ask_card(&self, _session_id: &SessionId, markdown: String) -> Result<()> {
            self.calls.lock().unwrap().push(markdown);
            Ok(())
        }
        async fn force_finish_current_card(
            &self,
            _session_id: &SessionId,
            _reason_for_log: &str,
        ) -> Result<()> {
            Ok(())
        }
    }

    struct ScriptedJudge {
        result: StdMutex<JudgeResult>,
    }
    #[async_trait]
    impl AskReplyJudge for ScriptedJudge {
        async fn judge_permission(
            &self,
            _model: &str,
            _tool_name: &str,
            _ask_message: &str,
            _suggestions: &[String],
            _user_reply: &str,
        ) -> JudgeResult {
            self.result.lock().unwrap().clone()
        }
        async fn judge_user_question(
            &self,
            _model: &str,
            _questions: &serde_json::Value,
            _user_reply: &str,
        ) -> JudgeResult {
            self.result.lock().unwrap().clone()
        }
    }

    fn make_coordinator(judge: Arc<dyn AskReplyJudge>) -> IMAskCoordinator {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
            judge,
        )
    }

    #[test]
    fn permission_markdown_contains_operation() {
        let text = format_pending_ask_markdown(&PendingAskKind::Permission {
            tool_call_id: ToolCallId::new("tool-1"),
            tool_name: "bash".into(),
            message: "命令：`ls /tmp`".into(),
            suggestions: vec!["cwd=/tmp".into()],
        });
        assert!(text.contains("bash"));
        assert!(text.contains("ls /tmp"));
        assert!(text.contains("cwd=/tmp"));
    }

    #[test]
    fn question_markdown_renders_options() {
        let text = format_pending_ask_markdown(&PendingAskKind::UserQuestion {
            interaction_id: InteractionId::new("ask-1"),
            tool_call_id: ToolCallId::new("tool-1"),
            questions: serde_json::json!({
                "questions": [{
                    "question": "用哪个数据源？",
                    "multiSelect": true,
                    "options": [{"label": "A"}, {"label": "B"}]
                }]
            }),
        });
        assert!(text.contains("用哪个数据源"));
        assert!(text.contains("可多选"));
        assert!(text.contains("- A"));
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_denies_permission_and_clears_slot() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let sink = Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
        });
        // Deadline test doesn't need the judge; use a scripted one that won't be called
        let judge = Arc::new(ScriptedJudge {
            result: StdMutex::new(JudgeResult::Ambiguous {
                reason: "unused".into(),
            }),
        });
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            sink,
            permission.clone(),
            interaction,
            judge,
        );

        let event = RuntimeEvent::new(
            SessionId::new("sess-im"),
            RunId::new("run-1"),
            RuntimeEventKind::PermissionAskRequired {
                tool_call_id: ToolCallId::new("tool-1"),
                tool_name: "bash".into(),
                message: "run ls".into(),
                suggestions: vec![],
                mode: PermissionMode::Default,
                remember_options: vec![],
                default_destination: None,
                primary_model: "deepseek-v3".into(),
            },
        );

        coordinator.on_event(&event).await.unwrap();
        // The spawned deadline task is queued but not yet polled. Yield once to let
        // it register its tokio::time::sleep with the timer driver.
        tokio::task::yield_now().await;
        // Now advance the clock past the deadline so the sleep future becomes ready,
        // then yield again to let the deadline task run resolve_deadline.
        tokio::time::advance(ASK_DEADLINE + Duration::from_secs(1)).await;
        // resolve_deadline has its own .await points; give it enough poll cycles.
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }

        assert!(coordinator.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn answered_permission_is_consumed() {
        let coordinator = make_coordinator(Arc::new(ScriptedJudge {
            result: StdMutex::new(JudgeResult::PermissionAnswered {
                allow: true,
                reason: "user allowed".into(),
            }),
        }));
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "bash".into(),
                    message: "run ls".into(),
                    suggestions: vec![],
                },
                cancel: CancellationToken::new(),
                primary_model: "deepseek-v3".into(),
            },
        );
        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "可以".into())
            .await
            .unwrap();
        assert_eq!(outcome, HandleOutcome::Consumed);
    }

    #[tokio::test]
    async fn abandoned_reply_is_rerouted() {
        let coordinator = make_coordinator(Arc::new(ScriptedJudge {
            result: StdMutex::new(JudgeResult::Abandoned {
                reason: "new topic".into(),
            }),
        }));
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                kind: PendingAskKind::UserQuestion {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    questions: serde_json::json!({"questions": []}),
                },
                cancel: CancellationToken::new(),
                primary_model: "deepseek-v3".into(),
            },
        );
        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "帮我查天气".into())
            .await
            .unwrap();
        assert_eq!(
            outcome,
            HandleOutcome::Reroute {
                content: "帮我查天气".into()
            }
        );
    }
}

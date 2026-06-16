use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::{ChatMessage, ToolDefinition};
use crate::models::settings::AppSettings;
use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::human_interaction::{
    AskQuestionSpec, HumanInteractionId, HumanInteractionKind, HumanInteractionRef,
    HumanInteractionRouter, HumanInteractionStatus, HumanReplyRoute, OutputBinding,
    PermissionAskSpec, PermissionDecisionIntent, PermissionGroup, PermissionGroupKey,
    PermissionGroupResolution, TurnOrigin,
};
use crate::runtime::ids::{RunId, SessionId, ToolCallId};
use crate::runtime::interaction::{
    InteractionId, InteractionResolution, PendingInteractionControlPlane,
};
use crate::runtime::pending::PendingQueueManager;
use crate::runtime::store::{PendingPermissionControlPlane, PendingPermissionResolution};
use crate::runtime::tools::permission::PermissionDestination;

use super::app_feedback::{AppFeedbackDecision, IMAppFeedbackCoordinator};

pub const DINGTALK_ASK_ROUTE: &str = "dingtalk-";
pub const TELEGRAM_ASK_ROUTE: &str = "tg-";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskKind {
    Permission,
    UserQuestion,
}

#[derive(Debug, Clone)]
pub struct AskDeliveryPayload {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub markdown: String,
    pub tool_call_id: String,
    pub interaction_id: String,
    pub kind: AskKind,
    pub followup: bool,
}

#[async_trait]
pub trait ImAskSink: Send + Sync {
    async fn deliver_ask(&self, payload: &AskDeliveryPayload) -> Result<()>;
    async fn force_finish_current_card(
        &self,
        session_id: &SessionId,
        reason_for_log: &str,
    ) -> Result<()>;
}

pub struct IMAskOutputRouter {
    sinks: RwLock<HashMap<String, Arc<dyn ImAskSink>>>,
    session_routes: RwLock<HashMap<String, String>>,
}

impl IMAskOutputRouter {
    pub fn new() -> Self {
        Self {
            sinks: RwLock::new(HashMap::new()),
            session_routes: RwLock::new(HashMap::new()),
        }
    }

    pub fn register_sink(&self, route_key: impl Into<String>, sink: Arc<dyn ImAskSink>) {
        self.sinks
            .write()
            .expect("im ask route sinks poisoned")
            .insert(route_key.into(), sink);
    }

    pub fn route_session(&self, session_id: impl Into<String>, route_key: impl Into<String>) {
        self.session_routes
            .write()
            .expect("im ask session routes poisoned")
            .insert(session_id.into(), route_key.into());
    }

    fn resolve_sink(&self, session_id: &SessionId) -> Option<(String, Arc<dyn ImAskSink>)> {
        if let Some(route_key) = self
            .session_routes
            .read()
            .expect("im ask session routes poisoned")
            .get(session_id.as_str())
            .cloned()
        {
            let sink = self
                .sinks
                .read()
                .expect("im ask route sinks poisoned")
                .get(&route_key)
                .cloned();
            return sink.map(|sink| (route_key, sink));
        }

        let sinks = self.sinks.read().expect("im ask route sinks poisoned");
        for (route_key, sink) in sinks.iter() {
            if session_id.as_str().starts_with(route_key) {
                return Some((route_key.clone(), sink.clone()));
            }
        }
        None
    }
}

impl Default for IMAskOutputRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ImAskSink for IMAskOutputRouter {
    async fn deliver_ask(&self, payload: &AskDeliveryPayload) -> Result<()> {
        let Some((route_key, sink)) = self.resolve_sink(&payload.session_id) else {
            log::warn!(
                "[im-ask-router] no sink route for session={}",
                payload.session_id.as_str()
            );
            return Ok(());
        };
        log::debug!(
            "[im-ask-router] deliver ask route={} session={} kind={:?}",
            route_key,
            payload.session_id.as_str(),
            payload.kind
        );
        sink.deliver_ask(payload).await
    }

    async fn force_finish_current_card(
        &self,
        session_id: &SessionId,
        reason_for_log: &str,
    ) -> Result<()> {
        let Some((_route_key, sink)) = self.resolve_sink(session_id) else {
            return Ok(());
        };
        sink.force_finish_current_card(session_id, reason_for_log)
            .await
    }
}

pub trait ChannelSessionRegistry: Send + Sync {
    fn is_channel_session(&self, session_id: &SessionId) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleOutcome {
    NotPending,
    NewTurnAfterAbandon,
    ApprovalResolved,
    AnswerResolved,
    InvalidApprovalAction { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalCommandDecision {
    AllowOnce,
    AllowAlways,
    Deny,
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingActionCommand {
    Approve {
        id: String,
        decision: ApprovalCommandDecision,
    },
    Answer {
        id: String,
        value: serde_json::Value,
    },
    AnswerCancel {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingPermissionReplyIntent {
    Resolve {
        decision: ApprovalCommandDecision,
        scope: Option<String>,
        reason: String,
    },
    NewTurn {
        reason: String,
    },
    ClarifyPending {
        message: String,
    },
    Unclear {
        message: String,
    },
}

#[async_trait]
pub trait PendingReplyJudge: Send + Sync {
    async fn judge_permission(
        &self,
        model: &str,
        tool_name: &str,
        ask_message: &str,
        suggestions: &[String],
        current_path_auth_scope: Option<&str>,
        requested_path: Option<&str>,
        user_reply: &str,
    ) -> PendingPermissionReplyIntent;
}

pub trait JudgeSettingsProvider: Send + Sync {
    fn load_settings(&self) -> AppSettings;
}

pub struct StaticJudgeSettingsProvider {
    settings: AppSettings,
}

impl StaticJudgeSettingsProvider {
    pub fn new(settings: AppSettings) -> Self {
        Self { settings }
    }
}

impl JudgeSettingsProvider for StaticJudgeSettingsProvider {
    fn load_settings(&self) -> AppSettings {
        self.settings.clone()
    }
}

pub struct UnavailablePendingReplyJudge;

#[async_trait]
impl PendingReplyJudge for UnavailablePendingReplyJudge {
    async fn judge_permission(
        &self,
        _model: &str,
        _tool_name: &str,
        _ask_message: &str,
        _suggestions: &[String],
        _current_path_auth_scope: Option<&str>,
        _requested_path: Option<&str>,
        _user_reply: &str,
    ) -> PendingPermissionReplyIntent {
        PendingPermissionReplyIntent::Unclear {
            message:
                "当前没有可用的语义解析器，请直接说“允许一次”“以后都允许”“拒绝”或“取消当前任务”。"
                    .into(),
        }
    }
}

pub struct GatewayPendingReplyJudge {
    gateway: Arc<crate::llm::gateway::LlmGateway>,
    settings_provider: Arc<dyn JudgeSettingsProvider>,
}

impl GatewayPendingReplyJudge {
    pub fn new(
        gateway: Arc<crate::llm::gateway::LlmGateway>,
        settings_provider: Arc<dyn JudgeSettingsProvider>,
    ) -> Self {
        Self {
            gateway,
            settings_provider,
        }
    }
}

#[derive(Debug, Deserialize)]
struct PermissionJudgeJson {
    intent: String,
    decision: Option<String>,
    scope: Option<String>,
    reason: Option<String>,
    message: Option<String>,
}

#[async_trait]
impl PendingReplyJudge for GatewayPendingReplyJudge {
    async fn judge_permission(
        &self,
        model: &str,
        tool_name: &str,
        ask_message: &str,
        suggestions: &[String],
        current_path_auth_scope: Option<&str>,
        requested_path: Option<&str>,
        user_reply: &str,
    ) -> PendingPermissionReplyIntent {
        let mut settings = self.settings_provider.load_settings();
        if !model.trim().is_empty() {
            settings.primary_model = model.to_string();
        }
        settings.cloud_model_type = "chat".to_string();
        settings.thinking_type = "disabled".to_string();

        let judge_input = permission_judge_input(
            tool_name,
            ask_message,
            suggestions,
            current_path_auth_scope,
            requested_path,
            user_reply,
        );

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.gateway.send_message(
                &settings,
                vec![ChatMessage::text("user", judge_input)],
                MaskingLevel::Relaxed,
                Some(PERMISSION_JUDGE_SYSTEM_CONTRACT),
                None,
                Some(vec![permission_reply_intent_tool()]),
            ),
        )
        .await;

        let Ok(Ok(response)) = response else {
            return PendingPermissionReplyIntent::Unclear {
                message:
                    "语义解析暂时失败了，请直接说“允许一次”“以后都允许”“拒绝”或“取消当前任务”。"
                        .into(),
            };
        };

        let intent = parse_permission_judge_response(&response).unwrap_or_else(|| {
            PendingPermissionReplyIntent::Unclear {
                message: "语义解析没有返回有效结构，请直接说“允许一次”“以后都允许”“拒绝”或“取消当前任务”。".into(),
            }
        });
        log::info!(
            "[im-ask] permission judge result tool={} requested_path={:?} path_auth_scope={:?} intent={:?}",
            tool_name,
            requested_path,
            current_path_auth_scope,
            intent
        );
        intent
    }
}

#[derive(Debug, Clone)]
pub enum PendingAskKind {
    Permission {
        tool_call_id: ToolCallId,
        tool_name: String,
        message: String,
        suggestions: Vec<String>,
        path_auth_scope: Option<String>,
    },
    UserQuestion {
        interaction_id: InteractionId,
        tool_call_id: ToolCallId,
        questions: serde_json::Value,
    },
}

fn ask_delivery_payload(
    session_id: &SessionId,
    run_id: &RunId,
    kind: &PendingAskKind,
    markdown: String,
    followup: bool,
) -> AskDeliveryPayload {
    match kind {
        PendingAskKind::Permission { tool_call_id, .. } => AskDeliveryPayload {
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            markdown,
            tool_call_id: tool_call_id.as_str().to_string(),
            interaction_id: tool_call_id.as_str().to_string(),
            kind: AskKind::Permission,
            followup,
        },
        PendingAskKind::UserQuestion {
            interaction_id,
            tool_call_id,
            ..
        } => AskDeliveryPayload {
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            markdown,
            tool_call_id: tool_call_id.as_str().to_string(),
            interaction_id: interaction_id.as_str().to_string(),
            kind: AskKind::UserQuestion,
            followup,
        },
    }
}

#[derive(Debug, Clone)]
struct PendingAsk {
    run_id: RunId,
    kind: PendingAskKind,
    primary_model: String,
}

#[derive(Debug, Clone)]
struct PermissionResolveTarget {
    pending: PendingAsk,
    tool_call_id: ToolCallId,
    requested_path: Option<String>,
    path_auth_scope: Option<String>,
}

impl PendingAsk {
    #[cfg(test)]
    fn tool_call_id(&self) -> &str {
        match &self.kind {
            PendingAskKind::Permission { tool_call_id, .. }
            | PendingAskKind::UserQuestion { tool_call_id, .. } => tool_call_id.as_str(),
        }
    }
}

#[derive(Debug, Default)]
struct PendingAskSlots {
    by_session: HashMap<String, Vec<PendingAsk>>,
}

impl PendingAskSlots {
    fn insert(&mut self, session_id: String, pending: PendingAsk) -> Option<PendingAsk> {
        self.by_session.entry(session_id).or_default().push(pending);
        None
    }

    fn get(&self, session_id: &str) -> Option<&PendingAsk> {
        self.by_session
            .get(session_id)
            .and_then(|items| items.first())
    }

    fn find_for_command(
        &self,
        session_id: &str,
        command: &PendingActionCommand,
    ) -> Option<&PendingAsk> {
        let items = self.by_session.get(session_id)?;
        items
            .iter()
            .rev()
            .find(|pending| match (&pending.kind, command) {
                (
                    PendingAskKind::Permission { tool_call_id, .. },
                    PendingActionCommand::Approve { id, .. },
                ) => id == tool_call_id.as_str(),
                (
                    PendingAskKind::UserQuestion { interaction_id, .. },
                    PendingActionCommand::Answer { id, .. }
                    | PendingActionCommand::AnswerCancel { id },
                ) => id == interaction_id.as_str(),
                _ => false,
            })
    }

    #[cfg(test)]
    fn contains_key(&self, session_id: &str) -> bool {
        self.by_session
            .get(session_id)
            .is_some_and(|items| !items.is_empty())
    }

    fn list_for_session(&self, session_id: &SessionId) -> Vec<PendingAsk> {
        self.by_session
            .get(session_id.as_str())
            .cloned()
            .unwrap_or_default()
    }

    fn remove_matching(&mut self, session_id: &str, expected: &PendingAsk) -> Option<PendingAsk> {
        let items = self.by_session.get_mut(session_id)?;
        let index = items
            .iter()
            .position(|current| pending_identity_matches(current, expected))?;
        let removed = items.remove(index);
        if items.is_empty() {
            self.by_session.remove(session_id);
        }
        Some(removed)
    }

    fn remove_run(&mut self, session_id: &str, run_id: &RunId) -> Vec<PendingAsk> {
        let Some(items) = self.by_session.get_mut(session_id) else {
            return Vec::new();
        };
        let mut removed = Vec::new();
        let mut kept = Vec::with_capacity(items.len());
        for item in std::mem::take(items) {
            if item.run_id == *run_id {
                removed.push(item);
            } else {
                kept.push(item);
            }
        }
        if kept.is_empty() {
            self.by_session.remove(session_id);
        } else {
            *self.by_session.get_mut(session_id).expect("session exists") = kept;
        }
        removed
    }
}

pub struct IMAskCoordinator {
    pending: Arc<Mutex<PendingAskSlots>>,
    registry: Arc<dyn ChannelSessionRegistry>,
    sink: Arc<dyn ImAskSink>,
    permission_cp: Arc<dyn PendingPermissionControlPlane>,
    interaction_cp: Arc<dyn PendingInteractionControlPlane>,
    judge: Arc<dyn PendingReplyJudge>,
    app_feedback: Option<Arc<IMAppFeedbackCoordinator>>,
    pending_queue: Option<Arc<PendingQueueManager>>,
}

impl IMAskCoordinator {
    pub fn new(
        registry: Arc<dyn ChannelSessionRegistry>,
        sink: Arc<dyn ImAskSink>,
        permission_cp: Arc<dyn PendingPermissionControlPlane>,
        interaction_cp: Arc<dyn PendingInteractionControlPlane>,
    ) -> Self {
        Self::new_with_judge(
            registry,
            sink,
            permission_cp,
            interaction_cp,
            Arc::new(UnavailablePendingReplyJudge),
        )
    }

    pub fn new_with_judge(
        registry: Arc<dyn ChannelSessionRegistry>,
        sink: Arc<dyn ImAskSink>,
        permission_cp: Arc<dyn PendingPermissionControlPlane>,
        interaction_cp: Arc<dyn PendingInteractionControlPlane>,
        judge: Arc<dyn PendingReplyJudge>,
    ) -> Self {
        Self {
            pending: Arc::new(Mutex::new(PendingAskSlots::default())),
            registry,
            sink,
            permission_cp,
            interaction_cp,
            judge,
            app_feedback: None,
            pending_queue: None,
        }
    }

    pub fn with_app_feedback(mut self, app_feedback: Arc<IMAppFeedbackCoordinator>) -> Self {
        self.app_feedback = Some(app_feedback);
        self
    }

    pub fn with_pending_queue(mut self, pending_queue: Arc<PendingQueueManager>) -> Self {
        self.pending_queue = Some(pending_queue);
        self
    }

    async fn pending_for_session(&self, session_id: &SessionId) -> Vec<PendingAsk> {
        self.pending.lock().await.list_for_session(session_id)
    }

    pub async fn try_handle_reply(
        &self,
        session_id: &SessionId,
        content: String,
    ) -> Result<HandleOutcome> {
        let command_like = looks_like_pending_action_command(&content);
        let command = parse_pending_action_command(&content);
        let pending = {
            let guard = self.pending.lock().await;
            command
                .as_ref()
                .and_then(|command| guard.find_for_command(session_id.as_str(), command))
                .or_else(|| guard.get(session_id.as_str()))
                .cloned()
        };
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
        if !self.is_pending_ask_live(&pending) {
            log::warn!(
                "[im-ask] dropping stale pending ask session={} kind={} because control plane no longer has it",
                session_id.as_str(),
                match &pending.kind {
                    PendingAskKind::Permission { .. } => "permission",
                    PendingAskKind::UserQuestion { .. } => "user_question",
                }
            );
            self.remove_pending_if_current(session_id, &pending).await;
            return Ok(match &pending.kind {
                PendingAskKind::Permission { .. } => HandleOutcome::InvalidApprovalAction {
                    message: "刚才那次权限请求已经失效，请重新发起需要权限的操作。".to_string(),
                },
                PendingAskKind::UserQuestion { .. } => HandleOutcome::NotPending,
            });
        }

        match (&pending.kind, command) {
            (
                PendingAskKind::Permission { tool_call_id, .. },
                Some(PendingActionCommand::Approve { id, decision }),
            ) if id == tool_call_id.as_str() => {
                if !self.resolve_permission_command(&pending, decision)? {
                    return Ok(HandleOutcome::InvalidApprovalAction {
                        message: "审批指令无效或已不匹配，请使用当前卡片上的按钮或指令。"
                            .to_string(),
                    });
                }
                self.deliver_app_feedback_for_pending(
                    &pending,
                    app_feedback_for_approval_decision(decision),
                )
                .await;
                self.remove_pending_if_current(session_id, &pending).await;
                Ok(HandleOutcome::ApprovalResolved)
            }
            (
                PendingAskKind::UserQuestion { interaction_id, .. },
                Some(PendingActionCommand::Answer { id, value }),
            ) if id == interaction_id.as_str() => {
                if !self.resolve_user_question_answer(&pending, value)? {
                    return Ok(HandleOutcome::InvalidApprovalAction {
                        message: "审批指令无效或已不匹配，请使用当前卡片上的按钮或指令。"
                            .to_string(),
                    });
                }
                self.deliver_app_feedback_for_pending(
                    &pending,
                    AppFeedbackDecision::InteractionSubmit,
                )
                .await;
                self.remove_pending_if_current(session_id, &pending).await;
                Ok(HandleOutcome::AnswerResolved)
            }
            (
                PendingAskKind::UserQuestion { interaction_id, .. },
                Some(PendingActionCommand::AnswerCancel { id }),
            ) if id == interaction_id.as_str() => {
                if !self.resolve_abandoned(&pending, "user cancelled interaction".to_string())? {
                    return Ok(HandleOutcome::InvalidApprovalAction {
                        message: "审批指令无效或已不匹配，请使用当前卡片上的按钮或指令。"
                            .to_string(),
                    });
                }
                self.deliver_app_feedback_for_pending(
                    &pending,
                    AppFeedbackDecision::InteractionCancel,
                )
                .await;
                self.remove_pending_if_current(session_id, &pending).await;
                Ok(HandleOutcome::AnswerResolved)
            }
            (_, Some(_)) => Ok(HandleOutcome::InvalidApprovalAction {
                message: "审批指令无效或已不匹配，请使用当前卡片上的按钮或指令。".to_string(),
            }),
            (_, None) if command_like => Ok(HandleOutcome::InvalidApprovalAction {
                message: "审批指令无效或已不匹配，请使用当前卡片上的按钮或指令。".to_string(),
            }),
            (
                PendingAskKind::Permission {
                    path_auth_scope,
                    tool_call_id,
                    suggestions,
                    tool_name,
                    message: ask_message,
                    ..
                },
                None,
            ) => {
                let requested_path = self
                    .permission_cp
                    .get_pending_request(tool_call_id)
                    .and_then(|request| {
                        extract_requested_path_from_tool_args(&request.original_request.args)
                    });
                let interaction_ref = HumanInteractionRef {
                    id: HumanInteractionId::new(tool_call_id.as_str().to_string()),
                    session_id: session_id.clone(),
                    run_id: pending.run_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    kind: HumanInteractionKind::PermissionAsk,
                    turn_origin: TurnOrigin::App,
                    output_binding: OutputBinding::AppOnly,
                    status: HumanInteractionStatus::Pending,
                };
                let spec = PermissionAskSpec {
                    tool_name: tool_name.clone(),
                    requested_path: requested_path.clone(),
                    current_scope: path_auth_scope.clone(),
                };
                match HumanInteractionRouter::route_permission_reply(
                    &interaction_ref,
                    &spec,
                    &content,
                ) {
                    HumanReplyRoute::ResolvePermission { intent } => {
                        return self
                            .resolve_permission_intent(
                                session_id,
                                &pending,
                                tool_call_id,
                                intent,
                                path_auth_scope.as_deref(),
                                requested_path.as_deref(),
                            )
                            .await;
                    }
                    HumanReplyRoute::AbandonAndStartNewTurn { reason, .. } => {
                        if !self.resolve_abandoned(&pending, reason)? {
                            return Ok(HandleOutcome::InvalidApprovalAction {
                                message: "当前审批已失效，请重新发送你的请求。".to_string(),
                            });
                        }
                        self.remove_pending_if_current(session_id, &pending).await;
                        return Ok(HandleOutcome::NewTurnAfterAbandon);
                    }
                    HumanReplyRoute::Clarify { .. } => {}
                    HumanReplyRoute::ResolveAskUserQuestion { .. } => unreachable!(
                        "permission router must not return ask user question resolution"
                    ),
                }
                let intent = self
                    .judge
                    .judge_permission(
                        &pending.primary_model,
                        tool_name,
                        ask_message,
                        suggestions,
                        path_auth_scope.as_deref(),
                        requested_path.as_deref(),
                        &content,
                    )
                    .await;
                match intent {
                    PendingPermissionReplyIntent::Resolve {
                        decision: ApprovalCommandDecision::AllowAlways,
                        scope: Some(scope),
                        ..
                    } => {
                        let Some(_) = resolve_permission_path_scope_override(
                            &scope,
                            path_auth_scope.as_deref(),
                            requested_path.as_deref(),
                        ) else {
                            return Ok(HandleOutcome::InvalidApprovalAction {
                                message: "我理解你想扩大授权范围，但这个范围没有覆盖当前请求路径。请明确回复“允许一次”，或说出包含当前文件的目录范围。".to_string(),
                            });
                        };
                        self.resolve_permission_intent(
                            session_id,
                            &pending,
                            tool_call_id,
                            PermissionDecisionIntent::AllowAlways { scope: Some(scope) },
                            path_auth_scope.as_deref(),
                            requested_path.as_deref(),
                        )
                        .await
                    }
                    PendingPermissionReplyIntent::Resolve {
                        decision, scope, ..
                    } => {
                        self.resolve_permission_intent(
                            session_id,
                            &pending,
                            tool_call_id,
                            permission_decision_intent_from_approval(decision, scope),
                            path_auth_scope.as_deref(),
                            requested_path.as_deref(),
                        )
                        .await
                    }
                    PendingPermissionReplyIntent::NewTurn { reason } => {
                        if !self.resolve_abandoned(&pending, reason)? {
                            return Ok(HandleOutcome::InvalidApprovalAction {
                                message: "当前审批已失效，请重新发送你的请求。".to_string(),
                            });
                        }
                        self.remove_pending_if_current(session_id, &pending).await;
                        Ok(HandleOutcome::NewTurnAfterAbandon)
                    }
                    PendingPermissionReplyIntent::ClarifyPending { message }
                    | PendingPermissionReplyIntent::Unclear { message } => {
                        Ok(HandleOutcome::InvalidApprovalAction { message })
                    }
                }
            }
            (
                PendingAskKind::UserQuestion {
                    interaction_id,
                    tool_call_id,
                    questions,
                },
                None,
            ) => {
                let spec = ask_question_spec_from_payload(questions);
                let interaction_ref = HumanInteractionRef {
                    id: HumanInteractionId::new(interaction_id.as_str().to_string()),
                    session_id: session_id.clone(),
                    run_id: pending.run_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    kind: HumanInteractionKind::AskUserQuestion,
                    turn_origin: TurnOrigin::App,
                    output_binding: OutputBinding::AppOnly,
                    status: HumanInteractionStatus::Pending,
                };
                match HumanInteractionRouter::route_ask_user_question(
                    &interaction_ref,
                    &spec,
                    &content,
                ) {
                    HumanReplyRoute::ResolveAskUserQuestion { answers, raw_text } => {
                        let value = serde_json::json!({
                            "answers": answers,
                            "rawText": raw_text,
                            "annotations": {
                                "rawText": raw_text,
                                "source": "im",
                                "answerMode": "freeText"
                            }
                        });
                        if !self.resolve_user_question_answer(&pending, value)? {
                            return Ok(HandleOutcome::InvalidApprovalAction {
                                message: "当前提问已失效，请重新发送你的请求。".to_string(),
                            });
                        }
                        self.deliver_app_feedback_for_pending(
                            &pending,
                            AppFeedbackDecision::InteractionSubmit,
                        )
                        .await;
                        self.remove_pending_if_current(session_id, &pending).await;
                        Ok(HandleOutcome::AnswerResolved)
                    }
                    HumanReplyRoute::AbandonAndStartNewTurn { reason, .. } => {
                        if !self.resolve_abandoned(&pending, reason)? {
                            return Ok(HandleOutcome::InvalidApprovalAction {
                                message: "当前提问已失效，请重新发送你的请求。".to_string(),
                            });
                        }
                        self.remove_pending_if_current(session_id, &pending).await;
                        Ok(HandleOutcome::NewTurnAfterAbandon)
                    }
                    HumanReplyRoute::Clarify { message } => {
                        Ok(HandleOutcome::InvalidApprovalAction { message })
                    }
                    HumanReplyRoute::ResolvePermission { .. } => unreachable!(
                        "ask user question router must not return permission resolution"
                    ),
                }
            }
        }
    }

    async fn resolve_permission_intent(
        &self,
        session_id: &SessionId,
        pending: &PendingAsk,
        tool_call_id: &ToolCallId,
        intent: PermissionDecisionIntent,
        path_auth_scope: Option<&str>,
        requested_path: Option<&str>,
    ) -> Result<HandleOutcome> {
        let targets = self
            .related_permission_targets(
                session_id,
                pending,
                tool_call_id,
                requested_path.map(ToOwned::to_owned),
                path_auth_scope.map(ToOwned::to_owned),
            )
            .await;
        if let Some(message) =
            self.permission_group_clarification(session_id, &targets, intent.clone())
        {
            return Ok(HandleOutcome::InvalidApprovalAction { message });
        }
        let run_id = pending.run_id.clone();
        for target in targets {
            self.resolve_single_permission_intent(
                session_id,
                &target.pending,
                &target.tool_call_id,
                intent.clone(),
                target.path_auth_scope.as_deref(),
                target.requested_path.as_deref(),
            )
            .await?;
        }
        if let Err(err) = self.remind_next_pending_ask(session_id, &run_id).await {
            log::warn!(
                "[im-ask] failed to remind next pending ask session={} run={} error={:#}",
                session_id.as_str(),
                run_id.as_str(),
                err
            );
        }
        Ok(HandleOutcome::ApprovalResolved)
    }

    async fn remind_next_pending_ask(&self, session_id: &SessionId, run_id: &RunId) -> Result<()> {
        let Some(next_pending) = self
            .pending_for_session(session_id)
            .await
            .into_iter()
            .find(|pending| pending.run_id == *run_id && self.is_pending_ask_live(pending))
        else {
            return Ok(());
        };
        log::info!(
            "[im-ask] reminding next pending ask session={} run={} kind={}",
            session_id.as_str(),
            run_id.as_str(),
            match &next_pending.kind {
                PendingAskKind::Permission { tool_name, .. } => format!("permission/{}", tool_name),
                PendingAskKind::UserQuestion { .. } => "user_question".to_string(),
            }
        );
        let payload = ask_delivery_payload(
            session_id,
            &next_pending.run_id,
            &next_pending.kind,
            format_pending_ask_markdown(&next_pending.kind),
            true,
        );
        self.sink.deliver_ask(&payload).await
    }

    async fn resolve_single_permission_intent(
        &self,
        session_id: &SessionId,
        pending: &PendingAsk,
        tool_call_id: &ToolCallId,
        intent: PermissionDecisionIntent,
        path_auth_scope: Option<&str>,
        requested_path: Option<&str>,
    ) -> Result<()> {
        match intent {
            PermissionDecisionIntent::AllowOnce => {
                if !self.resolve_permission_command(pending, ApprovalCommandDecision::AllowOnce)? {
                    return Ok(());
                }
                self.deliver_app_feedback_for_pending(
                    pending,
                    AppFeedbackDecision::PermissionAllow { remember: false },
                )
                .await;
                self.remove_pending_if_current(session_id, pending).await;
                Ok(())
            }
            PermissionDecisionIntent::Cancel { .. } => {
                if !self.resolve_permission_command(pending, ApprovalCommandDecision::Cancel)? {
                    return Ok(());
                }
                self.deliver_app_feedback_for_pending(
                    pending,
                    AppFeedbackDecision::PermissionCancel,
                )
                .await;
                self.remove_pending_if_current(session_id, pending).await;
                Ok(())
            }
            PermissionDecisionIntent::Deny { .. } => {
                let override_scope = requested_path.and_then(|requested_path| {
                    resolve_permission_path_scope_override(
                        requested_path,
                        path_auth_scope,
                        Some(requested_path),
                    )
                });
                self.permission_cp.resolve_pending_request(
                    tool_call_id,
                    PendingPermissionResolution::Deny {
                        message: "Denied from IM natural language approval.".to_string(),
                        remember: false,
                        destination: None,
                        path_auth_scope_override: override_scope,
                    },
                )?;
                self.deliver_app_feedback_for_pending(pending, AppFeedbackDecision::PermissionDeny)
                    .await;
                self.remove_pending_if_current(session_id, pending).await;
                Ok(())
            }
            PermissionDecisionIntent::AllowAlways { scope: Some(scope) } => {
                let Some(override_scope) =
                    resolve_permission_path_scope_override(&scope, path_auth_scope, requested_path)
                else {
                    return Ok(());
                };
                self.permission_cp.resolve_pending_request(
                    tool_call_id,
                    PendingPermissionResolution::Allow {
                        updated_input: None,
                        remember: true,
                        destination: Some(PermissionDestination::User),
                        message: None,
                        path_auth_scope_override: Some(override_scope),
                    },
                )?;
                self.deliver_app_feedback_for_pending(
                    pending,
                    AppFeedbackDecision::PermissionAllow { remember: true },
                )
                .await;
                self.remove_pending_if_current(session_id, pending).await;
                Ok(())
            }
            PermissionDecisionIntent::AllowAlways { scope: None } => {
                if !self
                    .resolve_permission_command(pending, ApprovalCommandDecision::AllowAlways)?
                {
                    return Ok(());
                }
                self.deliver_app_feedback_for_pending(
                    pending,
                    AppFeedbackDecision::PermissionAllow { remember: true },
                )
                .await;
                self.remove_pending_if_current(session_id, pending).await;
                Ok(())
            }
        }
    }

    async fn related_permission_targets(
        &self,
        session_id: &SessionId,
        current: &PendingAsk,
        current_tool_call_id: &ToolCallId,
        current_requested_path: Option<String>,
        current_path_auth_scope: Option<String>,
    ) -> Vec<PermissionResolveTarget> {
        let current_target = PermissionResolveTarget {
            pending: current.clone(),
            tool_call_id: current_tool_call_id.clone(),
            requested_path: current_requested_path.clone(),
            path_auth_scope: current_path_auth_scope.clone(),
        };
        let Some(current_key) = permission_group_key_for_pending(
            session_id,
            current,
            current_requested_path.as_deref(),
        ) else {
            return vec![current_target];
        };
        let mut targets = Vec::new();
        for pending in self.pending_for_session(session_id).await {
            if !self.is_pending_ask_live(&pending) {
                continue;
            }
            let PendingAskKind::Permission {
                tool_call_id,
                path_auth_scope,
                ..
            } = &pending.kind
            else {
                continue;
            };
            let requested_path = self
                .permission_cp
                .get_pending_request(tool_call_id)
                .and_then(|request| {
                    extract_requested_path_from_tool_args(&request.original_request.args)
                });
            let Some(candidate_key) =
                permission_group_key_for_pending(session_id, &pending, requested_path.as_deref())
            else {
                continue;
            };
            if candidate_key == current_key {
                targets.push(PermissionResolveTarget {
                    pending: pending.clone(),
                    tool_call_id: tool_call_id.clone(),
                    requested_path,
                    path_auth_scope: path_auth_scope.clone(),
                });
            }
        }
        if targets.is_empty() {
            vec![current_target]
        } else {
            targets
        }
    }

    fn permission_group_clarification(
        &self,
        session_id: &SessionId,
        targets: &[PermissionResolveTarget],
        intent: PermissionDecisionIntent,
    ) -> Option<String> {
        if targets.len() <= 1 {
            return None;
        }
        let first = targets.first()?;
        let key = permission_group_key_for_pending(
            session_id,
            &first.pending,
            first.requested_path.as_deref(),
        )?;
        let mut group = PermissionGroup::new(key);
        for target in targets {
            let Some(path) = target.requested_path.as_deref() else {
                return None;
            };
            group.push_request(target.tool_call_id.clone(), path);
        }
        match group.resolve(intent) {
            PermissionGroupResolution::NeedClarification { message } => Some(message),
            PermissionGroupResolution::ResolveAll | PermissionGroupResolution::ResolveOne(_) => {
                None
            }
        }
    }

    fn resolve_permission_command(
        &self,
        pending: &PendingAsk,
        decision: ApprovalCommandDecision,
    ) -> Result<bool> {
        if let PendingAskKind::Permission { tool_call_id, .. } = &pending.kind {
            if !self.permission_cp.is_pending(tool_call_id) {
                return Ok(false);
            }
            match decision {
                ApprovalCommandDecision::AllowOnce => self.permission_cp.resolve_pending_request(
                    tool_call_id,
                    PendingPermissionResolution::Allow {
                        updated_input: None,
                        remember: false,
                        destination: None,
                        message: None,
                        path_auth_scope_override: None,
                    },
                )?,
                ApprovalCommandDecision::AllowAlways => {
                    self.permission_cp.resolve_pending_request(
                        tool_call_id,
                        PendingPermissionResolution::Allow {
                            updated_input: None,
                            remember: true,
                            destination: Some(PermissionDestination::User),
                            message: None,
                            path_auth_scope_override: None,
                        },
                    )?
                }
                ApprovalCommandDecision::Deny => self.permission_cp.resolve_pending_request(
                    tool_call_id,
                    PendingPermissionResolution::Deny {
                        message: "Denied from IM approval command.".to_string(),
                        remember: false,
                        destination: None,
                        path_auth_scope_override: None,
                    },
                )?,
                ApprovalCommandDecision::Cancel => self.permission_cp.resolve_pending_request(
                    tool_call_id,
                    PendingPermissionResolution::Deny {
                        message: "Cancelled from IM approval command.".to_string(),
                        remember: false,
                        destination: None,
                        path_auth_scope_override: None,
                    },
                )?,
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn is_pending_ask_live(&self, pending: &PendingAsk) -> bool {
        match &pending.kind {
            PendingAskKind::Permission { tool_call_id, .. } => {
                self.permission_cp.is_pending(tool_call_id)
            }
            PendingAskKind::UserQuestion { interaction_id, .. } => {
                self.interaction_cp.is_pending(interaction_id)
            }
        }
    }

    fn resolve_user_question_answer(
        &self,
        pending: &PendingAsk,
        value: serde_json::Value,
    ) -> Result<bool> {
        if let PendingAskKind::UserQuestion { interaction_id, .. } = &pending.kind {
            if self.interaction_cp.is_pending(interaction_id) {
                self.interaction_cp
                    .resolve(interaction_id, InteractionResolution::Submit { value })?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn resolve_abandoned(&self, pending: &PendingAsk, reason: String) -> Result<bool> {
        match &pending.kind {
            PendingAskKind::Permission { tool_call_id, .. } => {
                if self.permission_cp.is_pending(tool_call_id) {
                    self.permission_cp.resolve_pending_request(
                        tool_call_id,
                        PendingPermissionResolution::Deny {
                            message: format!("User changed topic in IM channel: {}", reason),
                            remember: false,
                            destination: None,
                            path_auth_scope_override: None,
                        },
                    )?;
                    return Ok(true);
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
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    async fn deliver_app_feedback_for_pending(
        &self,
        pending: &PendingAsk,
        _decision: AppFeedbackDecision,
    ) {
        let Some(app_feedback) = self.app_feedback.as_ref() else {
            return;
        };
        match &pending.kind {
            PendingAskKind::Permission { tool_call_id, .. } => {
                app_feedback.take_permission(tool_call_id)
            }
            PendingAskKind::UserQuestion { interaction_id, .. } => {
                app_feedback.take_interaction(interaction_id)
            }
        };
    }

    async fn remove_pending_if_current(&self, session_id: &SessionId, pending: &PendingAsk) {
        let mut guard = self.pending.lock().await;
        let key = session_id.as_str();
        let removed = guard.remove_matching(key, pending);
        drop(guard);
        if let Some(removed) = removed {
            self.clear_app_feedback_for_kind(removed.kind);
        }
    }

    async fn remove_pending_for_run(&self, session_id: &SessionId, run_id: &RunId, reason: &str) {
        let mut guard = self.pending.lock().await;
        let key = session_id.as_str();
        let removed = guard.remove_run(key, run_id);
        drop(guard);
        for pending in removed {
            log::info!(
                "[im-ask] removed pending ask session={} run={} kind={} reason={}",
                session_id.as_str(),
                run_id.as_str(),
                match &pending.kind {
                    PendingAskKind::Permission { .. } => "permission",
                    PendingAskKind::UserQuestion { .. } => "user_question",
                },
                reason
            );
            self.clear_app_feedback_for_kind(pending.kind);
        }
    }

    fn clear_app_feedback_for_kind(&self, kind: PendingAskKind) {
        let Some(app_feedback) = self.app_feedback.as_ref() else {
            return;
        };
        match kind {
            PendingAskKind::Permission { tool_call_id, .. } => {
                app_feedback.clear_permission(&tool_call_id);
            }
            PendingAskKind::UserQuestion { interaction_id, .. } => {
                app_feedback.clear_interaction(&interaction_id);
            }
        }
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
        let payload =
            ask_delivery_payload(&event.session_id, &event.run_id, &kind, markdown, false);
        self.sink.deliver_ask(&payload).await?;
        if let Some(app_feedback) = self.app_feedback.as_ref() {
            match &kind {
                PendingAskKind::Permission { tool_call_id, .. } => {
                    app_feedback.register_permission(
                        tool_call_id.clone(),
                        event.session_id.clone(),
                        event.run_id.clone(),
                    );
                }
                PendingAskKind::UserQuestion { interaction_id, .. } => {
                    app_feedback.register_interaction(
                        interaction_id.clone(),
                        event.session_id.clone(),
                        event.run_id.clone(),
                    );
                }
            }
        }
        let pending = PendingAsk {
            run_id: event.run_id.clone(),
            kind,
            primary_model,
        };
        self.pending
            .lock()
            .await
            .insert(event.session_id.as_str().to_string(), pending);

        self.consume_late_queued_reply(&event.session_id).await?;

        Ok(())
    }

    async fn consume_late_queued_reply(&self, session_id: &SessionId) -> Result<()> {
        let Some(pending_queue) = self.pending_queue.as_ref() else {
            return Ok(());
        };
        let Some(item) = pending_queue
            .take_next_for_human_interaction(session_id)
            .await
        else {
            return Ok(());
        };
        let item_id = item.id.clone();
        let content = item.text.clone();
        log::info!(
            "[im-ask] consuming queued message as human-interaction reply session={} item_id={}",
            session_id.as_str(),
            item_id
        );
        match self.try_handle_reply(session_id, content).await? {
            HandleOutcome::ApprovalResolved | HandleOutcome::AnswerResolved => {}
            HandleOutcome::NewTurnAfterAbandon | HandleOutcome::NotPending => {
                pending_queue
                    .dispatch_taken_human_interaction_item_as_new_turn(session_id, item)
                    .await?;
            }
            HandleOutcome::InvalidApprovalAction { message } => {
                log::info!(
                    "[im-ask] queued human-interaction reply requires clarification session={} item_id={} message={}",
                    session_id.as_str(),
                    item_id,
                    message
                );
            }
        }
        Ok(())
    }
}

fn app_feedback_for_approval_decision(decision: ApprovalCommandDecision) -> AppFeedbackDecision {
    match decision {
        ApprovalCommandDecision::AllowOnce => {
            AppFeedbackDecision::PermissionAllow { remember: false }
        }
        ApprovalCommandDecision::AllowAlways => {
            AppFeedbackDecision::PermissionAllow { remember: true }
        }
        ApprovalCommandDecision::Deny => AppFeedbackDecision::PermissionDeny,
        ApprovalCommandDecision::Cancel => AppFeedbackDecision::PermissionCancel,
    }
}

fn permission_decision_intent_from_approval(
    decision: ApprovalCommandDecision,
    scope: Option<String>,
) -> PermissionDecisionIntent {
    match decision {
        ApprovalCommandDecision::AllowOnce => PermissionDecisionIntent::AllowOnce,
        ApprovalCommandDecision::AllowAlways => PermissionDecisionIntent::AllowAlways { scope },
        ApprovalCommandDecision::Deny => PermissionDecisionIntent::Deny { reason: None },
        ApprovalCommandDecision::Cancel => PermissionDecisionIntent::Cancel { reason: None },
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
                path_auth_scope,
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
                        path_auth_scope: path_auth_scope.clone(),
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
            RuntimeEventKind::TurnCompleted { .. } => {
                self.remove_pending_for_run(&event.session_id, &event.run_id, "turn_completed")
                    .await;
                Ok(())
            }
            RuntimeEventKind::RunCancelled | RuntimeEventKind::RunCompleted => {
                self.remove_pending_for_run(&event.session_id, &event.run_id, "run_finished")
                    .await;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

pub fn parse_pending_action_command(content: &str) -> Option<PendingActionCommand> {
    let trimmed = content.trim();
    let mut parts = trimmed
        .splitn(4, char::is_whitespace)
        .filter(|part| !part.is_empty());
    let command = parts.next()?;
    let id = parts.next()?.trim().to_string();
    let action = parts.next()?.trim();
    let action_lower = action.to_ascii_lowercase();

    match command.to_ascii_lowercase().as_str() {
        "/approve" | "approve" => {
            let decision = match action_lower.as_str() {
                "allow" | "允许" => ApprovalCommandDecision::AllowOnce,
                "always" | "allow-always" | "永久允许" => ApprovalCommandDecision::AllowAlways,
                "deny" | "拒绝" => ApprovalCommandDecision::Deny,
                "cancel" | "取消" => ApprovalCommandDecision::Cancel,
                _ => return None,
            };
            Some(PendingActionCommand::Approve { id, decision })
        }
        "/answer" | "answer" => {
            if action_lower == "cancel" || action == "取消" {
                return Some(PendingActionCommand::AnswerCancel { id });
            }
            let rest = parts.next().unwrap_or("").trim();
            let answer = if rest.is_empty() {
                action.to_string()
            } else {
                format!("{action} {rest}")
            };
            let value = serde_json::from_str::<serde_json::Value>(&answer)
                .unwrap_or_else(|_| serde_json::json!({ "answer": answer }));
            Some(PendingActionCommand::Answer { id, value })
        }
        _ => None,
    }
}

fn ask_question_spec_from_payload(payload: &serde_json::Value) -> AskQuestionSpec {
    let questions = payload
        .get("questions")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, question)| {
            question
                .get("id")
                .and_then(|value| value.as_str())
                .or_else(|| question.get("question").and_then(|value| value.as_str()))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("question_{}", index + 1))
        })
        .collect();
    AskQuestionSpec { questions }
}

const PERMISSION_REPLY_INTENT_TOOL: &str = "PermissionReplyIntent";

const PERMISSION_JUDGE_SYSTEM_CONTRACT: &str =
    "Return exactly one PermissionReplyIntent tool call. No prose.";

fn permission_judge_input(
    tool_name: &str,
    ask_message: &str,
    suggestions: &[String],
    current_path_auth_scope: Option<&str>,
    requested_path: Option<&str>,
    user_reply: &str,
) -> String {
    serde_json::json!({
        "pending": {
            "kind": "permission",
            "toolName": tool_name,
            "askMessage": ask_message,
            "suggestions": suggestions,
            "requestedPath": requested_path,
            "currentPathAuthScope": current_path_auth_scope,
        },
        "userReply": user_reply,
    })
    .to_string()
}

fn permission_reply_intent_tool() -> ToolDefinition {
    ToolDefinition {
        name: PERMISSION_REPLY_INTENT_TOOL.to_string(),
        description: "Build the program intent for one pending permission reply.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["intent", "reason"],
            "properties": {
                "intent": {
                    "type": "string",
                    "enum": ["resolve_permission", "new_turn", "clarify_pending", "unclear"]
                },
                "decision": {
                    "type": ["string", "null"],
                    "enum": ["allow_once", "allow_always", "deny", "cancel", null]
                },
                "scope": {
                    "type": ["string", "null"]
                },
                "reason": {
                    "type": "string"
                },
                "message": {
                    "type": ["string", "null"]
                }
            }
        }),
    }
}

fn parse_permission_judge_response(
    response: &crate::llm::streaming::LlmResponse,
) -> Option<PendingPermissionReplyIntent> {
    if let Some(tool_call) = response
        .tool_calls
        .iter()
        .find(|call| call.name == PERMISSION_REPLY_INTENT_TOOL)
    {
        return parse_permission_judge_value(tool_call.arguments.clone());
    }
    parse_permission_judge_json(&response.content)
}

fn parse_permission_judge_json(content: &str) -> Option<PendingPermissionReplyIntent> {
    let parsed = serde_json::from_str(strip_json_fence(content)).ok()?;
    parse_permission_judge_value(parsed)
}

fn parse_permission_judge_value(value: serde_json::Value) -> Option<PendingPermissionReplyIntent> {
    let parsed: PermissionJudgeJson = serde_json::from_value(value).ok()?;
    let reason = parsed.reason.unwrap_or_else(|| "judge result".to_string());
    match parsed.intent.as_str() {
        "resolve_permission" => {
            let decision = match parsed.decision.as_deref()? {
                "allow_once" => ApprovalCommandDecision::AllowOnce,
                "allow_always" => ApprovalCommandDecision::AllowAlways,
                "deny" => ApprovalCommandDecision::Deny,
                "cancel" => ApprovalCommandDecision::Cancel,
                _ => return None,
            };
            Some(PendingPermissionReplyIntent::Resolve {
                decision,
                scope: parsed.scope,
                reason,
            })
        }
        "new_turn" => Some(PendingPermissionReplyIntent::NewTurn { reason }),
        "clarify_pending" => Some(PendingPermissionReplyIntent::ClarifyPending {
            message: parsed
                .message
                .unwrap_or_else(|| "这次是在等待你确认是否允许执行当前工具请求。".to_string()),
        }),
        "unclear" => Some(PendingPermissionReplyIntent::Unclear {
            message: parsed.message.unwrap_or_else(|| {
                "我没法确认这是授权、拒绝，还是一个新任务。请换句话说明。".to_string()
            }),
        }),
        _ => None,
    }
}

fn strip_json_fence(input: &str) -> &str {
    let trimmed = input.trim();
    let without_prefix = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim()
}

fn resolve_permission_path_scope_override(
    raw_path: &str,
    current_path_auth_scope: Option<&str>,
    requested_path: Option<&str>,
) -> Option<String> {
    let current = current_path_auth_scope?;
    let raw_path = raw_path.trim().trim_matches(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                '`' | '"' | '\'' | '“' | '”' | '。' | '，' | ',' | '；' | ';'
            )
    });
    let (kind, current_path) = current.split_once(':')?;
    if kind != "path" && kind != "pathwrite" && kind != "pathdelete" {
        return None;
    }
    let (scope_kind, raw_path) = raw_path
        .strip_prefix("pathwrite:")
        .map(|value| ("pathwrite", value))
        .or_else(|| {
            raw_path
                .strip_prefix("pathdelete:")
                .map(|value| ("pathdelete", value))
        })
        .or_else(|| raw_path.strip_prefix("path:").map(|value| ("path", value)))
        .unwrap_or((kind, raw_path));
    if scope_kind != kind {
        return None;
    }
    let raw_path = raw_path.trim();
    let canonical =
        crate::runtime::path_auth::decide::canonicalize_or_ancestor(std::path::Path::new(raw_path))
            .ok()?;
    if let Some(requested_path) = requested_path {
        let requested_canonical = crate::runtime::path_auth::decide::canonicalize_or_ancestor(
            std::path::Path::new(requested_path),
        )
        .ok()?;
        if !requested_canonical.starts_with(&canonical) {
            return None;
        }
        return Some(format!("{}:{}", kind, canonical.display()));
    }
    let current_canonical = crate::runtime::path_auth::decide::canonicalize_or_ancestor(
        std::path::Path::new(current_path),
    )
    .ok()?;
    if !current_canonical.starts_with(&canonical) {
        return None;
    }
    Some(format!("{}:{}", kind, canonical.display()))
}

fn extract_requested_path_from_tool_args(args: &serde_json::Value) -> Option<String> {
    ["file_path", "path"]
        .into_iter()
        .find_map(|key| args.get(key).and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn looks_like_pending_action_command(content: &str) -> bool {
    matches!(
        content
            .trim()
            .split_whitespace()
            .next()
            .map(|command| command.to_ascii_lowercase()),
        Some(command) if matches!(command.as_str(), "/approve" | "approve" | "/answer" | "answer")
    )
}

fn pending_identity_matches(current: &PendingAsk, expected: &PendingAsk) -> bool {
    if current.run_id != expected.run_id {
        return false;
    }
    match (&current.kind, &expected.kind) {
        (
            PendingAskKind::Permission {
                tool_call_id: current_id,
                ..
            },
            PendingAskKind::Permission {
                tool_call_id: expected_id,
                ..
            },
        ) => current_id == expected_id,
        (
            PendingAskKind::UserQuestion {
                interaction_id: current_id,
                ..
            },
            PendingAskKind::UserQuestion {
                interaction_id: expected_id,
                ..
            },
        ) => current_id == expected_id,
        _ => false,
    }
}

fn permission_group_key_for_pending(
    session_id: &SessionId,
    pending: &PendingAsk,
    requested_path: Option<&str>,
) -> Option<PermissionGroupKey> {
    let PendingAskKind::Permission { .. } = &pending.kind else {
        return None;
    };
    Some(PermissionGroupKey::read_path(
        session_id.clone(),
        pending.run_id.clone(),
        requested_path?,
    ))
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
                "🔒 我需要你的确认才能继续\n\n工具：**{}**\n\n> {}\n\n请选择以下操作之一：",
                tool_name, message
            );
            if !suggestions.is_empty() {
                text.push_str("\n\n");
                for (idx, suggestion) in suggestions.iter().enumerate() {
                    text.push_str(&format!("{}. {}\n", idx + 1, suggestion));
                }
            } else {
                text.push_str("\n\n1. 仅本次允许\n2. 永久允许\n3. 拒绝\n4. 取消当前任务\n");
            }
            text.push_str("\n你也可以直接回复自然语言说明授权范围或调整要求。");
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
            text.push_str("\n你可以按选项回复，也可以直接用自然语言回答。");
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use crate::connector::im::shared::app_feedback::{AppFeedbackSink, IMAppFeedbackCoordinator};
    use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;
    use crate::runtime::chat::ChatTurnOutcome;
    use crate::runtime::ids::{RunId, SessionId, ToolCallId};
    use crate::runtime::interaction::{InteractionKind, InteractionRequest};
    use crate::runtime::pending::queue_manager::ConvDirResolver;
    use crate::runtime::pending::{PendingConfig, PendingItem, PendingQueueManager, PendingSource};
    use crate::runtime::run_registry::RuntimeRunRegistry;
    use crate::runtime::store::{PendingPermissionRequest, PendingPermissionResolution};
    use crate::runtime::tools::permission::PermissionMode;

    struct Registry(bool);
    impl ChannelSessionRegistry for Registry {
        fn is_channel_session(&self, _session_id: &SessionId) -> bool {
            self.0
        }
    }

    struct RecordingSink {
        calls: StdMutex<Vec<String>>,
        followup_calls: StdMutex<Vec<String>>,
    }
    #[async_trait]
    impl AskOutputSink for RecordingSink {
        async fn deliver_ask_card(
            &self,
            _session_id: &SessionId,
            _run_id: &RunId,
            markdown: String,
        ) -> Result<()> {
            self.calls.lock().unwrap().push(markdown);
            Ok(())
        }
        async fn deliver_followup_ask_card(
            &self,
            _session_id: &SessionId,
            markdown: String,
        ) -> Result<()> {
            self.followup_calls.lock().unwrap().push(markdown);
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

    struct RecordingAppFeedbackSink {
        calls: StdMutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl AppFeedbackSink for RecordingAppFeedbackSink {
        async fn deliver_app_feedback(
            &self,
            session_id: &SessionId,
            message: &str,
        ) -> anyhow::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push((session_id.as_str().to_string(), message.to_string()));
            Ok(())
        }
    }

    struct ScriptedJudge {
        intents: StdMutex<VecDeque<PendingPermissionReplyIntent>>,
    }

    impl ScriptedJudge {
        fn one(intent: PendingPermissionReplyIntent) -> Arc<Self> {
            Self::many(vec![intent])
        }

        fn many(intents: Vec<PendingPermissionReplyIntent>) -> Arc<Self> {
            Arc::new(Self {
                intents: StdMutex::new(VecDeque::from(intents)),
            })
        }
    }

    #[async_trait]
    impl PendingReplyJudge for ScriptedJudge {
        async fn judge_permission(
            &self,
            _model: &str,
            _tool_name: &str,
            _ask_message: &str,
            _suggestions: &[String],
            _current_path_auth_scope: Option<&str>,
            _requested_path: Option<&str>,
            _user_reply: &str,
        ) -> PendingPermissionReplyIntent {
            self.intents.lock().unwrap().pop_front().unwrap_or_else(|| {
                PendingPermissionReplyIntent::Unclear {
                    message: "scripted judge exhausted".into(),
                }
            })
        }
    }

    fn make_coordinator() -> IMAskCoordinator {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        )
    }

    fn make_coordinator_with_judge(
        permission: Arc<crate::runtime::store::PendingPermissionRequestStore>,
        interaction: Arc<crate::runtime::interaction::InMemoryInteractionControlPlane>,
        judge: Arc<dyn PendingReplyJudge>,
    ) -> IMAskCoordinator {
        IMAskCoordinator::new_with_judge(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
            judge,
        )
    }

    struct TestConvDirResolver(std::path::PathBuf);

    impl ConvDirResolver for TestConvDirResolver {
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

    fn original_request(tool_call_id: &str, tool_name: &str) -> RuntimeToolCallRequest {
        RuntimeToolCallRequest {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            args: serde_json::json!({}),
            purpose: None,
        }
    }

    fn permission_request(tool_call_id: &str) -> PendingPermissionRequest {
        PendingPermissionRequest {
            tool_call_id: ToolCallId::new(tool_call_id),
            session_id: SessionId::new("sess-im"),
            run_id: RunId::new("run-1"),
            tool_name: "bash".into(),
            capability_scopes: vec![],
            message: "run ls".into(),
            suggestions: vec![],
            mode: PermissionMode::Default,
            remember_options: vec![],
            default_destination: None,
            original_request: original_request(tool_call_id, "bash"),
            turn_origin: crate::runtime::human_interaction::TurnOrigin::App,
            output_binding: crate::runtime::human_interaction::OutputBinding::AppOnly,
            path_auth_scope: None,
        }
    }

    fn interaction_request(interaction_id: &str) -> InteractionRequest {
        InteractionRequest {
            interaction_id: InteractionId::new(interaction_id),
            session_id: SessionId::new("sess-im"),
            run_id: RunId::new("run-1"),
            tool_call_id: ToolCallId::new("tool-1"),
            tool_name: "AskUserQuestion".into(),
            kind: InteractionKind::AskUserQuestion,
            payload: serde_json::json!({"questions": []}),
            original_request: original_request("tool-1", "AskUserQuestion"),
            turn_origin: crate::runtime::human_interaction::TurnOrigin::App,
            output_binding: crate::runtime::human_interaction::OutputBinding::AppOnly,
        }
    }

    #[test]
    fn permission_markdown_contains_operation() {
        let text = format_pending_ask_markdown(&PendingAskKind::Permission {
            tool_call_id: ToolCallId::new("tool-1"),
            tool_name: "bash".into(),
            message: "命令：`ls /tmp`".into(),
            suggestions: vec!["cwd=/tmp".into()],
            path_auth_scope: None,
        });
        assert!(text.contains("bash"));
        assert!(text.contains("ls /tmp"));
        assert!(text.contains("cwd=/tmp"));
    }

    #[test]
    fn permission_markdown_hides_internal_approve_commands() {
        let text = format_pending_ask_markdown(&PendingAskKind::Permission {
            tool_call_id: ToolCallId::new("call_00_secret"),
            tool_name: "Read".into(),
            message: "该路径未授权，需要用户确认：路径=/private/tmp/a.txt".into(),
            suggestions: vec!["仅本次允许".into(), "永久允许".into(), "拒绝".into()],
            path_auth_scope: Some("path:/private/tmp".into()),
        });

        assert!(text.contains("我需要你的确认才能继续"));
        assert!(text.contains("Read"));
        assert!(text.contains("仅本次允许"));
        assert!(!text.contains("/approve"));
        assert!(!text.contains("call_00_secret"));
        assert!(!text.contains("备用指令"));
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

    #[test]
    fn user_question_markdown_hides_internal_answer_commands() {
        let text = format_pending_ask_markdown(&PendingAskKind::UserQuestion {
            interaction_id: InteractionId::new("ask-secret"),
            tool_call_id: ToolCallId::new("tool-1"),
            questions: serde_json::json!({
                "questions": [
                    {
                        "question": "专业领域",
                        "options": [
                            { "label": "HR/人事" },
                            { "label": "财务" }
                        ]
                    }
                ]
            }),
        });

        assert!(text.contains("我有几个问题想问你"));
        assert!(text.contains("专业领域"));
        assert!(text.contains("HR/人事"));
        assert!(!text.contains("/answer"));
        assert!(!text.contains("ask-secret"));
        assert!(!text.contains("备用指令"));
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_does_not_auto_resolve_permission_or_user_question() {
        let coordinator = make_coordinator();

        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "bash".into(),
                    message: "run ls".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );
        coordinator.pending.lock().await.insert(
            "sess-question".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::UserQuestion {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    questions: serde_json::json!({"questions": []}),
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        tokio::time::advance(Duration::from_secs(60 * 60)).await;
        tokio::task::yield_now().await;

        assert!(
            coordinator.pending.lock().await.contains_key("sess-im"),
            "pending permission should not be auto-denied by a timer",
        );
        assert!(
            coordinator
                .pending
                .lock()
                .await
                .contains_key("sess-question"),
            "pending user question should not be auto-cancelled by a timer",
        );
    }

    #[test]
    fn parse_approval_command_requires_explicit_format() {
        assert_eq!(
            parse_pending_action_command("/approve tool-1 allow"),
            Some(PendingActionCommand::Approve {
                id: "tool-1".to_string(),
                decision: ApprovalCommandDecision::AllowOnce,
            })
        );
        assert_eq!(
            parse_pending_action_command("/approve tool-1 always"),
            Some(PendingActionCommand::Approve {
                id: "tool-1".to_string(),
                decision: ApprovalCommandDecision::AllowAlways,
            })
        );
        assert_eq!(
            parse_pending_action_command("/approve tool-1 deny"),
            Some(PendingActionCommand::Approve {
                id: "tool-1".to_string(),
                decision: ApprovalCommandDecision::Deny,
            })
        );
        assert_eq!(
            parse_pending_action_command("/approve tool-1 cancel"),
            Some(PendingActionCommand::Approve {
                id: "tool-1".to_string(),
                decision: ApprovalCommandDecision::Cancel,
            })
        );
        assert_eq!(parse_pending_action_command("plain reply"), None);
        assert_eq!(parse_pending_action_command("plain new turn"), None);
    }

    #[test]
    fn parse_answer_command_preserves_answer_text() {
        assert_eq!(
            parse_pending_action_command("/answer ask-1 Main Branch"),
            Some(PendingActionCommand::Answer {
                id: "ask-1".to_string(),
                value: serde_json::json!({ "answer": "Main Branch" }),
            })
        );
        assert_eq!(
            parse_pending_action_command(r#"/answer ask-1 {"answers":{"q":"Main Branch"}}"#),
            Some(PendingActionCommand::Answer {
                id: "ask-1".to_string(),
                value: serde_json::json!({ "answers": { "q": "Main Branch" } }),
            })
        );
        assert_eq!(
            parse_pending_action_command("/answer ask-1 cancel"),
            Some(PendingActionCommand::AnswerCancel {
                id: "ask-1".to_string(),
            })
        );
    }

    #[test]
    fn permission_judge_json_builds_program_intent() {
        let intent = parse_permission_judge_json(
            r#"```json
{"intent":"resolve_permission","decision":"allow_always","scope":"/tmp","reason":"judge reason","message":null}
```"#,
        )
        .expect("judge JSON should parse");

        assert_eq!(
            intent,
            PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::AllowAlways,
                scope: Some("/tmp".into()),
                reason: "judge reason".into(),
            }
        );
    }

    #[test]
    fn permission_judge_tool_call_builds_program_intent() {
        let response = crate::llm::streaming::LlmResponse {
            content: String::new(),
            stop_reason: crate::llm::streaming::StopReason::ToolUse,
            usage: crate::llm::streaming::TokenUsage::default(),
            thinking_blocks: Vec::new(),
            tool_calls: vec![crate::llm::streaming::ToolCall {
                id: "judge-1".into(),
                name: PERMISSION_REPLY_INTENT_TOOL.into(),
                arguments: serde_json::json!({
                    "intent": "new_turn",
                    "decision": null,
                    "scope": null,
                    "reason": "user changed topic",
                    "message": null
                }),
            }],
        };

        assert_eq!(
            parse_permission_judge_response(&response),
            Some(PendingPermissionReplyIntent::NewTurn {
                reason: "user changed topic".into(),
            })
        );
    }

    #[test]
    fn permission_markdown_omits_explicit_approval_commands() {
        let text = format_pending_ask_markdown(&PendingAskKind::Permission {
            tool_call_id: ToolCallId::new("tool-1"),
            tool_name: "bash".into(),
            message: "run ls".into(),
            suggestions: vec![],
            path_auth_scope: None,
        });

        assert!(!text.contains("/approve tool-1 allow"));
        assert!(!text.contains("/approve tool-1 deny"));
        assert!(!text.contains("/approve tool-1 cancel"));
        assert!(!text.contains("备用指令"));
        assert!(text.contains("自然语言"));
        assert!(!text.contains("普通文字不会被当作审批"));
        assert!(!text.contains("会排队到当前任务之后"));
        assert!(!text.contains("不要"));
        assert!(!text.contains("超时"));
        assert!(!text.to_ascii_lowercase().contains("expires"));
    }

    #[test]
    fn user_question_markdown_omits_explicit_answer_commands() {
        let text = format_pending_ask_markdown(&PendingAskKind::UserQuestion {
            interaction_id: InteractionId::new("ask-1"),
            tool_call_id: ToolCallId::new("tool-1"),
            questions: serde_json::json!({
                "questions": [{ "id": "q1", "question": "Which branch?" }]
            }),
        });

        assert!(!text.contains("/answer ask-1"));
        assert!(!text.contains("/answer ask-1 cancel"));
        assert!(!text.contains("备用指令"));
        assert!(text.contains("自然语言"));
        assert!(!text.contains("超时"));
        assert!(!text.to_ascii_lowercase().contains("expires"));
    }

    #[tokio::test]
    async fn explicit_approval_command_is_resolved() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "bash".into(),
                    message: "run ls".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "/approve tool-1 allow".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        match resolution_rx.try_recv().expect("permission should resolve") {
            PendingPermissionResolution::Allow {
                updated_input,
                remember,
                destination,
                message,
                path_auth_scope_override,
            } => {
                assert!(updated_input.is_none());
                assert!(!remember);
                assert!(destination.is_none());
                assert!(message.is_none());
                assert!(path_auth_scope_override.is_none());
            }
            other => panic!("expected allow resolution, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn two_permission_asks_in_same_session_do_not_overwrite_each_other() {
        let coordinator = make_coordinator();
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-read-1"),
                    tool_name: "Read".into(),
                    message: "read a".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-read-2"),
                    tool_name: "Read".into(),
                    message: "read b".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let pending = coordinator
            .pending_for_session(&SessionId::new("sess-im"))
            .await;

        assert_eq!(pending.len(), 2);
        assert!(pending
            .iter()
            .any(|item| item.tool_call_id() == "tool-read-1"));
        assert!(pending
            .iter()
            .any(|item| item.tool_call_id() == "tool-read-2"));
    }

    #[tokio::test]
    async fn resolved_permission_removes_only_current_pending_ask() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut first_rx = permission
            .insert(permission_request("tool-read-1"))
            .unwrap();
        let _second_rx = permission
            .insert(permission_request("tool-read-2"))
            .unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-read-1"),
                    tool_name: "Read".into(),
                    message: "read a".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-read-2"),
                    tool_name: "Read".into(),
                    message: "read b".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "/approve tool-read-1 allow".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        assert!(matches!(
            first_rx
                .try_recv()
                .expect("first permission should resolve"),
            PendingPermissionResolution::Allow { .. }
        ));
        let remaining = coordinator
            .pending_for_session(&SessionId::new("sess-im"))
            .await;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].tool_call_id(), "tool-read-2");
    }

    #[tokio::test]
    async fn permission_explicit_deny_is_consumed_before_llm_judge() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator =
            make_coordinator_with_judge(permission, interaction, ScriptedJudge::many(Vec::new()));
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: "read file".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "好的，先拒绝吧".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        assert!(matches!(
            resolution_rx.try_recv().expect("permission should resolve"),
            PendingPermissionResolution::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn permission_new_topic_is_abandoned_before_llm_judge() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator =
            make_coordinator_with_judge(permission, interaction, ScriptedJudge::many(Vec::new()));
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: "read file".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "问我三个问题".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::NewTurnAfterAbandon);
        match resolution_rx
            .try_recv()
            .expect("permission should be abandoned")
        {
            PendingPermissionResolution::Deny { message, .. } => {
                assert!(message.contains("changed topic"));
            }
            other => panic!("expected abandoned permission, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn natural_permission_reply_resolves_all_related_pending_asks() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut request_1 = permission_request("tool-read-1");
        request_1.tool_name = "Read".into();
        request_1.original_request.args =
            serde_json::json!({"file_path": "/tmp/aijia-permission-test/secret1.txt"});
        let mut request_2 = permission_request("tool-read-2");
        request_2.tool_name = "Read".into();
        request_2.original_request.args =
            serde_json::json!({"file_path": "/tmp/aijia-permission-test/secret2.txt"});
        let mut rx_1 = permission.insert(request_1).unwrap();
        let mut rx_2 = permission.insert(request_2).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let sink = Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
            followup_calls: StdMutex::new(Vec::new()),
        });
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            sink.clone(),
            permission,
            interaction,
        );
        for (tool_call_id, message) in [
            ("tool-read-1", "read secret1"),
            ("tool-read-2", "read secret2"),
        ] {
            coordinator.pending.lock().await.insert(
                "sess-im".into(),
                PendingAsk {
                    run_id: RunId::new("run-1"),
                    kind: PendingAskKind::Permission {
                        tool_call_id: ToolCallId::new(tool_call_id),
                        tool_name: "Read".into(),
                        message: message.into(),
                        suggestions: vec![],
                        path_auth_scope: None,
                    },
                    primary_model: "deepseek-v3".into(),
                },
            );
        }

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "好的".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        assert!(matches!(
            rx_1.try_recv().expect("first permission should resolve"),
            PendingPermissionResolution::Allow { .. }
        ));
        assert!(matches!(
            rx_2.try_recv().expect("second permission should resolve"),
            PendingPermissionResolution::Allow { .. }
        ));
        assert!(coordinator
            .pending_for_session(&SessionId::new("sess-im"))
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn natural_permission_reply_resolves_cross_tool_same_scope_pending_asks() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut request_1 = permission_request("tool-grep-1");
        request_1.tool_name = "Grep".into();
        request_1.original_request.args =
            serde_json::json!({"path": "/tmp/aijia-permission-test", "pattern": "secret"});
        let mut request_2 = permission_request("tool-glob-1");
        request_2.tool_name = "Glob".into();
        request_2.original_request.args =
            serde_json::json!({"path": "/tmp/aijia-permission-test", "pattern": "*.txt"});
        let mut rx_1 = permission.insert(request_1).unwrap();
        let mut rx_2 = permission.insert(request_2).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let sink = Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
            followup_calls: StdMutex::new(Vec::new()),
        });
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            sink.clone(),
            permission,
            interaction,
        );
        for (tool_call_id, tool_name) in [("tool-grep-1", "Grep"), ("tool-glob-1", "Glob")] {
            coordinator.pending.lock().await.insert(
                "sess-im".into(),
                PendingAsk {
                    run_id: RunId::new("run-1"),
                    kind: PendingAskKind::Permission {
                        tool_call_id: ToolCallId::new(tool_call_id),
                        tool_name: tool_name.into(),
                        message: "inspect tmp".into(),
                        suggestions: vec![],
                        path_auth_scope: None,
                    },
                    primary_model: "deepseek-v3".into(),
                },
            );
        }

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "好的".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        assert!(matches!(
            rx_1.try_recv().expect("grep permission should resolve"),
            PendingPermissionResolution::Allow { .. }
        ));
        assert!(matches!(
            rx_2.try_recv().expect("glob permission should resolve"),
            PendingPermissionResolution::Allow { .. }
        ));
        assert!(coordinator
            .pending_for_session(&SessionId::new("sess-im"))
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn natural_permission_reply_targets_first_waiting_permission_when_scopes_differ() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut request_1 = permission_request("tool-glob-1");
        request_1.tool_name = "Glob".into();
        request_1.original_request.args =
            serde_json::json!({"path": "/Users/oayzz", "pattern": "*.ts"});
        let mut request_2 = permission_request("tool-read-1");
        request_2.tool_name = "Read".into();
        request_2.original_request.args = serde_json::json!({
            "file_path": "/Users/oayzz/.real/.bin/browser-runtime/src/infra/tmp-openclaw-dir.ts"
        });
        let mut rx_1 = permission.insert(request_1).unwrap();
        let mut rx_2 = permission.insert(request_2).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let sink = Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
            followup_calls: StdMutex::new(Vec::new()),
        });
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            sink.clone(),
            permission,
            interaction,
        );
        for (tool_call_id, tool_name, path_auth_scope) in [
            ("tool-glob-1", "Glob", "path:/Users/oayzz"),
            (
                "tool-read-1",
                "Read",
                "path:/Users/oayzz/.real/.bin/browser-runtime/src/infra",
            ),
        ] {
            coordinator.pending.lock().await.insert(
                "sess-im".into(),
                PendingAsk {
                    run_id: RunId::new("run-1"),
                    kind: PendingAskKind::Permission {
                        tool_call_id: ToolCallId::new(tool_call_id),
                        tool_name: tool_name.into(),
                        message: format!("该路径未授权，需要用户确认：路径={}", path_auth_scope),
                        suggestions: vec![],
                        path_auth_scope: Some(path_auth_scope.into()),
                    },
                    primary_model: "deepseek-v3".into(),
                },
            );
        }

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "好的".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        assert!(matches!(
            rx_1.try_recv()
                .expect("first waiting permission should resolve"),
            PendingPermissionResolution::Allow { .. }
        ));
        assert!(
            rx_2.try_recv().is_err(),
            "later different-scope permission should remain pending"
        );
        let pending = coordinator
            .pending_for_session(&SessionId::new("sess-im"))
            .await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].tool_call_id(), "tool-read-1");
        assert!(
            sink.calls.lock().unwrap().is_empty(),
            "follow-up reminder should not reuse the initial ask-card path"
        );
        let followup_calls = sink.followup_calls.lock().unwrap();
        assert_eq!(followup_calls.len(), 1);
        assert!(followup_calls[0].contains("工具：**Read**"));
        assert!(followup_calls[0].contains("/Users/oayzz/.real/.bin/browser-runtime/src/infra"));
    }

    #[tokio::test]
    async fn permission_judge_allow_once_reminds_next_waiting_permission_when_scopes_differ() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut request_1 = permission_request("tool-read-1");
        request_1.tool_name = "Read".into();
        request_1.original_request.args = serde_json::json!({
            "file_path": "/Users/oayzz/project/lotus/docs/a.md"
        });
        let mut request_2 = permission_request("tool-read-2");
        request_2.tool_name = "Read".into();
        request_2.original_request.args = serde_json::json!({
            "file_path": "/Users/oayzz/.real/.bin/browser-runtime/src/infra/tmp-openclaw-dir.ts"
        });
        let mut rx_1 = permission.insert(request_1).unwrap();
        let mut rx_2 = permission.insert(request_2).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let sink = Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
            followup_calls: StdMutex::new(Vec::new()),
        });
        let coordinator = IMAskCoordinator::new_with_judge(
            Arc::new(Registry(true)),
            sink.clone(),
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::AllowOnce,
                scope: None,
                reason: "judge returned allow once".into(),
            }),
        );
        for (tool_call_id, path_auth_scope) in [
            ("tool-read-1", "path:/Users/oayzz/project/lotus/docs"),
            (
                "tool-read-2",
                "path:/Users/oayzz/.real/.bin/browser-runtime/src/infra",
            ),
        ] {
            coordinator.pending.lock().await.insert(
                "sess-im".into(),
                PendingAsk {
                    run_id: RunId::new("run-1"),
                    kind: PendingAskKind::Permission {
                        tool_call_id: ToolCallId::new(tool_call_id),
                        tool_name: "Read".into(),
                        message: format!("该路径未授权，需要用户确认：路径={}", path_auth_scope),
                        suggestions: vec![],
                        path_auth_scope: Some(path_auth_scope.into()),
                    },
                    primary_model: "deepseek-v3".into(),
                },
            );
        }

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "haode".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        assert!(matches!(
            rx_1.try_recv()
                .expect("first judged permission should resolve"),
            PendingPermissionResolution::Allow { .. }
        ));
        assert!(
            rx_2.try_recv().is_err(),
            "later different-scope permission should remain pending"
        );
        let pending = coordinator
            .pending_for_session(&SessionId::new("sess-im"))
            .await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].tool_call_id(), "tool-read-2");
        let followup_calls = sink.followup_calls.lock().unwrap();
        assert_eq!(followup_calls.len(), 1);
        assert!(followup_calls[0].contains("工具：**Read**"));
        assert!(followup_calls[0].contains("/Users/oayzz/.real/.bin/browser-runtime/src/infra"));
    }

    #[tokio::test]
    async fn permission_judge_allow_once_intent_resolves_control_plane() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::AllowOnce,
                scope: None,
                reason: "judge returned allow once".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: "read file".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        match resolution_rx.try_recv().expect("permission should resolve") {
            PendingPermissionResolution::Allow {
                remember,
                destination,
                ..
            } => {
                assert!(!remember);
                assert!(destination.is_none());
            }
            other => panic!("expected allow once resolution, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_judge_deny_does_not_deliver_extra_im_feedback_card() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut pending_request = permission_request("tool-1");
        pending_request.original_request.args =
            serde_json::json!({"file_path": "/tmp/aijia-permission-test/secret3.txt"});
        pending_request.path_auth_scope =
            Some("path:/private/tmp/aijia-permission-test".to_string());
        let mut resolution_rx = permission.insert(pending_request).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let feedback = IMAppFeedbackCoordinator::new();
        let feedback_sink = Arc::new(RecordingAppFeedbackSink {
            calls: StdMutex::new(Vec::new()),
        });
        feedback.set_sink(feedback_sink.clone());
        feedback.register_permission(
            ToolCallId::new("tool-1"),
            SessionId::new("sess-im"),
            RunId::new("run-1"),
        );
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::Deny,
                scope: Some("path:/private/tmp/aijia-permission-test/secret3.txt".into()),
                reason: "user denied".into(),
            }),
        )
        .with_app_feedback(feedback);
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: "read file".into(),
                    suggestions: vec![],
                    path_auth_scope: Some("path:/private/tmp/aijia-permission-test".into()),
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        match resolution_rx.try_recv().expect("permission should resolve") {
            PendingPermissionResolution::Deny {
                path_auth_scope_override,
                ..
            } => {
                assert_eq!(
                    path_auth_scope_override.as_deref(),
                    Some("path:/private/tmp/aijia-permission-test/secret3.txt")
                );
            }
            other => panic!("expected deny resolution, got {:?}", other),
        }
        assert!(
            feedback_sink.calls.lock().unwrap().is_empty(),
            "IM natural-language approval should resume the run without a separate feedback card"
        );
    }

    #[tokio::test]
    async fn permission_judge_allow_always_intent_remembers_user() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::AllowAlways,
                scope: None,
                reason: "judge returned allow always".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: "read file".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        match resolution_rx.try_recv().expect("permission should resolve") {
            PendingPermissionResolution::Allow {
                remember,
                destination,
                ..
            } => {
                assert!(remember);
                assert_eq!(destination, Some(PermissionDestination::User));
            }
            other => panic!("expected allow always resolution, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_judge_unclear_intent_keeps_pending() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let _resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Unclear {
                message: "judge marked this reply as unclear".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: "read file".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(
            outcome,
            HandleOutcome::InvalidApprovalAction {
                message: "judge marked this reply as unclear".into(),
            }
        );
        assert!(coordinator.pending.lock().await.contains_key("sess-im"));
    }

    #[tokio::test]
    async fn explicit_user_question_answer_resolves_control_plane() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let mut resolution_rx = interaction
            .insert_pending(interaction_request("ask-1"))
            .expect("interaction insert");
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::UserQuestion {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    questions: serde_json::json!({"questions": []}),
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "/answer ask-1 Main Branch".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::AnswerResolved);
        match resolution_rx
            .try_recv()
            .expect("interaction should resolve")
        {
            InteractionResolution::Submit { value } => {
                assert_eq!(value, serde_json::json!({ "answer": "Main Branch" }));
            }
            other => panic!("expected submit resolution, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn explicit_user_question_cancel_resolves_control_plane() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let mut resolution_rx = interaction
            .insert_pending(interaction_request("ask-1"))
            .expect("interaction insert");
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::UserQuestion {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    questions: serde_json::json!({"questions": []}),
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "/answer ask-1 cancel".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::AnswerResolved);
        match resolution_rx
            .try_recv()
            .expect("interaction should resolve")
        {
            InteractionResolution::Cancel { message } => {
                assert!(message.contains("cancelled") || message.contains("cancel"));
            }
            other => panic!("expected cancel resolution, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn ordinary_user_question_reply_resolves_as_answer() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let mut resolution_rx = interaction
            .insert_pending(interaction_request("ask-1"))
            .expect("interaction insert");
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::UserQuestion {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    questions: serde_json::json!({
                        "questions": [
                            { "id": "domain", "question": "专业领域" },
                            { "id": "help", "question": "最需要协助" },
                            { "id": "style", "question": "输出风格" }
                        ]
                    }),
                },
                primary_model: "qwen-plus".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "HR/人事\n数据处理与分析\n结论优先".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::AnswerResolved);
        match resolution_rx
            .try_recv()
            .expect("interaction should resolve")
        {
            InteractionResolution::Submit { value } => {
                assert_eq!(
                    value,
                    serde_json::json!({
                        "answers": {
                            "domain": "HR/人事",
                            "help": "数据处理与分析",
                            "style": "结论优先"
                        },
                        "rawText": "HR/人事\n数据处理与分析\n结论优先",
                        "annotations": {
                            "rawText": "HR/人事\n数据处理与分析\n结论优先",
                            "source": "im",
                            "answerMode": "freeText"
                        }
                    })
                );
            }
            other => panic!("expected submit resolution, got {:?}", other),
        }
        assert!(
            !coordinator.pending.lock().await.contains_key("sess-im"),
            "resolved AskUserQuestion should be removed"
        );
    }

    #[tokio::test]
    async fn registering_user_question_consumes_late_queued_im_reply() {
        let tmp = tempfile::TempDir::new().unwrap();
        let run_registry = Arc::new(RuntimeRunRegistry::new());
        let event_bus = Arc::new(crate::runtime::event_bus::RuntimeEventBus::new());
        let pending_queue = PendingQueueManager::new(
            run_registry.clone(),
            event_bus,
            Arc::new(TestConvDirResolver(tmp.path().to_path_buf())),
            PendingConfig::default(),
        );
        let session = SessionId::new("sess-im");
        run_registry
            .reserve(session.as_str(), RunId::new("run-1"))
            .unwrap();
        pending_queue
            .enqueue_or_send(
                session.clone(),
                PendingItem::im_text_for_test(
                    PendingSource::ImDingtalk,
                    "HR/人事\n数据处理与分析\n结论优先",
                    "conv-ding",
                ),
            )
            .await
            .unwrap();

        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let mut resolution_rx = interaction
            .insert_pending(interaction_request("ask-1"))
            .expect("interaction insert");
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        )
        .with_pending_queue(pending_queue.clone());

        coordinator
            .on_event(&RuntimeEvent::new(
                session.clone(),
                RunId::new("run-1"),
                RuntimeEventKind::UserInteractionRequired {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "AskUserQuestion".into(),
                    kind: InteractionKind::AskUserQuestion,
                    payload: serde_json::json!({
                        "questions": [
                            { "id": "domain", "question": "专业领域" },
                            { "id": "help", "question": "最需要协助" },
                            { "id": "style", "question": "输出风格" }
                        ]
                    }),
                    primary_model: "qwen-plus".into(),
                },
            ))
            .await
            .unwrap();

        match resolution_rx
            .try_recv()
            .expect("queued IM message should answer the pending question")
        {
            InteractionResolution::Submit { value } => {
                assert_eq!(
                    value,
                    serde_json::json!({
                        "answers": {
                            "domain": "HR/人事",
                            "help": "数据处理与分析",
                            "style": "结论优先"
                        },
                        "rawText": "HR/人事\n数据处理与分析\n结论优先",
                        "annotations": {
                            "rawText": "HR/人事\n数据处理与分析\n结论优先",
                            "source": "im",
                            "answerMode": "freeText"
                        }
                    })
                );
            }
            other => panic!("expected submit resolution, got {:?}", other),
        }
        assert!(pending_queue.snapshot(&session).await.is_empty());
    }

    #[tokio::test]
    async fn ordinary_user_question_reply_without_question_ids_keeps_raw_text() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let mut resolution_rx = interaction
            .insert_pending(interaction_request("ask-1"))
            .expect("interaction insert");
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::UserQuestion {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    questions: serde_json::json!({
                        "questions": [
                            { "question": "专业领域" },
                            { "question": "最需要协助" }
                        ]
                    }),
                },
                primary_model: "qwen-plus".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "HR/人事\n数据处理与分析".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::AnswerResolved);
        match resolution_rx
            .try_recv()
            .expect("interaction should resolve")
        {
            InteractionResolution::Submit { value } => {
                assert_eq!(
                    value,
                    serde_json::json!({
                        "answers": {
                            "专业领域": "HR/人事",
                            "最需要协助": "数据处理与分析"
                        },
                        "rawText": "HR/人事\n数据处理与分析",
                        "annotations": {
                            "rawText": "HR/人事\n数据处理与分析",
                            "source": "im",
                            "answerMode": "freeText"
                        }
                    })
                );
            }
            other => panic!("expected submit resolution, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn user_question_topic_change_abandons_and_falls_through() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let mut resolution_rx = interaction
            .insert_pending(interaction_request("ask-1"))
            .expect("interaction insert");
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::UserQuestion {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    questions: serde_json::json!({
                        "questions": [{ "id": "topic", "question": "想写什么" }]
                    }),
                },
                primary_model: "qwen-plus".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "问我三个问题".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::NewTurnAfterAbandon);
        match resolution_rx.try_recv().expect("interaction should cancel") {
            InteractionResolution::Cancel { message } => {
                assert!(message.contains("changed topic"));
            }
            other => panic!("expected cancel resolution, got {:?}", other),
        }
        assert!(
            !coordinator.pending.lock().await.contains_key("sess-im"),
            "abandoned AskUserQuestion should be removed"
        );
    }

    #[tokio::test]
    async fn stale_permission_reply_does_not_claim_approval_success() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::AllowOnce,
                scope: None,
                reason: "user said yes later".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-old"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-missing"),
                    tool_name: "Read".into(),
                    message: "read old file".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "qwen-plus".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "刚刚那个权限我同意".into())
            .await
            .unwrap();

        assert_eq!(
            outcome,
            HandleOutcome::InvalidApprovalAction {
                message: "刚才那次权限请求已经失效，请重新发起需要权限的操作。".into()
            }
        );
        assert!(
            !coordinator.pending.lock().await.contains_key("sess-im"),
            "stale pending permission should be removed"
        );
    }

    #[tokio::test]
    async fn stale_user_question_pending_is_dropped_and_message_falls_through() {
        let coordinator = make_coordinator();
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::UserQuestion {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    questions: serde_json::json!({"questions": []}),
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "新的问题".into())
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::NotPending);
        assert!(
            !coordinator.pending.lock().await.contains_key("sess-im"),
            "stale pending user question should be removed"
        );
    }

    #[tokio::test]
    async fn turn_completed_for_other_run_does_not_clear_pending_ask() {
        let coordinator = make_coordinator();
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-waiting"),
                kind: PendingAskKind::UserQuestion {
                    interaction_id: InteractionId::new("ask-1"),
                    tool_call_id: ToolCallId::new("tool-1"),
                    questions: serde_json::json!({"questions": []}),
                },
                primary_model: "qwen-plus".into(),
            },
        );

        coordinator
            .on_event(&RuntimeEvent::new(
                SessionId::new("sess-im"),
                RunId::new("run-new-turn"),
                RuntimeEventKind::TurnCompleted {
                    outcome: ChatTurnOutcome::Success,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                    total_cache_creation_input_tokens: 0,
                    total_cache_read_input_tokens: 0,
                    total_cost_usd: None,
                    permission_denial_count: 0,
                },
            ))
            .await
            .unwrap();

        assert!(
            coordinator.pending.lock().await.contains_key("sess-im"),
            "a different run completing must not clear suspended AskUserQuestion"
        );
    }

    #[tokio::test]
    async fn run_cancelled_for_same_run_clears_pending_ask() {
        let coordinator = make_coordinator();
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-waiting"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "bash".into(),
                    message: "run ls".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "qwen-plus".into(),
            },
        );

        coordinator
            .on_event(&RuntimeEvent::new(
                SessionId::new("sess-im"),
                RunId::new("run-waiting"),
                RuntimeEventKind::RunCancelled,
            ))
            .await
            .unwrap();

        assert!(
            !coordinator.pending.lock().await.contains_key("sess-im"),
            "same run cancellation should clear its pending ask"
        );
    }

    #[tokio::test]
    async fn permission_judge_allow_once_intent_resolves() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::AllowOnce,
                scope: None,
                reason: "user allowed once".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: "read file".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        match resolution_rx.try_recv().expect("permission should resolve") {
            PendingPermissionResolution::Allow {
                remember,
                destination,
                path_auth_scope_override,
                ..
            } => {
                assert!(!remember);
                assert_eq!(destination, None);
                assert_eq!(path_auth_scope_override, None);
            }
            other => panic!("expected allow-once resolution, got {:?}", other),
        }
        assert!(
            !coordinator.pending.lock().await.contains_key("sess-im"),
            "resolved permission should clear pending ask"
        );
    }

    #[tokio::test]
    async fn permission_judge_new_turn_intent_abandons_pending_and_falls_through() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::NewTurn {
                reason: "model classified reply as a new turn".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "bash".into(),
                    message: "run ls".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );
        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::NewTurnAfterAbandon);
        match resolution_rx
            .try_recv()
            .expect("permission should be abandoned before dispatching new turn")
        {
            PendingPermissionResolution::Deny { message, .. } => {
                assert!(message.contains("model classified reply as a new turn"));
            }
            other => panic!("expected abandoned permission, got {:?}", other),
        }
        assert!(
            !coordinator.pending.lock().await.contains_key("sess-im"),
            "new turn should clear pending approval so the message can dispatch normally"
        );
    }

    #[tokio::test]
    async fn permission_judge_path_scope_intent_resolves_remembered_allow() {
        let temp = tempfile::tempdir().expect("tempdir");
        let canonical = std::fs::canonicalize(temp.path()).expect("canonical tempdir");
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut request = permission_request("tool-1");
        request.path_auth_scope = Some(format!("path:{}/secret.txt", canonical.display()));
        let mut resolution_rx = permission.insert(request).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::AllowAlways,
                scope: Some(canonical.display().to_string()),
                reason: "user allowed this directory".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: "read file".into(),
                    suggestions: vec![],
                    path_auth_scope: Some(format!("path:{}/secret.txt", canonical.display())),
                },
                primary_model: "qwen-plus".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        match resolution_rx.try_recv().expect("permission should resolve") {
            PendingPermissionResolution::Allow {
                remember,
                destination,
                path_auth_scope_override,
                ..
            } => {
                assert!(remember);
                assert_eq!(destination, Some(PermissionDestination::User));
                assert_eq!(
                    path_auth_scope_override,
                    Some(format!("path:{}", canonical.display()))
                );
            }
            other => panic!("expected allow resolution, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_judge_file_scope_can_resolve_directory_default_scope() {
        let temp = tempfile::tempdir().expect("tempdir");
        let canonical = std::fs::canonicalize(temp.path()).expect("canonical tempdir");
        let file_path = canonical.join("secret.txt");
        std::fs::write(&file_path, "secret").expect("write test file");

        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut request = permission_request("tool-1");
        request.path_auth_scope = Some(format!("path:{}", canonical.display()));
        request.original_request.args = serde_json::json!({
            "file_path": file_path.display().to_string()
        });
        let mut resolution_rx = permission.insert(request).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::AllowAlways,
                scope: Some(file_path.display().to_string()),
                reason: "judge allowed the current file".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: format!("read {}", file_path.display()),
                    suggestions: vec![],
                    path_auth_scope: Some(format!("path:{}", canonical.display())),
                },
                primary_model: "qwen-plus".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        match resolution_rx.try_recv().expect("permission should resolve") {
            PendingPermissionResolution::Allow {
                remember,
                destination,
                path_auth_scope_override,
                ..
            } => {
                assert!(remember);
                assert_eq!(destination, Some(PermissionDestination::User));
                assert_eq!(
                    path_auth_scope_override,
                    Some(format!("path:{}", file_path.display()))
                );
            }
            other => panic!("expected allow resolution, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_judge_encoded_path_scope_resolves_remembered_allow() {
        let temp = tempfile::tempdir().expect("tempdir");
        let canonical = std::fs::canonicalize(temp.path()).expect("canonical tempdir");
        let file_path = canonical.join("secret.txt");
        std::fs::write(&file_path, "secret").expect("write test file");
        let scope = format!("path:{}", canonical.display());

        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut request = permission_request("tool-1");
        request.path_auth_scope = Some(scope.clone());
        request.original_request.args = serde_json::json!({
            "file_path": file_path.display().to_string()
        });
        let mut resolution_rx = permission.insert(request).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::Resolve {
                decision: ApprovalCommandDecision::AllowAlways,
                scope: Some(scope.clone()),
                reason: "judge allowed the containing directory".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: format!("read {}", file_path.display()),
                    suggestions: vec![],
                    path_auth_scope: Some(scope.clone()),
                },
                primary_model: "qwen-plus".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::ApprovalResolved);
        match resolution_rx.try_recv().expect("permission should resolve") {
            PendingPermissionResolution::Allow {
                remember,
                destination,
                message,
                path_auth_scope_override,
                ..
            } => {
                assert!(remember);
                assert_eq!(destination, Some(PermissionDestination::User));
                assert_eq!(message, None);
                assert_eq!(path_auth_scope_override, Some(scope));
            }
            other => panic!("expected allow resolution, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_judge_new_turn_intent_clears_pending_without_consuming_message() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let mut resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::one(PendingPermissionReplyIntent::NewTurn {
                reason: "user moved to another file".into(),
            }),
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "Read".into(),
                    message: "read file".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "qwen-plus".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(
                &SessionId::new("sess-im"),
                "free text routed to judge".into(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, HandleOutcome::NewTurnAfterAbandon);
        match resolution_rx.try_recv().expect("permission should resolve") {
            PendingPermissionResolution::Deny { message, .. } => {
                assert!(message.contains("user moved to another file"));
            }
            other => panic!("expected deny/cancel resolution, got {:?}", other),
        }
        assert!(
            !coordinator.pending.lock().await.contains_key("sess-im"),
            "new turn intent should clear pending approval"
        );
    }

    #[tokio::test]
    async fn concurrent_ordinary_permission_replies_are_captured_without_clearing_pending() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let _resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = Arc::new(make_coordinator_with_judge(
            permission,
            interaction,
            ScriptedJudge::many(vec![
                PendingPermissionReplyIntent::Unclear {
                    message: "judge could not classify reply".into(),
                },
                PendingPermissionReplyIntent::Unclear {
                    message: "judge could not classify reply".into(),
                },
            ]),
        ));
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "bash".into(),
                    message: "run ls".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let first = {
            let coordinator = Arc::clone(&coordinator);
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                coordinator
                    .try_handle_reply(&SessionId::new("sess-im"), "第一条普通消息".into())
                    .await
                    .unwrap()
            })
        };
        let second = {
            let coordinator = Arc::clone(&coordinator);
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                coordinator
                    .try_handle_reply(&SessionId::new("sess-im"), "第二条普通消息".into())
                    .await
                    .unwrap()
            })
        };
        barrier.wait().await;

        let (first, second) = tokio::join!(first, second);
        let outcomes = vec![first.unwrap(), second.unwrap()];

        assert_eq!(
            outcomes,
            vec![
                HandleOutcome::InvalidApprovalAction {
                    message: "judge could not classify reply".into(),
                },
                HandleOutcome::InvalidApprovalAction {
                    message: "judge could not classify reply".into(),
                },
            ]
        );
        assert!(
            coordinator.pending.lock().await.contains_key("sess-im"),
            "ordinary permission replies must not clear pending approval"
        );
    }

    #[tokio::test]
    async fn invalid_approval_command_is_rejected_without_clearing_pending() {
        let permission = Arc::new(crate::runtime::store::PendingPermissionRequestStore::new());
        let _resolution_rx = permission.insert(permission_request("tool-1")).unwrap();
        let interaction =
            Arc::new(crate::runtime::interaction::InMemoryInteractionControlPlane::new());
        let coordinator = IMAskCoordinator::new(
            Arc::new(Registry(true)),
            Arc::new(RecordingSink {
                calls: StdMutex::new(Vec::new()),
                followup_calls: StdMutex::new(Vec::new()),
            }),
            permission,
            interaction,
        );
        coordinator.pending.lock().await.insert(
            "sess-im".into(),
            PendingAsk {
                run_id: RunId::new("run-1"),
                kind: PendingAskKind::Permission {
                    tool_call_id: ToolCallId::new("tool-1"),
                    tool_name: "bash".into(),
                    message: "run ls".into(),
                    suggestions: vec![],
                    path_auth_scope: None,
                },
                primary_model: "deepseek-v3".into(),
            },
        );

        let outcome = coordinator
            .try_handle_reply(&SessionId::new("sess-im"), "/approve tool-1 maybe".into())
            .await
            .unwrap();

        assert_eq!(
            outcome,
            HandleOutcome::InvalidApprovalAction {
                message: "审批指令无效或已不匹配，请使用当前卡片上的按钮或指令。".into(),
            }
        );
        assert!(coordinator.pending.lock().await.contains_key("sess-im"));
    }
}

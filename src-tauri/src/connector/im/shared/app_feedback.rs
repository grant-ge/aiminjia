use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::runtime::ids::{RunId, SessionId, ToolCallId};
use crate::runtime::interaction::InteractionId;

#[async_trait::async_trait]
pub trait AppFeedbackSink: Send + Sync {
    async fn deliver_app_feedback(
        &self,
        session_id: &SessionId,
        message: &str,
    ) -> anyhow::Result<()>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppFeedbackRoute {
    pub session_id: SessionId,
    pub run_id: RunId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppFeedbackDecision {
    PermissionAllow { remember: bool },
    PermissionDeny,
    PermissionCancel,
    InteractionSubmit,
    InteractionCancel,
}

pub fn feedback_message(decision: AppFeedbackDecision) -> &'static str {
    match decision {
        AppFeedbackDecision::PermissionAllow { remember: false } => {
            "已允许本次操作，任务继续执行。"
        }
        AppFeedbackDecision::PermissionAllow { remember: true } => "已记录授权范围，任务继续执行。",
        AppFeedbackDecision::PermissionDeny => "已拒绝本次权限请求。",
        AppFeedbackDecision::PermissionCancel => "已取消当前任务。",
        AppFeedbackDecision::InteractionSubmit => "已提交你的回答，任务继续执行。",
        AppFeedbackDecision::InteractionCancel => "已取消这次提问。",
    }
}

#[derive(Default)]
pub struct IMAppFeedbackCoordinator {
    permissions: Mutex<HashMap<String, AppFeedbackRoute>>,
    interactions: Mutex<HashMap<String, AppFeedbackRoute>>,
    sink: Mutex<Option<Arc<dyn AppFeedbackSink>>>,
}

impl IMAppFeedbackCoordinator {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn register_permission(&self, id: ToolCallId, session_id: SessionId, run_id: RunId) {
        self.permissions.lock().unwrap().insert(
            id.as_str().to_string(),
            AppFeedbackRoute { session_id, run_id },
        );
    }

    pub fn register_interaction(&self, id: InteractionId, session_id: SessionId, run_id: RunId) {
        self.interactions.lock().unwrap().insert(
            id.as_str().to_string(),
            AppFeedbackRoute { session_id, run_id },
        );
    }

    pub fn take_permission(&self, id: &ToolCallId) -> Option<AppFeedbackRoute> {
        self.permissions.lock().unwrap().remove(id.as_str())
    }

    pub fn take_interaction(&self, id: &InteractionId) -> Option<AppFeedbackRoute> {
        self.interactions.lock().unwrap().remove(id.as_str())
    }

    pub fn set_sink(&self, sink: Arc<dyn AppFeedbackSink>) {
        *self.sink.lock().unwrap() = Some(sink);
    }

    pub async fn deliver(&self, route: AppFeedbackRoute, message: &str) -> anyhow::Result<()> {
        let sink = self.sink.lock().unwrap().clone();
        let Some(sink) = sink else {
            return Ok(());
        };
        sink.deliver_app_feedback(&route.session_id, message).await
    }

    pub fn clear_permission(&self, id: &ToolCallId) {
        self.permissions.lock().unwrap().remove(id.as_str());
    }

    pub fn clear_interaction(&self, id: &InteractionId) {
        self.interactions.lock().unwrap().remove(id.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex as StdMutex;

    #[test]
    fn permission_feedback_messages_are_short_and_user_facing() {
        assert_eq!(
            feedback_message(AppFeedbackDecision::PermissionAllow { remember: false }),
            "已允许本次操作，任务继续执行。"
        );
        assert_eq!(
            feedback_message(AppFeedbackDecision::PermissionAllow { remember: true }),
            "已记录授权范围，任务继续执行。"
        );
        assert_eq!(
            feedback_message(AppFeedbackDecision::PermissionDeny),
            "已拒绝本次权限请求。"
        );
        assert_eq!(
            feedback_message(AppFeedbackDecision::PermissionCancel),
            "已取消当前任务。"
        );
    }

    #[test]
    fn interaction_feedback_messages_are_short_and_user_facing() {
        assert_eq!(
            feedback_message(AppFeedbackDecision::InteractionSubmit),
            "已提交你的回答，任务继续执行。"
        );
        assert_eq!(
            feedback_message(AppFeedbackDecision::InteractionCancel),
            "已取消这次提问。"
        );
    }

    #[test]
    fn routes_can_be_registered_and_taken_by_id() {
        let coordinator = IMAppFeedbackCoordinator::new();
        coordinator.register_permission(
            ToolCallId::new("tool-1"),
            SessionId::new("sess-im"),
            RunId::new("run-im"),
        );
        let route = coordinator.take_permission(&ToolCallId::new("tool-1"));
        assert_eq!(route.unwrap().session_id.as_str(), "sess-im");
        assert!(coordinator
            .take_permission(&ToolCallId::new("tool-1"))
            .is_none());

        coordinator.register_interaction(
            InteractionId::new("ask-1"),
            SessionId::new("sess-im"),
            RunId::new("run-im"),
        );
        let route = coordinator.take_interaction(&InteractionId::new("ask-1"));
        assert_eq!(route.unwrap().run_id.as_str(), "run-im");
    }

    struct RecordingSink {
        calls: StdMutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl AppFeedbackSink for RecordingSink {
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

    #[tokio::test]
    async fn deliver_noops_without_sink_and_uses_sink_when_set() {
        let coordinator = IMAppFeedbackCoordinator::new();
        coordinator
            .deliver(
                AppFeedbackRoute {
                    session_id: SessionId::new("sess-im"),
                    run_id: RunId::new("run-im"),
                },
                "已允许本次操作，任务继续执行。",
            )
            .await
            .unwrap();

        let sink = Arc::new(RecordingSink {
            calls: StdMutex::new(Vec::new()),
        });
        coordinator.set_sink(sink.clone());
        coordinator
            .deliver(
                AppFeedbackRoute {
                    session_id: SessionId::new("sess-im"),
                    run_id: RunId::new("run-im"),
                },
                "已允许本次操作，任务继续执行。",
            )
            .await
            .unwrap();

        assert_eq!(
            sink.calls.lock().unwrap().as_slice(),
            [(
                "sess-im".to_string(),
                "已允许本次操作，任务继续执行。".to_string()
            )]
        );
    }
}

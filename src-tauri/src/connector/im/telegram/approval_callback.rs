use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use super::api::{TelegramApi, TgCallbackQuery, TgInlineKeyboardButton, TgInlineKeyboardMarkup};
use super::types::TelegramSessionTarget;
use crate::connector::im::shared::ask_coordinator::{
    AskDeliveryPayload, AskKind, HandleOutcome, IMAskCoordinator, ImAskSink,
};
use crate::runtime::ids::SessionId;

const CALLBACK_PREFIX: &str = "aijia:ask";
const TOKEN_TTL: Duration = Duration::from_secs(30 * 60);
const TOKEN_CAP: usize = 4096;
const CLEANUP_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalAction {
    Allow,
    AllowAlways,
    Deny,
}

impl ApprovalAction {
    fn as_command_arg(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::AllowAlways => "always",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone)]
struct ApprovalTokenRecord {
    session_id: String,
    run_id: String,
    tool_call_id: String,
    interaction_id: String,
    chat_id: i64,
    user_id: i64,
    message_id: Option<i64>,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct CallbackSubmission {
    token: String,
    action: ApprovalAction,
    record: ApprovalTokenRecord,
}

#[derive(Default)]
struct ApprovalTokenStore {
    items: HashMap<String, ApprovalTokenRecord>,
}

impl ApprovalTokenStore {
    fn insert(&mut self, payload: &AskDeliveryPayload, target: &TelegramSessionTarget) -> String {
        self.sweep_expired();
        if self.items.len() >= TOKEN_CAP {
            if let Some(oldest_key) = self
                .items
                .iter()
                .min_by_key(|(_, record)| record.expires_at)
                .map(|(key, _)| key.clone())
            {
                self.items.remove(&oldest_key);
            }
        }
        let token = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(16)
            .collect::<String>();
        self.items.insert(
            token.clone(),
            ApprovalTokenRecord {
                session_id: payload.session_id.as_str().to_string(),
                run_id: payload.run_id.as_str().to_string(),
                tool_call_id: payload.tool_call_id.clone(),
                interaction_id: payload.interaction_id.clone(),
                chat_id: target.chat_id,
                user_id: target.user_id,
                message_id: None,
                expires_at: Instant::now() + TOKEN_TTL,
            },
        );
        token
    }

    fn mark_message(&mut self, token: &str, message_id: i64) {
        if let Some(record) = self.items.get_mut(token) {
            record.message_id = Some(message_id);
        }
    }

    fn remove(&mut self, token: &str) {
        self.items.remove(token);
    }

    fn take_valid(
        &mut self,
        token: &str,
        chat_id: i64,
        user_id: i64,
    ) -> Result<ApprovalTokenRecord, CallbackReject> {
        self.sweep_expired();
        let Some(record) = self.items.get(token).cloned() else {
            return Err(CallbackReject::Expired);
        };
        if record.expires_at <= Instant::now() {
            self.items.remove(token);
            return Err(CallbackReject::Expired);
        }
        if record.chat_id != chat_id || record.user_id != user_id {
            return Err(CallbackReject::MismatchedUser);
        }
        self.items.remove(token);
        Ok(record)
    }

    fn sweep_expired(&mut self) {
        let now = Instant::now();
        self.items.retain(|_, record| record.expires_at > now);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallbackReject {
    Expired,
    MismatchedUser,
}

pub struct TelegramApprovalManager {
    api: Arc<TelegramApi>,
    session_targets: Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
    tokens: Arc<Mutex<ApprovalTokenStore>>,
}

impl TelegramApprovalManager {
    pub fn new(
        api: Arc<TelegramApi>,
        session_targets: Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
    ) -> Self {
        Self {
            api,
            session_targets,
            tokens: Arc::new(Mutex::new(ApprovalTokenStore::default())),
        }
    }

    pub fn start_cleanup(&self, cancel: CancellationToken) {
        let tokens = Arc::clone(&self.tokens);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(CLEANUP_INTERVAL) => {
                        tokens.lock().await.sweep_expired();
                    }
                    _ = cancel.cancelled() => break,
                }
            }
        });
    }

    pub async fn handle_callback_query(
        &self,
        callback: &TgCallbackQuery,
        ask_coordinator: Option<&Arc<IMAskCoordinator>>,
    ) {
        let Some(submission) = self.take_submission(callback).await else {
            return;
        };
        let Some(coordinator) = ask_coordinator else {
            self.answer(callback, "审批暂不可用，请回到 AIjia 桌面端处理。", true)
                .await;
            return;
        };

        let command = format!(
            "/approve {} {}",
            submission.record.tool_call_id,
            submission.action.as_command_arg()
        );
        let session_id = SessionId::new(submission.record.session_id.clone());
        let result = coordinator.try_handle_reply(&session_id, command).await;
        match result {
            Ok(HandleOutcome::ApprovalResolved) | Ok(HandleOutcome::AnswerResolved) => {
                self.answer(callback, "已提交审批", false).await;
                if let Some(message_id) = submission.record.message_id {
                    if let Err(err) = self
                        .api
                        .edit_message_reply_markup(submission.record.chat_id, message_id, None)
                        .await
                    {
                        log::warn!(
                            "[telegram-approval] clear inline keyboard failed session={} run={} token={} interaction={} error={:?}",
                            submission.record.session_id,
                            submission.record.run_id,
                            submission.token,
                            submission.record.interaction_id,
                            err
                        );
                    }
                }
            }
            Ok(HandleOutcome::InvalidApprovalAction { message }) => {
                self.answer(callback, &message, true).await;
            }
            Ok(HandleOutcome::NotPending) | Ok(HandleOutcome::NewTurnAfterAbandon) => {
                self.answer(callback, "审批已失效", true).await;
            }
            Err(err) => {
                log::warn!(
                    "[telegram-approval] callback resolution failed session={} run={} token={} interaction={} error={:#}",
                    submission.record.session_id,
                    submission.record.run_id,
                    submission.token,
                    submission.record.interaction_id,
                    err
                );
                self.answer(callback, "审批处理失败，请稍后重试。", true)
                    .await;
            }
        }
    }

    async fn take_submission(&self, callback: &TgCallbackQuery) -> Option<CallbackSubmission> {
        let Some(data) = callback.data.as_deref() else {
            return None;
        };
        let Some((token, action)) = parse_callback_data(data) else {
            return None;
        };
        let Some(message) = callback.message.as_ref() else {
            self.answer(callback, "审批已失效", true).await;
            return None;
        };
        let record =
            match self
                .tokens
                .lock()
                .await
                .take_valid(&token, message.chat.id, callback.from.id)
            {
                Ok(record) => record,
                Err(CallbackReject::Expired) => {
                    self.answer(callback, "审批已失效", true).await;
                    return None;
                }
                Err(CallbackReject::MismatchedUser) => {
                    self.answer(callback, "这个审批按钮不属于当前会话。", true)
                        .await;
                    return None;
                }
            };
        Some(CallbackSubmission {
            token,
            action,
            record,
        })
    }

    async fn answer(&self, callback: &TgCallbackQuery, text: &str, show_alert: bool) {
        if let Err(err) = self
            .api
            .answer_callback_query(&callback.id, Some(text), show_alert)
            .await
        {
            log::warn!(
                "[telegram-approval] answerCallbackQuery failed id={} error={:?}",
                callback.id,
                err
            );
        }
    }
}

#[async_trait]
impl ImAskSink for TelegramApprovalManager {
    async fn deliver_ask(&self, payload: &AskDeliveryPayload) -> Result<()> {
        let target = {
            let targets = self.session_targets.read().await;
            targets.get(payload.session_id.as_str()).cloned()
        };
        let Some(target) = target else {
            log::warn!(
                "[telegram-approval] no session target for ask session={}",
                payload.session_id.as_str()
            );
            return Ok(());
        };

        if payload.kind != AskKind::Permission {
            let text = non_empty_ask_text(&payload.markdown);
            self.api
                .send_message_with_reply(
                    target.chat_id,
                    &text,
                    None,
                    target.last_inbound_message_id,
                )
                .await?;
            return Ok(());
        }

        let token = self.tokens.lock().await.insert(payload, &target);
        let markup = approval_keyboard(&token);

        let text = non_empty_ask_text(&payload.markdown);
        let result = self
            .api
            .send_message_with_inline_keyboard(
                target.chat_id,
                &text,
                None,
                target.last_inbound_message_id,
                markup,
            )
            .await;
        match result {
            Ok(message) => {
                self.tokens
                    .lock()
                    .await
                    .mark_message(&token, message.message_id);
                Ok(())
            }
            Err(err) => {
                self.tokens.lock().await.remove(&token);
                Err(err.into())
            }
        }
    }

    async fn force_finish_current_card(
        &self,
        _session_id: &SessionId,
        _reason_for_log: &str,
    ) -> Result<()> {
        Ok(())
    }
}

fn approval_keyboard(token: &str) -> TgInlineKeyboardMarkup {
    TgInlineKeyboardMarkup {
        inline_keyboard: vec![
            vec![
                TgInlineKeyboardButton {
                    text: "本次允许".to_string(),
                    callback_data: format!("{CALLBACK_PREFIX}:{token}:allow"),
                },
                TgInlineKeyboardButton {
                    text: "永久允许".to_string(),
                    callback_data: format!("{CALLBACK_PREFIX}:{token}:allow_always"),
                },
            ],
            vec![TgInlineKeyboardButton {
                text: "拒绝".to_string(),
                callback_data: format!("{CALLBACK_PREFIX}:{token}:deny"),
            }],
        ],
    }
}

fn non_empty_ask_text(markdown: &str) -> String {
    if markdown.trim().is_empty() {
        "需要你确认后才能继续。".to_string()
    } else {
        markdown.to_string()
    }
}

fn parse_callback_data(data: &str) -> Option<(String, ApprovalAction)> {
    let mut parts = data.split(':');
    if parts.next()? != "aijia" || parts.next()? != "ask" {
        return None;
    }
    let token = parts.next()?.trim();
    if token.is_empty() {
        return None;
    }
    let action = match parts.next()? {
        "allow" => ApprovalAction::Allow,
        "allow_always" | "always" => ApprovalAction::AllowAlways,
        "deny" => ApprovalAction::Deny,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((token.to_string(), action))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_callback_data_accepts_all_approval_actions() {
        assert_eq!(
            parse_callback_data("aijia:ask:tok123:allow"),
            Some(("tok123".to_string(), ApprovalAction::Allow))
        );
        assert_eq!(
            parse_callback_data("aijia:ask:tok123:allow_always"),
            Some(("tok123".to_string(), ApprovalAction::AllowAlways))
        );
        assert_eq!(
            parse_callback_data("aijia:ask:tok123:always"),
            Some(("tok123".to_string(), ApprovalAction::AllowAlways))
        );
        assert_eq!(
            parse_callback_data("aijia:ask:tok123:deny"),
            Some(("tok123".to_string(), ApprovalAction::Deny))
        );
        assert_eq!(parse_callback_data("aijia:ask:tok123:allow:extra"), None);
    }

    #[test]
    fn approval_action_command_args_match_shared_approval_parser() {
        assert_eq!(ApprovalAction::Allow.as_command_arg(), "allow");
        assert_eq!(ApprovalAction::AllowAlways.as_command_arg(), "always");
        assert_eq!(ApprovalAction::Deny.as_command_arg(), "deny");
    }

    #[test]
    fn approval_keyboard_uses_two_row_three_button_layout() {
        let markup = approval_keyboard("tok123");

        assert_eq!(markup.inline_keyboard.len(), 2);
        assert_eq!(markup.inline_keyboard[0].len(), 2);
        assert_eq!(markup.inline_keyboard[1].len(), 1);
        assert_eq!(markup.inline_keyboard[0][0].text, "本次允许");
        assert_eq!(
            markup.inline_keyboard[0][0].callback_data,
            "aijia:ask:tok123:allow"
        );
        assert_eq!(markup.inline_keyboard[0][1].text, "永久允许");
        assert_eq!(
            markup.inline_keyboard[0][1].callback_data,
            "aijia:ask:tok123:allow_always"
        );
        assert_eq!(markup.inline_keyboard[1][0].text, "拒绝");
        assert_eq!(
            markup.inline_keyboard[1][0].callback_data,
            "aijia:ask:tok123:deny"
        );
    }
}

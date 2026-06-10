use std::collections::BTreeMap;

use super::types::HumanInteractionRef;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskQuestionSpec {
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAskSpec {
    pub tool_name: String,
    pub requested_path: Option<String>,
    pub current_scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecisionIntent {
    AllowOnce,
    AllowAlways { scope: Option<String> },
    Deny { reason: Option<String> },
    Cancel { reason: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanReplyRoute {
    ResolveAskUserQuestion {
        answers: BTreeMap<String, String>,
        raw_text: String,
    },
    ResolvePermission {
        intent: PermissionDecisionIntent,
    },
    AbandonAndStartNewTurn {
        reason: String,
        text: String,
    },
    Clarify {
        message: String,
    },
}

pub struct HumanInteractionRouter;

impl HumanInteractionRouter {
    pub fn route_ask_user_question(
        _interaction: &HumanInteractionRef,
        spec: &AskQuestionSpec,
        text: &str,
    ) -> HumanReplyRoute {
        let trimmed = text.trim();
        if is_topic_change(trimmed) {
            return HumanReplyRoute::AbandonAndStartNewTurn {
                reason: "user changed topic while ask user question was pending".into(),
                text: trimmed.into(),
            };
        }

        let lines: Vec<&str> = trimmed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let mut answers = BTreeMap::new();
        if spec.questions.len() <= 1 {
            let key = spec
                .questions
                .first()
                .cloned()
                .unwrap_or_else(|| "answer".into());
            answers.insert(key, trimmed.into());
        } else {
            for (index, question) in spec.questions.iter().enumerate() {
                if let Some(line) = lines.get(index) {
                    answers.insert(question.clone(), (*line).to_string());
                }
            }
            if answers.is_empty() {
                answers.insert("rawText".into(), trimmed.into());
            }
        }

        HumanReplyRoute::ResolveAskUserQuestion {
            answers,
            raw_text: trimmed.into(),
        }
    }

    pub fn route_permission_reply(
        _interaction: &HumanInteractionRef,
        _spec: &PermissionAskSpec,
        text: &str,
    ) -> HumanReplyRoute {
        let trimmed = text.trim();
        if is_topic_change(trimmed) {
            return HumanReplyRoute::AbandonAndStartNewTurn {
                reason: "user changed topic while permission was pending".into(),
                text: trimmed.into(),
            };
        }
        if contains_any(trimmed, &["拒绝", "不允许", "先拒绝", "不行", "deny"]) {
            return HumanReplyRoute::ResolvePermission {
                intent: PermissionDecisionIntent::Deny { reason: None },
            };
        }
        if contains_any(trimmed, &["取消", "算了", "不用了", "cancel"]) {
            return HumanReplyRoute::ResolvePermission {
                intent: PermissionDecisionIntent::Cancel { reason: None },
            };
        }
        if contains_any(trimmed, &["以后", "永久", "都可以", "都允许", "always"]) {
            return HumanReplyRoute::ResolvePermission {
                intent: PermissionDecisionIntent::AllowAlways {
                    scope: extract_path_like_scope(trimmed),
                },
            };
        }
        if contains_any(trimmed, &["允许", "可以", "同意", "好的", "行", "allow"]) {
            return HumanReplyRoute::ResolvePermission {
                intent: PermissionDecisionIntent::AllowOnce,
            };
        }

        HumanReplyRoute::Clarify {
            message: "我需要确认这是允许、拒绝、取消，还是一个新任务。".into(),
        }
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn is_topic_change(text: &str) -> bool {
    contains_any(text, &["看看别的", "问我", "聊别的", "换个事", "新的任务"])
}

fn extract_path_like_scope(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|part| part.starts_with('/'))
        .map(|part| {
            part.trim_matches(|ch: char| {
                ch.is_whitespace() || matches!(ch, '，' | '。' | ',' | ';' | '；' | '`')
            })
            .to_string()
        })
}

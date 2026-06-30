//! Telegram draft stream state machine.
//!
//! This mirrors the OpenClaw Bot API flow at a lotus-app level: one preview
//! message is sent, later chunks edit it with a throttle, and finalization
//! either edits the preview or falls back to a normal send in sender.rs.

use std::time::{Duration, Instant};

pub(crate) const PREVIEW_EDIT_THROTTLE: Duration = Duration::from_secs(1);
pub(crate) const RECENT_FINALIZED_TTL: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DraftAction {
    None,
    SendPreview { text: String },
    EditPreview { message_id: i64, text: String },
    SendFinal { text: String },
    EditFinal { message_id: i64, text: String },
    SendFail { text: String },
    EditFail { message_id: i64, text: String },
}

#[derive(Debug, Default)]
pub(crate) struct TelegramDraftState {
    accumulated: String,
    message_id: Option<i64>,
    last_preview_at: Option<Instant>,
    stopped_preview: bool,
}

impl TelegramDraftState {
    pub(crate) fn observe_chunk(
        &mut self,
        delta: &str,
        final_chunk: bool,
        now: Instant,
    ) -> DraftAction {
        if !delta.is_empty() {
            self.accumulated.push_str(delta);
        }
        let text = self.accumulated.trim_end().to_string();
        if final_chunk {
            if text.trim().is_empty() {
                return DraftAction::None;
            }
            return match self.message_id {
                Some(message_id) => DraftAction::EditFinal { message_id, text },
                None => DraftAction::SendFinal { text },
            };
        }

        if self.stopped_preview || text.trim().is_empty() {
            return DraftAction::None;
        }
        match self.message_id {
            Some(message_id)
                if self.last_preview_at.map_or(true, |last| {
                    now.duration_since(last) >= PREVIEW_EDIT_THROTTLE
                }) =>
            {
                DraftAction::EditPreview { message_id, text }
            }
            Some(_) => DraftAction::None,
            None => DraftAction::SendPreview { text },
        }
    }

    pub(crate) fn observe_fail(&self) -> DraftAction {
        let text = "❌ 处理失败，请重试".to_string();
        match self.message_id {
            Some(message_id) => DraftAction::EditFail { message_id, text },
            None => DraftAction::SendFail { text },
        }
    }

    pub(crate) fn record_preview_sent(&mut self, message_id: i64, now: Instant) {
        self.message_id = Some(message_id);
        self.last_preview_at = Some(now);
        self.stopped_preview = false;
    }

    pub(crate) fn record_preview_edit(&mut self, now: Instant) {
        self.last_preview_at = Some(now);
        self.stopped_preview = false;
    }

    pub(crate) fn stop_preview(&mut self) {
        self.stopped_preview = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_delta_sends_preview_then_throttles_edits() {
        let t0 = Instant::now();
        let mut s = TelegramDraftState::default();

        assert_eq!(
            s.observe_chunk("hello", false, t0),
            DraftAction::SendPreview {
                text: "hello".into()
            }
        );
        s.record_preview_sent(17, t0);

        assert_eq!(s.observe_chunk(" world", false, t0), DraftAction::None);
        assert_eq!(
            s.observe_chunk(" again", false, t0 + PREVIEW_EDIT_THROTTLE),
            DraftAction::EditPreview {
                message_id: 17,
                text: "hello world again".into()
            }
        );
    }

    #[test]
    fn final_chunk_edits_existing_preview() {
        let t0 = Instant::now();
        let mut s = TelegramDraftState::default();
        s.observe_chunk("hello", false, t0);
        s.record_preview_sent(17, t0);

        assert_eq!(
            s.observe_chunk(" final", true, t0),
            DraftAction::EditFinal {
                message_id: 17,
                text: "hello final".into()
            }
        );
    }
}

//! AI Card 占位 + 增量编辑 状态机。spec v3 §6.1 + §3.11。

use std::time::{Duration, Instant};

const EDIT_THROTTLE: Duration = Duration::from_secs(2);
const EDIT_COUNT_LIMIT: u32 = 6;

#[derive(Debug, Default)]
pub struct WhatsAppAiCardSession {
    pub placeholder_msg_id: Option<String>,
    pub accumulated_text: String,
    pub last_edit_at: Option<Instant>,
    pub edit_count: u32,
    pub finalized: bool,
    pub reaction_sent: bool,
}

#[derive(Debug, PartialEq)]
pub enum AiCardAction {
    /// 不发任何消息（中间 chunk，未达节流阈值）
    Buffer,
    /// 1st chunk：发 reaction 到原消息 + 发 placeholder 文本（拿到 placeholder_msg_id 写回 session）
    StartPlaceholder { text: String },
    /// 后续 chunk 触发节流：edit placeholder（已有 placeholder_msg_id）
    EditPlaceholder { msg_id: String, text: String },
    /// final 到达且没有 placeholder（1st 就 final）：直接发完整文本，不走 placeholder
    SendFinal { text: String },
    /// final 到达且已有 placeholder：最后一次 edit 把完整结果落到 placeholder
    EditFinal { msg_id: String, text: String },
    /// AiCardFail：edit placeholder 到 "生成失败" 文案；如果没 placeholder 也无所谓（连占位都没发就 fail，跳过）
    EditFailMessage { msg_id: String },
    /// finalized 之后又收到 chunk：log warn，不动
    DropAfterFinalized,
    /// 已 finalized 又收到 fail：no-op
    Noop,
}

impl WhatsAppAiCardSession {
    pub fn observe_chunk(&mut self, delta: &str, final_chunk: bool, now: Instant) -> AiCardAction {
        if self.finalized {
            return AiCardAction::DropAfterFinalized;
        }
        self.accumulated_text.push_str(delta);

        match (self.placeholder_msg_id.as_ref(), final_chunk) {
            // 1st chunk + 非 final：发 placeholder
            (None, false) => AiCardAction::StartPlaceholder {
                text: "_正在生成回复..._".into(),
            },
            // 1st chunk + final：直接发完整文本
            (None, true) => {
                self.finalized = true;
                AiCardAction::SendFinal {
                    text: std::mem::take(&mut self.accumulated_text),
                }
            }
            // 后续 chunk
            (Some(msg_id), is_final) => {
                let msg_id_owned = msg_id.clone();
                let elapsed = self
                    .last_edit_at
                    .map(|t| now.duration_since(t))
                    .unwrap_or(Duration::ZERO);
                let should_edit =
                    is_final || (elapsed >= EDIT_THROTTLE && self.edit_count < EDIT_COUNT_LIMIT);
                if !should_edit {
                    return AiCardAction::Buffer;
                }
                if is_final {
                    self.finalized = true;
                    AiCardAction::EditFinal {
                        msg_id: msg_id_owned,
                        text: self.accumulated_text.clone(),
                    }
                } else {
                    AiCardAction::EditPlaceholder {
                        msg_id: msg_id_owned,
                        text: self.accumulated_text.clone(),
                    }
                }
            }
        }
    }

    /// caller 调 send 拿到 placeholder_msg_id 后写回 session。
    pub fn record_placeholder(&mut self, msg_id: String, now: Instant) {
        self.placeholder_msg_id = Some(msg_id);
        self.last_edit_at = Some(now);
        self.edit_count = 1;
    }

    /// caller edit_message 成功后调，更新 last_edit_at + edit_count。
    /// edit 失败时**不**调（spec PR6 决策 #4：静默丢，下次 chunk 重试）。
    pub fn record_edit_success(&mut self, now: Instant) {
        self.last_edit_at = Some(now);
        self.edit_count = self.edit_count.saturating_add(1);
    }

    pub fn observe_fail(&mut self) -> AiCardAction {
        if self.finalized {
            return AiCardAction::Noop;
        }
        self.finalized = true;
        match self.placeholder_msg_id.as_ref() {
            Some(msg_id) => AiCardAction::EditFailMessage {
                msg_id: msg_id.clone(),
            },
            None => AiCardAction::Noop, // 占位还没发就 fail，没法 edit；直接 noop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn first_chunk_non_final_emits_placeholder() {
        let mut s = WhatsAppAiCardSession::default();
        let a = s.observe_chunk("hello", false, t0());
        assert!(matches!(a, AiCardAction::StartPlaceholder { .. }));
    }

    #[test]
    fn first_chunk_final_sends_complete_and_skips_placeholder() {
        let mut s = WhatsAppAiCardSession::default();
        let a = s.observe_chunk("done", true, t0());
        match a {
            AiCardAction::SendFinal { text } => assert_eq!(text, "done"),
            other => panic!("expected SendFinal, got {other:?}"),
        }
        assert!(s.finalized);
    }

    #[test]
    fn chunk_within_throttle_returns_buffer() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        let a = s.observe_chunk("more", false, t0() + Duration::from_millis(500));
        assert_eq!(a, AiCardAction::Buffer);
    }

    #[test]
    fn chunk_after_throttle_emits_edit() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        let a = s.observe_chunk("more", false, t0() + Duration::from_secs(3));
        match a {
            AiCardAction::EditPlaceholder { msg_id, .. } => assert_eq!(msg_id, "P1"),
            other => panic!("expected EditPlaceholder, got {other:?}"),
        }
    }

    #[test]
    fn edit_count_caps_at_limit_returns_buffer() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        s.edit_count = EDIT_COUNT_LIMIT;
        let a = s.observe_chunk("more", false, t0() + Duration::from_secs(5));
        // 即使节流时间过了，count 满也不 edit
        assert_eq!(a, AiCardAction::Buffer);
    }

    #[test]
    fn final_after_throttle_emits_edit_final() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        let a = s.observe_chunk("end", true, t0() + Duration::from_secs(3));
        match a {
            AiCardAction::EditFinal { msg_id, .. } => assert_eq!(msg_id, "P1"),
            other => panic!("expected EditFinal, got {other:?}"),
        }
        assert!(s.finalized);
    }

    #[test]
    fn final_within_throttle_still_emits_edit_final() {
        // spec §6.3：final 强制突破 throttle/count 上限 1 次
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        s.edit_count = EDIT_COUNT_LIMIT; // count 满也突破
        let a = s.observe_chunk("end", true, t0() + Duration::from_millis(100));
        assert!(matches!(a, AiCardAction::EditFinal { .. }));
    }

    #[test]
    fn chunk_after_finalized_returns_drop() {
        let mut s = WhatsAppAiCardSession::default();
        s.finalized = true;
        let a = s.observe_chunk("late", false, t0());
        assert_eq!(a, AiCardAction::DropAfterFinalized);
    }

    #[test]
    fn fail_with_placeholder_emits_edit_fail_msg() {
        let mut s = WhatsAppAiCardSession::default();
        s.record_placeholder("P1".into(), t0());
        let a = s.observe_fail();
        match a {
            AiCardAction::EditFailMessage { msg_id } => assert_eq!(msg_id, "P1"),
            other => panic!("expected EditFailMessage, got {other:?}"),
        }
    }

    #[test]
    fn fail_without_placeholder_is_noop() {
        let mut s = WhatsAppAiCardSession::default();
        let a = s.observe_fail();
        assert_eq!(a, AiCardAction::Noop);
    }
}

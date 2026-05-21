//! 通用"流式 AI 卡片不支持"降级 buffer。
//!
//! 适用平台：capabilities.outbound_aicard == false（wecom / whatsapp / 个微）。
//! 接收到 ReplyContent::AiCardChunk { delta, final_chunk } 时，由 connector 内部
//! 维护一个 buffer 实例（按 session_id 分），按以下策略决定 IO：
//!
//! 1) 首次 chunk：累积，记 started_at
//! 2) 后续 chunks：累积，不发任何消息
//! 3) 超过 placeholder_after 仍未 final：发一次"思考中..."占位
//! 4) final：发完整文本
//!
//! 一次 AI 回复最多 2 条消息（占位 + 最终），通常只有 1 条（最终）。

use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct AiCardFallbackBuffer {
    accumulated: String,
    started_at: Option<Instant>,
    placeholder_after: Duration,
    placeholder_sent: bool,
}

#[derive(Debug)]
pub enum FallbackAction {
    /// 继续累积，无需发消息。
    Buffer,
    /// 发占位消息（"思考中..."），仅 1 次。
    SendPlaceholder { text: String },
    /// 发最终回复。
    SendFinal { text: String },
}

impl AiCardFallbackBuffer {
    pub fn new(placeholder_after: Duration) -> Self {
        Self {
            accumulated: String::new(),
            started_at: None,
            placeholder_after,
            placeholder_sent: false,
        }
    }

    /// Observe one streaming delta from the AI run.
    ///
    /// **Single-use after `SendFinal`.** Once this method returns
    /// `FallbackAction::SendFinal { .. }`, the buffer is considered exhausted
    /// (the accumulated text has been moved out, but `started_at` /
    /// `placeholder_sent` remain stale). Callers must drop or recreate the
    /// buffer before observing a new AI run — do not reuse the same instance.
    /// The wecom connector enforces this by removing the buffer from its
    /// per-session HashMap on `SendFinal`.
    pub fn observe(&mut self, delta: &str, final_chunk: bool) -> FallbackAction {
        self.accumulated.push_str(delta);
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
        }

        if final_chunk {
            return FallbackAction::SendFinal {
                text: std::mem::take(&mut self.accumulated),
            };
        }

        if !self.placeholder_sent {
            if let Some(started) = self.started_at {
                if started.elapsed() >= self.placeholder_after {
                    self.placeholder_sent = true;
                    return FallbackAction::SendPlaceholder {
                        text: "🤔 思考中...".into(),
                    };
                }
            }
        }

        FallbackAction::Buffer
    }
}

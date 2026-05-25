//! WhatsApp connector 内部类型。PR1 只放 reply target；PR2-PR8 按 spec §2
//! 逐步加 PairingState / MessageRef / 内部 JID newtype 等。

/// 反查表条目：把内部 session_id 映射回 WhatsApp JID（"86138...@s.whatsapp.net"）。
/// 入站消息到达时由 parser 写入，出站 send() 时读取。私聊 only 所以一个
/// session_id 唯一对应一个对端 JID。
#[allow(dead_code)] // PR4 parser 会用；PR1 引入是为了让模块文件结构跟 spec §2 对齐
#[derive(Debug, Clone)]
pub struct WhatsAppSessionTarget {
    /// 对端 WhatsApp JID（e.g. `8613800138000@s.whatsapp.net`）
    pub peer_jid: String,
}

use std::time::Instant;

/// QR 扫码登录的 4 状态机。spec v3 §3.5。
///
/// v2 设计的 `AwaitingDeviceConfirm` / `Expired` / `Cancelled` / `Failed` 砍掉，
/// 从超时 / 错误 event 派生即可，不需要单独存。`Instant` 不实现 Serialize，
/// 该 enum 是 connector 内部状态、不直接 emit 给前端；poll_registration 把它
/// 映射到 `ChannelRegistrationPollState`。
#[allow(dead_code)] // PR3 begin/poll_registration 会用；PR2 只定义类型
#[derive(Debug, Clone, Default)]
pub enum PairingState {
    /// 没开始扫码（manager 还没调 begin_registration，或上一次扫码已完成）
    #[default]
    Idle,
    /// bot.run() 起来但 `Event::PairingQrCode` 还没到
    AwaitingQr { started_at: Instant },
    /// QR 已下发；前端展示中。`expires_at` 来自 wa-rs `Event::PairingQrCode.timeout`
    QrIssued { code: String, expires_at: Instant },
    /// 扫码完成。`Event::PairSuccess` 提供 jid + push_name；`Event::Connected` 之后才到此态
    Connected { jid: String, push_name: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_state_idle_default() {
        let s = PairingState::Idle;
        assert!(matches!(s, PairingState::Idle));
    }

    #[test]
    fn pairing_state_qr_issued_holds_code_and_expiry() {
        use std::time::{Duration, Instant};
        let s = PairingState::QrIssued {
            code: "1@abc123def456".into(),
            expires_at: Instant::now() + Duration::from_secs(60),
        };
        match s {
            PairingState::QrIssued { ref code, .. } => assert_eq!(code, "1@abc123def456"),
            _ => panic!("expected QrIssued"),
        }
    }

    #[test]
    fn pairing_state_connected_holds_jid_and_push_name() {
        let s = PairingState::Connected {
            jid: "8613800138000@s.whatsapp.net".into(),
            push_name: "Alice".into(),
        };
        match s {
            PairingState::Connected { jid, push_name } => {
                assert!(jid.ends_with("@s.whatsapp.net"));
                assert_eq!(push_name, "Alice");
            }
            _ => panic!("expected Connected"),
        }
    }

    #[test]
    fn pairing_state_awaiting_qr_carries_start_time() {
        use std::time::Instant;
        let s = PairingState::AwaitingQr {
            started_at: Instant::now(),
        };
        assert!(matches!(s, PairingState::AwaitingQr { .. }));
    }
}

/// PR6：manager worker 把入站消息的元信息写到这里，send() 走 reaction/edit
/// 路径时读回来。spec §6.1 + §3.11。
#[derive(Debug, Clone)]
pub struct WhatsAppLastInbound {
    /// 对话方 jid（私聊场景 = chat jid = sender jid 同一个）
    pub chat_jid: String,
    /// 发送者 jid（群聊时跟 chat_jid 不同；私聊时同 chat_jid）
    pub sender_jid: String,
    /// 用户那条入站消息的 msg_id（react 时填 key.id）
    pub msg_id: String,
    /// 是否群聊（PR4 是 private-only，固定 false；留字段以备 future）
    pub is_group: bool,
}

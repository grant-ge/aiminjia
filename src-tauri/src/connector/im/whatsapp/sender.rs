//! 出站文本发送 + 错误映射。spec v3 §5.1 + §5.3。
//!
//! wa-rs `Client::send_message(to: Jid, msg: wa::Message) -> Result<String, anyhow::Error>`
//! 没有结构化错误枚举，所以错误分类靠文本关键字粗分（spec §5.3 4 类近似）。

use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;
use wa_rs::client::Client;
use wa_rs::wa_rs_proto::whatsapp as wa;
use wa_rs::Jid;

use crate::connector::im::trait_def::ConnectorError;

/// 把 plain text 包成 wa::Message::conversation 发出。
/// `external_key` 形如 `8613912345678@s.whatsapp.net`（PR4 parser 写入
/// ChannelMessage.conversation_key 的形态）。返回 Ok(sent_msg_id) / Err。
pub async fn send_text(
    client: &Arc<Client>,
    external_key: &str,
    body: &str,
) -> Result<String, ConnectorError> {
    let to = Jid::from_str(external_key)
        .map_err(|e| ConnectorError::Fatal(format!("invalid jid '{external_key}': {e}")))?;
    let msg = wa::Message {
        conversation: Some(body.to_string()),
        ..Default::default()
    };
    client.send_message(to, msg).await.map_err(map_send_error)
}

/// 把 wa-rs 裸 anyhow::Error 按文本关键字归到 ConnectorError 4 类。
/// **关键字大小写无关**。spec §5.3。
pub fn map_send_error(e: anyhow::Error) -> ConnectorError {
    let msg = format!("{e:#}");
    let low = msg.to_lowercase();

    // AuthExpired：用户在手机端登出 / 设备解链 / token 失效
    if contains_any(
        &low,
        &[
            "not logged in",
            "unauthorized",
            "401",
            "403",
            "auth",
            "revoked",
            "logged out",
        ],
    ) {
        return ConnectorError::AuthExpired(msg);
    }

    // Transient：网络抖动 / 连接断开 / 服务端重置
    if contains_any(
        &low,
        &[
            "timeout",
            "timed out",
            "connection",
            "refused",
            "reset",
            "closed",
            "network",
            "transport",
            "rate limit",
            "ratelimit",
            "429",
        ],
    ) {
        return ConnectorError::Transient(msg);
    }

    // 其它：归 Fatal（无效 jid / 消息过长 / 协议错）
    ConnectorError::Fatal(msg)
}

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

/// 给入站消息贴 emoji reaction。spec v3 §3.11。
///
/// - `chat_jid_str`：对话 JID（私聊用户 JID 或群 JID）
/// - `target_msg_id`：要 react 的那条消息的 WhatsApp message ID
/// - `sender_jid_str`：发出目标消息的用户 JID
/// - `is_group`：群聊时 true，私聊时 false（决定 key.participant 是否填充）
/// - `emoji`：要贴�� emoji（如 "👍"），传空串表示撤销
pub async fn send_reaction(
    client: &Arc<Client>,
    chat_jid_str: &str,
    target_msg_id: &str,
    sender_jid_str: &str,
    is_group: bool,
    emoji: &str,
) -> Result<(), ConnectorError> {
    let chat_jid = Jid::from_str(chat_jid_str)
        .map_err(|e| ConnectorError::Fatal(format!("invalid jid '{chat_jid_str}': {e}")))?;
    let key = wa::MessageKey {
        remote_jid: Some(chat_jid_str.to_string()),
        from_me: Some(false),
        id: Some(target_msg_id.to_string()),
        participant: if is_group {
            Some(sender_jid_str.to_string())
        } else {
            None
        },
    };
    let reaction = wa::message::ReactionMessage {
        key: Some(key),
        text: Some(emoji.to_string()),
        sender_timestamp_ms: Some(Utc::now().timestamp_millis()),
        ..Default::default()
    };
    let msg = wa::Message {
        reaction_message: Some(reaction),
        ..Default::default()
    };
    client
        .send_message(chat_jid, msg)
        .await
        .map_err(map_send_error)?;
    Ok(())
}

/// 编辑已发出的消息内容。spec v3 §6.1。
///
/// - `chat_jid_str`：对话 JID
/// - `original_msg_id`：要编辑的原消息 ID（由 `send_text` 返回的 sent_msg_id）
/// - `new_body`：新的文本内容
pub async fn edit_text(
    client: &Arc<Client>,
    chat_jid_str: &str,
    original_msg_id: &str,
    new_body: &str,
) -> Result<(), ConnectorError> {
    let to = Jid::from_str(chat_jid_str)
        .map_err(|e| ConnectorError::Fatal(format!("invalid jid '{chat_jid_str}': {e}")))?;
    let new_content = wa::Message {
        conversation: Some(new_body.to_string()),
        ..Default::default()
    };
    client
        .edit_message(to, original_msg_id.to_string(), new_content)
        .await
        .map_err(map_send_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(s: &str) -> anyhow::Error {
        anyhow::anyhow!("{s}")
    }

    #[test]
    fn auth_expired_when_not_logged_in() {
        let r = map_send_error(err("Not logged in"));
        assert!(matches!(r, ConnectorError::AuthExpired(_)), "got {r:?}");
    }

    #[test]
    fn auth_expired_when_unauthorized() {
        assert!(matches!(
            map_send_error(err("HTTP 401 Unauthorized")),
            ConnectorError::AuthExpired(_)
        ));
    }

    #[test]
    fn auth_expired_on_revoked() {
        assert!(matches!(
            map_send_error(err("device session revoked")),
            ConnectorError::AuthExpired(_)
        ));
    }

    #[test]
    fn transient_on_timeout() {
        assert!(matches!(
            map_send_error(err("operation timed out after 30s")),
            ConnectorError::Transient(_)
        ));
    }

    #[test]
    fn transient_on_connection_reset() {
        assert!(matches!(
            map_send_error(err("connection reset by peer")),
            ConnectorError::Transient(_)
        ));
    }

    #[test]
    fn transient_on_rate_limit() {
        assert!(matches!(
            map_send_error(err("HTTP 429 rate limit exceeded")),
            ConnectorError::Transient(_)
        ));
    }

    #[test]
    fn fatal_on_unknown_error() {
        assert!(matches!(
            map_send_error(err("internal protocol error: unknown stanza")),
            ConnectorError::Fatal(_)
        ));
    }

    #[test]
    fn fatal_on_invalid_message() {
        assert!(matches!(
            map_send_error(err("message exceeds maximum length")),
            ConnectorError::Fatal(_)
        ));
    }
}

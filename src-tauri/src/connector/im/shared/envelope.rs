use crate::runtime::human_interaction::{ImPlatform, OutputBinding, TurnOrigin};
use crate::runtime::pending::PendingSource;

pub fn im_platform_for_source(source: PendingSource) -> Option<ImPlatform> {
    match source {
        PendingSource::App => None,
        PendingSource::ImDingtalk => Some(ImPlatform::Dingtalk),
        PendingSource::ImFeishu => Some(ImPlatform::Feishu),
        PendingSource::ImWecom => Some(ImPlatform::Wecom),
        PendingSource::ImWechat => Some(ImPlatform::Wechat),
        PendingSource::ImTelegram => Some(ImPlatform::Telegram),
        PendingSource::ImWhatsapp => Some(ImPlatform::Whatsapp),
    }
}

pub fn im_origin_and_binding(
    source: PendingSource,
    session_id: &str,
    external_conversation_key: &str,
    sender_label: Option<String>,
    allow_streaming_reply: bool,
) -> (TurnOrigin, OutputBinding) {
    let Some(platform) = im_platform_for_source(source) else {
        return (TurnOrigin::App, OutputBinding::AppOnly);
    };

    (
        TurnOrigin::Im {
            platform,
            external_conversation_key: external_conversation_key.to_string(),
            sender_id: None,
            sender_label,
            account_id: None,
            thread_id: None,
        },
        OutputBinding::im(
            platform,
            session_id.to_string(),
            external_conversation_key.to_string(),
            allow_streaming_reply,
        ),
    )
}

//! 钉钉 AI Card API（create / deliver / stream / finish）

use anyhow::{Context, Result};
use serde_json::json;

use super::dingtalk_token::{get_access_token, TokenCache};

const DINGTALK_API: &str = "https://api.dingtalk.com";
const AI_CARD_TEMPLATE_ID: &str = "02fcf2f4-5e02-4a85-b672-46d1f715543e.schema";

/// 投放目标：群聊或私聊
#[derive(Debug, Clone)]
pub enum CardTarget {
    Group { open_conversation_id: String },
    Private { user_id: String },
}

/// 一个已创建并投放的 AI Card 实例
#[derive(Debug, Clone)]
pub struct CardInstance {
    pub card_instance_id: String,
    pub inputing_started: bool,
}

/// 创建 AI Card 并投放到目标会话。成功返回 CardInstance，失败返回 None（不中断主流程）。
pub async fn create_and_deliver_card(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    robot_code: &str,
    target: &CardTarget,
) -> Option<CardInstance> {
    match try_create_and_deliver(cache, app_key, app_secret, robot_code, target).await {
        Ok(inst) => Some(inst),
        Err(e) => {
            log::warn!("[dingtalk-card] create/deliver failed: {:#}", e);
            None
        }
    }
}

async fn try_create_and_deliver(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    robot_code: &str,
    target: &CardTarget,
) -> Result<CardInstance> {
    let token = get_access_token(cache, app_key, app_secret).await?;
    let client = reqwest::Client::new();

    let card_instance_id = format!(
        "card_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        &uuid::Uuid::new_v4().to_string()[..8]
    );

    // 1. 创建卡片实例
    let create_body = json!({
        "cardTemplateId": AI_CARD_TEMPLATE_ID,
        "outTrackId": card_instance_id,
        "cardData": {
            "cardParamMap": {
                "config": "{\"autoLayout\":true}"
            }
        },
        "callbackType": "STREAM",
        "imGroupOpenSpaceModel": { "supportForward": true },
        "imRobotOpenSpaceModel": { "supportForward": true }
    });

    let resp = client
        .post(format!("{}/v1.0/card/instances", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&create_body)
        .send()
        .await
        .context("Failed to create AI card")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("AI card create failed: {} {}", status, body);
    }

    // 2. 投放卡片
    let deliver_body = build_deliver_body(&card_instance_id, target, robot_code);

    let resp = client
        .post(format!("{}/v1.0/card/instances/deliver", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&deliver_body)
        .send()
        .await
        .context("Failed to deliver AI card")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("AI card deliver failed: {} {}", status, body);
    }

    log::info!("[dingtalk-card] card created and delivered: {}", card_instance_id);
    Ok(CardInstance { card_instance_id, inputing_started: false })
}

fn build_deliver_body(card_instance_id: &str, target: &CardTarget, robot_code: &str) -> serde_json::Value {
    match target {
        CardTarget::Group { open_conversation_id } => json!({
            "outTrackId": card_instance_id,
            "userIdType": 1,
            "openSpaceId": format!("dtv1.card//IM_GROUP.{}", open_conversation_id),
            "imGroupOpenDeliverModel": { "robotCode": robot_code }
        }),
        CardTarget::Private { user_id } => json!({
            "outTrackId": card_instance_id,
            "userIdType": 1,
            "openSpaceId": format!("dtv1.card//IM_ROBOT.{}", user_id),
            "imRobotOpenDeliverModel": {
                "spaceType": "IM_ROBOT",
                "robotCode": robot_code,
                "extension": { "dynamicSummary": "true" }
            }
        }),
    }
}

/// 流式更新 AI Card 内容（PUT /v1.0/card/streaming）。
/// 第一次调用时先将卡片切换到 INPUTING 状态（flowStatus=2）。
pub async fn stream_card(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    card: &mut CardInstance,
    content: &str,
    is_finalize: bool,
) -> Result<()> {
    let token = get_access_token(cache, app_key, app_secret).await?;
    let client = reqwest::Client::new();

    // 首次调用：切换到 INPUTING 状态
    if !card.inputing_started {
        let status_body = json!({
            "outTrackId": card.card_instance_id,
            "cardData": {
                "cardParamMap": {
                    "flowStatus": "2",
                    "msgContent": content,
                    "staticMsgContent": "",
                    "sys_full_json_obj": "{\"order\":[\"msgContent\"]}",
                    "config": "{\"autoLayout\":true}"
                }
            }
        });
        let resp = client
            .put(format!("{}/v1.0/card/instances", DINGTALK_API))
            .header("x-acs-dingtalk-access-token", &token)
            .json(&status_body)
            .send()
            .await
            .context("Failed to set INPUTING status")?;
        if !resp.status().is_success() {
            log::warn!("[dingtalk-card] INPUTING PUT returned {}", resp.status());
        }
        card.inputing_started = true;
    }

    let guid = format!(
        "{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        &uuid::Uuid::new_v4().to_string()[..8]
    );

    let stream_body = json!({
        "outTrackId": card.card_instance_id,
        "guid": guid,
        "key": "msgContent",
        "content": content,
        "isFull": true,
        "isFinalize": is_finalize,
        "isError": false
    });

    let resp = client
        .put(format!("{}/v1.0/card/streaming", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&stream_body)
        .send()
        .await
        .context("Failed to stream card content")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        log::warn!("[dingtalk-card] streaming PUT {} {}", status, body);
    }
    Ok(())
}

/// 完成 AI Card（先发 isFinalize=true 的 streaming，再 PUT flowStatus=3）。
pub async fn finish_card(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    card: &mut CardInstance,
    content: &str,
) -> Result<()> {
    stream_card(cache, app_key, app_secret, card, content, true).await?;

    let token = get_access_token(cache, app_key, app_secret).await?;
    let client = reqwest::Client::new();

    let finish_body = json!({
        "outTrackId": card.card_instance_id,
        "cardData": {
            "cardParamMap": {
                "flowStatus": "3",
                "msgContent": content,
                "staticMsgContent": "",
                "sys_full_json_obj": "{\"order\":[\"msgContent\"]}",
                "config": "{\"autoLayout\":true}"
            }
        },
        "cardUpdateOptions": { "updateCardDataByKey": true }
    });

    let resp = client
        .put(format!("{}/v1.0/card/instances", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&finish_body)
        .send()
        .await
        .context("Failed to finish AI card")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        log::warn!("[dingtalk-card] FINISHED PUT {} {}", status, body);
    }

    log::info!("[dingtalk-card] card finished: {}", card.card_instance_id);
    Ok(())
}

/// 将卡片标记为失败（flowStatus=5）。
pub async fn fail_card(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    card: &CardInstance,
) -> Result<()> {
    let token = get_access_token(cache, app_key, app_secret).await?;
    let client = reqwest::Client::new();

    let body = json!({
        "outTrackId": card.card_instance_id,
        "cardData": {
            "cardParamMap": {
                "flowStatus": "5",
                "msgContent": "处理失败，请稍后重试",
                "staticMsgContent": "",
                "sys_full_json_obj": "{\"order\":[\"msgContent\"]}",
                "config": "{\"autoLayout\":true}"
            }
        },
        "cardUpdateOptions": { "updateCardDataByKey": true }
    });

    let _ = client
        .put(format!("{}/v1.0/card/instances", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&body)
        .send()
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_deliver_body_group() {
        let body = build_deliver_body("card_123", &CardTarget::Group {
            open_conversation_id: "cid_abc".into(),
        }, "robot001");
        assert_eq!(body["openSpaceId"], "dtv1.card//IM_GROUP.cid_abc");
        assert_eq!(body["imGroupOpenDeliverModel"]["robotCode"], "robot001");
        assert!(body.get("imRobotOpenDeliverModel").is_none());
    }

    #[test]
    fn build_deliver_body_private() {
        let body = build_deliver_body("card_456", &CardTarget::Private {
            user_id: "user001".into(),
        }, "robot001");
        assert_eq!(body["openSpaceId"], "dtv1.card//IM_ROBOT.user001");
        assert_eq!(body["imRobotOpenDeliverModel"]["robotCode"], "robot001");
        assert_eq!(body["imRobotOpenDeliverModel"]["spaceType"], "IM_ROBOT");
        assert!(body.get("imGroupOpenDeliverModel").is_none());
    }

    #[test]
    fn card_instance_default_inputing_started_false() {
        let card = CardInstance {
            card_instance_id: "card_test".into(),
            inputing_started: false,
        };
        assert!(!card.inputing_started);
    }
}

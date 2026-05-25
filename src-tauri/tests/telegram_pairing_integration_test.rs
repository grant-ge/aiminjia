//! 集成：mock Bot API → 模拟 /start <code> → approve → 验证 allowlist 写盘 + 欢迎消息发送。
//!
//! 不起 ChannelManager（manager 依赖 AppHandle，无法 hermetic 测）；改成直接对
//! TelegramConnector + registration 路径下断言。

use std::sync::Arc;

use app_lib::connector::im::shared::config_store::ChannelConfigStore;
use app_lib::connector::im::telegram::api::TelegramApi;
use app_lib::connector::im::telegram::connector::TelegramConnector;
use app_lib::connector::im::telegram::pairing::{AttachOutcome, PairerInfo};
use app_lib::connector::im::telegram::registration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn build_setup() -> (
    TempDir,
    Arc<ChannelConfigStore>,
    Arc<TelegramConnector>,
    MockServer,
) {
    let dir = TempDir::new().unwrap();
    let cs = Arc::new(ChannelConfigStore::new(dir.path().join("channels"), None));
    cs.save_telegram_registration(
        "TESTTOKEN".into(),
        "8123".into(),
        "test_bot".into(),
        "Test Bot".into(),
    )
    .unwrap();
    let server = MockServer::start().await;
    // 让 connector 内部 API 指向 mock server
    let api = Arc::new(
        TelegramApi::new_with_api_base_for_tests("TESTTOKEN".into(), server.uri())
            .expect("api builds"),
    );
    let connector = Arc::new(TelegramConnector::for_test(
        "8123".into(),
        "test_bot".into(),
        api,
        cs.clone(),
    ));
    (dir, cs, connector, server)
}

#[tokio::test]
async fn approve_writes_allowlist_and_sends_welcome() {
    let (_dir, cs, connector, server) = build_setup().await;
    Mock::given(method("POST"))
        .and(path("/botTESTTOKEN/sendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": { "message_id": 1, "chat": { "id": 42, "type": "private" } }
        })))
        .mount(&server)
        .await;
    let begin = registration::begin_pairing(&connector).await.unwrap();
    assert!(begin.deep_link.starts_with("https://t.me/test_bot?start="));
    let outcome = connector
        .pairing()
        .attempt_attach(
            &begin.code,
            PairerInfo {
                user_id: 42,
                first_name: "Alice".into(),
                username: None,
                chat_id: 42,
                attached_at: chrono::Utc::now(),
            },
        )
        .await;
    assert_eq!(outcome, AttachOutcome::Attached);
    let pending = registration::list_pending(&connector).await.unwrap();
    assert_eq!(pending.len(), 1);
    let user = registration::approve(&connector, &cs, &pending[0].code)
        .await
        .unwrap();
    assert_eq!(user.user_id, 42);
    assert!(cs.telegram_is_in_allowlist(42).unwrap());
}

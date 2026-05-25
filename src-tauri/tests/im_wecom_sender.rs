//! Sender 在 cache hit / miss 时选对的发送通道（respond vs send_msg）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_lib::connector::im::trait_def::ReplyTarget;
use app_lib::connector::im::wecom::sender::{Sender, SessionMap};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Default)]
struct FakeAibot {
    pub respond_calls: Mutex<Vec<(String, Value)>>,
    pub send_msg_calls: Mutex<Vec<Value>>,
}

#[async_trait]
impl app_lib::connector::im::wecom::sender::AibotChannel for FakeAibot {
    async fn respond(&self, req_id: &str, body: Value) -> anyhow::Result<()> {
        self.respond_calls
            .lock()
            .unwrap()
            .push((req_id.to_string(), body));
        Ok(())
    }
    async fn send_msg(&self, body: Value) -> anyhow::Result<()> {
        self.send_msg_calls.lock().unwrap().push(body);
        Ok(())
    }
}

fn target(session_id: &str, ext: &str) -> ReplyTarget {
    ReplyTarget {
        session_id: session_id.into(),
        external_conversation_key: ext.into(),
    }
}

#[tokio::test]
async fn send_markdown_uses_respond_when_session_cached_fresh() {
    let fake = Arc::new(FakeAibot::default());
    let map = SessionMap::new(Duration::from_secs(60));
    map.record("SESS1", "REQ_A").await;
    let sender = Sender::new(fake.clone(), map);
    sender
        .send_markdown(&target("SESS1", "U1"), "hello")
        .await
        .unwrap();

    let calls = fake.respond_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "REQ_A");
    assert_eq!(calls[0].1["msgtype"], "markdown");
    assert_eq!(calls[0].1["markdown"]["content"], "hello");
    assert!(fake.send_msg_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn send_markdown_falls_back_to_send_msg_when_no_cache() {
    let fake = Arc::new(FakeAibot::default());
    let map = SessionMap::new(Duration::from_secs(60));
    let sender = Sender::new(fake.clone(), map);
    sender
        .send_markdown(&target("SESS2", "U2"), "hello")
        .await
        .unwrap();

    assert!(fake.respond_calls.lock().unwrap().is_empty());
    let calls = fake.send_msg_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["chatid"], "U2");
    assert_eq!(calls[0]["msgtype"], "markdown");
}

#[tokio::test]
async fn send_markdown_falls_back_when_cache_expired() {
    let fake = Arc::new(FakeAibot::default());
    let map = SessionMap::new(Duration::from_millis(20));
    map.record("SESS3", "REQ_OLD").await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let sender = Sender::new(fake.clone(), map);
    sender
        .send_markdown(&target("SESS3", "U3"), "hi")
        .await
        .unwrap();
    assert!(fake.respond_calls.lock().unwrap().is_empty());
    assert_eq!(fake.send_msg_calls.lock().unwrap().len(), 1);
}

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::api::TelegramApi;

const TYPING_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TypingKey {
    session_id: String,
    run_id: String,
}

#[derive(Clone)]
pub struct TelegramTypingHeartbeatManager {
    api: Arc<TelegramApi>,
    tasks: Arc<Mutex<HashMap<TypingKey, Arc<CancellationToken>>>>,
}

impl TelegramTypingHeartbeatManager {
    pub fn new(api: Arc<TelegramApi>) -> Self {
        Self {
            api,
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start_run(&self, session_id: String, run_id: String, chat_id: i64) {
        let key = TypingKey { session_id, run_id };
        let cancel = Arc::new(CancellationToken::new());
        let old = self
            .tasks
            .lock()
            .await
            .insert(key.clone(), Arc::clone(&cancel));
        if let Some(old) = old {
            old.cancel();
        }

        let api = Arc::clone(&self.api);
        let tasks = Arc::clone(&self.tasks);
        tokio::spawn(async move {
            loop {
                if cancel.is_cancelled() {
                    break;
                }
                if let Err(err) = api.send_chat_action(chat_id, "typing").await {
                    log::debug!(
                        "[telegram-typing] sendChatAction failed session={} run={} error={:?}",
                        key.session_id,
                        key.run_id,
                        err
                    );
                }
                tokio::select! {
                    _ = tokio::time::sleep(TYPING_HEARTBEAT_INTERVAL) => {}
                    _ = cancel.cancelled() => break,
                }
            }
            let mut guard = tasks.lock().await;
            if guard
                .get(&key)
                .map(|current| Arc::ptr_eq(current, &cancel))
                .unwrap_or(false)
            {
                guard.remove(&key);
            }
        });
    }

    pub async fn stop_run(&self, session_id: &str, run_id: &str) {
        let key = TypingKey {
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
        };
        if let Some(cancel) = self.tasks.lock().await.remove(&key) {
            cancel.cancel();
        }
    }

    pub async fn stop_session(&self, session_id: &str) {
        let cancels = {
            let mut guard = self.tasks.lock().await;
            let keys = guard
                .keys()
                .filter(|key| key.session_id == session_id)
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| guard.remove(&key))
                .collect::<Vec<_>>()
        };
        for cancel in cancels {
            cancel.cancel();
        }
    }

    pub async fn stop_all(&self) {
        let cancels = self
            .tasks
            .lock()
            .await
            .drain()
            .map(|(_, c)| c)
            .collect::<Vec<_>>();
        for cancel in cancels {
            cancel.cancel();
        }
    }
}

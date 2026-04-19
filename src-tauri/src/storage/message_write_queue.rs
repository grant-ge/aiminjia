use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;

use anyhow::{anyhow, Result};

use super::file_store::AppStorage;

const MESSAGE_WRITE_QUEUE_CAPACITY: usize = 128;

pub trait MessageWriteTarget: Send + Sync + 'static {
    fn insert_message(
        &self,
        id: &str,
        conversation_id: &str,
        role: &str,
        content_json: &str,
    ) -> Result<()>;

    fn update_message_content(
        &self,
        id: &str,
        conversation_id: &str,
        content_json: &str,
    ) -> Result<()>;
}

impl MessageWriteTarget for AppStorage {
    fn insert_message(
        &self,
        id: &str,
        conversation_id: &str,
        role: &str,
        content_json: &str,
    ) -> Result<()> {
        AppStorage::insert_message(self, id, conversation_id, role, content_json)
    }

    fn update_message_content(
        &self,
        id: &str,
        conversation_id: &str,
        content_json: &str,
    ) -> Result<()> {
        AppStorage::update_message_content(self, id, conversation_id, content_json)
    }
}

enum MessageWriteJob {
    Insert {
        id: String,
        conversation_id: String,
        role: String,
        content_json: String,
        reply: Option<mpsc::Sender<std::result::Result<(), String>>>,
    },
    Update {
        id: String,
        conversation_id: String,
        content_json: String,
        reply: Option<mpsc::Sender<std::result::Result<(), String>>>,
    },
    Flush {
        reply: mpsc::Sender<()>,
    },
}

impl MessageWriteJob {
    fn describe(&self) -> &'static str {
        match self {
            Self::Insert { .. } => "insert",
            Self::Update { .. } => "update",
            Self::Flush { .. } => "flush",
        }
    }
}

#[derive(Clone)]
pub struct MessageWriteQueue {
    sender: SyncSender<MessageWriteJob>,
}

pub struct MessageWriteCompletion {
    receiver: Receiver<std::result::Result<(), String>>,
}

impl MessageWriteCompletion {
    pub fn wait(self) -> Result<()> {
        match self
            .receiver
            .recv()
            .map_err(|_| anyhow!("message write worker dropped completion acknowledgement"))?
        {
            Ok(()) => Ok(()),
            Err(message) => Err(anyhow!(message)),
        }
    }
}

impl MessageWriteQueue {
    pub fn new(target: Arc<dyn MessageWriteTarget>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(MESSAGE_WRITE_QUEUE_CAPACITY);
        spawn_worker(target, receiver);
        Self { sender }
    }

    pub fn enqueue_insert(
        &self,
        id: String,
        conversation_id: String,
        role: String,
        content_json: String,
    ) -> Result<()> {
        self.try_send(MessageWriteJob::Insert {
            id,
            conversation_id,
            role,
            content_json,
            reply: None,
        })
    }

    pub fn enqueue_insert_with_ack(
        &self,
        id: String,
        conversation_id: String,
        role: String,
        content_json: String,
    ) -> Result<MessageWriteCompletion> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.try_send(MessageWriteJob::Insert {
            id,
            conversation_id,
            role,
            content_json,
            reply: Some(reply_tx),
        })?;
        Ok(MessageWriteCompletion { receiver: reply_rx })
    }

    pub fn enqueue_update(
        &self,
        id: String,
        conversation_id: String,
        content_json: String,
    ) -> Result<()> {
        self.try_send(MessageWriteJob::Update {
            id,
            conversation_id,
            content_json,
            reply: None,
        })
    }

    pub fn flush(&self) -> Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(MessageWriteJob::Flush { reply: reply_tx })
            .map_err(|_| anyhow!("message write worker is unavailable during flush"))?;
        reply_rx
            .recv()
            .map_err(|_| anyhow!("message write worker dropped flush acknowledgement"))?;
        Ok(())
    }

    fn try_send(&self, job: MessageWriteJob) -> Result<()> {
        match self.sender.try_send(job) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(job)) => Err(anyhow!(
                "message write queue is full while scheduling {}",
                job.describe()
            )),
            Err(TrySendError::Disconnected(job)) => Err(anyhow!(
                "message write queue is unavailable while scheduling {}",
                job.describe()
            )),
        }
    }
}

fn spawn_worker(target: Arc<dyn MessageWriteTarget>, receiver: Receiver<MessageWriteJob>) {
    thread::Builder::new()
        .name("message-write-worker".to_string())
        .spawn(move || run_worker(target, receiver))
        .expect("message write worker should start");
}

fn run_worker(target: Arc<dyn MessageWriteTarget>, receiver: Receiver<MessageWriteJob>) {
    while let Ok(job) = receiver.recv() {
        match job {
            MessageWriteJob::Insert {
                id,
                conversation_id,
                role,
                content_json,
                reply,
            } => {
                let result =
                    target.insert_message(&id, &conversation_id, &role, &content_json)
                .map_err(|err| {
                    log::error!(
                        "[message_write_queue] failed to persist assistant insert id={} conv={}: {}",
                        id,
                        conversation_id,
                        err
                    );
                    err.to_string()
                });
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            MessageWriteJob::Update {
                id,
                conversation_id,
                content_json,
                reply,
            } => {
                let result = target
                    .update_message_content(&id, &conversation_id, &content_json)
                    .map_err(|err| {
                    log::error!(
                        "[message_write_queue] failed to persist assistant update id={} conv={}: {}",
                        id,
                        conversation_id,
                        err
                    );
                        err.to_string()
                    });
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            MessageWriteJob::Flush { reply } => {
                let _ = reply.send(());
            }
        }
    }
}

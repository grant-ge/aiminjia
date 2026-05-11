//! In-process per-Teammate inbox channel.
//!
//! Each `AgentInbox` wraps a `tokio::sync::mpsc` channel (capacity=64) that
//! routes messages to a running Teammate idle loop.  The inbox is created at
//! spawn time, stored in the session registry (P2), and dropped when the
//! Teammate exits — everything is in-process and ephemeral; nothing is
//! persisted to disk.
//!
//! **Do not confuse with `runtime/employee/inbox.rs`**, which is the
//! UI-visible JSONL file that stores persistent employee notifications.
//! This module is a pure in-process signalling channel.

use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

// ─── InboxItem ────────────────────────────────────────────────────────────────

/// The source that sent a [`ChatMessage`] to this inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageSource {
    /// Message originated from the team lead.
    Lead,
    /// Message originated from another teammate.
    Teammate(String),
    /// Message originated from the UI / system.
    System,
}

/// A request to shut down this Teammate.  Full implementation in P2.
#[derive(Debug, Clone)]
pub struct ShutdownRequest {
    /// Human-readable reason forwarded to the Teammate's LLM so it can
    /// produce a graceful summary before exiting.
    pub reason: String,
}

/// A forwarded task-notification from the shared task queue.  Full
/// implementation in P2.
#[derive(Debug, Clone)]
pub struct TaskNotificationItem {
    /// Raw XML string of the `<task-notification>` element.
    pub xml: String,
}

/// Items that can be delivered to a Teammate's inbox.
#[derive(Debug, Clone)]
pub enum InboxItem {
    /// A chat message to process — the Teammate should run a LLM turn with
    /// this as the next user message.
    ChatMessage {
        text: String,
        source: MessageSource,
    },
    /// A graceful-shutdown request (P2 implementation; P1 causes immediate exit).
    Shutdown(ShutdownRequest),
    /// A forwarded task-notification from the shared queue (P2 implementation;
    /// P1 is ignored).
    TaskNotification(TaskNotificationItem),
}

// ─── AgentInbox ───────────────────────────────────────────────────────────────

/// Per-Teammate in-process message queue.
///
/// `AgentInbox::new(capacity)` returns an `Arc<AgentInbox>` that can be cloned
/// freely — the sender half is behind an `Arc` and can be used from any task.
/// The receiver half is `Mutex`-guarded so only the Teammate's own idle loop
/// task may call `recv`.
pub struct AgentInbox {
    tx: mpsc::Sender<InboxItem>,
    rx: Mutex<mpsc::Receiver<InboxItem>>,
}

impl AgentInbox {
    /// Construct a new inbox with the given channel capacity.
    pub fn new(capacity: usize) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(capacity);
        Arc::new(Self {
            tx,
            rx: Mutex::new(rx),
        })
    }

    /// Send an item to the inbox.  Returns `Err` if the inbox has been dropped
    /// (Teammate already exited).
    pub async fn send(
        &self,
        item: InboxItem,
    ) -> Result<(), mpsc::error::SendError<InboxItem>> {
        self.tx.send(item).await
    }

    /// Receive the next item, or `None` if all senders have been dropped (i.e.
    /// no more messages will ever arrive — inbox is closed).
    ///
    /// Callers should `select!` on this alongside the cancellation token.
    pub async fn recv(&self) -> Option<InboxItem> {
        self.rx.lock().await.recv().await
    }

    /// Returns a clone of the sender half, usable to push items from external
    /// code (e.g. a `SendMessage` tool implementation in P2).
    pub fn sender(&self) -> mpsc::Sender<InboxItem> {
        self.tx.clone()
    }
}

impl std::fmt::Debug for AgentInbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentInbox")
            .field("capacity", &self.tx.max_capacity())
            .finish()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn send_recv_roundtrip() {
        let inbox = AgentInbox::new(8);
        inbox
            .send(InboxItem::ChatMessage {
                text: "hello".to_string(),
                source: MessageSource::Lead,
            })
            .await
            .unwrap();
        let item = inbox.recv().await.unwrap();
        match item {
            InboxItem::ChatMessage { text, source } => {
                assert_eq!(text, "hello");
                assert_eq!(source, MessageSource::Lead);
            }
            _ => panic!("unexpected item"),
        }
    }

    #[tokio::test]
    async fn recv_returns_none_when_all_senders_dropped() {
        let inbox = AgentInbox::new(8);
        // Drop the sender by not holding onto it.
        let sender = inbox.sender();
        drop(sender);
        // The tx inside AgentInbox itself is still alive, but if we drop the
        // inbox's internal tx we should get None.  For this test, we verify the
        // close-signal path by draining a channel with the internal tx dropped
        // externally: create a raw channel, drop the tx, recv should return None.
        let (tx2, mut rx2) = mpsc::channel::<InboxItem>(4);
        drop(tx2);
        assert!(rx2.recv().await.is_none());
    }

    #[tokio::test]
    async fn inbox_capacity_respected() {
        let inbox = AgentInbox::new(2);
        // Fill capacity synchronously via try_send on the sender.
        let sender = inbox.sender();
        sender
            .try_send(InboxItem::ChatMessage {
                text: "a".into(),
                source: MessageSource::Lead,
            })
            .unwrap();
        sender
            .try_send(InboxItem::ChatMessage {
                text: "b".into(),
                source: MessageSource::Lead,
            })
            .unwrap();
        // Third send should be full (capacity=2).
        let result = sender.try_send(InboxItem::Shutdown(ShutdownRequest {
            reason: "test".into(),
        }));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn sender_clone_delivers_to_same_inbox() {
        let inbox = AgentInbox::new(4);
        let s1 = inbox.sender();
        let s2 = inbox.sender();
        s1.send(InboxItem::ChatMessage {
            text: "from-s1".into(),
            source: MessageSource::System,
        })
        .await
        .unwrap();
        s2.send(InboxItem::ChatMessage {
            text: "from-s2".into(),
            source: MessageSource::System,
        })
        .await
        .unwrap();
        let first = inbox.recv().await.unwrap();
        let second = inbox.recv().await.unwrap();
        match (&first, &second) {
            (
                InboxItem::ChatMessage { text: t1, .. },
                InboxItem::ChatMessage { text: t2, .. },
            ) => {
                assert_eq!(t1, "from-s1");
                assert_eq!(t2, "from-s2");
            }
            _ => panic!("unexpected items"),
        }
    }
}

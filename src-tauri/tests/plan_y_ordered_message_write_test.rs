use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use app_lib::storage::message_write_queue::{MessageWriteQueue, MessageWriteTarget};

struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    fn new() -> Self {
        Self {
            open: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.cv.wait(open).unwrap();
        }
    }

    fn open(&self) {
        let mut open = self.open.lock().unwrap();
        *open = true;
        self.cv.notify_all();
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

struct Signal {
    fired: Mutex<bool>,
    cv: Condvar,
}

impl Signal {
    fn new() -> Self {
        Self {
            fired: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    fn fire(&self) {
        let mut fired = self.fired.lock().unwrap();
        *fired = true;
        self.cv.notify_all();
    }

    fn wait(&self) {
        let mut fired = self.fired.lock().unwrap();
        while !*fired {
            fired = self.cv.wait(fired).unwrap();
        }
    }
}

impl Default for Signal {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct RecordingTarget {
    operations: Mutex<Vec<String>>,
    block_first_insert: AtomicBool,
    fail_first_insert: AtomicBool,
    first_insert_started: Signal,
    release_first_insert: Gate,
}

impl RecordingTarget {
    fn with_blocked_first_insert() -> Self {
        Self {
            block_first_insert: AtomicBool::new(true),
            ..Default::default()
        }
    }

    fn with_failed_first_insert() -> Self {
        Self {
            fail_first_insert: AtomicBool::new(true),
            ..Default::default()
        }
    }

    fn operations(&self) -> Vec<String> {
        self.operations.lock().unwrap().clone()
    }
}

impl MessageWriteTarget for RecordingTarget {
    fn insert_message(
        &self,
        id: &str,
        conversation_id: &str,
        role: &str,
        content_json: &str,
    ) -> Result<()> {
        if self.block_first_insert.swap(false, Ordering::SeqCst) {
            self.first_insert_started.fire();
            self.release_first_insert.wait();
        }

        if self.fail_first_insert.swap(false, Ordering::SeqCst) {
            self.operations.lock().unwrap().push(format!(
                "insert_failed:{id}:{conversation_id}:{role}:{content_json}"
            ));
            return Err(anyhow!("synthetic insert failure"));
        }

        self.operations.lock().unwrap().push(format!(
            "insert:{id}:{conversation_id}:{role}:{content_json}"
        ));
        Ok(())
    }

    fn update_message_content(
        &self,
        id: &str,
        conversation_id: &str,
        content_json: &str,
    ) -> Result<()> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("update:{id}:{conversation_id}:{content_json}"));
        Ok(())
    }
}

#[test]
fn plan_y_y1_queue_returns_before_blocked_write_finishes_and_preserves_order() {
    let target = Arc::new(RecordingTarget::with_blocked_first_insert());
    let queue = MessageWriteQueue::new(target.clone());

    queue
        .enqueue_insert(
            "assistant-1".to_string(),
            "conv-y1".to_string(),
            "assistant".to_string(),
            r#"{"text":"first"}"#.to_string(),
        )
        .expect("first enqueue should succeed");

    target.first_insert_started.wait();

    let start = std::time::Instant::now();
    queue
        .enqueue_update(
            "assistant-1".to_string(),
            "conv-y1".to_string(),
            r#"{"text":"second"}"#.to_string(),
        )
        .expect("second enqueue should succeed");
    assert!(
        start.elapsed() < Duration::from_millis(50),
        "second enqueue should return before the blocked write finishes"
    );

    assert!(
        target.operations().is_empty(),
        "worker must stay blocked until the test explicitly releases it"
    );

    target.release_first_insert.open();
    queue.flush().expect("flush should wait for queued writes");

    assert_eq!(
        target.operations(),
        vec![
            r#"insert:assistant-1:conv-y1:assistant:{"text":"first"}"#.to_string(),
            r#"update:assistant-1:conv-y1:{"text":"second"}"#.to_string(),
        ],
    );
}

#[test]
fn plan_y_y1_queue_logs_fail_open_and_keeps_processing_later_jobs() {
    let target = Arc::new(RecordingTarget::with_failed_first_insert());
    let queue = MessageWriteQueue::new(target.clone());

    queue
        .enqueue_insert(
            "assistant-fail".to_string(),
            "conv-y1".to_string(),
            "assistant".to_string(),
            r#"{"text":"boom"}"#.to_string(),
        )
        .expect("failed write should still enqueue successfully");

    queue
        .enqueue_insert(
            "assistant-ok".to_string(),
            "conv-y1".to_string(),
            "assistant".to_string(),
            r#"{"text":"after"}"#.to_string(),
        )
        .expect("second write should enqueue successfully");

    queue.flush().expect("flush should observe later jobs too");

    assert_eq!(
        target.operations(),
        vec![
            r#"insert_failed:assistant-fail:conv-y1:assistant:{"text":"boom"}"#.to_string(),
            r#"insert:assistant-ok:conv-y1:assistant:{"text":"after"}"#.to_string(),
        ],
    );
}

#[test]
fn plan_y_y1_queue_ack_reports_worker_failures_when_caller_needs_sync_confirmation() {
    let target = Arc::new(RecordingTarget::with_failed_first_insert());
    let queue = MessageWriteQueue::new(target);

    let completion = queue
        .enqueue_insert_with_ack(
            "assistant-sync".to_string(),
            "conv-y1".to_string(),
            "assistant".to_string(),
            r#"{"text":"sync"}"#.to_string(),
        )
        .expect("enqueue with ack should succeed");

    let err = completion
        .wait()
        .expect_err("ack should surface worker persistence failure");

    assert!(
        err.to_string().contains("synthetic insert failure"),
        "completion should preserve the worker error text"
    );
}

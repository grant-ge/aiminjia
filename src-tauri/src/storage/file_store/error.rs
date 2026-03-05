//! Storage error types for file-based storage.

use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Conversation locked by PID {pid}: {conversation_id}")]
    ConversationLocked {
        conversation_id: String,
        pid: u32,
    },

    #[error("Corrupted file: {path} — {reason}")]
    Corrupted {
        path: String,
        reason: String,
    },

    #[error("Invalid structure: {0}")]
    InvalidStructure(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

impl StorageError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn corrupted(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Corrupted {
            path: path.into(),
            reason: reason.into(),
        }
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidStructure(msg.into())
    }
}

// Note: StorageError implements std::error::Error (via thiserror),
// so anyhow's blanket `impl<E: Error> From<E> for anyhow::Error`
// handles conversion automatically. No manual impl needed.

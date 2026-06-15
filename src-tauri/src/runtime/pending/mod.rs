//! Pending message queue: per-session queue with debounced drain.
//!
//! See `docs/superpowers/specs/2026-05-11-pending-message-queue-design.md`.

pub mod aijia_resolver;
pub mod queue_manager;
pub mod store;
pub mod types;

pub use aijia_resolver::{AiJiaPendingResolver, CurrentUserPendingResolver};

pub use types::{
    EnqueueOutcome, EnqueueRejection, PendingAttachment, PendingConfig, PendingFileFormat,
    PendingItem, PendingSource,
};

pub use queue_manager::{ChatTurnDispatcher, ConvDirResolver, PendingQueueManager};

#[cfg(test)]
pub use queue_manager::build_request_from_batch_for_test;

#[cfg(test)]
mod types_test;

#[cfg(test)]
mod store_test;

#[cfg(test)]
mod queue_manager_test;

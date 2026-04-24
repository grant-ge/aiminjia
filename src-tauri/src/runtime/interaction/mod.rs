//! Interaction Runtime — first-class abstraction for tools that require user input.
//!
//! Distinct from the permission pipeline (PermissionDecision::Ask).

pub mod control_plane;
pub mod types;

pub use control_plane::{InMemoryInteractionControlPlane, PendingInteractionControlPlane};
pub use types::{InteractionId, InteractionKind, InteractionRequest, InteractionResolution};

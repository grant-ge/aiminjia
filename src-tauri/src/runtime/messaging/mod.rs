//! LTR messaging primitives — payloads, routing helpers, serde shapes.
//!
//! `StructuredMessage` is the discriminated union shipped through SendMessage
//! and stored on InboxItem.  Future protocol envelopes (e.g. routing
//! envelopes for broadcast) live alongside it.

pub mod structured;

pub use structured::StructuredMessage;

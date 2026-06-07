// Transport adapter modules.
// chat is the active transport entry point.
// Other adapters are structurally complete but generate_handler! still
// points to commands::*. When ready, switch generate_handler! to these.

pub mod agenda;
pub mod agents;
pub mod auth;
pub mod billing;
pub mod chat;
pub mod conversation_export;
pub mod file;
pub mod logging;
pub mod mcp;
pub mod network;
pub mod pending;
pub mod persona;
pub mod runtime;
pub mod settings;
pub mod turn_stage;
pub mod workspace;

// Transport adapter modules.
// chat is the active transport entry point.
// Other adapters are structurally complete but generate_handler! still
// points to commands::*. When ready, switch generate_handler! to these.

pub mod agenda;
pub mod agents;
pub mod auth;
pub mod chat;
pub mod file;
pub mod mcp;
pub mod persona;
pub mod runtime;
pub mod settings;
pub mod workspace;

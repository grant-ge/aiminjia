//! Python session management stub.
//!
//! This stub preserves the PythonSessionManager type for the compilation boundary while
//! providing no-op implementations. Callers that reference session_manager fields will
//! continue to compile; the manager simply does nothing.
//!
//! TODO: Remove remaining session_manager references from non-Python code paths
//! (transport/tauri_commands/chat.rs, runtime/conversation_service.rs, plugin/registry.rs,
//! llm/sub_agent.rs) in a follow-up cleanup pass, then delete this module entirely.

pub mod session;

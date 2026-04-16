//! First-class RuntimeTool implementations.
//! These tools do NOT use PluginContext — they use ToolExecutionContext + CapabilityContext.
//! network.rs and browser.rs use narrow Deps structs injected at construction time.
//! file.rs uses FileOperations via CapabilityContext.file_ops (no PluginContext bridge).
pub mod workspace;
pub mod network;
pub mod browser;
pub mod file;

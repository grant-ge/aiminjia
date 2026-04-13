//! First-class RuntimeTool implementations.
//! These tools do NOT use PluginContext — they use ToolExecutionContext + CapabilityContext.
//! network.rs and browser.rs use narrow Deps structs injected at construction time.
//! file.rs uses a TRANSITIONAL PluginContext bridge pending handle_load_file refactor.
pub mod workspace;
pub mod network;
pub mod browser;
pub mod file;

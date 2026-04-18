//! First-class RuntimeTool implementations.
//! These tools do NOT use PluginContext — they use ToolExecutionContext + CapabilityContext.
//! network.rs and browser.rs use narrow Deps structs injected at construction time.
//! file.rs uses FileOperations via CapabilityContext.file_ops (no PluginContext bridge).
pub mod bash;
pub mod browse_data;
pub mod grep;
pub mod workspace;
pub mod network;
pub mod browser;
pub mod file;
pub mod python;
pub mod python_execution;
pub mod report;
pub mod report_capability;
pub mod chart;
pub mod chart_capability;

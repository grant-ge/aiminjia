//! First-class RuntimeTool implementations.
//! These tools do NOT use PluginContext — they use ToolExecutionContext + CapabilityContext.
//! network.rs and browser.rs use narrow Deps structs injected at construction time.
//! file.rs uses FileOperations via CapabilityContext.file_ops (no PluginContext bridge).
pub mod ask_user_question;
#[cfg(not(windows))]
pub mod bash;
pub mod browse_data;
pub mod browser;
pub mod chart;
pub mod chart_capability;
pub mod file;
pub mod grep;
pub mod load_skill;
pub mod memory;
pub mod network;
#[cfg(windows)]
pub mod powershell;
#[cfg(windows)]
pub mod powershell_detect;
pub mod python;
pub mod python_execution;
pub mod report;
pub mod report_capability;
pub mod shell_common;
pub mod task_tools;
pub mod workspace;

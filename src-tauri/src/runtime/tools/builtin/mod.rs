//! First-class RuntimeTool implementations.
//! These tools do NOT use PluginContext — they use ToolExecutionContext + CapabilityContext.
//! network.rs uses narrow Deps structs injected at construction time.
pub mod ask_user_question;
#[cfg(not(windows))]
pub mod bash;
pub mod grep;
pub mod load_skill;
pub mod memory;
pub mod network;
#[cfg(windows)]
pub mod powershell;
#[cfg(windows)]
pub mod powershell_detect;
pub mod shell_common;
pub mod spawn_subagent;
pub mod task_tools;
pub mod task_output;
pub mod workspace;

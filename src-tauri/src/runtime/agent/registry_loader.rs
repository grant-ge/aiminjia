//! Build an `AgentRegistry` by merging three sources with precedence:
//!
//! ```
//! builtin < user_dir < project_dir
//! ```
//!
//! Same-name agents from a later source overwrite earlier ones (`HashMap::insert`).
//! Malformed `.md` files are logged with `log::warn!` and skipped — they do not
//! abort registry construction.

use std::path::Path;

use log::warn;

use crate::runtime::agent::markdown_loader::load_agent_from_markdown;
use crate::runtime::agent::registry::AgentRegistry;

/// Load registry: builtin defaults, then merge user dir, then project dir.
///
/// `user_dir` and `project_dir` may be `None` (no merge from that source).
/// Both directories are scanned for `*.md` files (non-recursive). Other
/// extensions and parse errors are silently skipped (with a warning log).
pub fn load_registry_with_user_dir(
    user_dir: Option<&Path>,
    project_dir: Option<&Path>,
) -> AgentRegistry {
    let reg = AgentRegistry::with_builtins();
    if let Some(dir) = user_dir {
        merge_dir(&reg, dir, "user");
    }
    if let Some(dir) = project_dir {
        merge_dir(&reg, dir, "project");
    }
    reg
}

fn merge_dir(reg: &AgentRegistry, dir: &Path, source_label: &str) {
    if !dir.is_dir() {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            warn!(
                "agent dir read failed [{source_label}] {}: {err}",
                dir.display()
            );
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        match load_agent_from_markdown(&path) {
            Ok(def) => reg.register(def),
            Err(err) => warn!(
                "agent md parse failed [{source_label}] {}: {err}",
                path.display()
            ),
        }
    }
}

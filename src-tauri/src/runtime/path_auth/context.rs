use std::collections::HashMap;
use std::path::PathBuf;

use crate::runtime::path_auth::op::PathOp;
use crate::runtime::tools::permission::PermissionMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSource {
    Session,
    UserSettings,
}

#[derive(Debug, Clone)]
pub struct PermissionRule {
    pub pattern: String,
    pub op: Option<PathOp>,
    pub source: RuleSource,
}

#[derive(Debug, Clone)]
pub struct ToolPermissionContext {
    pub mode: PermissionMode,
    pub primary_root: Option<PathBuf>,
    pub additional_working_dirs: HashMap<PathBuf, RuleSource>,
    pub allow_rules: Vec<PermissionRule>,
    pub deny_rules: Vec<PermissionRule>,
}

impl ToolPermissionContext {
    pub fn empty() -> Self {
        Self {
            mode: PermissionMode::Default,
            primary_root: None,
            additional_working_dirs: HashMap::new(),
            allow_rules: Vec::new(),
            deny_rules: Vec::new(),
        }
    }
}

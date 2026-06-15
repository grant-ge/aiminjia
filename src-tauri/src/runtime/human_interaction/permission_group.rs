use std::path::{Path, PathBuf};

use crate::runtime::ids::{RunId, SessionId, ToolCallId};

use super::PermissionDecisionIntent;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PermissionGroupKey {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub scope_key: String,
}

impl PermissionGroupKey {
    pub fn read_path(session_id: SessionId, run_id: RunId, path: impl AsRef<str>) -> Self {
        let path = normalize_path(path.as_ref());
        let scope_key = parent_dir(&path).unwrap_or(path);
        Self {
            session_id,
            run_id,
            scope_key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionGroupItem {
    pub tool_call_id: ToolCallId,
    pub requested_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionGroup {
    key: PermissionGroupKey,
    items: Vec<PermissionGroupItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionGroupResolution {
    ResolveAll,
    ResolveOne(ToolCallId),
    NeedClarification { message: String },
}

impl PermissionGroup {
    pub fn new(key: PermissionGroupKey) -> Self {
        Self {
            key,
            items: Vec::new(),
        }
    }

    pub fn key(&self) -> &PermissionGroupKey {
        &self.key
    }

    pub fn items(&self) -> &[PermissionGroupItem] {
        &self.items
    }

    pub fn push_request(&mut self, tool_call_id: ToolCallId, requested_path: impl AsRef<str>) {
        if self
            .items
            .iter()
            .any(|item| item.tool_call_id == tool_call_id)
        {
            return;
        }
        self.items.push(PermissionGroupItem {
            tool_call_id,
            requested_path: normalize_path(requested_path.as_ref()),
        });
    }

    pub fn coverage_scope(&self) -> Option<String> {
        let mut dirs = self
            .items
            .iter()
            .filter_map(|item| parent_dir(&item.requested_path));
        let first = dirs.next()?;
        if dirs.all(|dir| dir == first) {
            Some(first)
        } else {
            None
        }
    }

    pub fn resolve(&self, intent: PermissionDecisionIntent) -> PermissionGroupResolution {
        match intent {
            PermissionDecisionIntent::AllowOnce
            | PermissionDecisionIntent::Deny { .. }
            | PermissionDecisionIntent::Cancel { .. } => PermissionGroupResolution::ResolveAll,
            PermissionDecisionIntent::AllowAlways { scope } => {
                let Some(scope) = scope.or_else(|| self.coverage_scope()) else {
                    return PermissionGroupResolution::NeedClarification {
                        message: clarification_message(),
                    };
                };
                let scope = normalize_path(&scope);
                if self
                    .items
                    .iter()
                    .all(|item| path_contains(&scope, &item.requested_path))
                {
                    PermissionGroupResolution::ResolveAll
                } else {
                    PermissionGroupResolution::NeedClarification {
                        message: clarification_message(),
                    }
                }
            }
        }
    }
}

fn clarification_message() -> String {
    "授权范围没有覆盖全部待审批请求，请选择仅本次、拒绝，或说明包含全部文件的目录范围。".into()
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches('/');
    if trimmed == "/tmp" {
        return "/private/tmp".into();
    }
    trimmed
        .strip_prefix("/tmp/")
        .map(|rest| format!("/private/tmp/{rest}"))
        .unwrap_or_else(|| trimmed.to_string())
}

fn parent_dir(path: &str) -> Option<String> {
    Path::new(path)
        .parent()
        .map(PathBuf::from)
        .map(|path| path.to_string_lossy().trim_end_matches('/').to_string())
}

fn path_contains(scope: &str, path: &str) -> bool {
    let scope = scope.trim_end_matches('/');
    path == scope || path.starts_with(&format!("{scope}/"))
}

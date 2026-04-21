use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;

/// The three-state outcome of a permission check.
#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow {
        updated_input: Option<serde_json::Value>,
        reason: PermissionReason,
    },
    Deny {
        message: String,
        reason: PermissionReason,
    },
    Ask {
        message: String,
        suggestions: Vec<String>,
        remember_options: Vec<PermissionDestination>,
        default_destination: Option<PermissionDestination>,
        reason: PermissionReason,
    },
}

impl std::fmt::Display for PermissionDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PermissionDecision::Allow { .. } => write!(f, "allowed"),
            PermissionDecision::Deny { message, .. } => write!(f, "denied: {}", message),
            PermissionDecision::Ask { message, .. } => write!(f, "ask required: {}", message),
        }
    }
}

pub fn default_permission_ask() -> (Vec<PermissionDestination>, Option<PermissionDestination>) {
    (
        default_remember_options(),
        Some(PermissionDestination::Session),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    #[default]
    Default,
    Plan,
    DontAsk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionDestination {
    Session,
    Workspace,
    User,
}

/// Why the permission decision was made.
#[derive(Debug, Clone)]
pub enum PermissionReason {
    StoredPolicy,
    Capability,
    UnknownScope,
    Mode(String),
    Other(String),
}

pub trait PermissionPipeline: Send + Sync {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> PermissionDecision;
}

pub fn apply_permission_mode(
    decision: PermissionDecision,
    tool_name: &str,
    mode: PermissionMode,
) -> PermissionDecision {
    match (mode, decision) {
        (PermissionMode::DontAsk, PermissionDecision::Ask { .. }) => PermissionDecision::Deny {
            message: format!(
                "Tool '{}' requires permission, but current mode is dontAsk.",
                tool_name
            ),
            reason: PermissionReason::Mode("dontAsk".into()),
        },
        (PermissionMode::Plan, PermissionDecision::Ask { .. }) => PermissionDecision::Deny {
            message: format!(
                "Tool '{}' requires permission, but current mode is plan (read-only planning phase).",
                tool_name
            ),
            reason: PermissionReason::Mode("plan".into()),
        },
        (_, decision) => decision,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeCapabilityFailure {
    MissingWorkspace,
    MissingBrowser,
    UnknownScope,
}

fn check_scope_capability(
    scope: &str,
    ctx: &ToolExecutionContext,
) -> Option<ScopeCapabilityFailure> {
    match scope {
        "workspace:read" | "workspace:write" | "python:exec" => {
            if ctx
                .capability
                .as_ref()
                .and_then(|c| c.storage.as_ref())
                .is_none()
            {
                Some(ScopeCapabilityFailure::MissingWorkspace)
            } else {
                None
            }
        }
        "browser" => {
            let has_browser = ctx
                .capability
                .as_ref()
                .map(|c| c.has_browser_capability())
                .unwrap_or(false);
            if has_browser {
                None
            } else {
                Some(ScopeCapabilityFailure::MissingBrowser)
            }
        }
        "network" => None,
        _ => Some(ScopeCapabilityFailure::UnknownScope),
    }
}

#[derive(Clone, Default)]
pub struct AllowAllPermissionPipeline;

impl PermissionPipeline for AllowAllPermissionPipeline {
    fn authorize(
        &self,
        _definition: &ToolDefinition,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> PermissionDecision {
        PermissionDecision::Allow {
            updated_input: None,
            reason: PermissionReason::Other("allow_all".into()),
        }
    }
}

/// 基于 capability_scope 的权限管线。
///
/// 规则：
/// - 工具 `capability_scope` 为空 → 始终允许
/// - 含 `workspace:read` 或 `workspace:write` → 需要 `ctx.capability.storage` 存在
/// - 含 `browser` → 需要 `ctx.capability.has_browser_capability()` = true（目前默认 false）
/// - 含 `python:exec` → 需要 `ctx.capability.storage` 存在
/// - 含 `network` → 始终允许（网络访问不在本地 capability 层校验）
/// - 含 `mcp` → 视作 unknown scope（由上层 store / ask 流程决定）
/// - unknown scope → Deny（fail-closed）
#[derive(Clone, Default)]
pub struct CapabilityPermissionPipeline;

impl PermissionPipeline for CapabilityPermissionPipeline {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        _input: &Value,
        ctx: &ToolExecutionContext,
    ) -> PermissionDecision {
        if definition.capability_scope.is_empty() {
            return PermissionDecision::Allow {
                updated_input: None,
                reason: PermissionReason::Capability,
            };
        }
        for scope in &definition.capability_scope {
            match check_scope_capability(scope.as_str(), ctx) {
                Some(ScopeCapabilityFailure::MissingWorkspace)
                    if scope == "workspace:read" || scope == "workspace:write" =>
                {
                    return PermissionDecision::Deny {
                        message: format!(
                            "Tool '{}' requires workspace capability (scope: {}). \
                            Authorize a workspace directory first.",
                            definition.id, scope
                        ),
                        reason: PermissionReason::Capability,
                    };
                }
                Some(ScopeCapabilityFailure::MissingWorkspace) => {
                    return PermissionDecision::Deny {
                        message: format!(
                            "Tool '{}' requires a workspace context for Python execution.",
                            definition.id
                        ),
                        reason: PermissionReason::Capability,
                    };
                }
                Some(ScopeCapabilityFailure::MissingBrowser) => {
                    return PermissionDecision::Deny {
                        message: format!(
                            "Tool '{}' requires browser capability. \
                            A browser connector must be active.",
                            definition.id
                        ),
                        reason: PermissionReason::Capability,
                    };
                }
                Some(ScopeCapabilityFailure::UnknownScope) => {
                    log::debug!(
                        "Unknown capability scope '{}' for tool '{}' — denying.",
                        scope,
                        definition.id
                    );
                    return PermissionDecision::Deny {
                        message: format!(
                            "Tool '{}' requests unknown capability scope '{}'. Deny by default.",
                            definition.id, scope
                        ),
                        reason: PermissionReason::UnknownScope,
                    };
                }
                None => {}
            }
        }
        PermissionDecision::Allow {
            updated_input: None,
            reason: PermissionReason::Capability,
        }
    }
}

/// 基于 PermissionStore 的策略感知权限管线。
///
/// 决策优先级：
/// 1. 已持久化 AlwaysAllow / AlwaysDeny → 直接放行或拒绝，不再做 capability 检查
/// 2. 未记录决策 + unknown scope → Ask（请求用户确认）
/// 3. 未记录决策 + 已知 scope + 有 capability → Allow
/// 4. 未记录决策 + 已知 scope + 无 capability → Deny
///
/// **注意：** AlwaysAllow 绕过 capability 检查是设计意图，不是漏洞。
/// 用户显式持久化授权后，运行时 capability 存在性检查是多余的——
/// 若不绕过，持久化授权将对缺少 capability 的会话永远无效。
#[derive(Clone)]
pub struct StorePolicyPipeline {
    store: Arc<PermissionStore>,
}

impl StorePolicyPipeline {
    pub fn new(store: Arc<PermissionStore>) -> Self {
        Self { store }
    }
}

impl PermissionPipeline for StorePolicyPipeline {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        _input: &Value,
        ctx: &ToolExecutionContext,
    ) -> PermissionDecision {
        if definition.capability_scope.is_empty() {
            return PermissionDecision::Allow {
                updated_input: None,
                reason: PermissionReason::StoredPolicy,
            };
        }
        for scope in &definition.capability_scope {
            match self.store.get_for_scope(&definition.id, scope) {
                // An explicit Allow/AlwaysAllow decision supersedes capability checks.
                // The user has already granted this permission; re-checking capability
                // would defeat persistent grants for sessions without a live connector.
                Some(d) if d.is_allow() => continue,
                Some(_) => {
                    return PermissionDecision::Deny {
                        message: format!(
                            "Tool '{}' scope '{}' is denied by stored policy.",
                            definition.id, scope
                        ),
                        reason: PermissionReason::StoredPolicy,
                    };
                }
                None => {}
            }
            match check_scope_capability(scope.as_str(), ctx) {
                Some(ScopeCapabilityFailure::MissingWorkspace) => {
                    return PermissionDecision::Deny {
                        message: format!(
                            "Tool '{}' requires workspace capability (scope: {}).",
                            definition.id, scope
                        ),
                        reason: PermissionReason::Capability,
                    };
                }
                Some(ScopeCapabilityFailure::MissingBrowser) => {
                    return PermissionDecision::Deny {
                        message: format!("Tool '{}' requires browser capability.", definition.id),
                        reason: PermissionReason::Capability,
                    };
                }
                Some(ScopeCapabilityFailure::UnknownScope) => {
                    // Unknown scope with no stored policy → Ask the user.
                    // This is the key difference from CapabilityPermissionPipeline which
                    // uses fail-closed Deny. StorePolicyPipeline escalates to Ask so the
                    // user gets a chance to grant or deny persistently.
                    let message = if scope == "mcp" {
                        format!(
                            "Tool '{}' is an MCP tool and will call an external server. Do you want to allow it?",
                            definition.id
                        )
                    } else {
                        format!(
                            "Tool '{}' requests capability scope '{}' which is not recognized. \
                            Do you want to allow it?",
                            definition.id, scope
                        )
                    };
                    return PermissionDecision::Ask {
                        message,
                        suggestions: vec![
                            "Allow once".into(),
                            "Always allow".into(),
                            "Deny".into(),
                        ],
                        remember_options: default_remember_options(),
                        default_destination: Some(PermissionDestination::Session),
                        reason: PermissionReason::UnknownScope,
                    };
                }
                None => {}
            }
        }
        PermissionDecision::Allow {
            updated_input: None,
            reason: PermissionReason::StoredPolicy,
        }
    }
}

fn default_remember_options() -> Vec<PermissionDestination> {
    vec![
        PermissionDestination::Session,
        PermissionDestination::Workspace,
        PermissionDestination::User,
    ]
}

pub fn persist_permission_decision(
    store: &PermissionStore,
    tool_name: &str,
    scopes: &[String],
    decision: PolicyDecision,
    destination: PermissionDestination,
) {
    let source = match destination {
        PermissionDestination::Session => PermissionSource::Session,
        PermissionDestination::Workspace => PermissionSource::Workspace,
        PermissionDestination::User => PermissionSource::User,
    };

    for scope in scopes {
        store.record_to(
            destination,
            PermissionRule::simple(
                tool_name,
                PermissionScope::Scope(scope.clone()),
                decision.clone(),
                source,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::capability::CapabilityContext;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn ctx_without_capability() -> ToolExecutionContext {
        ToolExecutionContext::for_test("conv", "run", "tool-call")
    }

    fn ctx_with_workspace() -> ToolExecutionContext {
        let tmp = TempDir::new().expect("tempdir");
        let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
        ToolExecutionContext::for_test("conv", "run", "tool-call").with_capability(Arc::new(cap))
    }

    #[test]
    fn review_check_scope_capability_detects_workspace_requirement() {
        let missing_workspace = check_scope_capability("workspace:read", &ctx_without_capability());
        assert!(matches!(
            missing_workspace,
            Some(ScopeCapabilityFailure::MissingWorkspace)
        ));

        let missing_python_workspace =
            check_scope_capability("python:exec", &ctx_without_capability());
        assert!(matches!(
            missing_python_workspace,
            Some(ScopeCapabilityFailure::MissingWorkspace)
        ));

        let satisfied = check_scope_capability("workspace:write", &ctx_with_workspace());
        assert!(satisfied.is_none());
    }

    #[test]
    fn review_check_scope_capability_detects_browser_and_unknown_scopes() {
        let missing_browser = check_scope_capability("browser", &ctx_without_capability());
        assert!(matches!(
            missing_browser,
            Some(ScopeCapabilityFailure::MissingBrowser)
        ));

        let unknown = check_scope_capability("custom:unknown", &ctx_without_capability());
        assert!(matches!(
            unknown,
            Some(ScopeCapabilityFailure::UnknownScope)
        ));

        let network = check_scope_capability("network", &ctx_without_capability());
        assert!(network.is_none());
    }

}

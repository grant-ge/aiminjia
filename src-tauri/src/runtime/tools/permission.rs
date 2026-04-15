use serde_json::Value;
use std::sync::Arc;

use crate::runtime::store::permission_store::PermissionStore;
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
        reason: PermissionReason,
    },
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
            match scope.as_str() {
                "workspace:read" | "workspace:write" => {
                    if ctx
                        .capability
                        .as_ref()
                        .and_then(|c| c.storage.as_ref())
                        .is_none()
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
                }
                "browser" => {
                    let has_browser = ctx
                        .capability
                        .as_ref()
                        .map(|c| c.has_browser_capability())
                        .unwrap_or(false);
                    if !has_browser {
                        return PermissionDecision::Deny {
                            message: format!(
                                "Tool '{}' requires browser capability. \
                                A browser connector must be active.",
                                definition.id
                            ),
                            reason: PermissionReason::Capability,
                        };
                    }
                }
                "python:exec" => {
                    if ctx
                        .capability
                        .as_ref()
                        .and_then(|c| c.storage.as_ref())
                        .is_none()
                    {
                        return PermissionDecision::Deny {
                            message: format!(
                                "Tool '{}' requires a workspace context for Python execution.",
                                definition.id
                            ),
                            reason: PermissionReason::Capability,
                        };
                    }
                }
                "network" => {}
                other => {
                    log::debug!(
                        "Unknown capability scope '{}' for tool '{}' — denying.",
                        other,
                        definition.id
                    );
                    return PermissionDecision::Deny {
                        message: format!(
                            "Tool '{}' requests unknown capability scope '{}'. Deny by default.",
                            definition.id, other
                        ),
                        reason: PermissionReason::UnknownScope,
                    };
                }
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
            let key = format!("{}:{}", definition.id, scope);
            match self.store.get(&key) {
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
            match scope.as_str() {
                "workspace:read" | "workspace:write" | "python:exec" => {
                    if ctx
                        .capability
                        .as_ref()
                        .and_then(|c| c.storage.as_ref())
                        .is_none()
                    {
                        return PermissionDecision::Deny {
                            message: format!(
                                "Tool '{}' requires workspace capability (scope: {}).",
                                definition.id, scope
                            ),
                            reason: PermissionReason::Capability,
                        };
                    }
                }
                "browser" => {
                    let has = ctx
                        .capability
                        .as_ref()
                        .map(|c| c.has_browser_capability())
                        .unwrap_or(false);
                    if !has {
                        return PermissionDecision::Deny {
                            message: format!(
                                "Tool '{}' requires browser capability.",
                                definition.id
                            ),
                            reason: PermissionReason::Capability,
                        };
                    }
                }
                "network" => {}
                other => {
                    // Unknown scope with no stored policy → Ask the user.
                    // This is the key difference from CapabilityPermissionPipeline which
                    // uses fail-closed Deny. StorePolicyPipeline escalates to Ask so the
                    // user gets a chance to grant or deny persistently.
                    return PermissionDecision::Ask {
                        message: format!(
                            "Tool '{}' requests capability scope '{}' which is not recognized. \
                            Do you want to allow it?",
                            definition.id, other
                        ),
                        suggestions: vec![
                            "Allow once".into(),
                            "Always allow".into(),
                            "Deny".into(),
                        ],
                        reason: PermissionReason::UnknownScope,
                    };
                }
            }
        }
        PermissionDecision::Allow {
            updated_input: None,
            reason: PermissionReason::StoredPolicy,
        }
    }
}

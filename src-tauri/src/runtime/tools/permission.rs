use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

use crate::runtime::store::permission_store::{PermissionStore, PolicyDecision};
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;

pub trait PermissionPipeline: Send + Sync {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Result<()>;
}

#[derive(Clone, Default)]
pub struct AllowAllPermissionPipeline;

impl PermissionPipeline for AllowAllPermissionPipeline {
    fn authorize(
        &self,
        _definition: &ToolDefinition,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Result<()> {
        Ok(())
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
#[derive(Clone, Default)]
pub struct CapabilityPermissionPipeline;

impl PermissionPipeline for CapabilityPermissionPipeline {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        _input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Result<()> {
        if definition.capability_scope.is_empty() {
            return Ok(());
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
                        anyhow::bail!(
                            "Tool '{}' requires workspace capability (scope: {}). \
                            Authorize a workspace directory first.",
                            definition.id,
                            scope
                        );
                    }
                }
                "browser" => {
                    let has_browser = ctx
                        .capability
                        .as_ref()
                        .map(|c| c.has_browser_capability())
                        .unwrap_or(false);
                    if !has_browser {
                        anyhow::bail!(
                            "Tool '{}' requires browser capability. \
                            A browser connector must be active.",
                            definition.id
                        );
                    }
                }
                "python:exec" => {
                    if ctx
                        .capability
                        .as_ref()
                        .and_then(|c| c.storage.as_ref())
                        .is_none()
                    {
                        anyhow::bail!(
                            "Tool '{}' requires a workspace context for Python execution.",
                            definition.id
                        );
                    }
                }
                "network" => {}
                other => {
                    log::debug!(
                        "Unknown capability scope '{}' for tool '{}' — allowing.",
                        other,
                        definition.id
                    );
                }
            }
        }
        Ok(())
    }
}

/// 基于 PermissionStore 的策略感知权限管线。
///
/// 决策优先级：
/// 1. 已持久化 AlwaysAllow / AlwaysDeny → 直接放行或拒绝，不再做 capability 检查
/// 2. 未记录决策 → 执行与 CapabilityPermissionPipeline 相同的 capability 检查
/// 3. Unknown scope → fail-closed（不同于 CapabilityPermissionPipeline 的 fail-open）
///
/// **注意：** AlwaysAllow 绕过 capability 检查是设计意图，不是漏洞。
/// 用户显式持久化授权后，运行时 capability 存在性检查是多余的——
/// 若不绕过，持久化授权将对缺少 capability 的会话永远无效。
///
/// **当前状态：** 此管线已实现并通过测试，但尚未接入生产 dispatcher。
/// 生产路径（ToolRegistry::to_runtime_dispatcher 等）仍使用 CapabilityPermissionPipeline。
/// 接入生产 dispatcher 作为 P4 后续步骤处理。
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
    ) -> Result<()> {
        if definition.capability_scope.is_empty() {
            return Ok(());
        }
        for scope in &definition.capability_scope {
            let key = format!("{}:{}", definition.id, scope);
            match self.store.get(&key) {
                // An explicit Allow/AlwaysAllow decision supersedes capability checks.
                // The user has already granted this permission; re-checking capability
                // would defeat persistent grants for sessions without a live connector.
                Some(d) if d.is_allow() => continue,
                Some(_) => {
                    anyhow::bail!(
                        "Tool '{}' scope '{}' is denied by stored policy.",
                        definition.id,
                        scope
                    )
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
                        anyhow::bail!(
                            "Tool '{}' requires workspace capability (scope: {}).",
                            definition.id,
                            scope
                        );
                    }
                }
                "browser" => {
                    let has = ctx
                        .capability
                        .as_ref()
                        .map(|c| c.has_browser_capability())
                        .unwrap_or(false);
                    if !has {
                        anyhow::bail!("Tool '{}' requires browser capability.", definition.id);
                    }
                }
                "network" => {}
                other => {
                    anyhow::bail!(
                        "Tool '{}' requests unknown capability scope '{}'. Deny by default.",
                        definition.id,
                        other
                    );
                }
            }
        }
        Ok(())
    }
}

//! Stateless skill instruction loading via SKILL.md system.
//!
//! `load_skill` returns a skill's prompt body as a tool result. It does not
//! mutate session state, change the system prompt, or restrict tools.
//!
//! On registry miss it transparently retries once after a refresh — covers
//! the "same-turn install then use" case (LLM runs lotus_skill.py install
//! → immediately calls Skill('new-skill') before RefreshSkills RuntimeTool
//! runs). Throttled to at most one refresh per 5 seconds to avoid abuse.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::plugin::skill::enablement::{SkillEnablementState, SkillEnablementStore};
use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::skill::substitution::{substitute_skill_body, SkillSubstitutionContext};
use crate::runtime::tools::builtin::refresh_skills::SkillRegistryRefresher;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

/// Format the result of a forked skill execution.
pub fn format_fork_result(skill_name: &str, result_text: &str) -> String {
    format!(
        "Skill \"{}\" completed (forked execution).\n\nResult:\n{}",
        skill_name, result_text
    )
}

/// Throttle for miss-retry: at most one refresh per 5 seconds.
const REFRESH_THROTTLE: Duration = Duration::from_secs(5);

pub struct LoadSkillRuntimeTool {
    skill_registry: Arc<Mutex<SkillRegistry>>,
    enablement_store: Option<Arc<SkillEnablementStore>>,
    /// Optional refresh hook. test / legacy 路径可以传 None，此时 miss-retry
    /// 会跳过 refresh，直接走原 "not found" 路径。
    refresher: Option<Arc<dyn SkillRegistryRefresher>>,
    /// 最近一次因 miss 触发的 refresh 时间。throttle 用。
    last_refresh: Arc<Mutex<Option<Instant>>>,
}

impl LoadSkillRuntimeTool {
    pub fn new(skill_registry: Arc<Mutex<SkillRegistry>>) -> Self {
        Self {
            skill_registry,
            enablement_store: None,
            refresher: None,
            last_refresh: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_enablement(
        skill_registry: Arc<Mutex<SkillRegistry>>,
        enablement_store: Arc<SkillEnablementStore>,
    ) -> Self {
        Self {
            skill_registry,
            enablement_store: Some(enablement_store),
            refresher: None,
            last_refresh: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_refresher(
        skill_registry: Arc<Mutex<SkillRegistry>>,
        refresher: Arc<dyn SkillRegistryRefresher>,
    ) -> Self {
        Self {
            skill_registry,
            enablement_store: None,
            refresher: Some(refresher),
            last_refresh: Arc::new(Mutex::new(None)),
        }
    }

    /// 判断是否允许触发 refresh（throttle）。允许后立刻记录本次时间。
    pub fn with_refresher_and_enablement(
        skill_registry: Arc<Mutex<SkillRegistry>>,
        refresher: Arc<dyn SkillRegistryRefresher>,
        enablement_store: Arc<SkillEnablementStore>,
    ) -> Self {
        Self {
            skill_registry,
            enablement_store: Some(enablement_store),
            refresher: Some(refresher),
            last_refresh: Arc::new(Mutex::new(None)),
        }
    }

    fn enablement_state(&self) -> SkillEnablementState {
        self.enablement_store
            .as_ref()
            .map(|store| store.load_or_default())
            .unwrap_or_default()
    }

    fn unavailable_skill_error(skill_id: &str, available_ids: String) -> ToolError {
        ToolError::ExecutionFailed(format!(
            "Unknown or unavailable skill: {}. Available: {}",
            skill_id, available_ids
        ))
    }

    fn try_acquire_refresh_slot(&self) -> bool {
        let mut last = self.last_refresh.lock().unwrap();
        let now = Instant::now();
        let allow = match *last {
            None => true,
            Some(t) => now.duration_since(t) >= REFRESH_THROTTLE,
        };
        if allow {
            *last = Some(now);
        }
        allow
    }
}

#[async_trait]
impl RuntimeTool for LoadSkillRuntimeTool {
    fn id(&self) -> &str {
        "Skill"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        let ids = self
            .skill_registry
            .lock()
            .map(|reg| reg.enabled_skill_ids(&self.enablement_state()).join(", "))
            .unwrap_or_default();
        let available = if ids.is_empty() {
            "无可用专项技能".to_string()
        } else {
            ids
        };

        let description = format!(
            "加载一个专项技能的详细指令到当前对话。当用户需求匹配技能目录中的某个专项技能时，\
             调用此工具并传入 skill_id。无副作用：不改变系统提示、不限制工具、不持久化。\
             可用 skill_id：{}。",
            available
        );

        ToolDefinition::new("Skill", description)
            .with_kind(ToolKind::Support)
            .with_read_only(true)
            .with_max_result_size_chars(16_000)
            .with_preserve_tool_use_results(true)
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let skill_id = input
            .get("skill_id")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|id| !id.is_empty())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required field: skill_id".into()))?;

        let args = input
            .get("args")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        // Clone the DiskSkill out of the registry (short lock window)
        let enablement = self.enablement_state();
        let (skill, exists, available_ids) = {
            let reg = self
                .skill_registry
                .lock()
                .map_err(|e| ToolError::ExecutionFailed(format!("Registry lock failed: {}", e)))?;
            (
                reg.get_enabled(&skill_id, &enablement).cloned(),
                reg.get(&skill_id).is_some(),
                reg.enabled_skill_ids(&enablement).join(", "),
            )
        };

        // 兜底：registry miss 时，throttle 内尝试 refresh-then-retry。
        // 覆盖"LLM 同 turn install + 立即 Skill('new-skill')"的边缘场景。
        let skill = match skill {
            Some(s) => s,
            None if exists => {
                return Err(Self::unavailable_skill_error(&skill_id, available_ids));
            }
            None => {
                if self.try_acquire_refresh_slot() {
                    if let Some(refresher) = self.refresher.as_ref() {
                        let _ = refresher.refresh_skill_registry();
                    }
                }
                // 重查
                let enablement = self.enablement_state();
                let reg = self.skill_registry.lock().map_err(|e| {
                    ToolError::ExecutionFailed(format!("Registry lock failed: {}", e))
                })?;
                let available_ids = reg.enabled_skill_ids(&enablement).join(", ");
                reg.get_enabled(&skill_id, &enablement)
                    .cloned()
                    .ok_or_else(|| Self::unavailable_skill_error(&skill_id, available_ids))?
            }
        };

        // Check for fork mode (placeholder — full sub-agent wiring in follow-up)
        if skill.frontmatter.context.as_deref() == Some("fork") {
            // TODO: wire to AgentRuntime in follow-up
            let placeholder = format_fork_result(
                &skill.frontmatter.name,
                "fork mode: subagent dispatch will be wired in a follow-up task. Returning a placeholder body so the call doesn't fail.",
            );
            return Ok(ToolResult::new(
                "Skill",
                placeholder,
                Some(json!({
                    "skill_id": skill_id,
                    "display_name": skill.frontmatter.metadata.label.clone()
                        .unwrap_or_else(|| skill.frontmatter.name.clone()),
                    "context": "fork",
                })),
            ));
        }

        // Build substitution context
        let session_id_str = ctx.session_id.as_str().to_string();
        let sub_ctx = SkillSubstitutionContext {
            skill_dir: skill.root.clone(),
            session_id: session_id_str,
            args,
            argument_names: skill.frontmatter.arguments.clone(),
            execute_shell: false,
        };

        let substituted_body = substitute_skill_body(&skill.body, &sub_ctx)
            .map_err(|e| ToolError::ExecutionFailed(format!("Body substitution failed: {}", e)))?;

        let content = format!(
            "## {} ({})\n\nBase directory for this skill: {}\n\n{}",
            skill.frontmatter.name,
            skill_id,
            skill.root.display(),
            substituted_body
        );

        // Track invoked skill
        {
            if let Ok(mut reg) = self.skill_registry.lock() {
                reg.remember_invoked(None, &skill_id, substituted_body.clone());
            }
        }

        Ok(ToolResult::new(
            "Skill",
            content,
            Some(json!({
                "skill_id": skill_id,
                "display_name": skill.frontmatter.metadata.label.clone()
                    .unwrap_or_else(|| skill.frontmatter.name.clone()),
            })),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::skill::enablement::SkillEnablementStore;
    use crate::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillMetadata, SkillSource};
    use crate::runtime::tools::{RuntimeTool, ToolDescriptionContext, ToolExecutionContext};
    use crate::storage::{AiJiaHome, CurrentUserStorage, UserScope};
    use std::path::PathBuf;

    fn disk_skill(id: &str) -> DiskSkill {
        DiskSkill {
            id: id.to_string(),
            root: PathBuf::from("/tmp").join(id),
            frontmatter: SkillFrontmatter {
                name: id.to_string(),
                description: format!("description for {id}"),
                when_to_use: None,
                allowed_tools: vec![],
                argument_hint: None,
                arguments: vec![],
                model: None,
                effort: None,
                context: None,
                agent: None,
                user_invocable: true,
                disable_model_invocation: false,
                version: None,
                paths: vec![],
                hooks: Default::default(),
                shell: None,
                category: None,
                metadata: SkillMetadata::default(),
            },
            body: format!("body for {id}"),
            source: SkillSource::User,
        }
    }

    fn enablement_store(tmp: &tempfile::TempDir) -> Arc<SkillEnablementStore> {
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        current_user.activate_scope(UserScope::new(1, 2)).unwrap();
        Arc::new(SkillEnablementStore::new(current_user))
    }

    #[tokio::test]
    async fn skill_definition_lists_only_enabled_skill_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let enablement = enablement_store(&tmp);
        enablement
            .set_enabled("disabled-skill", false)
            .expect("disable skill");
        let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(vec![
            disk_skill("enabled-skill"),
            disk_skill("disabled-skill"),
        ])));
        let tool = LoadSkillRuntimeTool::with_enablement(registry, enablement);

        let definition = tool.definition(&ToolDescriptionContext::default()).await;

        assert!(definition.description.contains("enabled-skill"));
        assert!(!definition.description.contains("disabled-skill"));
    }

    #[tokio::test]
    async fn load_skill_rejects_disabled_skill_without_refresh() {
        let tmp = tempfile::tempdir().unwrap();
        let enablement = enablement_store(&tmp);
        enablement
            .set_enabled("disabled-skill", false)
            .expect("disable skill");
        let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(vec![disk_skill(
            "disabled-skill",
        )])));
        let tool = LoadSkillRuntimeTool::with_enablement(registry, enablement);

        let err = tool
            .execute(
                json!({ "skill_id": "disabled-skill" }),
                ToolExecutionContext::for_test("conv", "run", "tool-call"),
            )
            .await
            .unwrap_err();

        assert!(format!("{err:?}").contains("Unknown or unavailable skill"));
    }

    struct InsertingRefresher {
        registry: Arc<Mutex<SkillRegistry>>,
    }

    impl std::fmt::Debug for InsertingRefresher {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("InsertingRefresher").finish_non_exhaustive()
        }
    }

    impl SkillRegistryRefresher for InsertingRefresher {
        fn refresh_skill_registry(&self) -> Result<(), String> {
            self.registry
                .lock()
                .map_err(|e| e.to_string())?
                .insert(disk_skill("disabled-after-refresh"));
            Ok(())
        }
    }

    #[tokio::test]
    async fn load_skill_miss_refresh_still_rejects_disabled_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let enablement = enablement_store(&tmp);
        enablement
            .set_enabled("disabled-after-refresh", false)
            .expect("disable skill");
        let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(Vec::new())));
        let tool = LoadSkillRuntimeTool::with_refresher_and_enablement(
            registry.clone(),
            Arc::new(InsertingRefresher {
                registry: registry.clone(),
            }),
            enablement,
        );

        let err = tool
            .execute(
                json!({ "skill_id": "disabled-after-refresh" }),
                ToolExecutionContext::for_test("conv", "run", "tool-call"),
            )
            .await
            .unwrap_err();

        assert!(format!("{err:?}").contains("Unknown or unavailable skill"));
        assert!(registry
            .lock()
            .unwrap()
            .get("disabled-after-refresh")
            .is_some());
    }
}

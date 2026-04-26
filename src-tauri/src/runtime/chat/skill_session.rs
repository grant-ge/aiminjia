use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use log::warn;

use crate::plugin::skill_trait::{Skill, SkillState, StepAction, ToolFilter};
use crate::plugin::SkillRegistry;
use crate::runtime::store::MemoryStore;

#[derive(Debug, Clone)]
pub struct SkillTurnContext {
    pub skill_id: String,
    pub state: SkillState,
    pub system_prompt: String,
    pub allowed_tools: Option<HashSet<String>>,
}

#[derive(Default)]
pub struct SkillSessionStore {
    sessions: Mutex<HashMap<String, SkillState>>,
    memory_store: Option<Arc<dyn MemoryStore>>,
}

impl SkillSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_memory_store(memory_store: Arc<dyn MemoryStore>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            memory_store: Some(memory_store),
        }
    }

    pub fn clear_session(&self, conversation_id: &str) {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(conversation_id);
    }

    pub async fn switch_skill(
        &self,
        registry: &SkillRegistry,
        all_tool_names: &[String],
        conversation_id: &str,
        skill_id: &str,
        has_files: bool,
    ) -> Result<SkillTurnContext> {
        let _default_skill = registry.get_default().await.ok_or_else(|| {
            anyhow!(
                "default skill '{}' is not registered",
                registry.default_skill_id()
            )
        })?;
        let skill = registry
            .get(skill_id)
            .await
            .ok_or_else(|| anyhow!("skill '{}' is not registered", skill_id))?;

        let state = initial_state_for_skill(skill.as_ref(), has_files);
        let mut allowed_tools = resolve_allowed_tools(
            all_tool_names,
            skill.tool_filter(&state),
            skill.allowed_tool_names(&state),
        );
        allowed_tools = ensure_switch_skill_tool(allowed_tools);

        self.persist_state(conversation_id, &state)?;

        Ok(SkillTurnContext {
            skill_id: skill.id().to_string(),
            system_prompt: skill.system_prompt(&state),
            allowed_tools,
            state,
        })
    }

    pub async fn resolve_turn_context(
        &self,
        registry: &SkillRegistry,
        all_tool_names: &[String],
        conversation_id: &str,
        user_message: &str,
        has_files: bool,
    ) -> Result<SkillTurnContext> {
        let default_skill = registry.get_default().await.ok_or_else(|| {
            anyhow!(
                "default skill '{}' is not registered",
                registry.default_skill_id()
            )
        })?;
        let stored_state = self.load_state(conversation_id)?;

        let mut state = stored_state.unwrap_or_else(|| SkillState::new(default_skill.id()));
        let mut skill = registry
            .get(state.skill_id.as_str())
            .await
            .unwrap_or_else(|| default_skill.clone());

        if skill.id() != default_skill.id() {
            state =
                apply_step_transition(skill.as_ref(), default_skill.as_ref(), state, user_message);
            skill = registry
                .get(state.skill_id.as_str())
                .await
                .unwrap_or_else(|| default_skill.clone());
        }
        // LLM-based routing: 不做关键词匹配，把 skill 目录注入到 system prompt，
        // 让 LLM 通过 switch_skill 工具自行决定是否切换。

        // 只在 default skill 时注入 skill 目录
        let skill_directory = if skill.id() == default_skill.id() {
            build_skill_directory_prompt(registry, default_skill.id()).await
        } else {
            String::new()
        };

        state = initialize_state_for_turn(skill.as_ref(), state, has_files);
        let allowed_tools = ensure_switch_skill_tool(resolve_allowed_tools(
            all_tool_names,
            skill.tool_filter(&state),
            skill.allowed_tool_names(&state),
        ));

        self.persist_state(conversation_id, &state)?;

        let system_prompt = {
            let base = skill.system_prompt(&state);
            if skill_directory.is_empty() {
                base
            } else {
                format!("{}\n\n{}", base, skill_directory)
            }
        };

        Ok(SkillTurnContext {
            skill_id: skill.id().to_string(),
            system_prompt,
            allowed_tools,
            state,
        })
    }

    fn load_state(&self, conversation_id: &str) -> Result<Option<SkillState>> {
        if let Some(state) = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(conversation_id)
            .cloned()
        {
            return Ok(Some(state));
        }

        let Some(memory_store) = self.memory_store.as_ref() else {
            return Ok(None);
        };
        let Some(serialized_state) = memory_store.get(&skill_state_memory_key(conversation_id))?
        else {
            return Ok(None);
        };
        let state = match serde_json::from_str::<SkillState>(&serialized_state) {
            Ok(state) => state,
            Err(err) => {
                warn!(
                    "[skill_session] ignoring invalid persisted state for conversation {}: {}",
                    conversation_id, err
                );
                return Ok(None);
            }
        };
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(conversation_id.to_string(), state.clone());
        Ok(Some(state))
    }

    fn persist_state(&self, conversation_id: &str, state: &SkillState) -> Result<()> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(conversation_id.to_string(), state.clone());

        if let Some(memory_store) = self.memory_store.as_ref() {
            let serialized_state = serde_json::to_string(state)?;
            memory_store.set(&skill_state_memory_key(conversation_id), &serialized_state)?;
        }

        Ok(())
    }
}

async fn build_skill_directory_prompt(registry: &SkillRegistry, default_skill_id: &str) -> String {
    let skills = registry.list().await;
    let non_default: Vec<_> = skills
        .iter()
        .filter(|s| s.id != default_skill_id)
        .collect();

    if non_default.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "## 可用专项技能".to_string(),
        "如果用户需求与以下某个专项技能匹配，请调用 switch_skill 工具切换到该技能：".to_string(),
        String::new(),
    ];
    for skill in &non_default {
        lines.push(format!("- `{}`: {}", skill.id, skill.description));
    }
    lines.push(String::new());
    lines.push("如果没有匹配的专项技能，直接用通用能力回答即可。".to_string());

    lines.join("\n")
}

fn skill_state_memory_key(conversation_id: &str) -> String {
    format!("note:{}:active_skill_state", conversation_id)
}

fn ensure_switch_skill_tool(allowed_tools: Option<HashSet<String>>) -> Option<HashSet<String>> {
    allowed_tools.map(|mut names| {
        names.insert("switch_skill".to_string());
        names
    })
}

fn initialize_state_for_turn(
    skill: &dyn Skill,
    mut state: SkillState,
    has_files: bool,
) -> SkillState {
    state.skill_id = skill.id().to_string();
    state.has_files = has_files;
    state.resolved_step_prompt = None;

    if state.current_step.is_none() {
        if let Some(workflow) = skill.workflow() {
            let initial_step = workflow.initial_step;
            state.current_step = Some(initial_step.clone());
            state
                .step_status
                .entry(initial_step)
                .or_insert_with(|| "active".to_string());
        }
    }

    state
}

fn initial_state_for_skill(skill: &dyn Skill, has_files: bool) -> SkillState {
    initialize_state_for_turn(skill, SkillState::new(skill.id()), has_files)
}

fn apply_step_transition(
    current_skill: &dyn Skill,
    default_skill: &dyn Skill,
    mut state: SkillState,
    user_message: &str,
) -> SkillState {
    let previous_step = state.current_step.clone();
    match current_skill.on_step_complete(&mut state, user_message) {
        StepAction::WaitForUser => state,
        StepAction::AdvanceToStep(next_step) => {
            if let Some(previous_step) = previous_step {
                state
                    .step_status
                    .insert(previous_step, "completed".to_string());
            }
            state
                .step_status
                .insert(next_step.clone(), "active".to_string());
            state.current_step = Some(next_step);
            state.resolved_step_prompt = None;
            state
        }
        StepAction::Finish | StepAction::Abort => {
            initial_state_for_skill(default_skill, state.has_files)
        }
    }
}

fn resolve_allowed_tools(
    all_tool_names: &[String],
    tool_filter: ToolFilter,
    allowed_tool_names: Option<Vec<String>>,
) -> Option<HashSet<String>> {
    if let Some(allowed_tool_names) = allowed_tool_names {
        return Some(allowed_tool_names.into_iter().collect());
    }

    match tool_filter {
        ToolFilter::All => None,
        ToolFilter::Only(names) => Some(names.into_iter().collect()),
        ToolFilter::Exclude(excluded) => {
            let excluded: HashSet<String> = excluded.into_iter().collect();
            Some(
                all_tool_names
                    .iter()
                    .filter(|name| !excluded.contains(*name))
                    .cloned()
                    .collect(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::skill_trait::WorkflowDefinition;
    use crate::plugin::SkillRegistry;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct TestSkill {
        id: &'static str,
        trigger: Option<&'static str>,
        prompt_prefix: &'static str,
        default_tools: Vec<String>,
        workflow: Option<WorkflowDefinition>,
    }

    #[async_trait]
    impl Skill for TestSkill {
        fn id(&self) -> &str {
            self.id
        }

        fn display_name(&self) -> &str {
            self.id
        }

        fn description(&self) -> &str {
            self.id
        }

        fn system_prompt(&self, state: &SkillState) -> String {
            format!(
                "{}:{}",
                self.prompt_prefix,
                state.current_step.as_deref().unwrap_or("none")
            )
        }

        fn tool_filter(&self, _state: &SkillState) -> ToolFilter {
            ToolFilter::Only(self.default_tools.clone())
        }

        fn workflow(&self) -> Option<WorkflowDefinition> {
            self.workflow.clone()
        }

        fn allowed_tool_names(&self, state: &SkillState) -> Option<Vec<String>> {
            match state.current_step.as_deref() {
                Some("step0") => Some(vec!["search_files".to_string()]),
                Some("step1") => Some(vec!["read_workspace_file".to_string()]),
                _ => Some(self.default_tools.clone()),
            }
        }

        fn on_step_complete(&self, _state: &mut SkillState, user_message: &str) -> StepAction {
            match user_message.trim() {
                "继续" => StepAction::AdvanceToStep("step1".to_string()),
                "完成" => StepAction::Finish,
                _ => StepAction::WaitForUser,
            }
        }
    }

    fn workflow_skill() -> Arc<dyn Skill> {
        Arc::new(TestSkill {
            id: "comp-analysis",
            trigger: Some("分析"),
            prompt_prefix: "skill",
            default_tools: vec!["search_files".to_string()],
            workflow: Some(WorkflowDefinition {
                initial_step: "step0".to_string(),
                steps: vec![],
            }),
        })
    }

    fn daily_skill() -> Arc<dyn Skill> {
        Arc::new(TestSkill {
            id: "daily-assistant",
            trigger: None,
            prompt_prefix: "daily",
            default_tools: vec!["bash".to_string()],
            workflow: None,
        })
    }

    async fn registry_with_test_skills() -> SkillRegistry {
        let registry = SkillRegistry::new("daily-assistant");
        registry.register(daily_skill(), "test").await;
        registry.register(workflow_skill(), "test").await;
        registry
    }

    #[tokio::test]
    async fn resolves_default_skill_when_no_activation_matches() {
        let registry = registry_with_test_skills().await;
        let store = SkillSessionStore::new();

        let context = store
            .resolve_turn_context(
                &registry,
                &["bash".to_string(), "search_files".to_string()],
                "conv-default",
                "你好",
                false,
            )
            .await
            .expect("default context");

        assert_eq!(context.skill_id, "daily-assistant");
        // After LLM-based routing, default skill system_prompt includes the skill directory.
        assert!(
            context.system_prompt.starts_with("daily:none"),
            "system_prompt should start with base prompt, got: {}",
            context.system_prompt
        );
        assert!(
            context.system_prompt.contains("comp-analysis"),
            "system_prompt should list available skills in directory"
        );
        assert_eq!(
            context.allowed_tools,
            Some(HashSet::from([
                "bash".to_string(),
                "switch_skill".to_string()
            ]))
        );
    }

    #[tokio::test]
    async fn keyword_routing_removed_stays_on_default_skill() {
        // After removing keyword-based routing, messages that used to trigger
        // keyword activation now stay on the default skill. The LLM decides
        // routing via switch_skill tool call instead.
        let registry = registry_with_test_skills().await;
        let store = SkillSessionStore::new();

        let context = store
            .resolve_turn_context(
                &registry,
                &[
                    "bash".to_string(),
                    "search_files".to_string(),
                    "read_workspace_file".to_string(),
                ],
                "conv-activate",
                "请帮我分析这个表格",
                true,
            )
            .await
            .expect("skill context");

        // No keyword activation: stays on daily-assistant, system_prompt includes skill directory.
        assert_eq!(context.skill_id, "daily-assistant");
        assert!(
            context.system_prompt.starts_with("daily:none"),
            "should use default skill prompt, got: {}",
            context.system_prompt
        );
        assert!(
            context.system_prompt.contains("comp-analysis"),
            "system_prompt should include skill directory"
        );
    }

    #[tokio::test]
    async fn active_skill_advances_and_can_fall_back_to_default() {
        let registry = registry_with_test_skills().await;
        let store = SkillSessionStore::new();
        let all_tools = vec![
            "bash".to_string(),
            "search_files".to_string(),
            "read_workspace_file".to_string(),
            "switch_skill".to_string(),
        ];

        // Use explicit switch_skill to activate comp-analysis (LLM-based routing)
        let activated = store
            .switch_skill(&registry, &all_tools, "conv-progress", "comp-analysis", false)
            .await
            .expect("activate via explicit switch");
        assert_eq!(activated.state.current_step.as_deref(), Some("step0"));

        let advanced = store
            .resolve_turn_context(&registry, &all_tools, "conv-progress", "继续", false)
            .await
            .expect("advance");
        assert_eq!(advanced.skill_id, "comp-analysis");
        assert_eq!(advanced.state.current_step.as_deref(), Some("step1"));
        assert_eq!(advanced.system_prompt, "skill:step1");
        assert_eq!(
            advanced.allowed_tools,
            Some(HashSet::from([
                "read_workspace_file".to_string(),
                "switch_skill".to_string()
            ]))
        );

        let fallback = store
            .resolve_turn_context(&registry, &all_tools, "conv-progress", "完成", false)
            .await
            .expect("fallback");
        assert_eq!(fallback.skill_id, "daily-assistant");
        assert_eq!(fallback.state.current_step, None);
    }

    #[tokio::test]
    async fn explicit_switch_uses_selected_skill_even_without_keyword_match() {
        let registry = registry_with_test_skills().await;
        let store = SkillSessionStore::new();
        let all_tools = vec![
            "bash".to_string(),
            "search_files".to_string(),
            "read_workspace_file".to_string(),
            "switch_skill".to_string(),
        ];

        let switched = store
            .switch_skill(
                &registry,
                &all_tools,
                "conv-selected-token",
                "comp-analysis",
                false,
            )
            .await
            .expect("selected skill should switch explicitly");

        assert_eq!(switched.skill_id, "comp-analysis");
        assert_eq!(switched.system_prompt, "skill:step0");
        assert_eq!(switched.state.current_step.as_deref(), Some("step0"));

        let persisted = store
            .resolve_turn_context(&registry, &all_tools, "conv-selected-token", "继续", false)
            .await
            .expect("explicitly selected skill should persist");
        assert_eq!(persisted.skill_id, "comp-analysis");
        assert_eq!(persisted.state.current_step.as_deref(), Some("step1"));
    }

    #[tokio::test]
    async fn explicit_switch_updates_session_state_and_initializes_workflow() {
        let registry = registry_with_test_skills().await;
        let store = SkillSessionStore::new();
        let all_tools = vec![
            "bash".to_string(),
            "search_files".to_string(),
            "read_workspace_file".to_string(),
            "switch_skill".to_string(),
        ];

        let switched = store
            .switch_skill(
                &registry,
                &all_tools,
                "conv-explicit-switch",
                "comp-analysis",
                true,
            )
            .await
            .expect("explicit switch should succeed");

        assert_eq!(switched.skill_id, "comp-analysis");
        assert_eq!(switched.state.current_step.as_deref(), Some("step0"));
        assert!(switched.state.has_files);
        assert_eq!(
            switched.allowed_tools,
            Some(HashSet::from([
                "search_files".to_string(),
                "switch_skill".to_string()
            ]))
        );

        let persisted = store
            .resolve_turn_context(&registry, &all_tools, "conv-explicit-switch", "继续", true)
            .await
            .expect("switch should persist into next turn");
        assert_eq!(persisted.skill_id, "comp-analysis");
        assert_eq!(persisted.state.current_step.as_deref(), Some("step1"));
    }

    #[tokio::test]
    async fn persists_skill_state_and_restores_it_from_memory_store() {
        let registry = registry_with_test_skills().await;
        let memory_store = Arc::new(crate::runtime::store::InMemoryMemoryStore::default());
        let store = SkillSessionStore::with_memory_store(memory_store.clone());
        let all_tools = vec![
            "bash".to_string(),
            "search_files".to_string(),
            "read_workspace_file".to_string(),
            "switch_skill".to_string(),
        ];

        store
            .switch_skill(
                &registry,
                &all_tools,
                "conv-persisted-skill",
                "comp-analysis",
                true,
            )
            .await
            .expect("explicit switch should persist the initial state");

        let advanced = store
            .resolve_turn_context(&registry, &all_tools, "conv-persisted-skill", "继续", true)
            .await
            .expect("state should advance before restart");
        assert_eq!(advanced.state.current_step.as_deref(), Some("step1"));

        let persisted = crate::runtime::store::MemoryStore::get(
            memory_store.as_ref(),
            "note:conv-persisted-skill:active_skill_state",
        )
        .expect("memory store should read persisted skill state")
        .expect("persisted skill state should exist");
        let persisted_state: SkillState =
            serde_json::from_str(&persisted).expect("persisted skill state should be valid json");
        assert_eq!(persisted_state.skill_id, "comp-analysis");
        assert_eq!(persisted_state.current_step.as_deref(), Some("step1"));

        let restored_store = SkillSessionStore::with_memory_store(memory_store);
        let restored = restored_store
            .resolve_turn_context(
                &registry,
                &all_tools,
                "conv-persisted-skill",
                "我先看看",
                true,
            )
            .await
            .expect("new store should restore persisted state");

        assert_eq!(restored.skill_id, "comp-analysis");
        assert_eq!(restored.state.current_step.as_deref(), Some("step1"));
        assert_eq!(
            restored.allowed_tools,
            Some(HashSet::from([
                "read_workspace_file".to_string(),
                "switch_skill".to_string()
            ]))
        );
    }

    #[tokio::test]
    async fn clear_session_allows_state_to_restore_from_persistence() {
        let registry = registry_with_test_skills().await;
        let memory_store = Arc::new(crate::runtime::store::InMemoryMemoryStore::default());
        let store = SkillSessionStore::with_memory_store(memory_store);
        let all_tools = vec![
            "bash".to_string(),
            "search_files".to_string(),
            "read_workspace_file".to_string(),
            "switch_skill".to_string(),
        ];

        store
            .switch_skill(
                &registry,
                &all_tools,
                "conv-reconnect",
                "comp-analysis",
                true,
            )
            .await
            .expect("switch should persist state before reconnect");
        store
            .resolve_turn_context(&registry, &all_tools, "conv-reconnect", "继续", true)
            .await
            .expect("state should advance before reconnect");

        store.clear_session("conv-reconnect");

        let restored = store
            .resolve_turn_context(&registry, &all_tools, "conv-reconnect", "我回来了", true)
            .await
            .expect("cleared in-memory state should restore from persistence");

        assert_eq!(restored.skill_id, "comp-analysis");
        assert_eq!(restored.state.current_step.as_deref(), Some("step1"));
    }
}

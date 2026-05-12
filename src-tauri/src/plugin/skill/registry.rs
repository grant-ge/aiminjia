use std::collections::{HashMap, HashSet};

use super::catalog_prompt::format_skill_catalog_with_budget;
use super::invoked::InvokedSkillStore;
use super::types::DiskSkill;

#[derive(Default)]
pub struct SkillRegistry {
    skills: HashMap<String, DiskSkill>,
    sent_skill_names: HashMap<String, HashSet<String>>,
    invoked: InvokedSkillStore,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_skills(skills: Vec<DiskSkill>) -> Self {
        let mut registry = Self::new();
        for skill in skills {
            registry.insert(skill);
        }
        registry
    }

    pub fn insert(&mut self, skill: DiskSkill) {
        self.skills.insert(skill.id.clone(), skill);
    }

    /// Wholesale replace all skills (used after a successful install / uninstall).
    /// Also clears per-agent sent_skill_names so the catalog re-emits all entries.
    pub fn replace_all(&mut self, skills: Vec<DiskSkill>) {
        self.skills.clear();
        for skill in skills {
            self.skills.insert(skill.id.clone(), skill);
        }
        self.sent_skill_names.clear();
    }

    pub fn get(&self, id: &str) -> Option<&DiskSkill> {
        self.skills.get(id)
    }

    pub fn skill_ids(&self) -> Vec<String> {
        let mut ids = self.skills.keys().cloned().collect::<Vec<_>>();
        ids.sort();
        ids
    }

    /// Render the full skill catalog every call. Stateless and safe to invoke
    /// from any conversation / turn without coordinating with other callers.
    ///
    /// `catalog_delta_for_agent` exists for per-subagent delta optimization but
    /// requires a real session-scoped key + reset hook to be correct; until that
    /// wiring lands, the main chat path must use this full-catalog variant so
    /// that every new conversation sees the skill listing.
    pub fn format_full_catalog(&self, context_window_tokens: usize) -> String {
        let mut skills = self.skills.values().cloned().collect::<Vec<_>>();
        skills.sort_by(|a, b| a.id.cmp(&b.id));
        format_skill_catalog_with_budget(&skills, context_window_tokens)
    }

    pub fn catalog_delta_for_agent(&mut self, agent_id: Option<&str>, context_window_tokens: usize) -> String {
        let key = agent_id.unwrap_or("").to_string();
        let sent = self.sent_skill_names.entry(key).or_default();
        let mut new_skills = self
            .skills
            .values()
            .filter(|skill| !sent.contains(&skill.id))
            .cloned()
            .collect::<Vec<_>>();
        new_skills.sort_by(|a, b| a.id.cmp(&b.id));
        if new_skills.is_empty() {
            return String::new();
        }
        for skill in &new_skills {
            sent.insert(skill.id.clone());
        }
        format_skill_catalog_with_budget(&new_skills, context_window_tokens)
    }

    pub fn reset_sent_skill_names(&mut self) {
        self.sent_skill_names.clear();
    }

    pub fn remember_invoked(&mut self, agent_id: Option<&str>, skill_id: &str, body: String) {
        self.invoked.remember(agent_id, skill_id, body);
    }
}

#[cfg(test)]
mod replace_all_tests {
    use super::*;
    use crate::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillMetadata, SkillSource};
    use std::path::PathBuf;

    fn skill(id: &str) -> DiskSkill {
        DiskSkill {
            id: id.to_string(),
            root: PathBuf::from("/tmp"),
            frontmatter: SkillFrontmatter {
                name: id.to_string(),
                description: "desc".to_string(),
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
            body: String::new(),
            source: SkillSource::User,
        }
    }

    #[test]
    fn replace_all_drops_old_skills_and_inserts_new() {
        let mut reg = SkillRegistry::from_skills(vec![skill("old-a"), skill("old-b")]);
        reg.replace_all(vec![skill("new-x"), skill("new-y")]);
        let ids = reg.skill_ids();
        assert_eq!(ids, vec!["new-x".to_string(), "new-y".to_string()]);
    }

    #[test]
    fn replace_all_resets_sent_skill_names() {
        let mut reg = SkillRegistry::from_skills(vec![skill("a")]);

        let first_delta = reg.catalog_delta_for_agent(Some("agent-1"), 100_000);
        assert!(first_delta.contains("`a`"));

        let second_delta = reg.catalog_delta_for_agent(Some("agent-1"), 100_000);
        assert!(!second_delta.contains("`a`"));

        reg.replace_all(vec![skill("a"), skill("b")]);

        let post_replace_delta = reg.catalog_delta_for_agent(Some("agent-1"), 100_000);
        assert!(post_replace_delta.contains("`a`"));
        assert!(post_replace_delta.contains("`b`"));
    }

    #[test]
    fn format_full_catalog_returns_all_skills_on_every_call() {
        let reg = SkillRegistry::from_skills(vec![skill("a"), skill("b")]);

        let first = reg.format_full_catalog(100_000);
        let second = reg.format_full_catalog(100_000);

        assert!(first.contains("`a`"));
        assert!(first.contains("`b`"));
        assert_eq!(first, second, "full catalog must be stable across calls");
    }

    #[test]
    fn format_full_catalog_unaffected_by_prior_delta_calls() {
        let mut reg = SkillRegistry::from_skills(vec![skill("a"), skill("b")]);

        let _ = reg.catalog_delta_for_agent(Some("agent-1"), 100_000);
        let _ = reg.catalog_delta_for_agent(None, 100_000);

        let full = reg.format_full_catalog(100_000);
        assert!(full.contains("`a`"));
        assert!(full.contains("`b`"));
    }
}

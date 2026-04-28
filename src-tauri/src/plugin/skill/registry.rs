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

    pub fn get(&self, id: &str) -> Option<&DiskSkill> {
        self.skills.get(id)
    }

    pub fn skill_ids(&self) -> Vec<String> {
        let mut ids = self.skills.keys().cloned().collect::<Vec<_>>();
        ids.sort();
        ids
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

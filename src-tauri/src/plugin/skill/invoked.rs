use std::collections::HashMap;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct InvokedSkillInfo {
    pub skill_id: String,
    pub body: String,
    pub invoked_at: SystemTime,
}

#[derive(Default)]
pub struct InvokedSkillStore {
    entries: HashMap<String, InvokedSkillInfo>,
}

impl InvokedSkillStore {
    pub fn remember(&mut self, agent_id: Option<&str>, skill_id: &str, body: String) {
        let key = format!("{}:{}", agent_id.unwrap_or(""), skill_id);
        self.entries.insert(
            key,
            InvokedSkillInfo {
                skill_id: skill_id.to_string(),
                body,
                invoked_at: SystemTime::now(),
            },
        );
    }
}

use std::collections::HashMap;

use crate::runtime::agent::builtin::{
    browse_data_agent::browse_data_agent_definition,
    daily_assistant_agent::daily_assistant_agent_definition, explore::explore_agent_definition,
    general_purpose::general_purpose_agent_definition,
};
use crate::runtime::agent::definition::AgentDefinition;

pub struct AgentRegistry {
    agents: HashMap<String, AgentDefinition>,
}

impl AgentRegistry {
    pub fn with_builtins() -> Self {
        let mut registry = Self {
            agents: HashMap::new(),
        };
        registry.register(browse_data_agent_definition());
        registry.register(daily_assistant_agent_definition());
        registry.register(general_purpose_agent_definition());
        registry.register(explore_agent_definition());
        registry
    }

    pub fn register(&mut self, def: AgentDefinition) {
        self.agents.insert(def.name.clone(), def);
    }

    pub fn get(&self, name: &str) -> Option<&AgentDefinition> {
        self.agents.get(name)
    }

    pub fn list(&self) -> Vec<&AgentDefinition> {
        let mut list: Vec<&AgentDefinition> = self.agents.values().collect();
        list.sort_by_key(|d| &d.name);
        list
    }
}

#[derive(Clone, Debug)]
pub enum AgentModel {
    Inherit,
    Fixed(String),
}

#[derive(Clone, Debug)]
pub enum AgentPrompt {
    Inline(String),
    File(std::path::PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSource {
    Builtin,
    User,
}

#[derive(Clone, Debug)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub max_iterations: usize,
    pub model: AgentModel,
    pub system_prompt: AgentPrompt,
    pub source: AgentSource,
}

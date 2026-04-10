#[derive(Clone, Debug)]
pub struct ToolDefinition {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub capability_scope: Vec<String>,
}

impl ToolDefinition {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            display_name: id.clone(),
            id,
            description: description.into(),
            capability_scope: Vec::new(),
        }
    }
}

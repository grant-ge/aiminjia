use super::PromptAssembly;

pub struct OpenAiChatPromptRenderer;

impl OpenAiChatPromptRenderer {
    pub fn render_system_message(assembly: &PromptAssembly) -> Option<serde_json::Value> {
        let content = assembly.flatten();
        if content.trim().is_empty() {
            return None;
        }
        Some(serde_json::json!({ "role": "system", "content": content }))
    }
}

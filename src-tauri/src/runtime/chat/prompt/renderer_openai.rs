use super::{PromptAssembly, PromptCachePolicy};

pub struct OpenAiChatPromptRenderer;

impl OpenAiChatPromptRenderer {
    /// 输出 OpenAI 兼容的 system message：
    /// content 是一个数组，每个元素一个 PromptBlock，
    /// StaticPrefix / SessionDynamic 段带 cache_control，Volatile 不带。
    pub fn render_system_message(assembly: &PromptAssembly) -> Option<serde_json::Value> {
        let blocks: Vec<serde_json::Value> = assembly
            .blocks()
            .iter()
            .filter(|b| !b.text.trim().is_empty())
            .map(|b| {
                let mut item = serde_json::json!({
                    "type": "text",
                    "text": b.text,
                });
                match b.cache_policy {
                    PromptCachePolicy::StaticPrefix | PromptCachePolicy::SessionDynamic => {
                        item["cache_control"] =
                            serde_json::json!({ "type": "ephemeral" });
                    }
                    PromptCachePolicy::Volatile => {}
                }
                item
            })
            .collect();

        if blocks.is_empty() {
            return None;
        }
        Some(serde_json::json!({
            "role": "system",
            "content": blocks,
        }))
    }

    /// 兼容降级：某些 OpenAI 兼容端点不支持 content 数组形式。
    /// 调用方判断 provider capability 决定走哪个。
    pub fn render_system_message_flat(assembly: &PromptAssembly) -> Option<serde_json::Value> {
        let content = assembly.flatten();
        if content.trim().is_empty() {
            return None;
        }
        Some(serde_json::json!({ "role": "system", "content": content }))
    }
}

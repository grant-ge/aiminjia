use crate::llm::prompts;

use super::{PromptAssembly, PromptBlock, PromptCachePolicy, PromptSectionId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSectionSpec {
    pub section_id: PromptSectionId,
    pub cache_policy: PromptCachePolicy,
    pub cache_break_reason: Option<String>,
}

impl PromptSectionSpec {
    pub fn static_prefix(section_id: PromptSectionId) -> Self {
        Self {
            section_id,
            cache_policy: PromptCachePolicy::StaticPrefix,
            cache_break_reason: None,
        }
    }

    pub fn session_dynamic(section_id: PromptSectionId) -> Self {
        Self {
            section_id,
            cache_policy: PromptCachePolicy::SessionDynamic,
            cache_break_reason: None,
        }
    }

    pub fn volatile(section_id: PromptSectionId, reason: impl Into<String>) -> Self {
        Self {
            section_id,
            cache_policy: PromptCachePolicy::Volatile,
            cache_break_reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptBuildContext<'a> {
    pub persona: Option<&'a crate::storage::file_store::persona::Persona>,
    pub product_name: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct PromptAssembler;

impl PromptAssembler {
    pub fn build_system_prompt(&self, ctx: PromptBuildContext<'_>) -> PromptAssembly {
        let fragments = prompts::get_prompt_fragment_snapshot();
        let system = apply_product_name(fragments.system, ctx.product_name);

        let mut blocks = vec![PromptBlock::static_block(
            PromptSectionId::new("system"),
            system,
        )];

        if let Some(persona) = ctx.persona {
            let persona_text = render_persona_section(persona);
            if !persona_text.trim().is_empty() {
                blocks.push(PromptBlock::dynamic_block(
                    PromptSectionId::new("persona"),
                    persona_text,
                ));
            }
        }

        PromptAssembly::new(blocks)
    }
}

fn apply_product_name(text: String, product_name: Option<&str>) -> String {
    match product_name {
        Some(name) if !name.is_empty() && name != "AI小家" => text.replace("AI小家", name),
        _ => text,
    }
}

fn render_persona_section(persona: &crate::storage::file_store::persona::Persona) -> String {
    let mut parts = Vec::new();
    if !persona.identity.is_empty() {
        parts.push(format!("【角色设定】{}", persona.identity));
    }
    if !persona.expertise.is_empty() {
        parts.push(format!("【专业领域】{}", persona.expertise.join("、")));
    }
    if !persona.memory_hints.is_empty() {
        let hints = persona
            .memory_hints
            .iter()
            .map(|hint| format!("- {hint}"))
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!("【记忆管理（白名单制）】\n{hints}"));
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::apply_product_name;

    #[test]
    fn product_name_replacement_covers_system_prompt_text() {
        let text = "你是 AI小家 system".to_string();

        let replaced = apply_product_name(text, Some("小新助手"));

        assert!(replaced.contains("你是 小新助手 system"));
        assert!(!replaced.contains("AI小家"));
    }

    #[test]
    fn product_name_replacement_keeps_default_brand_text() {
        let text = "你是 AI小家".to_string();

        assert_eq!(apply_product_name(text.clone(), None), text);
        assert_eq!(apply_product_name(text.clone(), Some("AI小家")), text);
    }
}

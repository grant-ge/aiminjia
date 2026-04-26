use crate::llm::prompts::{self, PromptMode};

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
    pub mode: PromptMode,
    pub persona: Option<&'a crate::storage::file_store::persona::Persona>,
    pub product_name: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct PromptAssembler;

impl PromptAssembler {
    pub fn build_system_prompt(&self, ctx: PromptBuildContext<'_>) -> PromptAssembly {
        let base_raw = prompts::get_prompt_fragment("base");
        let base = match ctx.product_name {
            Some(name) if !name.is_empty() && name != "AI小家" => {
                base_raw.replace("AI小家", name)
            }
            _ => base_raw,
        };

        let mut blocks = vec![
            PromptBlock::static_block(PromptSectionId::new("base"), base),
            PromptBlock::static_block(
                PromptSectionId::new("tool_preference"),
                prompts::tool_preference_section(),
            ),
            PromptBlock::static_block(
                PromptSectionId::new("memory_mechanics"),
                prompts::memory_mechanics_section(),
            ),
        ];

        if let Some(persona) = ctx.persona {
            let persona_text = render_persona_section(persona);
            if !persona_text.trim().is_empty() {
                blocks.push(PromptBlock::dynamic_block(
                    PromptSectionId::new("persona"),
                    persona_text,
                ));
            }
        }

        match ctx.mode {
            PromptMode::Daily => {
                let daily = prompts::get_prompt_fragment("daily");
                if !daily.trim().is_empty() {
                    let has_persona_memory =
                        ctx.persona.is_some_and(|p| !p.memory_hints.is_empty());
                    let daily = if has_persona_memory {
                        strip_memory_section(&daily)
                    } else {
                        daily
                    };
                    if !daily.trim().is_empty() {
                        blocks.push(PromptBlock::dynamic_block(
                            PromptSectionId::new("daily"),
                            daily,
                        ));
                    }
                }
            }
            PromptMode::BrowserAgent => {
                let browser = prompts::get_prompt_fragment("browser_agent");
                if !browser.trim().is_empty() {
                    blocks.push(PromptBlock::dynamic_block(
                        PromptSectionId::new("browser_agent"),
                        browser,
                    ));
                }
            }
        }

        PromptAssembly::new(blocks)
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

fn strip_memory_section(prompt: &str) -> String {
    let mut result = Vec::new();
    let mut skip = false;

    for line in prompt.lines() {
        if line.contains("记忆管理") && line.contains("白名单") {
            skip = true;
            continue;
        }

        if skip {
            if !line.trim().is_empty() && !line.trim().starts_with("- ") {
                skip = false;
            } else {
                continue;
            }
        }

        result.push(line);
    }

    result.join("\n")
}

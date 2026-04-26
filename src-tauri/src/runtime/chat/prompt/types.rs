use serde::{Deserialize, Serialize};

pub const SYSTEM_PROMPT_DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PromptSectionId(String);

impl PromptSectionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptCachePolicy {
    StaticPrefix,
    SessionDynamic,
    Volatile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptBlock {
    pub section_id: PromptSectionId,
    pub text: String,
    pub cache_policy: PromptCachePolicy,
    pub cache_break_reason: Option<String>,
}

impl PromptBlock {
    pub fn static_block(section_id: PromptSectionId, text: impl Into<String>) -> Self {
        Self {
            section_id,
            text: text.into(),
            cache_policy: PromptCachePolicy::StaticPrefix,
            cache_break_reason: None,
        }
    }

    pub fn dynamic_block(section_id: PromptSectionId, text: impl Into<String>) -> Self {
        Self {
            section_id,
            text: text.into(),
            cache_policy: PromptCachePolicy::SessionDynamic,
            cache_break_reason: None,
        }
    }

    pub fn volatile_block(
        section_id: PromptSectionId,
        text: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            section_id,
            text: text.into(),
            cache_policy: PromptCachePolicy::Volatile,
            cache_break_reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSystemView {
    pub blocks: Vec<PromptBlock>,
}

impl PromptSystemView {
    pub fn flatten(&self) -> String {
        self.blocks
            .iter()
            .map(|block| block.text.as_str())
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptAssembly {
    blocks: Vec<PromptBlock>,
}

impl PromptAssembly {
    pub fn new(blocks: Vec<PromptBlock>) -> Self {
        Self { blocks }
    }

    pub fn blocks(&self) -> &[PromptBlock] {
        &self.blocks
    }

    pub fn to_system_view(&self) -> PromptSystemView {
        PromptSystemView {
            blocks: self.blocks.clone(),
        }
    }

    pub fn flatten(&self) -> String {
        self.to_system_view().flatten()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnPromptSnapshot {
    assembly: PromptAssembly,
    initial_user_reminders: Vec<serde_json::Value>,
}

impl TurnPromptSnapshot {
    pub fn new(assembly: PromptAssembly, initial_user_reminders: Vec<serde_json::Value>) -> Self {
        Self {
            assembly,
            initial_user_reminders,
        }
    }

    pub fn assembly(&self) -> &PromptAssembly {
        &self.assembly
    }

    pub fn system_view(&self) -> PromptSystemView {
        self.assembly.to_system_view()
    }

    pub fn compat_system_prompt(&self) -> String {
        self.assembly.flatten()
    }

    pub fn initial_user_reminders(&self) -> &[serde_json::Value] {
        &self.initial_user_reminders
    }
}

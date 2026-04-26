use super::{PromptCachePolicy, PromptSectionId};

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

use serde::Serialize;

use super::{PromptAssembly, PromptCachePolicy};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PromptSectionDiagnostic {
    pub section_id: String,
    pub chars: usize,
    pub cache_policy: String,
    pub cache_break_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PromptDiagnostics {
    pub total_chars: usize,
    pub sections: Vec<PromptSectionDiagnostic>,
}

impl PromptDiagnostics {
    pub fn from_assembly(assembly: &PromptAssembly) -> Self {
        let sections = assembly
            .blocks()
            .iter()
            .map(|block| PromptSectionDiagnostic {
                section_id: block.section_id.as_str().to_string(),
                chars: block.text.chars().count(),
                cache_policy: cache_policy_label(block.cache_policy).to_string(),
                cache_break_reason: block.cache_break_reason.clone(),
            })
            .collect::<Vec<_>>();
        let total_chars = sections.iter().map(|section| section.chars).sum();
        Self {
            total_chars,
            sections,
        }
    }
}

fn cache_policy_label(policy: PromptCachePolicy) -> &'static str {
    match policy {
        PromptCachePolicy::StaticPrefix => "static_prefix",
        PromptCachePolicy::SessionDynamic => "session_dynamic",
        PromptCachePolicy::Volatile => "volatile",
    }
}

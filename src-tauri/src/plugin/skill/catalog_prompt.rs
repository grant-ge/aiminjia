use super::types::DiskSkill;

const SKILL_BUDGET_CONTEXT_PERCENT: f64 = 0.01;
const CHARS_PER_TOKEN: usize = 4;
const DEFAULT_CHAR_BUDGET: usize = 8_000;
const MAX_LISTING_DESC_CHARS: usize = 250;

pub fn format_skill_catalog_with_budget(skills: &[DiskSkill], context_window_tokens: usize) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut budget = if context_window_tokens == 0 {
        DEFAULT_CHAR_BUDGET
    } else {
        ((context_window_tokens as f64) * SKILL_BUDGET_CONTEXT_PERCENT) as usize * CHARS_PER_TOKEN
    };
    budget = budget.max(512);

    let mut lines = Vec::new();
    for skill in skills {
        let mut desc = skill.frontmatter.description.clone();
        if let Some(when) = &skill.frontmatter.when_to_use {
            desc.push(' ');
            desc.push_str(when);
        }
        if desc.chars().count() > MAX_LISTING_DESC_CHARS {
            desc = desc.chars().take(MAX_LISTING_DESC_CHARS).collect::<String>();
            desc.push('…');
        }
        lines.push(format!("- `{}` — {}", skill.id, desc));
    }

    let header = "The following skills are available for use with the Skill tool:\n\n";
    let footer = "\nUse Skill({ skill_id: \"<id>\" }) to load detailed instructions when a skill matches the user request.";
    let mut content = format!("{}{}{}", header, lines.join("\n"), footer);
    if content.len() > budget {
        content = format!(
            "{}{}{}",
            header,
            skills
                .iter()
                .map(|s| format!("- `{}`", s.id))
                .collect::<Vec<_>>()
                .join("\n"),
            footer
        );
    }
    content
}

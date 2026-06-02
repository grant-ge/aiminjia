use super::types::DiskSkill;

// budget 比例从 1% 提到 3%：200K context window 下 budget 从 8K → 24K，
// 配合 MAX_LISTING_DESC_CHARS 收紧到 80，可容纳 ~300 个 skill × 80 chars 全描述。
// 原 1% × 250 chars 在 34+ skill 时必然触发 fallback 降级（只剩 skill_id 无 description），
// 直接导致 LLM 看不到触发场景描述 → 自然语言无法触发新装技能。
const SKILL_BUDGET_CONTEXT_PERCENT: f64 = 0.03;
const CHARS_PER_TOKEN: usize = 4;
const DEFAULT_CHAR_BUDGET: usize = 8_000;
const MAX_LISTING_DESC_CHARS: usize = 80;

pub fn format_skill_catalog_with_budget(
    skills: &[DiskSkill],
    context_window_tokens: usize,
) -> String {
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
            desc = desc
                .chars()
                .take(MAX_LISTING_DESC_CHARS)
                .collect::<String>();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillSource};
    use std::path::PathBuf;

    fn mk_skill(id: &str, desc: &str) -> DiskSkill {
        let mut fm = SkillFrontmatter::default();
        fm.name = id.to_string();
        fm.description = desc.to_string();
        DiskSkill {
            id: id.to_string(),
            root: PathBuf::from(format!("/tmp/skills/{}", id)),
            frontmatter: fm,
            body: String::new(),
            source: SkillSource::User,
        }
    }

    #[test]
    fn description_kept_for_realistic_skill_count() {
        // 模拟 ~40 个 skill × ~80 chars desc，验证 description 不被砍掉
        let skills: Vec<DiskSkill> = (0..40)
            .map(|i| {
                mk_skill(
                    &format!("skill-{:02}", i),
                    "用户说「触发这个技能」时使用本技能，覆盖一个具体的工作场景，包含详细操作指令",
                )
            })
            .collect();
        // 200K context window → budget = 200_000 × 0.03 × 4 = 24_000 chars
        let out = format_skill_catalog_with_budget(&skills, 200_000);
        // 每个 skill 都应该带 desc：含 "- `skill-XX` — 用户说"
        for i in 0..40 {
            let id = format!("skill-{:02}", i);
            let line = format!("- `{}` — 用户说", id);
            assert!(
                out.contains(&line),
                "skill {} 的 description 被错误地砍掉了\nout:\n{}",
                id,
                out
            );
        }
    }

    #[test]
    fn fallback_to_ids_only_when_truly_overbudget() {
        // 拼一个一定爆 budget 的：大量 skill 都顶满 MAX_LISTING_DESC_CHARS
        let big_desc = "x".repeat(MAX_LISTING_DESC_CHARS + 10);
        let skills: Vec<DiskSkill> = (0..500)
            .map(|i| mk_skill(&format!("s{:03}", i), &big_desc))
            .collect();
        let out = format_skill_catalog_with_budget(&skills, 200_000);
        // 降级路径里每行只有 `- \`id\``，没有 " — "
        assert!(
            !out.contains(" — "),
            "expected fallback (ids only) but got full content; len={}",
            out.len()
        );
        // 但所有 skill_id 都应该在
        for i in 0..500 {
            assert!(out.contains(&format!("- `s{:03}`", i)), "missing s{:03}", i);
        }
    }

    #[test]
    fn empty_skills_returns_empty_string() {
        assert_eq!(format_skill_catalog_with_budget(&[], 200_000), "");
    }
}

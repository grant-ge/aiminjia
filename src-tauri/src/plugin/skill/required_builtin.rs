#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequiredBuiltinSkill {
    pub id: &'static str,
    pub display_alias: &'static str,
    pub default_enabled: bool,
}

pub const REQUIRED_BUILTIN_SKILLS: &[RequiredBuiltinSkill] = &[
    RequiredBuiltinSkill {
        id: "create-skill",
        display_alias: "create-skill",
        default_enabled: true,
    },
    RequiredBuiltinSkill {
        id: "skill-creator",
        display_alias: "create-skill",
        default_enabled: true,
    },
    RequiredBuiltinSkill {
        id: "dws",
        display_alias: "dws",
        default_enabled: true,
    },
    RequiredBuiltinSkill {
        id: "dingtalk-workspace",
        display_alias: "dws",
        default_enabled: true,
    },
    RequiredBuiltinSkill {
        id: "browser",
        display_alias: "browser",
        default_enabled: true,
    },
    RequiredBuiltinSkill {
        id: "find-skills",
        display_alias: "find-skills",
        default_enabled: true,
    },
];

pub fn is_required_builtin_skill(id: &str) -> bool {
    REQUIRED_BUILTIN_SKILLS.iter().any(|skill| skill.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_builtin_allowlist_contains_only_confirmed_core_skills() {
        let ids = REQUIRED_BUILTIN_SKILLS
            .iter()
            .map(|skill| skill.id)
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "create-skill",
                "skill-creator",
                "dws",
                "dingtalk-workspace",
                "browser",
                "find-skills"
            ]
        );
        assert!(REQUIRED_BUILTIN_SKILLS
            .iter()
            .all(|skill| skill.default_enabled));
    }

    #[test]
    fn required_builtin_check_rejects_market_skills() {
        assert!(is_required_builtin_skill("dingtalk-workspace"));
        assert!(is_required_builtin_skill("create-skill"));
        assert!(is_required_builtin_skill("browser"));
        assert!(is_required_builtin_skill("find-skills"));
        assert!(!is_required_builtin_skill("market-only"));
    }
}

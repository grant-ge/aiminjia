use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalizedText {
    pub name: String,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub composer_placeholder: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertPersonaSnapshot {
    pub stable_name: String,
    #[serde(default)]
    pub emoji: String,
    #[serde(default)]
    pub display_i18n: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub prompt_i18n: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpertTeamSnapshot {
    pub team_id: String,
    pub version: String,
    pub facilitation_style: String,
    pub display_i18n: BTreeMap<String, LocalizedText>,
    #[serde(default)]
    pub experts: Vec<ExpertPersonaSnapshot>,
    #[serde(default)]
    pub director_prompt_i18n: BTreeMap<String, BTreeMap<String, String>>,
}

const BOOTSTRAP_JSON: &str = include_str!("expert_teams_bootstrap.json");

pub fn bootstrap_teams() -> Result<Vec<ExpertTeamSnapshot>> {
    serde_json::from_str(BOOTSTRAP_JSON).context("parse expert team bootstrap JSON")
}

pub fn cache_path(cache_dir: &Path, team_id: &str, version: &str) -> PathBuf {
    cache_dir.join(team_id).join(format!("{version}.json"))
}

pub fn write_cache(cache_dir: &Path, snapshot: &ExpertTeamSnapshot) -> Result<PathBuf> {
    let dir = cache_dir.join(&snapshot.team_id);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", snapshot.version));
    fs::write(&path, serde_json::to_vec_pretty(snapshot)?)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_teams_parse() {
        let teams = bootstrap_teams().expect("bootstrap teams should parse");
        assert_eq!(teams.len(), 8);

        assert!(teams.iter().any(|team| team.team_id == "strategy"));
        for team in &teams {
            assert!(
                team.display_i18n.contains_key("zh-CN"),
                "{} should have zh-CN display",
                team.team_id
            );
            assert!(
                team.display_i18n.contains_key("en-US"),
                "{} should have en-US display",
                team.team_id
            );
        }

        let roundtable = teams
            .iter()
            .find(|team| team.team_id == "roundtable")
            .expect("roundtable team should exist");
        assert!(roundtable.experts.is_empty());

        let debate = teams
            .iter()
            .find(|team| team.team_id == "debate")
            .expect("debate team should exist");
        assert_eq!(debate.facilitation_style, "debate");
    }

    #[test]
    fn cache_path_is_team_and_version_scoped() {
        assert_eq!(
            cache_path(Path::new("/tmp/cache"), "strategy", "1.0.0"),
            PathBuf::from("/tmp/cache/strategy/1.0.0.json")
        );
    }
}

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

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|extension| extension.to_str()).unwrap_or("tmp")
    ));
    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("renaming {}", tmp.display()))?;
    Ok(())
}

pub fn bootstrap_teams() -> Result<Vec<ExpertTeamSnapshot>> {
    serde_json::from_str(BOOTSTRAP_JSON).context("parse expert team bootstrap JSON")
}

pub fn cache_path(cache_dir: &Path, team_id: &str, version: &str) -> PathBuf {
    cache_dir.join(team_id).join(format!("{version}.json"))
}

pub fn write_cache(cache_dir: &Path, snapshot: &ExpertTeamSnapshot) -> Result<PathBuf> {
    let dir = cache_dir.join(&snapshot.team_id);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{}.json", snapshot.version));
    let json = serde_json::to_vec_pretty(snapshot)?;
    write_atomic(&path, &json)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

            for locale in ["zh-CN", "en-US"] {
                let template = team
                    .director_prompt_i18n
                    .get(locale)
                    .and_then(|entry| entry.get("template"))
                    .unwrap_or_else(|| {
                        panic!("{} should have {locale} director template", team.team_id)
                    });
                assert!(
                    template.contains("{teamName}"),
                    "{} {locale} template should keep teamName placeholder",
                    team.team_id
                );
                assert!(
                    template.contains("{topic}"),
                    "{} {locale} template should keep topic placeholder",
                    team.team_id
                );
                assert!(
                    template.contains("TeamCreate"),
                    "{} {locale} template should instruct TeamCreate",
                    team.team_id
                );
                assert!(
                    template.contains("Agent"),
                    "{} {locale} template should instruct Agent spawn",
                    team.team_id
                );

                if team.facilitation_style == "open" {
                    assert!(
                        template.contains("动态专家")
                            || template.contains("3-5")
                            || template.contains("dynamic experts")
                            || template.contains("3 to 5"),
                        "{} {locale} open template should describe dynamic expert selection",
                        team.team_id
                    );
                } else {
                    assert!(
                        template.contains("{roster}"),
                        "{} {locale} template should keep roster placeholder",
                        team.team_id
                    );
                }
            }
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

    #[test]
    fn write_cache_round_trips_snapshot() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let snapshot = bootstrap_teams()
            .expect("bootstrap teams should parse")
            .into_iter()
            .find(|team| team.team_id == "strategy")
            .expect("strategy team should exist");

        let path = write_cache(tmp.path(), &snapshot).expect("cache write should succeed");

        assert!(path.exists());
        let content = fs::read_to_string(&path).expect("cache file should be readable");
        let parsed: ExpertTeamSnapshot =
            serde_json::from_str(&content).expect("cache file should parse");
        assert_eq!(parsed, snapshot);
    }
}

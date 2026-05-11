//! TEAM_CONTEXT attachment (P2.3b) — boot-time `<system-reminder>` user
//! message injected into a Teammate's LLM history on its very first turn.
//!
//! Different from TEAMMATE_ADDENDUM (P2.3):
//! - TEAMMATE_ADDENDUM is appended to the system prompt every turn.
//! - team_context is a single user message inserted once at boot.
//!
//! Mirrors claude-code-best `getTeamContextAttachment`.  Renders into a
//! plain `String` so caller decides how to wrap it (inbox push, history seed,
//! transcript line, etc).

use std::path::Path;

pub const TEAM_CONTEXT_TEMPLATE: &str = r#"<system-reminder>
# 团队协作

你是团队 "{team_name}" 的成员。

**你的身份**：
- 名字: {agent_name}

**团队资源**：
- 团队配置: {team_json_path}
- 任务列表: {tasks_dir_path}

**团队负责人**：Lead 的名字是 "team-lead"。把进度和完成情况发给 Lead。

读取团队配置文件了解队友名单。定期检查任务列表。需要分工时创建新任务，完成后标记任务为 resolved。

**重要**：始终用名字（如 "team-lead", "researcher", "analyzer"）称呼队友，绝不用 UUID。发消息时直接用名字：

```json
{
  "to": "team-lead",
  "message": "你的消息内容",
  "summary": "5-10 字预览"
}
```
</system-reminder>"#;

/// Render the team_context attachment for a Teammate's first turn.
///
/// `team_json_path` points at `<aijia_home>/users/{scope}/conversations/{conv}/team.json`
/// (the Team snapshot written by P1.1b) and `tasks_dir` at the V2 tasks root
/// `<aijia_home>/users/{scope}/conversations/{conv}/tasks/`.
pub fn render(
    team_name: &str,
    agent_name: &str,
    team_json_path: &Path,
    tasks_dir: &Path,
) -> String {
    TEAM_CONTEXT_TEMPLATE
        .replace("{team_name}", team_name)
        .replace("{agent_name}", agent_name)
        .replace("{team_json_path}", &team_json_path.display().to_string())
        .replace("{tasks_dir_path}", &tasks_dir.display().to_string())
}

/// Convenience derivation: given the conversation root directory, build
/// `team.json` and `tasks/` paths in the same place P1.1b / P1.5 write them.
/// `conv_dir` is `<aijia_home>/users/{scope}/conversations/{conv_id}`.
pub fn render_for_conv_dir(team_name: &str, agent_name: &str, conv_dir: &Path) -> String {
    let team_json = conv_dir.join("team.json");
    let tasks = conv_dir.join("tasks");
    render(team_name, agent_name, &team_json, &tasks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn render_substitutes_all_placeholders() {
        let team_json = PathBuf::from("/home/u/.renlijia/users/u1/conversations/c1/team.json");
        let tasks = PathBuf::from("/home/u/.renlijia/users/u1/conversations/c1/tasks");
        let out = render("research-team", "alice", &team_json, &tasks);

        for needle in [
            "<system-reminder>",
            "research-team",
            "alice",
            team_json.display().to_string().as_str(),
            tasks.display().to_string().as_str(),
            "team-lead",
        ] {
            assert!(out.contains(needle), "missing {needle:?}; got:\n{out}");
        }
        assert!(!out.contains("{team_name}"));
        assert!(!out.contains("{agent_name}"));
        assert!(!out.contains("{team_json_path}"));
        assert!(!out.contains("{tasks_dir_path}"));
    }

    #[test]
    fn render_for_conv_dir_derives_canonical_subpaths() {
        let conv_dir = PathBuf::from("/data/users/scope/conversations/conv-7");
        let out = render_for_conv_dir("t", "n", &conv_dir);
        assert!(out.contains("/data/users/scope/conversations/conv-7/team.json"));
        assert!(out.contains("/data/users/scope/conversations/conv-7/tasks"));
    }

    #[test]
    fn rendered_output_advises_against_uuids() {
        let out = render("t", "n", Path::new("/a"), Path::new("/b"));
        assert!(
            out.contains("绝不用 UUID") || out.contains("不要 UUID") || out.contains("UUID"),
            "should explicitly warn against UUID addressing"
        );
    }
}

use crate::runtime::agent::definition::{
    AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
};

pub fn explore_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "explore".into(),
        description: "只读探索：搜索/读取文件，不修改".into(),
        allowed_tools: vec![
            "read_workspace_file".into(),
            "grep_content".into(),
            "search_files".into(),
            "list_directory".into(),
            "web_search".into(),
        ],
        disallowed_tools: vec![],
        max_iterations: 100,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(
            "You are a read-only explorer. Search and read files to answer questions. Never modify anything.".into()
        ),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::AutoDeny,
        background_default: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::tool_whitelist::resolve_agent_tools;

    /// M2.2 / F6 regression: explore.allowed_tools must use the canonical tool
    /// names from runtime/tools/catalog.rs. Earlier names like "read_file" /
    /// "grep" / "glob" don't exist in the catalog, so resolve_agent_tools
    /// would silently strip them and leave explore with only list_directory +
    /// web_search.
    #[test]
    fn explore_tools_match_canonical_catalog_names() {
        let def = explore_agent_definition();
        // Names actually exposed by the runtime tool catalog.
        let available: Vec<String> = [
            "read_workspace_file",
            "grep_content",
            "search_files",
            "list_directory",
            "web_search",
            "write_file", // not requested → must not appear
            "bash",       // not requested → must not appear
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let resolved = resolve_agent_tools(
            &def.allowed_tools,
            &def.disallowed_tools,
            &available,
            false, // is_async
            false, // allow_recursive_spawn
        );

        for required in [
            "read_workspace_file",
            "grep_content",
            "search_files",
            "list_directory",
            "web_search",
        ] {
            assert!(
                resolved.contains(&required.to_string()),
                "explore must retain {required} after whitelist; got {resolved:?}"
            );
        }
        assert!(
            !resolved.contains(&"write_file".to_string()),
            "explore must not gain write_file"
        );
        assert!(
            !resolved.contains(&"bash".to_string()),
            "explore must not gain bash"
        );
    }
}

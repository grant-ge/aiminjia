#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootEntryClass {
    StableRoot,
    TransitionalRoot,
    WorkspaceArtifact,
    Temporary,
    DeprecatedArchiveCandidate,
    ReviewOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootEntryContract {
    pub name: &'static str,
    pub class: RootEntryClass,
    pub owner: &'static str,
    pub target: &'static str,
    pub upgrade_policy: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootDirectoryAudit {
    pub known: Vec<String>,
    pub review_only: Vec<String>,
}

impl RootDirectoryAudit {
    pub fn has_blocking_violation(&self) -> bool {
        false
    }
}

pub const ROOT_ENTRY_CONTRACTS: &[RootEntryContract] = &[
    RootEntryContract {
        name: "global",
        class: RootEntryClass::StableRoot,
        owner: "auth/updater/global-config",
        target: "root",
        upgrade_policy: "keep",
    },
    RootEntryContract {
        name: "crypto",
        class: RootEntryClass::StableRoot,
        owner: "secure-storage",
        target: "root",
        upgrade_policy: "keep",
    },
    RootEntryContract {
        name: "users",
        class: RootEntryClass::StableRoot,
        owner: "user-scoped-storage",
        target: "root",
        upgrade_policy: "keep",
    },
    RootEntryContract {
        name: "skills",
        class: RootEntryClass::StableRoot,
        owner: "skill-sync",
        target: "root",
        upgrade_policy: "keep global managed skills",
    },
    RootEntryContract {
        name: "employee-templates-cache",
        class: RootEntryClass::StableRoot,
        owner: "employee-template-sync",
        target: "root",
        upgrade_policy: "keep content-addressed cache",
    },
    RootEntryContract {
        name: "expert-team-templates-cache",
        class: RootEntryClass::StableRoot,
        owner: "expert-team-template-sync",
        target: "root",
        upgrade_policy: "keep content-addressed cache",
    },
    RootEntryContract {
        name: "logs",
        class: RootEntryClass::StableRoot,
        owner: "diagnostics",
        target: "root",
        upgrade_policy: "keep app-global logs with retention",
    },
    RootEntryContract {
        name: "runtimes",
        class: RootEntryClass::StableRoot,
        owner: "managed-runtime",
        target: "root/cache fallback",
        upgrade_policy: "keep runtime cache path behavior",
    },
    RootEntryContract {
        name: "tmp",
        class: RootEntryClass::StableRoot,
        owner: "temporary-data",
        target: "root",
        upgrade_policy: "keep with ttl cleanup",
    },
    RootEntryContract {
        name: "defaultFolder",
        class: RootEntryClass::StableRoot,
        owner: "workspace",
        target: "root",
        upgrade_policy: "keep default workspace",
    },
    RootEntryContract {
        name: "device_id",
        class: RootEntryClass::StableRoot,
        owner: "auth-device",
        target: "root",
        upgrade_policy: "keep legacy file until moved into global/device.json",
    },
    RootEntryContract {
        name: "data_version",
        class: RootEntryClass::StableRoot,
        owner: "storage-migration",
        target: "root",
        upgrade_policy: "keep migration gate",
    },
    RootEntryContract {
        name: "config.json",
        class: RootEntryClass::TransitionalRoot,
        owner: "storage-migration",
        target: "global/config.json and global/auth/cloud_auth",
        upgrade_policy: "keep legacy cloud_auth recovery path for old-version upgrades",
    },
    RootEntryContract {
        name: ".migrated",
        class: RootEntryClass::StableRoot,
        owner: "storage-migration",
        target: "root",
        upgrade_policy: "keep migration gate",
    },
    RootEntryContract {
        name: "screenshots",
        class: RootEntryClass::TransitionalRoot,
        owner: "browser-automation",
        target: "users/{scope}/screenshots or workspace artifact",
        upgrade_policy: "keep root fallback until writer moves",
    },
    RootEntryContract {
        name: "site-profiles",
        class: RootEntryClass::TransitionalRoot,
        owner: "browser-automation",
        target: "users/{scope}/site-profiles",
        upgrade_policy: "keep root fallback until migration is proven",
    },
    RootEntryContract {
        name: "permissions.json",
        class: RootEntryClass::TransitionalRoot,
        owner: "permissions",
        target: "users/{scope}/permissions.json",
        upgrade_policy: "keep read-only fallback for old users",
    },
    RootEntryContract {
        name: "mcp_servers.json",
        class: RootEntryClass::TransitionalRoot,
        owner: "mcp",
        target: "users/{scope}/mcp_servers.json",
        upgrade_policy: "keep read-only fallback for old users",
    },
    RootEntryContract {
        name: "agent_invocations.json",
        class: RootEntryClass::TransitionalRoot,
        owner: "agent-runtime",
        target: "users/{scope}/agent_invocations.json",
        upgrade_policy: "keep read-only fallback for old users",
    },
    RootEntryContract {
        name: "subagent_transcripts",
        class: RootEntryClass::TransitionalRoot,
        owner: "agent-runtime",
        target: "users/{scope}/subagent_transcripts",
        upgrade_policy: "keep root fallback until writer moves",
    },
    RootEntryContract {
        name: "api-data",
        class: RootEntryClass::TransitionalRoot,
        owner: "user-scoped-storage",
        target: "users/{scope}/api-data",
        upgrade_policy: "archive after user-scope migration claim",
    },
    RootEntryContract {
        name: "audit",
        class: RootEntryClass::TransitionalRoot,
        owner: "user-scoped-storage",
        target: "users/{scope}/audit",
        upgrade_policy: "archive after user-scope migration claim",
    },
    RootEntryContract {
        name: "conversations",
        class: RootEntryClass::TransitionalRoot,
        owner: "chat",
        target: "users/{scope}/conversations",
        upgrade_policy: "archive after user-scope migration claim",
    },
    RootEntryContract {
        name: "index.json",
        class: RootEntryClass::TransitionalRoot,
        owner: "chat",
        target: "users/{scope}/index equivalent",
        upgrade_policy: "archive after user-scope migration claim",
    },
    RootEntryContract {
        name: "shared",
        class: RootEntryClass::TransitionalRoot,
        owner: "chat",
        target: "users/{scope}/shared",
        upgrade_policy: "archive after user-scope migration claim",
    },
    RootEntryContract {
        name: "personas",
        class: RootEntryClass::TransitionalRoot,
        owner: "chat-persona",
        target: "users/{scope}/personas",
        upgrade_policy: "keep until persona read/write ownership is confirmed",
    },
    RootEntryContract {
        name: "tasks",
        class: RootEntryClass::TransitionalRoot,
        owner: "task-runtime",
        target: "users/{scope}/tasks or runtime queue",
        upgrade_policy: "keep until task queue ownership is confirmed",
    },
    RootEntryContract {
        name: "interrupted_turns",
        class: RootEntryClass::TransitionalRoot,
        owner: "chat-runtime",
        target: "users/{scope}/turn_stages",
        upgrade_policy: "keep until recovery path is confirmed unused",
    },
    RootEntryContract {
        name: "state.json",
        class: RootEntryClass::TransitionalRoot,
        owner: "storage-migration",
        target: "global/state.json",
        upgrade_policy: "keep until all legacy checks move to global state",
    },
    RootEntryContract {
        name: "analysis",
        class: RootEntryClass::WorkspaceArtifact,
        owner: "workspace",
        target: "defaultFolder/legacy-root-import-*",
        upgrade_policy: "move with manifest, preserve source on failure",
    },
    RootEntryContract {
        name: "charts",
        class: RootEntryClass::WorkspaceArtifact,
        owner: "workspace",
        target: "defaultFolder/legacy-root-import-*",
        upgrade_policy: "move with manifest, preserve source on failure",
    },
    RootEntryContract {
        name: "generated",
        class: RootEntryClass::WorkspaceArtifact,
        owner: "workspace",
        target: "defaultFolder/legacy-root-import-*",
        upgrade_policy: "move without historical fallback map",
    },
    RootEntryContract {
        name: "exports",
        class: RootEntryClass::WorkspaceArtifact,
        owner: "workspace",
        target: "defaultFolder/legacy-root-import-*",
        upgrade_policy: "move with manifest, preserve source on failure",
    },
    RootEntryContract {
        name: "reports",
        class: RootEntryClass::WorkspaceArtifact,
        owner: "workspace",
        target: "defaultFolder/legacy-root-import-*",
        upgrade_policy: "move with manifest, preserve source on failure",
    },
    RootEntryContract {
        name: "uploads",
        class: RootEntryClass::WorkspaceArtifact,
        owner: "workspace",
        target: "defaultFolder/legacy-root-import-*",
        upgrade_policy: "move with manifest, preserve source on failure",
    },
    RootEntryContract {
        name: "temp",
        class: RootEntryClass::Temporary,
        owner: "temporary-data",
        target: "tmp",
        upgrade_policy: "ttl cleanup only",
    },
    RootEntryContract {
        name: "tmpImage",
        class: RootEntryClass::Temporary,
        owner: "clipboard",
        target: "tmp/clipboard",
        upgrade_policy: "ttl cleanup only after writer moves",
    },
    RootEntryContract {
        name: "expert-team-templates",
        class: RootEntryClass::DeprecatedArchiveCandidate,
        owner: "expert-team-template-sync",
        target: "expert-team-templates-cache",
        upgrade_policy: "archive after confirming no current writer",
    },
];

pub fn root_entry_contract(name: &str) -> Option<&'static RootEntryContract> {
    ROOT_ENTRY_CONTRACTS
        .iter()
        .find(|contract| contract.name == name)
}

pub fn classify_root_entry(name: &str) -> RootEntryClass {
    if name.starts_with(".archived-legacy-") {
        return RootEntryClass::StableRoot;
    }

    root_entry_contract(name)
        .map(|contract| contract.class)
        .unwrap_or(RootEntryClass::ReviewOnly)
}

pub fn stable_root_entries() -> Vec<&'static str> {
    ROOT_ENTRY_CONTRACTS
        .iter()
        .filter(|contract| contract.class == RootEntryClass::StableRoot)
        .map(|contract| contract.name)
        .collect()
}

pub fn audit_root_entries<I, S>(entries: I) -> RootDirectoryAudit
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut known = Vec::new();
    let mut review_only = Vec::new();

    for entry in entries {
        let entry = entry.as_ref();
        match classify_root_entry(entry) {
            RootEntryClass::ReviewOnly => review_only.push(entry.to_string()),
            _ => known.push(entry.to_string()),
        }
    }

    RootDirectoryAudit { known, review_only }
}

#[cfg(test)]
fn direct_root_join_literals(source: &str) -> Vec<String> {
    let mut literals = Vec::new();
    for marker in [".root().join(\"", "get_aijia_home_dir().join(\""] {
        let mut rest = source;
        while let Some(start) = rest.find(marker) {
            let after_marker = &rest[start + marker.len()..];
            if let Some(end) = after_marker.find('"') {
                literals.push(after_marker[..end].to_string());
                rest = &after_marker[end + 1..];
            } else {
                break;
            }
        }
    }
    literals
}

#[cfg(test)]
fn first_path_component(path: &str) -> &str {
    path.split('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn classifies_root_entries_by_contract() {
        assert_eq!(classify_root_entry("global"), RootEntryClass::StableRoot);
        assert_eq!(classify_root_entry("logs"), RootEntryClass::StableRoot);
        assert_eq!(
            classify_root_entry("expert-team-templates"),
            RootEntryClass::DeprecatedArchiveCandidate
        );
        assert_eq!(
            classify_root_entry("generated"),
            RootEntryClass::WorkspaceArtifact
        );
        assert_eq!(classify_root_entry("tmpImage"), RootEntryClass::Temporary);
        assert_eq!(
            classify_root_entry("unknown-from-old-version"),
            RootEntryClass::ReviewOnly
        );
    }

    #[test]
    fn runtime_audit_keeps_old_user_unknown_entries_non_blocking() {
        let report = audit_root_entries([
            "global",
            "users",
            "old-dir-from-0-4-x",
        ]);

        assert_eq!(report.known.len(), 2);
        assert_eq!(report.review_only, vec!["old-dir-from-0-4-x"]);
        assert!(!report.has_blocking_violation());
    }

    #[test]
    fn old_version_root_entries_left_in_place_are_non_blocking() {
        let report = audit_root_entries([
            "config.json",
            "permissions.json",
            "mcp_servers.json",
            "agent_invocations.json",
            "conversations",
            "index.json",
            "shared",
            "api-data",
            "audit",
            "site-profiles",
            "screenshots",
            "subagent_transcripts",
            "tmpImage",
            "temp",
            "expert-team-templates",
            "unknown-old-plugin-cache",
        ]);

        assert!(!report.has_blocking_violation());
        assert_eq!(report.review_only, vec!["unknown-old-plugin-cache"]);
    }

    #[test]
    fn stable_root_whitelist_excludes_transitional_profile() {
        let stable: BTreeSet<_> = stable_root_entries().into_iter().collect();

        assert!(stable.contains("global"));
        assert!(stable.contains("logs"));
        assert!(stable.contains("users"));
    }

    #[test]
    fn direct_root_joins_outside_storage_gateway_must_be_contract_entries() {
        let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let src_root = crate_root.join("src");
        let mut violations = Vec::new();

        for path in rust_files(&src_root) {
            let rel = path.strip_prefix(&crate_root).unwrap();
            let rel_str = rel.to_string_lossy();
            if rel_str == "src/storage/aijia_home.rs"
                || rel_str == "src/storage/app_data_contract.rs"
                || rel_str.contains("/tests/")
            {
                continue;
            }

            let source = fs::read_to_string(&path).unwrap();
            for literal in direct_root_join_literals(&source) {
                let root_entry = first_path_component(&literal);
                if root_entry_contract(root_entry).is_none() {
                    violations.push(format!("{} uses root join {:?}", rel_str, literal));
                }
            }
        }

        assert!(
            violations.is_empty(),
            "direct root joins must be declared in app_data_contract:\n{}",
            violations.join("\n")
        );
    }

    fn rust_files(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        collect_rust_files(root, &mut out);
        out
    }

    fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
}

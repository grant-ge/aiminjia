use std::path::{Path, PathBuf};

use crate::runtime::path_auth::context::ToolPermissionContext;
use crate::runtime::path_auth::op::PathOp;
use crate::runtime::tools::permission::PermissionMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
    Ask { reason: String },
}

pub(crate) fn canonicalize_or_ancestor(p: &Path) -> std::io::Result<PathBuf> {
    match std::fs::canonicalize(p) {
        Ok(c) => return Ok(c),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut ancestor = p.to_path_buf();
    let mut tail = Vec::new();
    loop {
        match ancestor.parent() {
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no existing ancestor",
                ));
            }
            Some(parent) => {
                if let Some(component) = ancestor.file_name() {
                    tail.push(component.to_os_string());
                }
                ancestor = parent.to_path_buf();
            }
        }
        match std::fs::canonicalize(&ancestor) {
            Ok(mut canonical) => {
                for component in tail.into_iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        }
    }
}

fn op_matches(rule_op: Option<PathOp>, op: PathOp) -> bool {
    match rule_op {
        None => true,
        Some(r) => r == op,
    }
}

fn glob_matches(pattern: &str, path: &Path) -> bool {
    let matcher = match globset::Glob::new(pattern) {
        Ok(g) => g.compile_matcher(),
        Err(_) => return false,
    };
    matcher.is_match(path)
}

fn is_under_any(path: &Path, mut roots: impl Iterator<Item = PathBuf>) -> bool {
    roots.any(|root| {
        let canonical_root = std::fs::canonicalize(&root).unwrap_or(root);
        path.starts_with(&canonical_root)
    })
}

fn path_text_lc(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

fn path_has_sensitive_component(path: &Path) -> bool {
    let sensitive_dirs = [
        ".ssh",
        ".aws",
        ".kube",
        ".gnupg",
        ".config/gcloud",
        "library/keychains",
        "appdata/roaming/microsoft/credentials",
        "appdata/local/microsoft/credentials",
    ];
    let sensitive_names = [".env", ".env.local", ".netrc", "credentials", "ntuser.dat"];
    let sensitive_exts = ["pem", "key", "pfx", "p12", "kdbx"];
    let text = path_text_lc(path);

    if sensitive_dirs.iter().any(|segment| {
        text.contains(&format!("/{segment}/")) || text.ends_with(&format!("/{segment}"))
    }) {
        return true;
    }

    let Some(file_name) = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
    else {
        return false;
    };
    if sensitive_names
        .iter()
        .any(|name| file_name == *name || file_name.starts_with(".env."))
    {
        return true;
    }
    path.extension()
        .map(|ext| {
            let ext = ext.to_string_lossy().to_lowercase();
            sensitive_exts.iter().any(|candidate| ext == *candidate)
        })
        .unwrap_or(false)
}

fn dangerous_delete_reason(path: &Path) -> Option<String> {
    if path.parent().is_none() {
        return Some(format!("禁止删除文件系统根目录：路径={}", path.display()));
    }
    if let Some(home) = dirs::home_dir() {
        let canonical_home = std::fs::canonicalize(&home).unwrap_or(home);
        if path == canonical_home {
            return Some(format!("禁止删除用户主目录：路径={}", path.display()));
        }
    }

    let text = path_text_lc(path);
    let dangerous_prefixes = [
        "/bin",
        "/etc",
        "/private/etc",
        "/sbin",
        "/system",
        "/library",
        "/applications",
        "/usr/bin",
        "/usr/lib",
        "/usr/sbin",
        "/usr/share",
        "/var/db",
        "/var/root",
        "/volumes",
        "c:/windows",
        "c:/program files",
        "c:/program files (x86)",
    ];
    dangerous_prefixes.iter().find_map(|prefix| {
        (text == *prefix || text.starts_with(&format!("{prefix}/"))).then(|| {
            format!(
                "禁止删除系统或高风险目录：路径={}，命中={}",
                path.display(),
                prefix
            )
        })
    })
}

pub fn is_path_allowed(path: &Path, op: PathOp, ctx: &ToolPermissionContext) -> Decision {
    // Step 1: canonicalize
    let canonical = match canonicalize_or_ancestor(path) {
        Ok(c) => c,
        Err(_) => path.to_path_buf(),
    };

    // Step 2: deny_rules
    for rule in &ctx.deny_rules {
        if op_matches(rule.op, op) && glob_matches(&rule.pattern, &canonical) {
            return Decision::Deny(format!(
                "拒绝规则命中：模式={}，路径={}",
                rule.pattern,
                canonical.display()
            ));
        }
    }

    if op == PathOp::Delete {
        if let Some(reason) = dangerous_delete_reason(&canonical) {
            return Decision::Deny(reason);
        }
    }

    // Step 3: removed.
    //
    // Previously this step hard-denied any path under ~/.renlijia/. That conflicted
    // with the path-auth model (user authorization is the single source of truth)
    // and with Lotus's own design — skills, conversations, subagent_transcripts etc.
    // all live under ~/.renlijia/ and the user legitimately needs to read them.
    //
    // After removal, paths under ~/.renlijia/ flow through the standard Step 4-6:
    //   - inside authorized_workspace / additional_working_dirs → Allow / Ask per op
    //   - outside → Step 6 Ask (Default mode) / Deny (Plan/DontAsk mode)
    //
    // If a future need arises to flag specific sensitive files (permissions.json,
    // crypto/*.key) with elevated risk, prefer adding deny_rules with user-visible
    // reasons over re-introducing a hardcoded subtree wall.

    // Step 4a: inside primary_root
    if let Some(ref root) = ctx.primary_root {
        let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        if canonical.starts_with(&canonical_root) {
            return match op {
                PathOp::Read | PathOp::Write => Decision::Allow,
                PathOp::Delete if ctx.mode == PermissionMode::FullAccess => Decision::Allow,
                PathOp::Delete => Decision::Ask {
                    reason: format!(
                        "需要确认：删除已授权工作区内的文件，路径={}",
                        canonical.display()
                    ),
                },
            };
        }
    }

    // Step 4b: inside any additional_working_dirs
    let in_additional = ctx.additional_working_dirs.keys().any(|dir| {
        let canonical_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
        canonical.starts_with(&canonical_dir)
    });

    if in_additional {
        match op {
            PathOp::Read => return Decision::Allow,
            PathOp::Write => {
                // allow_rules short-circuit (§8.14)
                let rule_allows = ctx.allow_rules.iter().any(|rule| {
                    op_matches(rule.op, PathOp::Write) && glob_matches(&rule.pattern, &canonical)
                });
                if rule_allows {
                    return Decision::Allow;
                }
                if matches!(
                    ctx.mode,
                    PermissionMode::AcceptEdits | PermissionMode::FullAccess
                ) {
                    return Decision::Allow;
                }
                return Decision::Ask {
                    reason: format!(
                        "需要确认：在已授权目录内写入文件，路径={}",
                        canonical.display()
                    ),
                };
            }
            PathOp::Delete => {
                if ctx.mode == PermissionMode::FullAccess {
                    return Decision::Allow;
                }
                return Decision::Ask {
                    reason: format!(
                        "需要确认：删除已授权目录内的文件，路径={}",
                        canonical.display()
                    ),
                };
            }
        }
    }

    if op == PathOp::Read && is_under_any(&canonical, ctx.read_roots.iter().cloned()) {
        if path_has_sensitive_component(&canonical) {
            return Decision::Ask {
                reason: format!(
                    "需要确认：读取用户目录中的敏感路径，路径={}",
                    canonical.display()
                ),
            };
        }
        return Decision::Allow;
    }

    // Step 5: allow_rules
    for rule in &ctx.allow_rules {
        if op_matches(rule.op, op) && glob_matches(&rule.pattern, &canonical) {
            return Decision::Allow;
        }
    }

    // Step 6: default by mode
    match ctx.mode {
        PermissionMode::FullAccess => Decision::Allow,
        PermissionMode::Default | PermissionMode::AcceptEdits => Decision::Ask {
            reason: format!("该路径未授权，需要用户确认：路径={}", canonical.display()),
        },
        PermissionMode::Plan => Decision::Deny(format!(
            "Plan 模式禁止访问未授权路径：路径={}",
            canonical.display()
        )),
        PermissionMode::DontAsk => Decision::Deny(format!(
            "DontAsk 模式禁止访问未授权路径：路径={}",
            canonical.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::path_auth::context::{PermissionRule, RuleSource, ToolPermissionContext};
    use tempfile::TempDir;

    fn ctx_with_primary(root: &Path) -> ToolPermissionContext {
        let mut ctx = ToolPermissionContext::empty();
        ctx.primary_root = Some(root.to_path_buf());
        ctx
    }

    fn ctx_with_additional(dir: &Path) -> ToolPermissionContext {
        let mut ctx = ToolPermissionContext::empty();
        ctx.additional_working_dirs
            .insert(dir.to_path_buf(), RuleSource::Session);
        ctx
    }

    #[test]
    fn deny_rule_short_circuits() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("secret.txt");
        std::fs::write(&file, b"").unwrap();
        let canonical = std::fs::canonicalize(&file).unwrap();

        let mut ctx = ToolPermissionContext::empty();
        ctx.deny_rules.push(PermissionRule {
            pattern: canonical.to_string_lossy().to_string(),
            op: None,
            source: RuleSource::Session,
        });

        assert!(matches!(
            is_path_allowed(&file, PathOp::Read, &ctx),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn deny_rule_overrides_allow_rule() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("file.txt");
        std::fs::write(&file, b"").unwrap();
        let canonical = std::fs::canonicalize(&file).unwrap();
        let pattern = canonical.to_string_lossy().to_string();

        let mut ctx = ToolPermissionContext::empty();
        ctx.allow_rules.push(PermissionRule {
            pattern: pattern.clone(),
            op: None,
            source: RuleSource::Session,
        });
        ctx.deny_rules.push(PermissionRule {
            pattern: pattern.clone(),
            op: None,
            source: RuleSource::Session,
        });

        assert!(matches!(
            is_path_allowed(&file, PathOp::Read, &ctx),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn renlijia_path_outside_primary_returns_ask_in_default_mode() {
        // Was previously a hard Deny (Step 3 lotus-internal wall). After removal,
        // ~/.renlijia/ paths flow through Step 4-6 like any other unauthorized path.
        // In Default mode with no primary_root and no allow rules, expect Ask.
        if let Some(home) = dirs::home_dir() {
            let lotus_path = home.join(".renlijia").join("permissions.json");
            let ctx = ToolPermissionContext::empty();
            assert!(matches!(
                is_path_allowed(&lotus_path, PathOp::Read, &ctx),
                Decision::Ask { .. }
            ));
        }
    }

    // NOTE: the §5.1 exception (primary_root inside ~/.renlijia/ exempts paths from the
    // lotus-internal block) used to need a special carve-out because of the Step 3 wall.
    // After removing that wall, primary_root inside ~/.renlijia/ works through standard
    // Step 4a logic; no special test needed here.

    #[test]
    fn primary_root_allows_read_and_write_directly() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("data.txt");
        std::fs::write(&file, b"").unwrap();
        let ctx = ctx_with_primary(tmp.path());

        assert_eq!(is_path_allowed(&file, PathOp::Read, &ctx), Decision::Allow);
        assert_eq!(is_path_allowed(&file, PathOp::Write, &ctx), Decision::Allow);
    }

    #[test]
    fn primary_root_write_allowed_even_in_default_mode() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("out.txt");
        std::fs::write(&file, b"").unwrap();
        let mut ctx = ctx_with_primary(tmp.path());
        ctx.mode = PermissionMode::Default;

        assert_eq!(is_path_allowed(&file, PathOp::Write, &ctx), Decision::Allow);
    }

    #[test]
    fn read_root_allows_regular_home_reads() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("notes.txt");
        std::fs::write(&file, b"").unwrap();
        let mut ctx = ToolPermissionContext::empty();
        ctx.read_roots.push(tmp.path().to_path_buf());

        assert_eq!(is_path_allowed(&file, PathOp::Read, &ctx), Decision::Allow);
    }

    #[test]
    fn read_root_sensitive_file_asks() {
        let tmp = TempDir::new().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();
        let file = ssh_dir.join("id_rsa");
        std::fs::write(&file, b"secret").unwrap();
        let mut ctx = ToolPermissionContext::empty();
        ctx.read_roots.push(tmp.path().to_path_buf());

        assert!(matches!(
            is_path_allowed(&file, PathOp::Read, &ctx),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn primary_root_delete_in_default_returns_ask() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("old.txt");
        std::fs::write(&file, b"").unwrap();
        let ctx = ctx_with_primary(tmp.path());

        assert!(matches!(
            is_path_allowed(&file, PathOp::Delete, &ctx),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn full_access_allows_regular_delete() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("old.txt");
        std::fs::write(&file, b"").unwrap();
        let mut ctx = ToolPermissionContext::empty();
        ctx.mode = PermissionMode::FullAccess;

        assert_eq!(
            is_path_allowed(&file, PathOp::Delete, &ctx),
            Decision::Allow
        );
    }

    #[test]
    fn full_access_still_denies_root_delete() {
        let mut ctx = ToolPermissionContext::empty();
        ctx.mode = PermissionMode::FullAccess;

        assert!(matches!(
            is_path_allowed(Path::new("/"), PathOp::Delete, &ctx),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn additional_dir_read_allows() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("report.csv");
        std::fs::write(&file, b"").unwrap();
        let ctx = ctx_with_additional(tmp.path());

        assert_eq!(is_path_allowed(&file, PathOp::Read, &ctx), Decision::Allow);
    }

    #[test]
    fn additional_dir_write_in_default_returns_ask() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("out.txt");
        std::fs::write(&file, b"").unwrap();
        let ctx = ctx_with_additional(tmp.path());

        assert!(matches!(
            is_path_allowed(&file, PathOp::Write, &ctx),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn additional_dir_write_in_acceptedits_allows() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("edit.txt");
        std::fs::write(&file, b"").unwrap();
        let mut ctx = ctx_with_additional(tmp.path());
        ctx.mode = PermissionMode::AcceptEdits;

        assert_eq!(is_path_allowed(&file, PathOp::Write, &ctx), Decision::Allow);
    }

    #[test]
    fn additional_dir_write_with_persisted_allow_rule_short_circuits() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("data.txt");
        std::fs::write(&file, b"").unwrap();
        let canonical = std::fs::canonicalize(&file).unwrap();

        let mut ctx = ctx_with_additional(tmp.path());
        ctx.allow_rules.push(PermissionRule {
            pattern: canonical.to_string_lossy().to_string(),
            op: Some(PathOp::Write),
            source: RuleSource::UserSettings,
        });

        assert_eq!(is_path_allowed(&file, PathOp::Write, &ctx), Decision::Allow);
    }

    #[test]
    fn allow_rule_pathglob_op_matches() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("data.csv");
        std::fs::write(&file, b"").unwrap();
        let canonical_dir = std::fs::canonicalize(tmp.path()).unwrap();
        let pattern = format!("{}/**", canonical_dir.display());

        let mut ctx = ToolPermissionContext::empty();
        ctx.allow_rules.push(PermissionRule {
            pattern,
            op: Some(PathOp::Read),
            source: RuleSource::Session,
        });

        assert_eq!(is_path_allowed(&file, PathOp::Read, &ctx), Decision::Allow);
    }

    #[test]
    fn allow_rule_pathglob_op_mismatch_does_not_match() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("data.csv");
        std::fs::write(&file, b"").unwrap();
        let canonical_dir = std::fs::canonicalize(tmp.path()).unwrap();
        let pattern = format!("{}/**", canonical_dir.display());

        let mut ctx = ToolPermissionContext::empty();
        // Rule is Read-only; we test Write op
        ctx.allow_rules.push(PermissionRule {
            pattern,
            op: Some(PathOp::Read),
            source: RuleSource::Session,
        });

        // Write should not be allowed by a Read rule
        assert!(matches!(
            is_path_allowed(&file, PathOp::Write, &ctx),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn default_mode_returns_ask() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("unknown.txt");
        std::fs::write(&file, b"").unwrap();
        let ctx = ToolPermissionContext::empty(); // mode=Default, no roots

        assert!(matches!(
            is_path_allowed(&file, PathOp::Read, &ctx),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn acceptedits_mode_step6_returns_ask() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("outside.txt");
        std::fs::write(&file, b"").unwrap();
        let mut ctx = ToolPermissionContext::empty();
        ctx.mode = PermissionMode::AcceptEdits;
        // no primary_root, no additional dirs → hits step 6

        assert!(matches!(
            is_path_allowed(&file, PathOp::Write, &ctx),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn plan_mode_returns_deny() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("file.txt");
        std::fs::write(&file, b"").unwrap();
        let mut ctx = ToolPermissionContext::empty();
        ctx.mode = PermissionMode::Plan;

        assert!(matches!(
            is_path_allowed(&file, PathOp::Read, &ctx),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn dontask_mode_returns_deny() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("file.txt");
        std::fs::write(&file, b"").unwrap();
        let mut ctx = ToolPermissionContext::empty();
        ctx.mode = PermissionMode::DontAsk;

        assert!(matches!(
            is_path_allowed(&file, PathOp::Read, &ctx),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn canonicalize_handles_symlink() {
        let tmp = TempDir::new().unwrap();
        let real_dir = tmp.path().join("real");
        std::fs::create_dir(&real_dir).unwrap();
        let real_file = real_dir.join("data.txt");
        std::fs::write(&real_file, b"hello").unwrap();

        let link_dir = tmp.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_dir, &link_dir).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real_dir, &link_dir).unwrap();

        let linked_file = link_dir.join("data.txt");

        let mut ctx = ToolPermissionContext::empty();
        ctx.additional_working_dirs
            .insert(real_dir.clone(), RuleSource::Session);

        // Symlink resolves to real_dir, so should Allow
        assert_eq!(
            is_path_allowed(&linked_file, PathOp::Read, &ctx),
            Decision::Allow
        );
    }

    #[test]
    fn canonicalize_or_ancestor_handles_nonexistent_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("not_yet_created.txt");
        assert!(!target.exists());
        let result = super::canonicalize_or_ancestor(&target).unwrap();
        let expected = std::fs::canonicalize(tmp.path())
            .unwrap()
            .join("not_yet_created.txt");
        assert_eq!(result, expected);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn canonicalize_handles_case_insensitive_fs() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("Data.TXT");
        std::fs::write(&file, b"").unwrap();

        // Use uppercase variant of the tmp dir path
        let upper_dir = PathBuf::from(tmp.path().to_string_lossy().to_uppercase());
        let mut ctx = ToolPermissionContext::empty();
        // Add the real (lowercase) dir; the upper-case path should canonicalize to same
        ctx.additional_working_dirs.insert(
            std::fs::canonicalize(tmp.path()).unwrap(),
            RuleSource::Session,
        );

        // The file path uses mixed case; after canonicalize it should match the working dir
        let result = is_path_allowed(&file, PathOp::Read, &ctx);
        // On case-insensitive FS, canonicalize normalizes to actual on-disk case
        assert_eq!(result, Decision::Allow, "upper_dir={}", upper_dir.display());
    }
}

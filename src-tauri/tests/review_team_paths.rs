//! PR7 review test: forbid `team.json` / `team-chat.jsonl` / `teammates/`
//! literal path strings outside `team_paths.rs` so all per-team disk paths
//! must go through `TeamPaths` (per-team disk layout spec §3).
//!
//! Allow-list lives at the bottom — tests, doc comments and the team_paths
//! module itself are exempt.

use std::fs;
use std::path::Path;

#[derive(Debug)]
struct Hit {
    path: String,
    line_no: usize,
    line: String,
    needle: &'static str,
}

fn scan(root: &Path, needles: &[&'static str], allow_paths: &[&str]) -> Vec<Hit> {
    let mut hits = Vec::new();
    walk(root, &mut |path: &Path| {
        let path_str = path.to_string_lossy().to_string();
        if allow_paths.iter().any(|p| path_str.contains(p)) {
            return;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return;
        };
        if ext != "rs" {
            return;
        }
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            // Skip comment-only lines.
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
                continue;
            }
            for needle in needles {
                if line.contains(needle) {
                    hits.push(Hit {
                        path: path_str.clone(),
                        line_no: i + 1,
                        line: line.to_string(),
                        needle,
                    });
                }
            }
        }
    });
    hits
}

fn walk(dir: &Path, action: &mut dyn FnMut(&Path)) {
    if dir.is_file() {
        action(dir);
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(
                name,
                "target" | "node_modules" | ".git" | "dist" | "build" | "docs"
            ) {
                continue;
            }
            walk(&path, action);
        } else if path.is_file() {
            action(&path);
        }
    }
}

fn src_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn no_team_json_literal_outside_team_paths() {
    // r#""team.json""# is the literal we forbid.  team_paths.rs is the only
    // file allowed to mention the on-disk filename.
    let allow = &[
        "runtime/agent/team_paths.rs",
        // tests/ dir is exempt so historical fixtures aren't broken; this
        // file lives in tests/ so it self-excludes via path walking from src/.
    ];
    let hits = scan(&src_root(), &[r#""team.json""#], allow);
    assert!(
        hits.is_empty(),
        "Found `\"team.json\"` literal outside team_paths.rs:\n{}",
        hits.iter()
            .map(|h| format!("  {}:{}  {}", h.path, h.line_no, h.line.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn no_team_chat_jsonl_literal_outside_team_paths() {
    let allow = &["runtime/agent/team_paths.rs"];
    let hits = scan(&src_root(), &[r#""team-chat.jsonl""#], allow);
    assert!(
        hits.is_empty(),
        "Found `\"team-chat.jsonl\"` literal outside team_paths.rs:\n{}",
        hits.iter()
            .map(|h| format!("  {}:{}  {}", h.path, h.line_no, h.line.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn no_join_teammates_literal_outside_team_paths() {
    // We forbid `.join("teammates")` style usages — any code wanting the
    // teammates dir must go through `TeamPaths::for_team(...).teammates_dir()`.
    //
    // Only `runtime/agent/team_paths.rs` may mention the literal, since it
    // defines the on-disk layout.  Earlier revisions also exempted
    // `runtime/agent/output_writer.rs` for a `(Teammate, None)` legacy
    // fallback; that fallback was removed in PR12 (per-team disk layout v2
    // §3 — Teammates always live under a team), so the exemption is gone.
    let allow = &["runtime/agent/team_paths.rs"];
    let hits = scan(&src_root(), &[r#"join("teammates")"#], allow);
    assert!(
        hits.is_empty(),
        "Found `.join(\"teammates\")` literal outside team_paths.rs:\n{}",
        hits.iter()
            .map(|h| format!("  {}:{}  {}", h.path, h.line_no, h.line.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

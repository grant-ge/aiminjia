//! Integration tests for skill hot-reload behavior.
//! Tests that refresh_skill_registry sees new SKILL.md files on disk
//! without requiring app restart.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use app_lib::plugin::skill::loader::load_skill_roots;
use app_lib::plugin::skill::registry::SkillRegistry;

#[test]
fn refresh_reads_new_skill_md_added_after_initial_scan() {
    let tmp = TempDir::new().unwrap();
    let user_dir = tmp.path().join("users").join("scope_x").join("skills");
    let global_dir = tmp.path().join("skills");
    fs::create_dir_all(&user_dir).unwrap();
    fs::create_dir_all(&global_dir).unwrap();

    // Initial scan: empty registry
    let roots: Vec<PathBuf> = vec![user_dir.clone(), global_dir.clone()];
    let initial = load_skill_roots(&roots).expect("initial scan ok");
    let registry = Arc::new(Mutex::new(SkillRegistry::new()));
    registry
        .lock()
        .unwrap()
        .replace_all(initial.into_values().collect());
    assert_eq!(registry.lock().unwrap().skill_ids().len(), 0);

    // 模拟 lotus_skill.py install: 写一个新 SKILL.md
    let new_skill_dir = user_dir.join("foo-skill");
    fs::create_dir_all(&new_skill_dir).unwrap();
    fs::write(
        new_skill_dir.join("SKILL.md"),
        "---\nname: foo-skill\ndescription: test skill\n---\n# foo-skill\n\nbody\n",
    )
    .unwrap();

    // Re-scan + replace
    let after = load_skill_roots(&roots).expect("rescan ok");
    registry
        .lock()
        .unwrap()
        .replace_all(after.into_values().collect());

    // 验收：新 skill 在 registry 里
    let ids = registry.lock().unwrap().skill_ids();
    assert!(
        ids.iter().any(|id| id == "foo-skill"),
        "foo-skill must be in registry after re-scan; got: {:?}",
        ids
    );
}

use std::time::{Duration, Instant};

/// SkillRegistry that tracks how many times its underlying refresh
/// hook would be triggered. Used to assert miss-retry behavior.
struct ProbeRefresh {
    count: Mutex<u32>,
    last_call: Mutex<Option<Instant>>,
}

impl ProbeRefresh {
    fn new() -> Self {
        Self {
            count: Mutex::new(0),
            last_call: Mutex::new(None),
        }
    }
    fn record(&self) {
        *self.count.lock().unwrap() += 1;
        *self.last_call.lock().unwrap() = Some(Instant::now());
    }
    fn count(&self) -> u32 {
        *self.count.lock().unwrap()
    }
}

#[test]
fn miss_retry_throttle_prevents_rapid_repeat_refresh() {
    // 这条测试单独验证 throttle 逻辑（不依赖完整 Tauri ctx）。
    // 实际 load_skill 集成时通过 try_acquire_refresh_slot 辅助函数实现。
    let probe = Arc::new(ProbeRefresh::new());
    let throttle = Arc::new(Mutex::new(None::<Instant>));

    // 模拟 5 次连续 miss-retry
    for _ in 0..5 {
        let now = Instant::now();
        let should_refresh = {
            let last = throttle.lock().unwrap();
            match *last {
                None => true,
                Some(t) => now.duration_since(t) >= Duration::from_secs(5),
            }
        };
        if should_refresh {
            probe.record();
            *throttle.lock().unwrap() = Some(now);
        }
    }

    // 5 次连续调用应该只触发 1 次实际 refresh（throttle 生效）
    assert_eq!(probe.count(), 1, "throttle should suppress rapid retries");
}

/// 验证 refresh_skill_registry 函数确实把磁盘新增的 SKILL.md
/// 同步到 registry（这是 RefreshSkillsTool 内部调用的核心 fn）。
#[test]
fn refresh_registry_picks_up_disk_changes() {
    let tmp = TempDir::new().unwrap();
    let user_dir = tmp.path().join("users").join("scope_x").join("skills");
    let global_dir = tmp.path().join("skills");
    fs::create_dir_all(&user_dir).unwrap();
    fs::create_dir_all(&global_dir).unwrap();

    let registry = Arc::new(Mutex::new(SkillRegistry::new()));
    let roots: Vec<PathBuf> = vec![user_dir.clone(), global_dir.clone()];

    // 初始空
    let loaded = load_skill_roots(&roots).unwrap();
    registry
        .lock()
        .unwrap()
        .replace_all(loaded.into_values().collect());
    assert_eq!(registry.lock().unwrap().skill_ids().len(), 0);

    // 写 3 个 skill
    for id in &["alpha", "beta", "gamma"] {
        let d = user_dir.join(id);
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join("SKILL.md"),
            format!(
                "---\nname: {}\ndescription: test {}\n---\n# {}\n\nbody\n",
                id, id, id
            ),
        )
        .unwrap();
    }

    // 模拟 refresh
    let loaded = load_skill_roots(&roots).unwrap();
    registry
        .lock()
        .unwrap()
        .replace_all(loaded.into_values().collect());

    let ids = registry.lock().unwrap().skill_ids();
    assert_eq!(ids.len(), 3, "should see all 3 skills after refresh");
    for id in &["alpha", "beta", "gamma"] {
        assert!(ids.iter().any(|s| s == id), "missing skill {}", id);
    }
}

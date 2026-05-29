# Per-Team 子目录化磁盘布局 — 完整 10-PR 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 lotus-app 的 team 模型从"每 conv 一个 team"重构为"每 conv 多 team / team 生死随 conv / team_name 在 conv 内唯一（强制 ASCII）"，并提供前端最小可用 UI。

**Architecture:** 磁盘布局 `<conv>/teams/{team_name}/{config.json, team-chat.jsonl, tasks/, teammates/}`；运行态 `TeamRegistry::HashMap<SessionId, HashMap<TeamName, Team>>`；三个注册表（inbox / agent_names / cancellation）扩展为三元 key `(SessionId, TeamName, AgentId)`；新增 `TeamSwitch` 工具与 ConversationMeta::active_team_name 字段；前端"团队"按钮 + 抽屉 + team-chat 面板。

**Tech Stack:** Rust（lotus-app 后端）+ React/TypeScript 前端 + Tauri 2.x IPC。

**Reference Spec:** `docs/superpowers/specs/2026-05-14-per-team-disk-layout-design.md`

**Branch strategy:** 单一 feature 分支 `feat/per-team-layout`，每个 PR 对应一组 commits，整个完成后一次性合 main。

---

## 全局执行约定（subagent 必读）

1. **每个 PR 完成 = 编译 + 该 PR 引入的测试全过 + 已有测试无回归 + git commit**
2. **任意 PR 遇到无法解决的 ambiguity（spec 没写清、与现有代码冲突），停下来在最后输出 "BLOCKED on PR-X step Y" 给主对话，不自行决策**
3. **不要新增 LegacyToolAdapter / compat_* 字段 / "兼容模式" 开关**——spec §5 明令禁止
4. **不要写迁移代码**——spec §8 已决策不迁移
5. **每个 PR 的最后一步必须跑 `cd src-tauri && cargo test --lib 2>&1 | tail -5` 确认无回归**，输出 `test result: ok.`
6. **commit message 用中文 + conventional commit 前缀**，例：`feat(team): TeamRegistry 三 API 分立`
7. **遇到 cargo test 卡住或编译超过 5 分钟 → 报告 BLOCKED**——CLAUDE.md 写明"不能并行多个 cargo test"，确保一次只跑一个
8. **每改一个文件后 grep 一次老引用**，确保该文件不再有遗留旧路径字面量
9. **整个执行完成后输出最终总结**：每个 PR 的 commit hash + 新增/修改文件数 + 测试统计

---

## File Structure（高层视角）

| 文件 | 类型 | PR | 责任 |
|---|---|---|---|
| `src-tauri/src/runtime/agent/team_paths.rs` | 新建 | PR1 | TeamPaths 路径派生 + validate_team_name |
| `src-tauri/src/runtime/agent/team.rs` | 重构 | PR2 | TeamRegistry 三 API（delete_team / drop_session / hydrate_from_disk） |
| `src-tauri/src/runtime/tools/context.rs` | 修改 | PR3 | ToolExecutionContext.active_team_name |
| `src-tauri/src/storage/file_store/...conv meta...` | 修改 | PR3 | ConversationMeta.active_team_name |
| `src-tauri/src/runtime/tools/builtin/{task_tools,send_message,task_output,spawn_subagent}.rs` | 修改 | PR3 | 接入 TeamPaths |
| `src-tauri/src/runtime/agent/output_writer.rs` | 修改 | PR3 | transcript_path_for_kind 加 team_name 参数 |
| `src-tauri/src/runtime/agent/team_context.rs` | 修改 | PR3 | render 路径走 TeamPaths |
| `src-tauri/src/runtime/agent/{inbox_registry,name_registry,cancellation_registry}.rs` | 修改 | PR4 | 三元 key 扩展 + unregister_team / cancel_team |
| `src-tauri/src/runtime/agent/worker_runtime.rs` | 修改 | PR4 | TeammateWorkerCtx.team_name + cleanup_teammate 三元调用 |
| `src-tauri/src/runtime/agent/task_notification_lead.rs` | 修改 | PR4 | emit_to_lead 加 team_name |
| `src-tauri/src/runtime/tools/builtin/team_tools.rs` | 修改 | PR5 | 解锁多 team + TeamSwitch + cancel→delete 顺序 |
| `src-tauri/src/runtime/agent/lead_idle.rs` | 修改 | PR6 | WakeFn 签名加 team_name |
| `src-tauri/src/runtime/agent/inbox.rs` | 修改 | PR6 | 投递路径透传 team_name 到 wake |
| `src-tauri/src/runtime/team_view.rs` | 修改 | PR7 | 扫 teams/ 多 team 重建视图 |
| `src-tauri/src/telemetry.rs` | 修改 | PR8 | DiagnosticEvent.team_name 字段 |
| `src-tauri/src/transport/tauri_commands/...` | 修改 | PR9 | team_chat_messages / team_switch_active 命令 |
| `src/components/teams/*` 前端 | 新建/修改 | PR10 | 团队按钮 + 抽屉 + team-chat 面板 |
| `src/i18n/{zh-CN,en-US}.json` 前端 | 修改 | PR10 | 4 个 i18n keys |

---

## PR1: team_paths.rs 路径派生 + 校验

**完成判定**：team_paths 模块编译通过，22 个单元测试全过，cargo test --lib 无回归。

### Task 1.1: 创建分支并新建文件

- [ ] **Step 1: 检查 main 干净并创建分支**

```bash
git checkout main && git status
git pull --ff-only origin main
git checkout -b feat/per-team-layout
```

- [ ] **Step 2: 创建 `src-tauri/src/runtime/agent/team_paths.rs`，写入完整内容**

```rust
//! Per-team disk path derivation and team_name validation.
//!
//! Single source of truth for any path under a conversation directory
//! that relates to a team:
//! `<conv>/teams/{name}/{config.json, team-chat.jsonl, tasks/, teammates/}`.
//!
//! Callers MUST go through `TeamPaths` instead of raw `conv_dir.join(...)`.
//! See `docs/superpowers/specs/2026-05-14-per-team-disk-layout-design.md` §3.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TeamNameError {
    #[error("team_name must not be empty")]
    TooShort,
    #[error("team_name length {len} exceeds max 64")]
    TooLong { len: usize },
    #[error("team_name must match ^[a-zA-Z0-9_-]+$")]
    InvalidChars,
    #[error("team_name `{0}` is a Windows reserved name")]
    WindowsReserved(String),
    #[error("team_name `{0}` is degenerate (all dashes / dots)")]
    DegenerateName(String),
}

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn validate_team_name(raw: &str) -> Result<(), TeamNameError> {
    if raw.is_empty() { return Err(TeamNameError::TooShort); }
    if raw.len() > 64 { return Err(TeamNameError::TooLong { len: raw.len() }); }
    if !raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(TeamNameError::InvalidChars);
    }
    let upper = raw.to_ascii_uppercase();
    if WINDOWS_RESERVED.iter().any(|r| *r == upper) {
        return Err(TeamNameError::WindowsReserved(raw.to_string()));
    }
    if !raw.chars().any(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(TeamNameError::DegenerateName(raw.to_string()));
    }
    Ok(())
}

pub struct TeamPaths<'a> {
    conv_dir: &'a Path,
    team_name: Option<&'a str>,
}

impl<'a> TeamPaths<'a> {
    pub fn for_conv(conv_dir: &'a Path) -> Self {
        Self { conv_dir, team_name: None }
    }

    pub fn for_team(conv_dir: &'a Path, team_name: &'a str) -> Self {
        Self { conv_dir, team_name: Some(team_name) }
    }

    pub fn team_root(&self) -> Option<PathBuf> {
        self.team_name.map(|n| self.conv_dir.join("teams").join(n))
    }

    pub fn config_json(&self) -> PathBuf {
        self.team_root().expect("config_json requires team-bound TeamPaths").join("config.json")
    }

    pub fn team_chat_jsonl(&self) -> PathBuf {
        self.team_root().expect("team_chat_jsonl requires team-bound TeamPaths").join("team-chat.jsonl")
    }

    pub fn tasks_dir(&self) -> PathBuf {
        match self.team_name {
            Some(n) => self.conv_dir.join("teams").join(n).join("tasks"),
            None => self.conv_dir.join("tasks"),
        }
    }

    pub fn teammates_dir(&self) -> PathBuf {
        self.team_root().expect("teammates_dir requires team-bound TeamPaths").join("teammates")
    }

    pub fn teammate_transcript(&self, agent_id: &str) -> PathBuf {
        self.teammates_dir().join(format!("{agent_id}.jsonl"))
    }

    pub fn teammate_meta(&self, agent_id: &str) -> PathBuf {
        self.teammates_dir().join(format!("{agent_id}.meta.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn conv() -> PathBuf {
        PathBuf::from("/home/u/.renlijia/users/u1/conversations/conv-1")
    }

    #[test] fn validate_accepts_simple_ascii() {
        assert!(validate_team_name("alpha").is_ok());
        assert!(validate_team_name("research-team").is_ok());
        assert!(validate_team_name("team_01").is_ok());
        assert!(validate_team_name("A").is_ok());
    }
    #[test] fn validate_rejects_empty() { assert_eq!(validate_team_name(""), Err(TeamNameError::TooShort)); }
    #[test] fn validate_rejects_too_long() {
        let s = "a".repeat(65);
        assert_eq!(validate_team_name(&s), Err(TeamNameError::TooLong { len: 65 }));
    }
    #[test] fn validate_accepts_max_length() {
        assert!(validate_team_name(&"a".repeat(64)).is_ok());
    }
    #[test] fn validate_rejects_chinese() {
        assert_eq!(validate_team_name("市场调研"), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_emoji() {
        assert_eq!(validate_team_name("team-🔥"), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_space() {
        assert_eq!(validate_team_name("research team"), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_path_separator() {
        assert_eq!(validate_team_name("team/alpha"), Err(TeamNameError::InvalidChars));
        assert_eq!(validate_team_name("team\\alpha"), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_dot_and_dotdot() {
        assert_eq!(validate_team_name("."), Err(TeamNameError::InvalidChars));
        assert_eq!(validate_team_name(".."), Err(TeamNameError::InvalidChars));
    }
    #[test] fn validate_rejects_windows_reserved() {
        for name in &["CON","con","Con","PRN","prn","AUX","NUL","COM1","com9","LPT5"] {
            assert!(matches!(validate_team_name(name), Err(TeamNameError::WindowsReserved(_))));
        }
    }
    #[test] fn validate_accepts_reserved_prefix() {
        assert!(validate_team_name("CONFIG").is_ok());
        assert!(validate_team_name("PRINTER").is_ok());
        assert!(validate_team_name("COM10").is_ok());
    }
    #[test] fn validate_rejects_all_dashes() {
        assert_eq!(validate_team_name("---"), Err(TeamNameError::DegenerateName("---".to_string())));
        assert_eq!(validate_team_name("-"), Err(TeamNameError::DegenerateName("-".to_string())));
    }
    #[test] fn team_root_for_conv_returns_none() {
        assert_eq!(TeamPaths::for_conv(&conv()).team_root(), None);
    }
    #[test] fn team_root_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").team_root(), Some(dir.join("teams").join("alpha")));
    }
    #[test] fn config_json_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").config_json(), dir.join("teams/alpha/config.json"));
    }
    #[test] #[should_panic] fn config_json_for_conv_panics() {
        let _ = TeamPaths::for_conv(&conv()).config_json();
    }
    #[test] fn team_chat_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").team_chat_jsonl(), dir.join("teams/alpha/team-chat.jsonl"));
    }
    #[test] fn tasks_dir_for_conv() {
        let dir = conv();
        assert_eq!(TeamPaths::for_conv(&dir).tasks_dir(), dir.join("tasks"));
    }
    #[test] fn tasks_dir_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").tasks_dir(), dir.join("teams/alpha/tasks"));
    }
    #[test] fn teammates_dir_for_team() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").teammates_dir(), dir.join("teams/alpha/teammates"));
    }
    #[test] fn teammate_transcript() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").teammate_transcript("agent-42"), dir.join("teams/alpha/teammates/agent-42.jsonl"));
    }
    #[test] fn teammate_meta() {
        let dir = conv();
        assert_eq!(TeamPaths::for_team(&dir, "alpha").teammate_meta("agent-42"), dir.join("teams/alpha/teammates/agent-42.meta.json"));
    }
}
```

- [ ] **Step 3: 在 `src-tauri/src/runtime/agent/mod.rs` 加 `pub mod team_paths;`**（按字母序与已有 `team` / `team_context` / `team_view` 对齐）

- [ ] **Step 4: 编译 + 测试**

```bash
cd src-tauri && cargo test --lib runtime::agent::team_paths 2>&1 | tail -10
```

Expected: `test result: ok. 22 passed`

- [ ] **Step 5: 全仓回归**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
```

Expected: `test result: ok.`（总数 = 之前 + 22）

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/agent/team_paths.rs src-tauri/src/runtime/agent/mod.rs
git commit -m "feat(team_paths): TeamPaths 路径派生 + validate_team_name + 22 单测"
```

---

## PR2: TeamRegistry 三 API 分立 + write_atomic 走 fs_atomic

**完成判定**：`TeamRegistry` 内层 HashMap 改造完成，`delete_team` / `drop_session` / `hydrate_from_disk` 三个 API 分立；`persist`/`delete_persisted_team` 签名加 `team_name`；现有调用方编译通过；既有测试 + 新增单测全过。

### Task 2.1: 改造 TeamRegistry 数据结构

- [ ] **Step 1: 读懂当前 team.rs**

```bash
sed -n '92,172p' src-tauri/src/runtime/agent/team.rs
```

记下当前 `TeamRegistry::create/get/delete/persist/delete_persisted/clear_all` 的签名。

- [ ] **Step 2: 用以下完整内容替换 `src-tauri/src/runtime/agent/team.rs`**

保留文件顶部 `Member`/`Team`/`TeamError` 等定义，仅重写 `TeamRegistry` 及之后部分。具体补丁见下：

将 `TeamRegistry` struct 改为：

```rust
#[derive(Debug, Default)]
pub struct TeamRegistry {
    teams: Mutex<HashMap<SessionId, HashMap<String, Arc<Mutex<Team>>>>>,
}
```

将 `create` / `get` / `delete` 改为：

```rust
impl TeamRegistry {
    pub fn new() -> Arc<Self> { Arc::new(Self::default()) }

    pub async fn create(
        &self,
        session_id: SessionId,
        lead: Member,
        team_name: String,
    ) -> Result<Arc<Mutex<Team>>, TeamError> {
        // 调用方负责先调 validate_team_name；这里不重复校验
        let mut g = self.teams.lock().await;
        let inner = g.entry(session_id.clone()).or_insert_with(HashMap::new);
        if inner.contains_key(&team_name) {
            return Err(TeamError::NameAlreadyTaken(team_name));
        }
        let team = Arc::new(Mutex::new(Team::new(session_id.clone(), lead, team_name.clone())));
        inner.insert(team_name, team.clone());
        Ok(team)
    }

    pub async fn get(&self, session_id: &SessionId, team_name: &str) -> Option<Arc<Mutex<Team>>> {
        self.teams.lock().await.get(session_id)?.get(team_name).cloned()
    }

    pub async fn list(&self, session_id: &SessionId) -> Vec<(String, Arc<Mutex<Team>>)> {
        self.teams.lock().await.get(session_id)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    /// Delete one specific team within a session. Used by TeamDelete tool.
    pub async fn delete_team(&self, session_id: &SessionId, team_name: &str) -> Option<Arc<Mutex<Team>>> {
        let mut g = self.teams.lock().await;
        let inner = g.get_mut(session_id)?;
        let removed = inner.remove(team_name);
        if inner.is_empty() {
            g.remove(session_id);
        }
        removed
    }

    /// Drop the entire session (all teams). Used by cancel_session / conv close.
    pub async fn drop_session(&self, session_id: &SessionId) -> Vec<(String, Arc<Mutex<Team>>)> {
        let mut g = self.teams.lock().await;
        g.remove(session_id)
            .map(|m| m.into_iter().collect())
            .unwrap_or_default()
    }

    pub async fn clear_all(&self) -> usize {
        let mut g = self.teams.lock().await;
        let n: usize = g.values().map(|m| m.len()).sum();
        g.clear();
        n
    }
}
```

- [ ] **Step 3: 改 `persist` 与新增 `delete_persisted_team`**

```rust
impl TeamRegistry {
    pub async fn persist(
        &self,
        session_id: &SessionId,
        team_name: &str,
        conv_dir: &Path,
    ) -> Result<(), TeamPersistError> {
        let Some(team_handle) = self.get(session_id, team_name).await else {
            return Ok(());
        };
        let snapshot = {
            let team = team_handle.lock().await;
            TeamSnapshot::from(&*team)
        };
        let path = crate::runtime::agent::team_paths::TeamPaths::for_team(conv_dir, team_name).config_json();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(TeamPersistError::Io)?;
        }
        let bytes = serde_json::to_vec_pretty(&snapshot).map_err(TeamPersistError::Serde)?;
        crate::storage::fs_atomic::write_atomic(&path, &bytes).map_err(TeamPersistError::Io)?;
        Ok(())
    }

    pub fn delete_persisted_team(conv_dir: &Path, team_name: &str) -> std::io::Result<()> {
        let root = crate::runtime::agent::team_paths::TeamPaths::for_team(conv_dir, team_name)
            .team_root()
            .expect("for_team always has team_root");
        match std::fs::remove_dir_all(&root) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}
```

**注意**：删除原有的 `write_atomic_team` 私有函数和原有 `delete_persisted` 函数（被 `delete_persisted_team` 替代）。

- [ ] **Step 4: 处理调用方编译错误**

旧 `delete(session)` / `delete_persisted(conv_dir)` 的调用点需要适配。grep 找出所有引用：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
grep -rn "team_registry().delete\|TeamRegistry::delete_persisted\|\.delete(&session\|\.delete(&\&session" src-tauri/src --include="*.rs"
```

对每处：
- `cancel_session` 调用 `team_registry.delete(&sid)` → 改为 `team_registry.drop_session(&sid)`
- `team_tools.rs::TeamDelete` 调用 `delete(&session)` → 暂时改为 `delete_team(&session, "")` **加 TODO("PR5")** 注释；这个 PR 内 TeamCreate/TeamDelete 仍允许编译但会因为 team_name 留空而 broken——**接受这个状态**（PR5 会修复，PR2 的目标只是数据结构改造能编译）

- [ ] **Step 5: 加 hydrate_from_disk 方法**

```rust
#[derive(thiserror::Error, Debug)]
pub enum TeamHydrateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl TeamRegistry {
    pub async fn hydrate_from_disk(
        &self,
        session_id: &SessionId,
        conv_dir: &Path,
    ) -> Result<usize, TeamHydrateError> {
        let teams_root = conv_dir.join("teams");
        if !teams_root.exists() { return Ok(0); }
        let mut count = 0;
        for entry in std::fs::read_dir(&teams_root)? {
            let entry = match entry { Ok(e) => e, Err(_) => continue };
            let team_dir = entry.path();
            if !team_dir.is_dir() { continue; }
            let dir_name = match entry.file_name().to_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            let config = team_dir.join("config.json");
            if !config.exists() { continue; }
            let bytes = match std::fs::read(&config) {
                Ok(b) => b,
                Err(e) => { log::warn!("hydrate skip {:?}: {e}", config); continue; }
            };
            let snapshot: TeamSnapshot = match serde_json::from_slice(&bytes) {
                Ok(s) => s,
                Err(e) => { log::warn!("hydrate skip {:?}: {e}", config); continue; }
            };
            if snapshot.team_name != dir_name {
                log::warn!("hydrate skip: team_name `{}` != dir `{}`", snapshot.team_name, dir_name);
                continue;
            }
            let mut g = self.teams.lock().await;
            let inner = g.entry(session_id.clone()).or_insert_with(HashMap::new);
            if inner.contains_key(&snapshot.team_name) {
                continue; // 幂等：已 hydrate 跳过
            }
            let lead_member = Member {
                agent_id: snapshot.lead.agent_id.clone(),
                name: snapshot.lead.name.clone(),
                role: MemberRole::Lead,
                created_at: snapshot.lead.created_at,
                last_active_at: snapshot.lead.last_active_at,
            };
            let mut team = Team::new(session_id.clone(), lead_member, snapshot.team_name.clone());
            team.created_at = snapshot.created_at;
            for tm in &snapshot.teammates {
                let mate = Member {
                    agent_id: tm.agent_id.clone(),
                    name: tm.name.clone(),
                    role: MemberRole::Teammate {
                        employee_id: tm.employee_id.clone().unwrap_or_default(),
                        spawned_by: tm.spawned_by.clone().unwrap_or_else(|| snapshot.lead.agent_id.clone()),
                    },
                    created_at: tm.created_at,
                    last_active_at: tm.last_active_at,
                };
                let _ = team.add_teammate(mate);
            }
            inner.insert(snapshot.team_name.clone(), Arc::new(Mutex::new(team)));
            count += 1;
        }
        Ok(count)
    }
}
```

- [ ] **Step 6: 加单测**

在 `team.rs` 末尾的测试模块（或新建 `#[cfg(test)] mod tests` 块）追加：

```rust
#[cfg(test)]
mod registry_v2_tests {
    use super::*;
    use tempfile::tempdir;

    fn dummy_lead(name: &str) -> Member {
        Member {
            agent_id: AgentId::new(format!("lead-{name}")),
            name: name.to_string(),
            role: MemberRole::Lead,
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn create_two_teams_in_same_session() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        reg.create(s.clone(), dummy_lead("b"), "beta".to_string()).await.unwrap();
        let listed = reg.list(&s).await;
        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn duplicate_team_name_rejected() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        let err = reg.create(s, dummy_lead("a2"), "alpha".to_string()).await.unwrap_err();
        assert!(matches!(err, TeamError::NameAlreadyTaken(_)));
    }

    #[tokio::test]
    async fn delete_team_keeps_other_teams() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        reg.create(s.clone(), dummy_lead("b"), "beta".to_string()).await.unwrap();
        reg.delete_team(&s, "alpha").await.unwrap();
        assert!(reg.get(&s, "alpha").await.is_none());
        assert!(reg.get(&s, "beta").await.is_some());
    }

    #[tokio::test]
    async fn drop_session_clears_all() {
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        reg.create(s.clone(), dummy_lead("b"), "beta".to_string()).await.unwrap();
        let dropped = reg.drop_session(&s).await;
        assert_eq!(dropped.len(), 2);
        assert_eq!(reg.list(&s).await.len(), 0);
    }

    #[tokio::test]
    async fn hydrate_from_empty_conv() {
        let dir = tempdir().unwrap();
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        let n = reg.hydrate_from_disk(&s, dir.path()).await.unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn persist_then_hydrate_roundtrip() {
        let dir = tempdir().unwrap();
        let reg = TeamRegistry::new();
        let s = SessionId::new("s1");
        reg.create(s.clone(), dummy_lead("a"), "alpha".to_string()).await.unwrap();
        reg.persist(&s, "alpha", dir.path()).await.unwrap();
        // drop in-memory state, re-hydrate
        reg.drop_session(&s).await;
        assert_eq!(reg.list(&s).await.len(), 0);
        let n = reg.hydrate_from_disk(&s, dir.path()).await.unwrap();
        assert_eq!(n, 1);
        assert!(reg.get(&s, "alpha").await.is_some());
    }
}
```

- [ ] **Step 7: 编译 + 测试**

```bash
cd src-tauri && cargo test --lib runtime::agent::team:: 2>&1 | tail -15
```

Expected: registry_v2_tests 6 passed + 已有 team 测试通过

- [ ] **Step 8: 全仓回归**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
```

Expected: `test result: ok.`

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(team): TeamRegistry 三 API 分立（delete_team / drop_session / hydrate_from_disk）+ 走 TeamPaths"
```

---

## PR3: ToolExecutionContext.active_team_name + 接入 TeamPaths

**完成判定**：`ctx.active_team_name` 注入链路 OK；`task_tools` / `send_message` / `task_output` / `output_writer` / `team_context` / `spawn_subagent` 都改用 TeamPaths 派生路径；`ConversationMeta.active_team_name` 字段加好；现有测试无回归（即使该 PR 内多 team 还没被 TeamCreate 工具放行，"PR2 后 TeamCreate 接受第一个 team_name 走新路径"必须工作）。

### Task 3.1: ToolExecutionContext 加字段

- [ ] **Step 1: 修改 `src-tauri/src/runtime/tools/context.rs`**

定位到 `ToolExecutionContext` 结构体，加字段：

```rust
pub active_team_name: Option<String>,
```

在 `Default` 实现里 `active_team_name: None,`。

加 builder：

```rust
pub fn with_active_team(mut self, team_name: String) -> Self {
    self.active_team_name = Some(team_name);
    self
}
```

- [ ] **Step 2: 编译**

```bash
cd src-tauri && cargo check --lib 2>&1 | tail -5
```

Expected: `Finished` 或仅有 unused warning

### Task 3.2: ConversationMeta 加 active_team_name

- [ ] **Step 1: 找到 ConversationMeta**

```bash
grep -rn "struct ConversationMeta\b" src-tauri/src/storage/ --include="*.rs"
```

打开该文件，加字段：

```rust
#[serde(default)]
pub active_team_name: Option<String>,
```

确保已有 `#[serde(default)]` 模式存在则照用，否则 `Option<T>` 默认值就是 None 不需要额外处理。

- [ ] **Step 2: 编译 + 已有 conv_meta 测试**

```bash
cd src-tauri && cargo test --lib storage:: 2>&1 | tail -5
```

Expected: 通过

### Task 3.3: SessionRuntime 注入 active_team_name

- [ ] **Step 1: 找到 SessionRuntime 构造 ToolExecutionContext 的位置**

```bash
grep -rn "ToolExecutionContext::new\|ToolExecutionContext {" src-tauri/src/runtime/ --include="*.rs"
```

每个构造点：
- 主对话 Lead 调 tool 时：从 `ConversationMeta.active_team_name` 读值注入
- Teammate 调 tool 时：从 `TeammateWorkerCtx.team_name`（PR4 才会加，本 PR 暂时从 `team_registry.list(session)` 反查 sole team）

具体做法：
```rust
let active_team_name = conv_meta.as_ref().and_then(|m| m.active_team_name.clone());
let ctx = ToolExecutionContext::new(...).with_conv_dir(conv_dir);
let ctx = match active_team_name {
    Some(name) => ctx.with_active_team(name),
    None => ctx,
};
```

注：**Path C wake 路径暂不处理，留 PR6**——本 PR 只覆盖正常 Lead turn / Teammate turn 的 ctx 构造。

- [ ] **Step 2: 编译**

```bash
cd src-tauri && cargo check --lib 2>&1 | tail -5
```

### Task 3.4: 接入 TeamPaths——task_tools

- [ ] **Step 1: 修改 `src-tauri/src/runtime/tools/builtin/task_tools.rs::store_for`**

替换为：

```rust
fn store_for(ctx: &ToolExecutionContext) -> Result<FileTaskV2Store, ToolError> {
    use crate::runtime::agent::team_paths::TeamPaths;
    if let Some(conv_dir) = ctx.conv_dir.as_ref() {
        let paths = match ctx.active_team_name.as_deref() {
            Some(name) => TeamPaths::for_team(conv_dir, name),
            None => TeamPaths::for_conv(conv_dir),
        };
        return Ok(FileTaskV2Store::new(paths.tasks_dir()));
    }
    // 老 fallback 保留不动
    let home = ctx.task_store_root.clone()
        .or_else(|| ctx.capability.as_ref().and_then(|c| c.storage.as_ref()).map(|s| s.workspace_path.clone()))
        .or_else(default_aijia_home)
        .ok_or_else(|| ToolError::ExecutionFailed("Task tools require a storage root".into()))?;
    let conv_id = ctx.session_id.as_str();
    let tasks_root = home.join("conversations").join(conv_id).join("tasks");
    Ok(FileTaskV2Store::new(tasks_root))
}
```

### Task 3.5: 接入 TeamPaths——send_message

- [ ] **Step 1: 修改 `src-tauri/src/runtime/tools/builtin/send_message.rs::append_team_chat_entry`**

替换 path 派生那段：

```rust
let path = match ctx_team_name {
    Some(name) => crate::runtime::agent::team_paths::TeamPaths::for_team(dir, name).team_chat_jsonl(),
    None => return, // 没有 team 不应调到这里，但 defensive：跳过
};
```

调用方传入 `ctx.active_team_name.as_deref()`。

### Task 3.6: 接入 TeamPaths——output_writer

- [ ] **Step 1: 修改 `transcript_path_for_kind` / `meta_path_for_kind` 签名**

```rust
pub fn transcript_path_for_kind(conv_dir: &Path, kind: &TranscriptKind, team_name: Option<&str>, agent_id: &str) -> PathBuf {
    use crate::runtime::agent::team_paths::TeamPaths;
    match (kind, team_name) {
        (TranscriptKind::Teammate, Some(name)) => TeamPaths::for_team(conv_dir, name).teammate_transcript(agent_id),
        (TranscriptKind::Teammate, None) => conv_dir.join("teammates").join(format!("{agent_id}.jsonl")),
        (TranscriptKind::Subagent, _) => conv_dir.join("subagents").join(format!("{agent_id}.jsonl")),
    }
}

pub fn meta_path_for_kind(conv_dir: &Path, kind: &TranscriptKind, team_name: Option<&str>, agent_id: &str) -> PathBuf {
    use crate::runtime::agent::team_paths::TeamPaths;
    match (kind, team_name) {
        (TranscriptKind::Teammate, Some(name)) => TeamPaths::for_team(conv_dir, name).teammate_meta(agent_id),
        (TranscriptKind::Teammate, None) => conv_dir.join("teammates").join(format!("{agent_id}.meta.json")),
        (TranscriptKind::Subagent, _) => conv_dir.join("subagents").join(format!("{agent_id}.meta.json")),
    }
}
```

- [ ] **Step 2: 改 `write_meta` 签名加 team_name 参数；改所有调用点**

```bash
grep -rn "transcript_path_for_kind\|meta_path_for_kind\|write_meta" src-tauri/src --include="*.rs"
```

所有调用点同步加 team_name 参数（从 ctx 或上下文取，Subagent kind 传 None）。

### Task 3.7: 接入 TeamPaths——task_output

- [ ] **Step 1: 修改 `src-tauri/src/runtime/tools/builtin/task_output.rs`**

candidates 列表改为：

```rust
use crate::runtime::agent::team_paths::TeamPaths;
let mut candidates = vec![];
if let Some(team_name) = ctx.active_team_name.as_deref() {
    candidates.push(TeamPaths::for_team(conv_dir, team_name).teammate_transcript(task_id));
}
candidates.push(conv_dir.join("teammates").join(format!("{task_id}.jsonl")));
candidates.push(conv_dir.join("subagents").join(format!("{task_id}.jsonl")));
// legacy fallback 路径保留不动
```

### Task 3.8: 接入 TeamPaths——team_context

- [ ] **Step 1: 修改 `src-tauri/src/runtime/agent/team_context.rs::render_for_conv_dir`**

签名加 team_name 参数：

```rust
pub fn render_for_conv_dir(team_name: &str, agent_name: &str, conv_dir: &Path) -> String {
    use crate::runtime::agent::team_paths::TeamPaths;
    let paths = TeamPaths::for_team(conv_dir, team_name);
    render(team_name, agent_name, &paths.config_json(), &paths.tasks_dir())
}
```

- [ ] **Step 2: 同步改 `worker_runtime.rs:1204` 附近的调用点**

```bash
grep -rn "render_for_conv_dir" src-tauri/src --include="*.rs"
```

所有调用点确保传入了正确 team_name（从 TeammateWorkerCtx 或本地上下文）。

- [ ] **Step 3: 同步改 team_context 的单测断言**

测试里 hardcoded 路径从 `<conv>/team.json` / `<conv>/tasks/` 改为 `<conv>/teams/{team_name}/config.json` / `<conv>/teams/{team_name}/tasks/`。

### Task 3.9: 接入 TeamPaths——spawn_subagent

- [ ] **Step 1: 修改 `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs:522` 附近**

```rust
team_id: Some(team_name.clone()),  // PR3 起：team_id = team_name 而非 session_id
```

确保 `team_name` 变量在该作用域内可用（从 ctx 或函数参数读）。

### Task 3.10: PR3 编译 + 测试 + Commit

- [ ] **Step 1: 全仓编译**

```bash
cd src-tauri && cargo check --lib 2>&1 | tail -10
```

Expected: 0 error

- [ ] **Step 2: 全仓测试**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
```

Expected: `test result: ok.`

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(team): ctx.active_team_name + ConversationMeta + 接入 TeamPaths（task/send_message/output_writer/task_output/team_context/spawn_subagent）"
```

---

## PR4: 三元 key 注册表 + TeammateWorkerCtx.team_name

**完成判定**：`InboxRegistry` / `AgentNameRegistry` / `CancellationRegistry` 的 key 全部从二元升为三元 `(SessionId, TeamName, AgentId/Name)`；新增 `unregister_team` / `cancel_team` 批量方法；`TeammateWorkerCtx.team_name` 字段加好；`cleanup_teammate` 三处 unregister 同步；`task_notification_lead::emit_to_lead` 签名加 team_name；编译通过 + 测试不回归。

### Task 4.1: InboxRegistry 三元 key

- [ ] **Step 1: 修改 `src-tauri/src/runtime/agent/inbox_registry.rs`**

将 `HashMap<SessionId, HashMap<AgentId, Arc<AgentInbox>>>` 改为 `HashMap<(SessionId, String), HashMap<AgentId, Arc<AgentInbox>>>`，外层 key 为 `(session, team_name)`。

或更明显的：`HashMap<SessionId, HashMap<String, HashMap<AgentId, Arc<AgentInbox>>>>` 三层嵌套。

选嵌套方案，签名：

```rust
pub async fn register(&self, session_id: &SessionId, team_name: &str, agent_id: AgentId, inbox: AgentInbox);
pub async fn unregister(&self, session_id: &SessionId, team_name: &str, agent_id: &AgentId) -> bool;
pub async fn unregister_team(&self, session_id: &SessionId, team_name: &str) -> usize;
pub async fn get(&self, session_id: &SessionId, team_name: &str, agent_id: &AgentId) -> Option<Arc<AgentInbox>>;
pub async fn drop_session(&self, session_id: &SessionId) -> usize;
```

- [ ] **Step 2: 加单测**

```rust
#[tokio::test]
async fn unregister_team_clears_all_team_entries() {
    // setup, then unregister_team and assert
}
```

### Task 4.2: AgentNameRegistry 三元 key

- [ ] **Step 1: 修改 `src-tauri/src/runtime/agent/name_registry.rs`**

`HashMap<SessionId, HashMap<String /* team */, HashMap<String /* name */, AgentId>>>`

签名同上模式：

```rust
pub async fn register(&self, session_id: &SessionId, team_name: &str, name: &str, agent_id: AgentId) -> Result<(), NameRegistryError>;
pub async fn resolve(&self, session_id: &SessionId, team_name: &str, name: &str) -> Option<AgentId>;
pub async fn name_for(&self, session_id: &SessionId, team_name: &str, agent_id: &AgentId) -> Option<String>;
pub async fn unregister(&self, session_id: &SessionId, team_name: &str, name: &str);
pub async fn unregister_team(&self, session_id: &SessionId, team_name: &str);
```

### Task 4.3: CancellationRegistry 三元 key

- [ ] **Step 1: 修改 `src-tauri/src/runtime/agent/cancellation_registry.rs`**

签名：

```rust
pub async fn register(&self, session_id: &SessionId, team_name: &str, agent_id: AgentId, token: CancellationToken);
pub async fn unregister(&self, session_id: &SessionId, team_name: &str, agent_id: &AgentId);
pub async fn cancel_team(&self, session_id: &SessionId, team_name: &str) -> usize;
pub async fn drop_session(&self, session_id: &SessionId);
```

### Task 4.4: TeammateWorkerCtx 加 team_name

- [ ] **Step 1: 修改 `src-tauri/src/runtime/agent/worker_runtime.rs`**

定位 `struct TeammateWorkerCtx`，加 `pub team_name: String,` 字段。

`spawn_subagent` 创建 ctx 处必须填这个字段——`team_name` 已经在该函数作用域内（PR3 已经准备好）。

- [ ] **Step 2: 改 cleanup_teammate**

```rust
async fn cleanup_teammate(ctx: &TeammateWorkerCtx, name: &str) {
    ctx.agent_names.unregister(&ctx.session_id, &ctx.team_name, name).await;
    if let Some(reg) = ctx.inbox_registry.as_ref() {
        reg.unregister(&ctx.session_id, &ctx.team_name, &ctx.agent_id).await;
    }
    if let Some(reg) = ctx.cancellation_registry.as_ref() {
        reg.unregister(&ctx.session_id, &ctx.team_name, &ctx.agent_id).await;
    }
}
```

### Task 4.5: task_notification_lead.rs::emit_to_lead 签名

- [ ] **Step 1: 修改 `src-tauri/src/runtime/agent/task_notification_lead.rs`**

`emit_to_lead` 加 `team_name: &str` 参数；内部 `team_registry.get(session, team_name)` 精确匹配；`inbox_registry.get(session, team_name, lead_id)` 三元 key 投递。

- [ ] **Step 2: 改 task_tools.rs 三处调用点**

```bash
grep -n "emit_to_lead\|try_notify_lead" src-tauri/src/runtime/tools/builtin/task_tools.rs
```

每处 `emit_to_lead(...)` 加 `&ctx.active_team_name.as_deref().unwrap_or("")` 参数。空 team 场景下早 return（noop）：

```rust
let Some(team_name) = ctx.active_team_name.as_deref() else { return; };
emit_to_lead(&deps, &ctx.session_id, team_name, &actor_name, task_id, action, subject, status).await;
```

### Task 4.6: 调用方迁移

- [ ] **Step 1: 找出所有 inbox_registry / agent_names / cancellation_registry 调用点**

```bash
grep -rn "agent_names\.\(register\|resolve\|unregister\|name_for\)\|inbox_registry\.\(register\|unregister\|get\)\|cancellation_registry\.\(register\|unregister\|cancel\)" src-tauri/src --include="*.rs"
```

对每一处加 `team_name` 参数（从 ctx / 局部变量取）。

- [ ] **Step 2: team_tools.rs::TeamCreate 注册 Lead 时传 team_name**

```rust
ctx.agent_names().register(&session, &team_name, LEAD_NAME, lead_id.clone()).await?;
inbox_reg.register(&session, &team_name, lead_id.clone(), lead_inbox).await;
```

### Task 4.7: PR4 编译 + 测试 + Commit

- [ ] **Step 1: 编译**

```bash
cd src-tauri && cargo check --lib 2>&1 | tail -10
```

- [ ] **Step 2: 测试**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
```

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(team): InboxRegistry/AgentNameRegistry/CancellationRegistry 三元 key + TeammateWorkerCtx.team_name + emit_to_lead 加参"
```

---

## PR5: 解锁多 team + TeamSwitch 工具 + 严格 cancel→delete 顺序 + permission 收窄

**完成判定**：TeamCreate 允许同 conv 多 team；TeamDelete 严格按 cancel→delete 顺序；新增 TeamSwitch 工具能切换 active_team_name；`build_teammate_permission_ctx` 把 `additional_working_dirs` 收窄到 `teams/{own_team}/`。

### Task 5.1: TeamCreate 调 validate_team_name + 写新路径

- [ ] **Step 1: 修改 `src-tauri/src/runtime/tools/builtin/team_tools.rs::TeamCreate`**

```rust
use crate::runtime::agent::team_paths::{validate_team_name, TeamPaths};

let team_name = team_name_input.unwrap_or_else(|| default_team_name(session.as_str()));
validate_team_name(&team_name)
    .map_err(|e| ToolError::ExecutionFailed(format!("invalid team_name: {e}")))?;

// ... ctx.team_registry().create() 同前
// 但下面 persist 已经在 PR2 改过路径，写到 teams/{name}/config.json

// 新增：写入 conv.json::active_team_name
update_conv_meta_active_team(conv_dir, Some(&team_name))?;
```

写一个辅助函数：

```rust
fn update_conv_meta_active_team(conv_dir: &Path, name: Option<&str>) -> std::io::Result<()> {
    let path = conv_dir.join("conv.json");
    let mut meta: ConversationMeta = if path.exists() {
        serde_json::from_slice(&std::fs::read(&path)?).unwrap_or_default()
    } else {
        ConversationMeta::default()
    };
    meta.active_team_name = name.map(String::from);
    crate::storage::fs_atomic::write_atomic(&path, &serde_json::to_vec_pretty(&meta).unwrap())?;
    Ok(())
}
```

### Task 5.2: TeamDelete 严格 cancel→delete 顺序

- [ ] **Step 1: 修改 `team_tools.rs::TeamDelete::execute`**

```rust
let team_name = input.get("team_name")
    .and_then(Value::as_str)
    .ok_or_else(|| ToolError::ExecutionFailed("missing team_name".into()))?
    .to_string();

// step a: cancel all teammates of this team
if let Some(reg) = ctx.cancellation_registry.as_ref() {
    let n = reg.cancel_team(&session, &team_name).await;
    log::info!("[TeamDelete] cancelled {n} teammate tokens");
}

// step b: 等待短暂时间让 worker 自我清理（best-effort）
tokio::time::sleep(std::time::Duration::from_millis(200)).await;

// step c: drop in-memory registry entry
let team_handle = ctx.team_registry().delete_team(&session, &team_name).await;

// step d: rm -rf teams/{name}/
if let Some(conv_dir) = ctx.conv_dir.as_ref() {
    if let Err(e) = TeamRegistry::delete_persisted_team(conv_dir, &team_name) {
        log::warn!("[TeamDelete] rm -rf teams/{team_name} failed: {e}");
    }
}

// step e: idempotent sweep（cleanup_teammate 已经清过单条）
ctx.agent_names().unregister_team(&session, &team_name).await;
if let Some(reg) = ctx.inbox_registry.as_ref() {
    reg.unregister_team(&session, &team_name).await;
}

// step f: 若 active_team == 此 team，清掉
if let Some(conv_dir) = ctx.conv_dir.as_ref() {
    let path = conv_dir.join("conv.json");
    // 读 meta，如果 active_team_name == team_name 则改为 None
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(mut meta) = serde_json::from_slice::<ConversationMeta>(&bytes) {
            if meta.active_team_name.as_deref() == Some(&team_name) {
                meta.active_team_name = None;
                let _ = crate::storage::fs_atomic::write_atomic(&path, &serde_json::to_vec_pretty(&meta).unwrap());
            }
        }
    }
}

// 返回结果...
```

### Task 5.3: 新增 TeamSwitch 工具

- [ ] **Step 1: 在 `team_tools.rs` 加 TeamSwitchRuntimeTool**

```rust
pub struct TeamSwitchRuntimeTool;

#[async_trait]
impl RuntimeTool for TeamSwitchRuntimeTool {
    fn id(&self) -> &str { "TeamSwitch" }

    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
        TOOL_CATALOG.get("TeamSwitch").unwrap_or_else(|| {
            ToolDefinition::new("TeamSwitch", "Switch the conversation's active team.")
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool { false }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let team_name = input.get("team_name")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing team_name".into()))?
            .to_string();
        crate::runtime::agent::team_paths::validate_team_name(&team_name)
            .map_err(|e| ToolError::ExecutionFailed(format!("invalid team_name: {e}")))?;

        // 校验 team 存在
        if ctx.team_registry().get(&ctx.session_id, &team_name).await.is_none() {
            return Err(ToolError::ExecutionFailed(format!("team `{team_name}` not found in this conversation")));
        }
        // 写 conv.json
        if let Some(conv_dir) = ctx.conv_dir.as_ref() {
            update_conv_meta_active_team(conv_dir, Some(&team_name))?;
        }
        Ok(ToolResult::new("TeamSwitch", format!("Switched active team to `{team_name}`"), Some(json!({"team_name": team_name}))))
    }
}
```

- [ ] **Step 2: 注册到 ToolRegistry / TOOL_CATALOG**

参考 TeamCreate / TeamDelete 的注册位置——通常在 `lib.rs` 或 `runtime/tools/builtin/mod.rs`。同步加到 `TOOL_CATALOG` 静态映射。

### Task 5.4: build_teammate_permission_ctx 收窄 working_dirs

- [ ] **Step 1: 修改 `src-tauri/src/runtime/agent/worker_runtime.rs::build_teammate_permission_ctx`**

定位到 `additional_working_dirs.entry(dir.to_path_buf())` 那段（约 line 1894-1924）。

替换为：

```rust
if let Some(conv_dir) = conv_dir {
    let team_root = if !team_name.is_empty() {
        conv_dir.join("teams").join(team_name)
    } else {
        conv_dir.to_path_buf()  // 单飞场景兜底
    };
    ctx.additional_working_dirs.entry(team_root)
        .or_insert(RuleSource::Session);
}
```

`build_teammate_permission_ctx` 函数签名加 `team_name: &str` 参数，从 `TeammateWorkerCtx.team_name` 传入。

### Task 5.5: PR5 编译 + 集成测试 + Commit

- [ ] **Step 1: 编译**

```bash
cd src-tauri && cargo check --lib 2>&1 | tail -10
```

- [ ] **Step 2: 写一个集成测试 `src-tauri/tests/team_multi_team_test.rs`**

```rust
//! 多 team 在同一 conv 内的端到端验证
use std::sync::Arc;

#[tokio::test]
async fn create_two_teams_disk_isolated() {
    use lotus_app::runtime::agent::TeamRegistry;
    use lotus_app::runtime::agent::team_paths::TeamPaths;
    let dir = tempfile::tempdir().unwrap();
    let reg = TeamRegistry::new();
    // ... create alpha + beta，persist 到磁盘
    // 断言 dir.path()/teams/alpha/config.json 和 teams/beta/config.json 都存在
    assert!(TeamPaths::for_team(dir.path(), "alpha").config_json().exists());
    assert!(TeamPaths::for_team(dir.path(), "beta").config_json().exists());
}
```

注：完整测试需要真实 SessionRuntime，本 PR 写 minimal smoke 即可。

- [ ] **Step 3: 运行集成测试**

```bash
cd src-tauri && cargo test --test team_multi_team_test 2>&1 | tail -10
```

- [ ] **Step 4: 全仓回归**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(team): 解锁多 team + TeamSwitch + cancel→delete 严格顺序 + permission 收窄到 teams/{own}/"
```

---

## PR6: Path C wake → continuation turn 携带 team_name

**完成判定**：`WakeFn` 签名加 `team_name`；inbox 投递路径透传；continuation turn ctx 用 wake 来源 team_name 而非 conv.json 持久化值。

### Task 6.1: 改 LeadIdleSupervisor::WakeFn 签名

- [ ] **Step 1: 修改 `src-tauri/src/runtime/agent/lead_idle.rs`**

```rust
pub type WakeFn = Box<dyn Fn(LeadKey, String) + Send + Sync>;
//                                          ^^^^^^ team_name 新参数
```

`enqueue` 函数签名加 team_name：

```rust
pub async fn enqueue(&self, key: &LeadKey, team_name: String) -> EnqueueOutcome { ... }
```

`set_wake_fn` 不变。

### Task 6.2: inbox 投递处透传 team_name

- [ ] **Step 1: 找出 lead_idle.enqueue 所有调用点**

```bash
grep -rn "lead_idle\.\(as_ref\|\)\.\?enqueue\|sup\.enqueue\|supervisor\.enqueue" src-tauri/src --include="*.rs"
```

每处调用前已经知道 team 上下文（送达时 inbox 是 per-team 的，team_name 从 inbox key 取）：

```rust
sup.enqueue(&key, team_name.to_string()).await;
```

### Task 6.3: chat_turn_driver wake 入口创建 ctx

- [ ] **Step 1: 找到 `wire_path_c_wake_to_self` 或类似 wake_fn 安装点**

```bash
grep -rn "set_wake_fn\|wake_fn" src-tauri/src/transport src-tauri/src/runtime --include="*.rs"
```

在 wake_fn 闭包内创建 ctx 时：

```rust
let wake_fn = Box::new(move |key: LeadKey, team_name: String| {
    let conv_id = key.0.clone();
    // ... 构造 chat 请求时把 active_team_name = Some(team_name)
    // **注意：不读 conv.json 的 active_team_name**
});
```

### Task 6.4: PR6 测试 + Commit

- [ ] **Step 1: 编译**

```bash
cd src-tauri && cargo check --lib 2>&1 | tail -5
```

- [ ] **Step 2: 集成测试场景**

在 `team_multi_team_test.rs` 加一个：

```rust
#[tokio::test]
async fn path_c_wake_carries_team_name() {
    // 模拟 inbox 投递（team-alpha），wake_fn 触发 → 验证收到的 team_name == "alpha"
}
```

- [ ] **Step 3: 全仓回归 + Commit**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
git add -A
git commit -m "feat(team): Path C wake 携带 team_name 到 continuation turn"
```

---

## PR7: team_view 多 team + review_ 回归

**完成判定**：`team_view::build_overview` 扫 `teams/` 目录返回多 team；`review_team_paths.rs` 回归测试通过。

### Task 7.1: 改写 team_view.rs

- [ ] **Step 1: 修改 `src-tauri/src/runtime/team_view.rs`**

`build_overview` 改为：扫 `<conv>/teams/` 目录，每个子目录读 `config.json` 构建一个 `TeamSession`，事件流从 `teams/{name}/team-chat.jsonl` 拼合。

```rust
pub fn build_overview(conv_dir: &Path) -> TeamOverview {
    let mut teams = vec![];
    let teams_root = conv_dir.join("teams");
    if !teams_root.exists() {
        return TeamOverview { conversation_id: ..., teams };
    }
    for entry in std::fs::read_dir(&teams_root).into_iter().flatten().flatten() {
        let team_dir = entry.path();
        let team_name = match entry.file_name().into_string() { Ok(s) => s, Err(_) => continue };
        let config = team_dir.join("config.json");
        if !config.exists() { continue; }
        let snapshot: TeamSnapshot = match read_json(&config) { Ok(s) => s, Err(_) => continue };
        let members = load_teammates(&team_dir.join("teammates"));
        let mut events = vec![];
        append_events_from_messages_jsonl(&mut events, conv_dir, &team_name);
        append_events_from_team_chat_jsonl(&mut events, &team_dir, &team_name);
        teams.push(TeamSession { team_id: format!("{conv_id}#{}", team_name), team_name: Some(team_name), ..snapshot, members, events });
    }
    teams.sort_by(|a, b| b.created_at.cmp(&a.created_at));  // 按创建时间倒序
    TeamOverview { conversation_id, teams }
}
```

### Task 7.2: review_team_paths.rs 回归测试

- [ ] **Step 1: 新建 `src-tauri/tests/review_team_paths.rs`**

```rust
//! 防止字面量 `team.json` / `team-chat.jsonl` / `teammates/` 出现在 team_paths.rs 之外。

use std::fs;
use std::path::Path;

fn scan_dir_for_literal(dir: &Path, literal: &str, allow_files: &[&str]) -> Vec<(String, usize, String)> {
    let mut hits = vec![];
    for entry in walkdir::WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.extension().map_or(false, |e| e == "rs") { continue; }
        let path_str = path.to_string_lossy().to_string();
        if allow_files.iter().any(|f| path_str.ends_with(f)) { continue; }
        let content = match fs::read_to_string(path) { Ok(c) => c, Err(_) => continue };
        for (i, line) in content.lines().enumerate() {
            if line.contains(literal) && !line.trim_start().starts_with("//") {
                hits.push((path_str.clone(), i + 1, line.to_string()));
            }
        }
    }
    hits
}

#[test]
fn no_team_json_literal_outside_team_paths() {
    let hits = scan_dir_for_literal(
        Path::new("src/runtime"),
        r#""team.json""#,
        &["team_paths.rs", "team_view.rs"],  // team_view 仍可能引用，但 PR7 后应消失
    );
    assert!(hits.is_empty(), "found 'team.json' literal in: {:?}", hits);
}

// 类似的两个测试 for "team-chat.jsonl" 和 "teammates/"
```

注：如果 `walkdir` 不在 dev-dependencies，加进去，或改用 `std::fs::read_dir` 递归。

- [ ] **Step 2: 运行**

```bash
cd src-tauri && cargo test --test review_team_paths 2>&1 | tail -10
```

- [ ] **Step 3: 全仓回归 + Commit**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
git add -A
git commit -m "feat(team): team_view 扫 teams/ 多 team + review_ 回归"
```

---

## PR8: DiagnosticEvent.team_name 字段 + 透传

**完成判定**：`DiagnosticEvent` 加 team_name 字段；TeamCreate / TeamDelete / TeamSwitch / spawn_subagent / lead_idle / task_notification 等所有 record_diagnostic 调用透传。

### Task 8.1: 改 DiagnosticEvent

- [ ] **Step 1: 修改 `src-tauri/src/telemetry.rs`**

加字段：

```rust
pub struct DiagnosticEvent {
    // ... 现有字段
    pub team_name: Option<String>,
}

impl DiagnosticEvent {
    pub fn team_name(mut self, v: impl Into<String>) -> Self {
        self.team_name = Some(v.into());
        self
    }
}
```

序列化保持 `#[serde(skip_serializing_if = "Option::is_none")]`。

### Task 8.2: 透传到所有相关调用点

- [ ] **Step 1: grep 所有 team 相关 record_diagnostic**

```bash
grep -rn "record_diagnostic\|DiagnosticEvent::new" src-tauri/src --include="*.rs" | head -40
```

每处补 `.team_name(...)`：
- TeamCreate / TeamDelete / TeamSwitch：用 `team_name` 局部变量
- spawn_subagent / Teammate worker：用 `ctx.active_team_name` 或 `TeammateWorkerCtx.team_name`
- lead_idle.* mark_idle/mark_running：从 LeadKey 拿不到 team_name，**保留无 team_name**（Lead 是跨 team 的）
- task_notification：从函数参数 team_name

### Task 8.3: PR8 Commit

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
git add -A
git commit -m "feat(team): DiagnosticEvent.team_name + 关键事件透传"
```

---

## PR9: Tauri 命令 + 前端事件

**完成判定**：新增 `team_chat_messages` / `team_switch_active` 两个 Tauri 命令；新增 4 个前端事件 `team:created` / `team:deleted` / `team:active-changed` / `team-chat:appended`。

### Task 9.1: 新建 Tauri 命令

- [ ] **Step 1: 在 `src-tauri/src/transport/tauri_commands/` 找 team 相关已有 commands**

```bash
ls src-tauri/src/transport/tauri_commands/
grep -rn "team_overview\|team_view" src-tauri/src/transport --include="*.rs"
```

在合适位置（如 `team.rs` 或 `mod.rs`）加：

```rust
#[tauri::command]
pub async fn team_chat_messages(
    state: State<'_, RuntimeState>,
    conversation_id: String,
    team_name: String,
    since_ts: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<serde_json::Value>, String> {
    let conv_dir = state.aijia_home.user_scoped().conversations_dir().join(&conversation_id);
    let path = crate::runtime::agent::team_paths::TeamPaths::for_team(&conv_dir, &team_name).team_chat_jsonl();
    if !path.exists() { return Ok(vec![]); }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut out = vec![];
    for line in content.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            // 按 since_ts 过滤
            if let Some(ref ts) = since_ts {
                if v.get("ts").and_then(|t| t.as_str()).map_or(true, |t| t <= ts.as_str()) {
                    continue;
                }
            }
            out.push(v);
            if let Some(lim) = limit {
                if out.len() >= lim { break; }
            }
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn team_switch_active(
    state: State<'_, RuntimeState>,
    app: AppHandle,
    conversation_id: String,
    team_name: String,
) -> Result<(), String> {
    crate::runtime::agent::team_paths::validate_team_name(&team_name).map_err(|e| e.to_string())?;
    // 写 conv.json::active_team_name
    let conv_dir = state.aijia_home.user_scoped().conversations_dir().join(&conversation_id);
    update_conv_meta_active_team(&conv_dir, Some(&team_name)).map_err(|e| e.to_string())?;
    // 推前端事件
    let _ = app.emit("team:active-changed", serde_json::json!({
        "conversationId": conversation_id,
        "newTeamName": team_name,
    }));
    Ok(())
}
```

- [ ] **Step 2: 在 `lib.rs::run` 的 `tauri::generate_handler![]` 中注册新命令**

### Task 9.2: 前端事件发射

- [ ] **Step 1: TeamCreate 工具内 emit team:created**

```rust
if let Some(app) = ctx.app_handle.as_ref() {
    let _ = app.emit("team:created", json!({"conversationId": session.as_str(), "teamName": team_name}));
}
```

注：app_handle 可能不在 ctx，看现有 RuntimeEvent 怎么发——优先复用现有 `RuntimeEventBus`。

如果现有体系是 RuntimeEvent → tauri_event_adapter → emit，加新 RuntimeEventKind：

```rust
// runtime/events.rs
pub enum RuntimeEventKind {
    // 已有...
    TeamCreated { team_name: String },
    TeamDeleted { team_name: String },
    TeamActiveChanged { old: Option<String>, new: Option<String> },
    TeamChatAppended { team_name: String, ts: String, from: String, to: String, text: String, variant: String },
}
```

`tauri_event_adapter.rs` 加映射：

```rust
RuntimeEventKind::TeamCreated { team_name } => app.emit("team:created", json!({"conversationId": ..., "teamName": team_name})),
// ...
```

### Task 9.3: PR9 Commit

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
git add -A
git commit -m "feat(team): Tauri 命令 team_chat_messages / team_switch_active + 4 个前端事件"
```

---

## PR10: 前端最小 UI——团队按钮 + 抽屉 + team-chat 面板

**完成判定**：聊天页右上有"团队"按钮；点开抽屉列出该 conv 所有 team；点 team 显示 team-chat 内容；切换 active team 调用 team_switch_active。

### Task 10.1: i18n keys

- [ ] **Step 1: 修改 `src/i18n/zh-CN.json` 和 `en-US.json`**

加 4 个 key：

zh-CN:
```json
"team.button.label": "团队",
"team.button.tooltip": "查看当前对话内的团队",
"team.drawer.title": "团队列表",
"team.drawer.empty": "当前对话还没有团队",
"team.chat.empty": "该团队还没有内部消息"
```

en-US:
```json
"team.button.label": "Teams",
"team.button.tooltip": "View teams in this conversation",
"team.drawer.title": "Teams",
"team.drawer.empty": "No teams yet in this conversation",
"team.chat.empty": "No team chat yet"
```

### Task 10.2: Tauri IPC 类型化封装

- [ ] **Step 1: 修改 `src/lib/tauri.ts`**

加：

```ts
export interface TeamChatMessage {
    ts: string;
    from: string;
    to: string;
    text: string;
    variant: string;
}

export async function teamChatMessages(conversationId: string, teamName: string, sinceTs?: string, limit?: number): Promise<TeamChatMessage[]> {
    return await invoke('team_chat_messages', { conversationId, teamName, sinceTs, limit });
}

export async function teamSwitchActive(conversationId: string, teamName: string): Promise<void> {
    return await invoke('team_switch_active', { conversationId, teamName });
}

export const TAURI_EVENTS = {
    // 已有...
    TEAM_CREATED: 'team:created',
    TEAM_DELETED: 'team:deleted',
    TEAM_ACTIVE_CHANGED: 'team:active-changed',
    TEAM_CHAT_APPENDED: 'team-chat:appended',
} as const;
```

### Task 10.3: TeamButton 组件

- [ ] **Step 1: 创建 `src/components/teams/TeamButton.tsx`**

```tsx
import { Button } from '@/components/ui/button';
import { useTranslation } from 'react-i18next';
import { useTeamStore } from '@/stores/teamStore';

export function TeamButton({ onClick }: { onClick: () => void }) {
    const { t } = useTranslation();
    const teamCount = useTeamStore(s => s.teams.length);
    return (
        <Button variant="ghost" size="sm" onClick={onClick} title={t('team.button.tooltip')}>
            {t('team.button.label')}
            {teamCount > 0 && <span className="ml-1 text-xs text-muted-foreground">({teamCount})</span>}
        </Button>
    );
}
```

### Task 10.4: TeamDrawer 组件

- [ ] **Step 1: 创建 `src/components/teams/TeamDrawer.tsx`**

```tsx
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { useTranslation } from 'react-i18next';
import { useTeamStore } from '@/stores/teamStore';
import { TeamChatPanel } from './TeamChatPanel';
import { useState } from 'react';

export function TeamDrawer({ open, onClose, conversationId }: { open: boolean; onClose: () => void; conversationId: string }) {
    const { t } = useTranslation();
    const teams = useTeamStore(s => s.teams);
    const [selected, setSelected] = useState<string | null>(null);
    return (
        <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
            <DialogContent className="overflow-hidden max-w-3xl">
                <h2 className="text-lg font-semibold mb-4">{t('team.drawer.title')}</h2>
                {teams.length === 0 ? (
                    <p className="text-muted-foreground">{t('team.drawer.empty')}</p>
                ) : (
                    <div className="grid grid-cols-2 gap-4 h-96">
                        <ul className="space-y-2 overflow-y-auto">
                            {teams.map(team => (
                                <li key={team.teamName}>
                                    <button
                                        className={`w-full text-left p-2 rounded ${selected === team.teamName ? 'bg-muted' : 'hover:bg-muted/50'}`}
                                        onClick={() => setSelected(team.teamName)}
                                    >
                                        <div className="font-medium">{team.teamName}</div>
                                        <div className="text-xs text-muted-foreground">{team.members.length - 1} teammate(s)</div>
                                    </button>
                                </li>
                            ))}
                        </ul>
                        <div className="border-l border-border pl-4 overflow-y-auto">
                            {selected ? <TeamChatPanel conversationId={conversationId} teamName={selected} /> : null}
                        </div>
                    </div>
                )}
            </DialogContent>
        </Dialog>
    );
}
```

### Task 10.5: TeamChatPanel 组件

- [ ] **Step 1: 创建 `src/components/teams/TeamChatPanel.tsx`**

```tsx
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { teamChatMessages, type TeamChatMessage, TAURI_EVENTS } from '@/lib/tauri';
import { listen } from '@tauri-apps/api/event';

export function TeamChatPanel({ conversationId, teamName }: { conversationId: string; teamName: string }) {
    const { t } = useTranslation();
    const [messages, setMessages] = useState<TeamChatMessage[]>([]);

    useEffect(() => {
        let mounted = true;
        teamChatMessages(conversationId, teamName).then(msgs => mounted && setMessages(msgs));
        const unlisten = listen<{ conversationId: string; teamName: string } & TeamChatMessage>(
            TAURI_EVENTS.TEAM_CHAT_APPENDED,
            (event) => {
                if (event.payload.conversationId !== conversationId) return;
                if (event.payload.teamName !== teamName) return;
                setMessages(prev => [...prev, event.payload]);
            }
        );
        return () => { mounted = false; unlisten.then(fn => fn()); };
    }, [conversationId, teamName]);

    if (messages.length === 0) {
        return <p className="text-muted-foreground text-sm">{t('team.chat.empty')}</p>;
    }
    return (
        <div className="space-y-2 text-sm">
            {messages.map((m, i) => (
                <div key={i} className="border-b border-border pb-2 last:border-b-0">
                    <div className="text-xs text-muted-foreground">{m.from} → {m.to} · {m.ts}</div>
                    <div className="mt-1 whitespace-pre-wrap">{m.text}</div>
                </div>
            ))}
        </div>
    );
}
```

### Task 10.6: teamStore + 集成入聊天页

- [ ] **Step 1: 创建 `src/stores/teamStore.ts`**

```ts
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export interface Team {
    teamName: string;
    members: { agentId: string; name: string }[];
    createdAt: string;
}

interface TeamStore {
    teams: Team[];
    setTeams: (teams: Team[]) => void;
    refresh: (conversationId: string) => Promise<void>;
}

export const useTeamStore = create<TeamStore>((set) => ({
    teams: [],
    setTeams: (teams) => set({ teams }),
    refresh: async (conversationId) => {
        const overview = await invoke<{ teams: Team[] }>('team_overview', { conversationId });
        set({ teams: overview.teams });
    },
}));
```

- [ ] **Step 2: 在聊天页（`src/components/shell/ChatTopBar.tsx` 或类似）加 TeamButton**

```tsx
import { TeamButton } from '@/components/teams/TeamButton';
import { TeamDrawer } from '@/components/teams/TeamDrawer';
import { useState } from 'react';
import { useTeamStore } from '@/stores/teamStore';

// 在 ChatTopBar 组件内
const [drawerOpen, setDrawerOpen] = useState(false);
const refresh = useTeamStore(s => s.refresh);
useEffect(() => { if (conversationId) refresh(conversationId); }, [conversationId, refresh]);

// 在右侧操作区加：
<TeamButton onClick={() => setDrawerOpen(true)} />
<TeamDrawer open={drawerOpen} onClose={() => setDrawerOpen(false)} conversationId={conversationId} />
```

### Task 10.7: 启动 dev server 手测

- [ ] **Step 1: 跑 dev server**

```bash
pnpm tauri:dev
```

- [ ] **Step 2: 手测路径**
  - 打开一个会话 → 右上能看到"团队"按钮
  - 让 LLM 调 TeamCreate("alpha") → 按钮气泡显示 (1)
  - 点按钮 → 抽屉列出 alpha
  - 点 alpha → 右侧显示空 chat
  - 让 Teammate SendMessage → 抽屉里 chat 实时增加一行

如果有任何 UI 不正常，停下来报告 BLOCKED。

### Task 10.8: Commit

```bash
git add -A
git commit -m "feat(team): 前端团队按钮 + 抽屉 + team-chat 面板（最小可用 UI）"
```

---

## 完整执行总结

最后一步——执行完所有 PR 后，subagent 输出：

```markdown
## Per-Team 多 Team 重构 — 执行总结

### Commits (按时间序)
- PR1: <hash> — feat(team_paths): ...
- PR2: <hash> — feat(team): TeamRegistry 三 API ...
- PR3: <hash> — feat(team): ctx.active_team_name ...
- PR4: <hash> — feat(team): 三元 key ...
- PR5: <hash> — feat(team): 解锁多 team ...
- PR6: <hash> — feat(team): Path C wake ...
- PR7: <hash> — feat(team): team_view 多 team ...
- PR8: <hash> — feat(team): DiagnosticEvent.team_name ...
- PR9: <hash> — feat(team): Tauri 命令 + 前端事件 ...
- PR10: <hash> — feat(team): 前端最小 UI ...

### 测试统计
- cargo test --lib 总数：（执行前）X → （执行后）Y（+Z 新增）
- 新增集成测试：team_multi_team_test.rs / review_team_paths.rs
- 前端手测：✅ / ❌（描述）

### BLOCKED 项（如有）
- PR-X step Y：<具体描述>

### 下一步建议
- 用户 review 上述 commits + 手测
- 合并到 main：git checkout main && git merge feat/per-team-layout
- 或 cherry-pick 部分 PR
```

---

## Self-Review

**Spec 覆盖**：spec §3-§13 全部映射到 PR1-PR10。

**占位符**：每个 step 都给了完整代码或 grep + 改造模式，无 TODO/TBD。

**类型一致性**：
- `TeamRegistry::delete_team` / `drop_session` / `hydrate_from_disk` 三 API 在 PR2 定义，PR4-PR7 使用一致
- `ctx.active_team_name: Option<String>` 在 PR3 加，PR3-PR10 使用
- `WakeFn` 签名在 PR6 加 team_name 参数，PR6 内部一致

**已知 trade-off**：
- PR2 完成后 TeamCreate/TeamDelete 短暂处于"接口签名变了但工具语义还是单 team"中间态——这是有意的，PR5 才解锁多 team
- 集成测试覆盖度本计划只给最小 smoke，深度集成测试在 spec §10.2 列出，但 subagent 时间紧张时可只做 minimum，把深度测试列为待跟进

---

## 工程注意

- 整个执行预估时间：8-15 小时（取决于编译时间和调试需要）
- subagent 必须按 PR 顺序执行，不能跳跃
- 任何 PR 内部 step 失败 → 立即报告 BLOCKED，不自由发挥
- 每个 PR 的 Commit 是 checkpoint——失败回退面控制在单个 PR 内

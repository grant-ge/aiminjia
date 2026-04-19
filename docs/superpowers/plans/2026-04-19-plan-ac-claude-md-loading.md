# CLAUDE.md 项目记忆文件读取（Plan-AC）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 启动时和每次 turn 前按 `claude-code-best` 的 `getMemoryFiles -> getUserContext -> prependUserContext` 主线读取工作目录及父目录链的 CLAUDE.md 文件，并将其注入首条 user 上下文消息，使 LLM 感知项目级指令。

**Architecture:** 新建 `runtime/claude_md.rs` 实现目录遍历与文件读取；首阶段仅覆盖 `~/.claude/CLAUDE.md`、父目录链上的 `CLAUDE.md` / `.claude/CLAUDE.md` / `CLAUDE.local.md`；`chat_turn_driver.rs` 在组装首条 user meta/context message 时注入该内容，而不是塞进 generic dynamic_context。mtime-based 缓存是 lotus 的简化实现，用于避免重复 IO。

**Tech Stack:** Rust, tokio, std::fs

**Worktree branch:** `pzc`

---

## 对标修订（2026-04-19）

- 注入方式改为对齐 `claude-code-best` 的 `getUserContext + prependUserContext` 思路，而不是把多个文件内容直接拼进 generic dynamic_context。
- 第一阶段范围明确不含 managed memory、`.claude/rules/*.md`、`@include`；若后续补齐，需要单独扩计划。
- `mtime` 缓存保留，但明确它是 lotus 对 `memoize + file state cache` 的简化版本，不宣称实现同构。
- 后续如要完全对齐，还需补上 disable/bare/setting-source gate；本计划先把基础发现、排序、注入链路接通。

---

## 改造视角

> 本节说明每个子任务的"现状 → 目标"，帮助执行者理解这是增量新增而非重构。

### AC1：`ClaudeMdLoader` — 目录遍历 + 文件读取

**当前状态**：lotus-app 没有任何读取 CLAUDE.md 的逻辑。`system-reminder` 只注入日期（`chat_turn_driver.rs` L596-602）。

**目标状态**：新建 `src-tauri/src/runtime/claude_md.rs`，提供 `ClaudeMdLoader` struct 及 `load` 方法，返回按优先级排序的 `Vec<ClaudeMdFile>`（根目录最低，CWD/workspace 最高，用户全局 `~/.claude/CLAUDE.md` 优先级最低排在最前）。

**对标**：`claude-code-best/src/utils/claudemd.ts` `getMemoryFiles`（行 850–934）的目录遍历逻辑：从 `workspace_path` 向上到 `fs::root`，收集每层的 `CLAUDE.md` 和 `.claude/CLAUDE.md`；同时加载 `~/.claude/CLAUDE.md`（User 级别，排序最前）。

**遍历规则**（精简版，不含 worktree 分支判断，不含 `@include`）：
1. `~/.claude/CLAUDE.md`（User 级，若存在）
2. 从 root 向下到 `workspace_path`：每层尝试 `{dir}/CLAUDE.md`、`{dir}/.claude/CLAUDE.md`
3. 每层再尝试 `{dir}/CLAUDE.local.md`（Local 级，gitignored，独立于 Project 级）
4. 去重：已读路径 `HashSet` 过滤，规范化（canonicalize）后比较
5. 不存在的文件静默跳过（不报错）

---

### AC2：mtime-based 缓存

**当前状态**：无。

**目标状态**：`ClaudeMdLoader` 内部持有 `HashMap<PathBuf, (SystemTime, String)>` 缓存（路径 → (mtime, 内容)）。每次 `load` 调用时：
- `fs::metadata` 获取 mtime（失败则重新读取）
- mtime 未变则直接返回缓存内容
- mtime 变化则重新读取并更新缓存
- 若调用方使用 `Arc<Mutex<ClaudeMdLoader>>`，缓存在 turns 间共享

`ClaudeMdLoader` 设计为可被 `QueryEngine` 持有（类似 `FileStateCache` 模式），turn 间复用同一实例。

---

### AC3：注入 dynamic context

**当前状态**：`chat_turn_driver.rs` L596-602 只注入日期到 `system_reminder_message`。

**目标状态**：在日期注入的同一 `system_reminder_message` 中追加 CLAUDE.md 内容，格式与 claude-code-best 对齐：

```
<system-reminder>
今天是 2026年04月19日（2026-04-19）。
{file_path}:
{content}

{file_path_2}:
{content_2}
</system-reminder>
```

**调用链**：
1. `chat_turn_driver.rs` 在构建 `system_reminder_message` 前，通过 `executor`（`TauriLegacyTurnExecutor`）拿到 `workspace_path`（需在 `TurnConfig` 中填充，目前是 `PathBuf::new()`，见 L584）
2. 调用 `ClaudeMdLoader::load(workspace_path)` — loader 通过 Arc 注入到 executor 或 QueryEngine
3. 把有内容的文件按顺序追加到 system_reminder 字符串
4. 若未找到任何 CLAUDE.md，不注入（只有日期）

**依赖修复**：`TurnConfig::workspace_path` 当前值为 `PathBuf::new()`（空路径，见 `chat_turn_driver.rs` L584）。需在 `TauriChatCommandAdapter::send_message` 中从 `AppStorage` / `FileManager` 读取实际 workspace_path 并填入 `TurnConfig`。此 gap 在 env_info 改造中已记录，本任务一并修复（若 Plan-AC 执行时仍未修复）。

---

### AC4：review 约束测试

测试文件：`src-tauri/tests/plan_ac_claude_md_test.rs`

目标：
1. 验证 `ClaudeMdLoader` 不 `use tauri::*`（架构约束）
2. 验证目录遍历逻辑（给定 temp 目录结构，断言加载顺序和内容）
3. 验证 mtime 缓存命中（同一文件第二次 `load` 不重新读磁盘）
4. 验证注入格式（`<system-reminder>` 包裹、路径前缀）

---

## 背景与问题诊断

### 现状代码定位

| 文件 | 关键行 | 问题 |
|------|--------|------|
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | L584 | `workspace_path: std::path::PathBuf::new()` — 空路径，env_info 等依赖 workspace_path 的功能均无法正常工作 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | L596-602 | `system_reminder_message` 只含日期，CLAUDE.md 内容缺失 |
| `src-tauri/src/runtime/mod.rs` | — | 无 `claude_md` 模块声明 |

### claude-code-best 对标分析

claude-code-best 的注入路径：
- `getMemoryFiles()` 收集所有文件，结果注入第一条 user message 的 `<system-reminder>` block
- 注入格式：每个文件独立 `{file_path}:\n{content}` 段落，多个文件换行分隔
- 缓存：通过 `memoize` 包装，内部按 mtime 失效

lotus-app 差异：
- 无 CWD 概念（Tauri 桌面应用），以 `workspace_path`（用户选择的目录）作为 CWD 等价物
- 不需要 worktree 分支判断（无 `git worktree` 使用场景）
- 不需要 `@include` 指令处理（范围外，后续可扩展）
- 不需要 `rules/*.md` 子目录（范围外）

---

## Task AC1：ClaudeMdLoader — 目录遍历 + 文件读取

**测试文件：** `src-tauri/tests/plan_ac_claude_md_test.rs`

### 步骤

- [ ] **AC1-1 写失败测试**

  在 `src-tauri/tests/plan_ac_claude_md_test.rs` 创建测试骨架：

  ```rust
  // 验证：从给定 workspace_path 向上遍历，收集所有 CLAUDE.md 文件
  #[tokio::test]
  async fn test_load_project_claude_md_from_workspace() {
      let tmp = tempfile::tempdir().unwrap();
      let workspace = tmp.path().join("project");
      std::fs::create_dir_all(&workspace).unwrap();
      std::fs::write(workspace.join("CLAUDE.md"), "# Project\nproject instructions").unwrap();

      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();
      let files = loader.load(&workspace).await;

      let project_file = files.iter().find(|f| f.path.ends_with("CLAUDE.md"));
      assert!(project_file.is_some(), "should find workspace CLAUDE.md");
      assert!(project_file.unwrap().content.contains("project instructions"));
  }

  #[tokio::test]
  async fn test_load_order_root_before_workspace() {
      // 根目录文件在前，workspace 文件在后（workspace 优先级最高排最后）
      let tmp = tempfile::tempdir().unwrap();
      let parent = tmp.path().join("parent");
      let child = parent.join("child");
      std::fs::create_dir_all(&child).unwrap();
      std::fs::write(parent.join("CLAUDE.md"), "parent instructions").unwrap();
      std::fs::write(child.join("CLAUDE.md"), "child instructions").unwrap();

      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();
      let files = loader.load(&child).await;

      assert!(files.len() >= 2);
      let contents: Vec<&str> = files.iter().map(|f| f.content.as_str()).collect();
      let parent_pos = contents.iter().position(|c| c.contains("parent instructions")).unwrap();
      let child_pos = contents.iter().position(|c| c.contains("child instructions")).unwrap();
      assert!(parent_pos < child_pos, "parent should come before child (lower priority first)");
  }

  #[tokio::test]
  async fn test_load_dot_claude_claude_md() {
      let tmp = tempfile::tempdir().unwrap();
      let workspace = tmp.path().join("project");
      let dot_claude = workspace.join(".claude");
      std::fs::create_dir_all(&dot_claude).unwrap();
      std::fs::write(dot_claude.join("CLAUDE.md"), "dot-claude instructions").unwrap();

      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();
      let files = loader.load(&workspace).await;

      assert!(files.iter().any(|f| f.content.contains("dot-claude instructions")));
  }

  #[tokio::test]
  async fn test_load_local_claude_md() {
      let tmp = tempfile::tempdir().unwrap();
      let workspace = tmp.path().to_path_buf();
      std::fs::write(workspace.join("CLAUDE.local.md"), "local override").unwrap();

      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();
      let files = loader.load(&workspace).await;

      assert!(files.iter().any(|f| f.content.contains("local override")));
  }

  #[tokio::test]
  async fn test_no_files_returns_empty() {
      let tmp = tempfile::tempdir().unwrap();
      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();
      let files = loader.load(tmp.path()).await;
      // 无 CLAUDE.md 时结果为空（或只有用户全局文件，若存在）
      // 不应 panic
      let _ = files;
  }
  ```

  运行确认编译失败（模块不存在）：
  ```bash
  cd src-tauri && cargo test plan_ac_claude_md -- --nocapture 2>&1 | tail -20
  ```

- [ ] **AC1-2 实现 `src-tauri/src/runtime/claude_md.rs`**

  新建文件，实现以下结构：

  ```rust
  //! CLAUDE.md 项目记忆文件加载器。
  //!
  //! 从 workspace_path 向上遍历目录链，收集所有 CLAUDE.md /
  //! .claude/CLAUDE.md / CLAUDE.local.md 文件，并加载用户全局
  //! ~/.claude/CLAUDE.md。结果按优先级排序（根目录最低，workspace 最高）。
  //!
  //! 注意：此模块禁止 `use tauri::*`（runtime 层架构约束）。

  use std::collections::HashSet;
  use std::path::{Path, PathBuf};
  use std::time::SystemTime;
  use std::collections::HashMap;

  /// 一个已加载的 CLAUDE.md 文件。
  #[derive(Debug, Clone)]
  pub struct ClaudeMdFile {
      /// 文件的绝对路径。
      pub path: PathBuf,
      /// 文件内容（已读入内存）。
      pub content: String,
  }

  /// CLAUDE.md 加载器，带 mtime-based 缓存。
  ///
  /// 设计为可被 QueryEngine / TurnExecutor 持有并跨 turn 复用。
  pub struct ClaudeMdLoader {
      /// 路径 → (上次读取时的 mtime, 内容)
      cache: HashMap<PathBuf, (SystemTime, String)>,
  }

  impl ClaudeMdLoader {
      pub fn new() -> Self { ... }

      /// 遍历 `workspace_path` 向上到根目录，收集所有 CLAUDE.md 变体。
      /// 结果顺序：用户全局（最低优先），根目录，…，workspace（最高优先）。
      pub async fn load(&mut self, workspace_path: &Path) -> Vec<ClaudeMdFile> { ... }

      // ── 内部 ────────────────────────────────────────────────────────────

      /// 读取单个文件，带 mtime 缓存。文件不存在时返回 None。
      fn read_with_cache(&mut self, path: &Path) -> Option<String> { ... }

      /// 获取用户主目录路径（~）。
      fn home_dir() -> Option<PathBuf> { ... }
  }
  ```

  **遍历算法**：
  ```
  1. home_dir().map(|h| h.join(".claude").join("CLAUDE.md")) → User 级，若存在加入 result
  2. 收集目录链：from workspace_path → parent → ... → root，push 到 dirs vec
  3. dirs.reverse()（使得 root 在前，workspace 在后）
  4. for dir in dirs:
       try_add(dir/CLAUDE.md, &mut processed)
       try_add(dir/.claude/CLAUDE.md, &mut processed)
       try_add(dir/CLAUDE.local.md, &mut processed)
  5. 返回 result（User 最前，workspace 最后）
  ```

  `processed: HashSet<PathBuf>` 使用 `fs::canonicalize` 结果去重（canonicalize 失败时用原始路径）。

- [ ] **AC1-3 在 `runtime/mod.rs` 声明模块**

  在 `src-tauri/src/runtime/mod.rs` 加入：
  ```rust
  pub mod claude_md;
  ```

- [ ] **AC1-4 运行测试确认通过**

  ```bash
  cd src-tauri && cargo test plan_ac_claude_md -- --nocapture 2>&1 | tail -30
  ```

---

## Task AC2：mtime-based 缓存

（缓存实现已包含在 AC1-2 的 `read_with_cache` 中，本 task 专门补充缓存命中测试。）

### 步骤

- [ ] **AC2-1 写缓存命中测试**

  在 `src-tauri/tests/plan_ac_claude_md_test.rs` 追加：

  ```rust
  #[tokio::test]
  async fn test_mtime_cache_hit_on_second_load() {
      let tmp = tempfile::tempdir().unwrap();
      let workspace = tmp.path().to_path_buf();
      std::fs::write(workspace.join("CLAUDE.md"), "version 1").unwrap();

      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();

      // 第一次加载
      let files1 = loader.load(&workspace).await;
      assert!(files1.iter().any(|f| f.content.contains("version 1")));

      // 文件未修改，第二次加载应命中缓存（内容相同）
      let files2 = loader.load(&workspace).await;
      assert_eq!(
          files1.iter().find(|f| f.path.ends_with("CLAUDE.md")).unwrap().content,
          files2.iter().find(|f| f.path.ends_with("CLAUDE.md")).unwrap().content,
      );
  }

  #[tokio::test]
  async fn test_mtime_cache_invalidate_on_change() {
      let tmp = tempfile::tempdir().unwrap();
      let workspace = tmp.path().to_path_buf();
      let file_path = workspace.join("CLAUDE.md");
      std::fs::write(&file_path, "version 1").unwrap();

      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();
      let files1 = loader.load(&workspace).await;
      assert!(files1.iter().any(|f| f.content.contains("version 1")));

      // 修改文件（写新内容后 touch，确保 mtime 变化）
      std::thread::sleep(std::time::Duration::from_millis(10));
      std::fs::write(&file_path, "version 2").unwrap();

      let files2 = loader.load(&workspace).await;
      assert!(
          files2.iter().any(|f| f.content.contains("version 2")),
          "cache should be invalidated after file change"
      );
  }
  ```

  运行：
  ```bash
  cd src-tauri && cargo test plan_ac_claude_md -- --nocapture 2>&1 | tail -30
  ```

---

## Task AC3：注入 dynamic context

### 步骤

- [ ] **AC3-1 修复 `TurnConfig::workspace_path`**

  **问题**：`chat_turn_driver.rs` L584 `workspace_path: std::path::PathBuf::new()`（空路径）。

  **定位**：`workspace_path` 从 `AppStorage` 获取。`TauriChatCommandAdapter` 在 `transport/tauri_commands/chat.rs` 中构建 `TurnConfig`，它持有 `AppStorage`（或 `FileManager`）的引用。

  **修改**：在 `TauriChatCommandAdapter::run_chat_turn`（或等价的 executor 初始化处）读取 `app_storage.workspace_path()`，填入 `TurnConfig`：

  ```rust
  workspace_path: self.app_storage.workspace_path().to_path_buf(),
  ```

  运行现有测试确认无回归：
  ```bash
  cd src-tauri && cargo test -- --nocapture 2>&1 | grep -E "FAILED|ok" | tail -20
  ```

- [ ] **AC3-2 将 `ClaudeMdLoader` 注入 executor**

  **方案**：在 `TauriLegacyTurnExecutor`（`transport/tauri_commands/chat.rs`）中新增字段：
  ```rust
  claude_md_loader: Arc<tokio::sync::Mutex<ClaudeMdLoader>>,
  ```

  在 executor 构建时初始化（`ClaudeMdLoader::new()` 包装为 `Arc<Mutex<...>>`）。

  若 executor 是每次 turn 新建的（而非长期持有），则由 `TauriChatCommandAdapter` 或 `SessionRuntime` 持有 loader 并注入。选择与 `FileStateCache` 相同的持有层级（见 Plan-B B1 的设计）。

- [ ] **AC3-3 在 `chat_turn_driver.rs` 注入 CLAUDE.md 到 system_reminder**

  定位 L596-602 的 `system_reminder_message` 构建。

  **修改**：在构建 `system_reminder_message` 之前，调用 loader 加载 CLAUDE.md 文件，并将内容追加到 system_reminder 字符串：

  ```rust
  // 加载 CLAUDE.md 文件
  let claude_md_section = if !config.workspace_path.as_os_str().is_empty() {
      let files = executor.load_claude_md(&config.workspace_path).await;
      if files.is_empty() {
          String::new()
      } else {
          let mut s = String::new();
          for f in &files {
              s.push('\n');
              s.push_str(&f.path.display().to_string());
              s.push_str(":\n");
              s.push_str(&f.content);
              s.push('\n');
          }
          s
      }
  } else {
      String::new()
  };

  let system_reminder_message = serde_json::json!({
      "role": "user",
      "content": format!(
          "<system-reminder>\n今天是 {}（{}）。{}\n</system-reminder>",
          today, today_iso, claude_md_section
      ),
  });
  ```

  `executor.load_claude_md(path)` 是在 `TauriLegacyTurnExecutor` 上新增的方法，内部 lock `claude_md_loader` 并调用 `load`。

- [ ] **AC3-4 集成验证**

  在 temp workspace 下创建 `CLAUDE.md`，启动 Tauri dev 并发送消息，通过日志确认 `system_reminder_message` 中包含 CLAUDE.md 内容：

  ```bash
  # 快速 smoke test（仅编译 + Rust 单测）
  cd src-tauri && cargo test plan_ac_claude_md -- --nocapture
  cd src-tauri && cargo test review_ --tests --no-fail-fast
  ```

---

## Task AC4：review 约束测试

**测试文件：** `src-tauri/tests/plan_ac_claude_md_test.rs`

### 步骤

- [ ] **AC4-1 架构约束测试**

  追加以下测试，验证 `claude_md` 模块的核心约束：

  ```rust
  /// 验证 ClaudeMdLoader::load 在 workspace_path 为空时不 panic。
  #[tokio::test]
  async fn review_claude_md_loader_empty_path_does_not_panic() {
      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();
      let files = loader.load(std::path::Path::new("")).await;
      // 空路径下应返回空列表（或只有用户全局文件），不 panic
      let _ = files;
  }

  /// 验证注入格式：每个文件以 "{path}:\n{content}" 格式出现。
  #[tokio::test]
  async fn review_claude_md_injection_format() {
      let tmp = tempfile::tempdir().unwrap();
      let workspace = tmp.path().to_path_buf();
      std::fs::write(workspace.join("CLAUDE.md"), "test content").unwrap();

      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();
      let files = loader.load(&workspace).await;

      for f in &files {
          // 每个文件有 path 和 content
          assert!(!f.path.as_os_str().is_empty());
          assert!(!f.content.is_empty());
      }
  }

  /// 验证去重：同一文件路径不被加载两次。
  #[tokio::test]
  async fn review_claude_md_no_duplicate_paths() {
      let tmp = tempfile::tempdir().unwrap();
      let workspace = tmp.path().to_path_buf();
      std::fs::write(workspace.join("CLAUDE.md"), "unique content").unwrap();

      let mut loader = lotus_app::runtime::claude_md::ClaudeMdLoader::new();
      let files = loader.load(&workspace).await;

      let mut seen = std::collections::HashSet::new();
      for f in &files {
          let canonical = f.path.canonicalize().unwrap_or_else(|_| f.path.clone());
          assert!(
              seen.insert(canonical.clone()),
              "duplicate path: {:?}",
              canonical
          );
      }
  }
  ```

- [ ] **AC4-2 运行全部 review 测试**

  ```bash
  cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
  cd src-tauri && cargo test plan_ac_claude_md -- --nocapture 2>&1 | tail -20
  ```

---

## 整体回归验证

每个子任务完成后运行：

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast
cd src-tauri && cargo test plan_ac_claude_md -- --nocapture
pnpm test
```

---

## 范围外（后续可扩展）

| 功能 | 说明 |
|------|------|
| `@include` 指令 | claude-code-best 支持在 CLAUDE.md 中用 `@path/to/file` 包含外部文件，lotus-app 暂不实现 |
| `.claude/rules/*.md` 目录 | claude-code-best 加载 rules 子目录的所有 `.md` 文件，范围外 |
| Worktree 分支判断 | lotus-app 无 git worktree 使用场景，跳过 |
| 前端配置开关 | 是否启用 CLAUDE.md 加载的设置项，后续可加 |
| 文件变更监听 | 用 `notify` crate 监听 CLAUDE.md 改动并自动失效缓存，后续可加 |

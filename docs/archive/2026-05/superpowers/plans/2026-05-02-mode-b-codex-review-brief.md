# Mode B 收尾（M2+M3）codex review 简报

**日期**：2026-05-02
**审查范围**：commits `cc2f8db..HEAD`（7 commits，8 文件，+771/-5）
**Master plan**：`docs/superpowers/plans/2026-04-30-lotus-subagent-mode-b-master-plan.md`
**Handoff**：`docs/superpowers/plans/2026-04-30-mode-b-progress-handoff.md`（含每片完整 spec：代码块、文件路径、验证命令、commit message、禁止清单）

## 0. 背景

Mode B = lotus-app 多代理（subagent）架构第 2 阶段（M2 收尾）+ 第 3 阶段（M3 测试覆盖与架构约束）。本次 7 片任务在前作 28 commits 基础上完成 P8.1c/d/e（task_output 工厂注册 + 集成测试 + launch_async 写 transcript）、P10.1 follow-up（Fixed-model 分支测试）、P10.2（async e2e）、P10.3（review_ 架构约束回归）、#43（browse_data SECURITY 注释）。

**已知 pre-existing failures**（已通过 cc2f8db checkout 验证，**非本批引入**，不在本次 review 范围）：
- `tests/review_single_loop_owner_test.rs::review_send_message_clears_gateway_busy_after_runtime_returns`
- `tests/review_sub_agent_background_reachability_test.rs::review_sub_agent_should_not_hardcode_foreground_child_runs`
- `storage::file_store::messages` 5 个失败（与 Mode B 无关）

## 1. Commits（按时间序）

| Commit | 片 | 主题 |
|---|---|---|
| `8efd75d` | 1 / P8.1c | task_output 工厂注册 + UserScopedPathResolver dyn-cast |
| `ed6f352` | 2 / P8.1d | task_output 集成测试（4 tests） |
| `6f1f3ca` | 3 / P8.1e | launch_async 接 output_writer + 填 AsyncTaskHandle.output_file |
| `6ee5af2` | 4 / P10.2 | async e2e（5 tests，StubLauncher 同步完成） |
| `b159752` | 5 / P10.1 follow-up | AgentModel::Fixed 分支测试（2 tests） |
| `ad21d29` | 6 / P10.3 | review_ 架构约束回归（3 tests） |
| `78235d7` | 7 / #43 follow-up | browse_data SECURITY 注释（9 行） |

`git log --oneline cc2f8db..HEAD` 应返回这 7 行。

## 2. 文件清单（diff --stat）

```
 src-tauri/src/lib.rs                               |   1 +
 src-tauri/src/llm/tool_executor/internal_system.rs |   9 +
 src-tauri/src/llm/tool_executor/spawn_subagent.rs  |  41 ++-
 src-tauri/src/plugin/registry.rs                   |  38 +++
 src-tauri/tests/e2e_spawn_subagent_async.rs        | 315 +++++++++++++++++++++
 src-tauri/tests/review_agent_b_constraints.rs      |  61 ++++
 src-tauri/tests/spawn_subagent_tool_basic_test.rs  | 143 ++++++++++
 src-tauri/tests/task_output_tool_test.rs           | 168 +++++++++++
 8 files changed, 771 insertions(+), 5 deletions(-)
```

## 3. 分批 review（建议 codex 顺序消化）

### 批 A — 生产代码（4 文件，89 行净增，**重点 review**）

| 文件 | LOC | 关注点 |
|---|---|---|
| `src-tauri/src/lib.rs` | +1 | UserScopedPathResolver dyn-cast 注册（与 schedule_runner 同样模式，第 562 行） |
| `src-tauri/src/plugin/registry.rs` | +38 | `task_output` 工厂分支 + `spawn_subagent` 加 path_resolver lookup；两处 fail-closed `return None` |
| `src-tauri/src/llm/tool_executor/spawn_subagent.rs` | +41/-3 | `DefaultSpawnSubagentLauncher` 加 paths 字段；`launch_async` 写 transcript（best-effort，empty PathBuf 兜底）；3 个 spawn 分支（Ok/Err/Panic）写 TranscriptLine；XML 第 3 参数从 "" 改 transcript path |
| `src-tauri/src/llm/tool_executor/internal_system.rs` | +9 | 纯注释：browse_data 子 agent allowed_tools SECURITY 注释 |

**review 命令**：
```bash
git show 8efd75d -- src-tauri/src/lib.rs src-tauri/src/plugin/registry.rs
git show 6f1f3ca -- src-tauri/src/llm/tool_executor/spawn_subagent.rs src-tauri/src/plugin/registry.rs
git show 78235d7 -- src-tauri/src/llm/tool_executor/internal_system.rs
```

**特别核对**：
1. **fail-closed vs best-effort 边界**：plugin/registry.rs 工厂层是 fail-closed（缺 state 时 `return None` + log::error!）；spawn_subagent.rs `launch_async` 在 tokio::spawn 内是 best-effort（缺 user scope 时 fallback 到 `PathBuf::new()` + log::warn!）。两处不同处理是有意的，是否一致表达？
2. **`output_writer::append_line(&PathBuf::new(), ...)` 的退化路径**：`path.parent()` → `Some("")` → `create_dir_all("")` Err → 整个调用 Err → `let _ = ...` 吞掉。silent 退化是否符合预期？是否需要 log？
3. **lifetime**：`let p_str = transcript_path_for_task.to_string_lossy(); ... &p_str` Cow<str> 在每个分支内本地创建，借用范围内保有效。
4. **递归保护边界**：browse_data 不在 SECURITY 注释提到的 hardcoded list 中是隐式安全边界 — 注释是否完整地表达了"如果列表改为动态，必须把 browse_data 加入 ALL_AGENT_DISALLOWED"这一约束？

### 批 B — 集成测试（3 文件，626 行新增）

| 文件 | LOC | 主题 |
|---|---|---|
| `src-tauri/tests/task_output_tool_test.rs` | +168 (new) | 4 tests：empty / offset 0 / offset 2 / incremental append |
| `src-tauri/tests/e2e_spawn_subagent_async.rs` | +315 (new) | 5 tests：返回值 / store register / notif enqueue / 无 name 跳过 register / state Completed |
| `src-tauri/tests/spawn_subagent_tool_basic_test.rs` | +143 | 2 新 tests + 1 helper：Fixed-model 解析 / caller-model override |

**review 命令**：
```bash
git show ed6f352 -- src-tauri/tests/task_output_tool_test.rs
git show 6ee5af2 -- src-tauri/tests/e2e_spawn_subagent_async.rs
git show b159752 -- src-tauri/tests/spawn_subagent_tool_basic_test.rs
```

**特别核对**：
1. **StubLauncher 同步完成**：`e2e_spawn_subagent_async.rs::launch_async` 内不调 `tokio::spawn`，register → update_state(Completed) → enqueue 全部 inline 完成后 `Ok(SpawnAsyncOutcome)`。这是为了避免 tokio 时序波动导致 flaky；但这意味着测试不覆盖真实 lifecycle 时序，是否需要补"真 tokio::spawn"测试？（注：本批不补；slice 3 的 `spawn_subagent_async_test` 已覆盖部分时序）
2. **`b159752` LOC 偏胖**（143 vs 60 估计）：两个测试各自内联定义 `ModelCapture` + `SpawnSubagentLauncher` impl（重复 ~30 行）。已知非阻塞建议：可提取到模块级。是否值得现在动？
3. **`task_output_tool_test.rs` 4 tests 是否覆盖到 `transcript_path` 不存在的边界**？测试 1 `returns_empty_for_nonexistent_task` 覆盖了。

### 批 C — 架构约束回归（1 文件，61 行）

| 文件 | LOC | 主题 |
|---|---|---|
| `src-tauri/tests/review_agent_b_constraints.rs` | +61 (new) | 3 tests：runtime/agent 不导 tauri / spawn_subagent 是 concurrency-safe / async agent 默认禁 ask_user_question |

**review 命令**：
```bash
git show ad21d29 -- src-tauri/tests/review_agent_b_constraints.rs
```

**特别核对**：
1. **`agent_modules_do_not_use_tauri_directly`**：硬编码 7 文件路径。如果未来 runtime/agent/ 加新文件，测试不会覆盖到。是否值得改成 walkdir 扫整个 `src/runtime/agent/`？（注：当前 7 文件覆盖了所有 Mode B 引入的；扩展是 follow-up）
2. **`async_agent_default_disallows_ask_user_question`**：`resolve_agent_tools(&[], &[], &["ask_user_question", "read_file"], true, false)` — 即 def_allowed=空（=全部允许）+ available 含 ask_user_question + is_async=true → 期望 ask_user_question 被剔除。这覆盖 ASYNC_AGENT_ALLOWED 子集过滤；是否还需要额外 case 验证 def_allowed 显式包含 ask_user_question 时也被剔除？
3. **`spawn_subagent_tool_is_concurrency_safe`**：`tool.is_concurrency_safe(&Value::Null) == true`。NoopLauncher 的两个 launch_* 都 `unreachable!()` — 仅校验声明属性，不校验运行时安全。是否值得加 stress test（多线程同时 execute 不 panic）？

## 4. 验证命令（所有应 PASS）

```bash
cd src-tauri && cargo build --tests 2>&1 | tail -3

# 7 片新增/触及的测试
cargo test --test task_output_tool_test                       # 4 pass
cargo test --test e2e_spawn_subagent_async                    # 5 pass
cargo test --test spawn_subagent_tool_basic_test              # 13 pass
cargo test --test review_agent_b_constraints                  # 3 pass

# 回归（不应破坏）
cargo test --test spawn_subagent_async_test \
           --test spawn_subagent_parallel_dispatch_test \
           --test e2e_spawn_subagent_explore \
           --test agent_registry_merge_test \
           --test task_notification_injection_test
cargo test --lib runtime::tools::builtin::task_output         # 7 pass
cargo test --lib runtime::agent::output_writer                # 7 pass

# review_ 系列（已知 2 个 pre-existing 失败，参见 §0）
cargo test review_ --tests --no-fail-fast
```

## 5. 已跳过/决议（明确不在范围）

- **P9.2 browse_data 兼容包装**：用户决议跳过，browse_data 维持 legacy ToolPlugin 路径
- **#34 P2.2-fix** model_override 在 cloud/custom 路由模式下失效：被 P9.1.5 + memory note + P-router-model-passthrough 取代
- **#39 P2.3-fix**：(1) silent fallback ✅ 已 fail-closed (P6.2-fix) (2) async placeholder echo parent_tool_use_id ✅ 已通过 P6.2 SpawnAsyncOutcome 解决
- **P-router-model-passthrough**：8 个 LLM provider 收敛为单一 OpenAI-兼容实现 + endpoint/认证。8 个文件已加 deprecation 注释（commit `74b75aa`）。详见 memory `project_lotus_llm_routing.md`。**长期专项，不在 Mode B 范围**。

## 6. review 关注重点（建议 codex 必答）

请 codex 按批 A → 批 B → 批 C 顺序 review，每批完成后给出：

1. **直接缺陷**：是否有空指针 / borrow 错误 / 死锁 / 资源泄漏？
2. **横向一致性**：fail-closed/best-effort 处理是否在所有相同语义场景统一？
3. **纵向：上下游协议**：append_line 退化路径下游消费者（task_output tool）能否优雅处理空文件？
4. **时间维度**：tokio::spawn 内可能 race 的资源（self.paths.require_paths() 是 sync，OK）；transcript_path_for_task 是否被多线程写？（answer：每个 agent_id 独立路径，单线程写）
5. **失败模式**：append_line 错被 silent 吞掉，是否会把 bug 隐藏？建议 telemetry/log？
6. **安全**：browse_data 递归保护是否真的只靠 hardcoded list？SECURITY 注释是否充分？
7. **计划对齐**：本批输出与 master plan §2 "P0–P10 完成后软件状态" 是否一致？

每条 finding 请给出：文件:行号、严重等级（Block/Warn/Info）、证据、建议修法。

---

**HEAD commit**：`78235d7`
**Branch**：`main`（worktree path `/Users/a20250311/.codex/worktrees/4dc8/lotus-app`）
**未 push**（按 handoff 文档约定本批禁 push）

# Mode B Subagent 实施进度（会话切换交接 v3）

> **当前状态**：2026-05-02
> **branch**: main
> **基线 commit**: `d58b3e8` (Merge branch 'pzc' into main)
> **最新 commit**: `753a4d1` (P8.1a + P8.1b)
> **commit count since baseline**: 26
>
> **大计划**: `docs/superpowers/plans/2026-04-30-lotus-subagent-mode-b-master-plan.md`
> **对标文档**: `docs/superpowers/plans/2026-04-30-subagent-benchmark-vs-claude-code-best.md` (v2)

---

## 里程碑状态

| 里程碑 | 状态 | 说明 |
|---|---|---|
| **M1 — sync subagent 全功能可用** | ✅ DONE | P0–P5 + P9.1/P9.1.5 + P10.1 全部 commit + review pass |
| **M2 — async + notification 全功能** | ⚙️ in progress (≈80%) | P6.x ✅ P7.2 ✅ P8.1a/b ✅ ；P8.1c/d/e + P10.2 待做 |
| **M3 — 测试齐全** | ⏸️ 待启动 | P10.3 + 多个 review-follow-up |

---

## 已完成 commit（26 个，按时间顺序）

| commit | task | 说明 |
|---|---|---|
| `900b26e` | P0.2 | UserScopedPaths::agents_dir() |
| `3c594e8` | P7.1 | TaskNotificationQueue + XML builder |
| `f85ea82` | P0.1 | AgentDefinition 字段扩展 |
| `d7d31b8` | P4.1 | 三层工具白名单 |
| `f821065` | P4.1-fix | mod.rs 注册 tool_whitelist |
| `42ac4c9` | P0.0 | baseline thinking/dingtalk_bridge 字段补齐 |
| `3d058cf` | P1.1 | markdown frontmatter loader |
| `5232fe3` | P1.2 | three-tier registry merge |
| `565c6ee` | P1.3 | wire AgentRegistry into Tauri startup |
| `60b5514` | P2.1 | SubAgentConfig: model_override / agent_name / disallowed_tools |
| `e403dd8` | P1.3-fix | registry_loader infallible |
| `172e245` | P2.2 | effective_settings_for_subagent 透传 model_override |
| `8793691` | P2.3 | SpawnSubagentRuntimeTool + DefaultLauncher (sync only) |
| `c2d9775` | P2.4 | spawn_subagent 加 DAILY_ALLOWED_TOOLS |
| `5dc0ae8` | P4.2 | worker_runtime 接通三层白名单 |
| `13d4e12` | P5.1 | dispatcher 并行 dispatch 验证测试 |
| `c39ea48` | P9.1 | builtin general-purpose / explore agent |
| `9ee2958` | P9.1.5 | explore.model = Inherit + plan §12 校正 |
| `8cb48de` | P10.1 | e2e spawn_subagent 通过 dispatcher (3 测试) |
| `74b75aa` | (旁支) | 8 个 LLM provider 全部加 deprecation 注释 |
| `dba20e1` | P6.1 | AsyncAgentTaskStore (6 测试) |
| `144ce40` | P6.2 | async spawn_subagent + tokio::spawn (5 测试) |
| `1e44160` | P6.2-fix | fail-closed registry + spawn panic catch + propagate err string |
| `5fc6406` | P7.2 | chat_turn_driver 注入 task notification (3 测试) |
| `52da194` | P7.2-fix | drain 改 capture-and-re-enqueue + chat.rs fail-closed 一致 |
| `753a4d1` | **P8.1a + P8.1b** | output_writer 模块 + task_output RuntimeTool (14 测试) |

---

## 剩余 task（拆分到可独立交付的小片，每片 prompt 独立、上下文小）

> 每个子片均假定从 baseline `d58b3e8..HEAD` 全部 commit 已应用，工作目录是 `/Users/a20250311/.codex/worktrees/4dc8/lotus-app`，分支 main，**禁止开 worktree**。

### 🟢 立刻可做（无依赖）

#### **片 1 — P8.1c: task_output 工厂注册 + Resolver wiring**
**单文件改动 + 1 处 lib.rs 微调**。
- 在 `src-tauri/src/plugin/registry.rs::try_build_request_scoped_tool` 的 match 里加 `"task_output"` 分支：从 `app.try_state::<Arc<dyn UserScopedPathResolver>>()` 拿 resolver；按 P6.2 follow-up 的 fail-closed 模式 — `try_state` miss 直接 `log::error! + return None`。
- 在 `lib.rs:561` 附近加：`app.manage(current_user_storage.clone() as Arc<dyn storage::UserScopedPathResolver>);`（**注意**：lib.rs:576 已有相同 cast 给 `schedule_runner`，模式照抄）。
- 验证：`cargo build --tests`；不写测试。

#### **片 2 — P8.1d: task_output 集成测试**
新建 `src-tauri/tests/task_output_tool_test.rs`。
- 用 `TestResolver`（参考 task_output.rs:97 单测里的 `TestResolver { paths: UserScopedPaths }`）+ `TempDir`
- 4 个测试：nonexistent → 空；offset=0 读 3 行 → 3 lines + new_offset=3；offset=2 → 1 line + new_offset=3；写 3 行后写 1 → offset=3 拿到第 4 行
- 直接 `tool.execute(json!(...), ToolExecutionContext::for_test(...))`，不必走 dispatcher

#### **片 3 — P8.1e: spawn_subagent::launch_async 接 output_writer**
改 `src-tauri/src/llm/tool_executor/spawn_subagent.rs::launch_async`：
- 给 `DefaultSpawnSubagentLauncher` 加字段 `paths: Arc<dyn UserScopedPathResolver>`
- 更新 `from_runtime_deps` 构造函数（多收一参）
- 更新 `plugin/registry.rs:884` 调用站点（多传一个 Arc，参考 task_output 工厂的 resolver 解析模式）
- launch_async 计算 `transcript_path = output_writer::transcript_path(&paths.subagent_transcripts_dir(), agent_id.as_str())`
- `AsyncTaskHandle::output_file = transcript_path.clone()`（之前是 PathBuf::new()）
- tokio::spawn body 三个分支（Ok/Err/panic）追加 `output_writer::append_line(&path, &TranscriptLine::xxx).ok()`
- `build_task_notification_xml` 第 3 参数 `""` 改为 `transcript_path.to_string_lossy().as_ref()`
- 测试：现有 spawn_subagent_async_test 5 个用 stub launcher 不会触发实际写盘，应不破坏。可选加一个 unit test 验证 transcript_path 被正确填入。

> 这三片**互相独立**，可任意顺序执行；推荐顺序 1→2→3（先 wiring 让全栈可走，再补测，最后接 lifecycle）

### 🟡 中等依赖

#### **片 4 — P10.2: async e2e 测试**
依赖：片 1+片 3 完成（否则 task_output 在 dispatcher 里不可用、output_file 是空字符串）。
- 新建 `src-tauri/tests/e2e_spawn_subagent_async.rs`
- spawn_subagent({run_in_background: true, name: "w1", prompt: "..."}) → 立即返回 agent_id
- 用 stub launcher 模拟 sub-agent 完成 → 验证 task_store 状态变 Completed、notif_queue 队列非空、若接了 P8.1e 还可验 transcript 文件存在
- 期望 5–6 个测试

### 🔴 收尾

#### **片 5 — P10.1 follow-up: Fixed-model + 负向测试**
review #13 留的两个 minor。新建一个测试 helper 注册一个 `AgentDefinition { model: Fixed("test-id") }` 进 ad-hoc registry，验证 effective_model = Some("test-id")；再加一个 unknown subagent_type 的负向测试。

#### **片 6 — P10.3: review_ 架构约束回归**
新建 `src-tauri/tests/review_agent_b_constraints.rs`：
- agent 模块不导入 `tauri::*`
- spawn_subagent.is_concurrency_safe = true
- async agent 默认 disallow ask_user_question（resolve_agent_tools 边界）

#### **片 7 — 收尾 follow-ups #43**
在 `internal_system.rs:368` 加 SECURITY 注释（标记 browse_data 递归屏障，重构必须保证 list 不含 browse_data）。

---

## 已跳过/决议

- **P9.2 browse_data 兼容包装**：用户决议跳过，browse_data 维持 legacy ToolPlugin 路径
- **#34 P2.2-fix** model_override 在 cloud/custom 路由模式下失效：被 P9.1.5 + memory note + P-router-model-passthrough 取代
- **#39 P2.3-fix**：(1) silent fallback ✅ 已 fail-closed (P6.2-fix) (2) async placeholder echo parent_tool_use_id ✅ 已通过 P6.2 SpawnAsyncOutcome 解决

---

## 长期专项（不在 Mode B 范围）

- **P-router-model-passthrough**：8 个 LLM provider 收敛为单一 OpenAI-兼容实现 + endpoint/认证。8 个文件已加 deprecation 注释（commit `74b75aa`）。详见 memory `project_lotus_llm_routing.md`。

---

## 执行参数（已约定）

| 维度 | 设置 |
|---|---|
| 工作目录 | `/Users/a20250311/.codex/worktrees/4dc8/lotus-app` |
| 分支 | main，禁止开 worktree |
| Implementer 模型 | 按复杂度选 haiku/sonnet（机械活 haiku，多文件 TDD sonnet） |
| Reviewer 模型 | **opus**（用户明确要求） |
| 并行规则 | 串行（避免 git reset 互相污染） |
| Review 7 维度 | 每次 review 强制：直接 / 横向 / 纵向 / 时间 / 失败 / 安全 / 计划对齐 |

---

## 重要架构事实（已验证，记账）

1. **LlmGateway 是 stateless w.r.t. model**：model 走 `AppSettings.primary_model`（实际是 endpoint key）和 `cloud_model`/`custom_model_name`（实际 model id）
2. **AgentRegistry 注入位置**：`lib.rs::app.manage(Arc<AgentRegistry>)` setup 阶段一次注册，不按用户切换刷新
3. **runtime/ 层纯度**：`runtime/tools/builtin/spawn_subagent.rs` 不导 LlmGateway/SubAgentConfig/tauri::*；DefaultSpawnSubagentLauncher 在 `llm/tool_executor/spawn_subagent.rs`
4. **三层工具白名单**（resolve_agent_tools）：def_allowed → ALL_AGENT_DISALLOWED 过滤 → ASYNC_AGENT_ALLOWED 子集（async only）→ 删 spawn_subagent（除非 allow_recursive_spawn=true）
5. **browse_data 递归保护**：旧 run_sub_agent 守卫已删；新保护 = `internal_system.rs:368` 硬编码 allowed_tools 列表（不含 browse_data）。**这是隐式安全边界** — 待片 7 加 SECURITY 注释
6. **lotus LLM 路由**：所有 provider 走 OpenAI 协议；只有 lotus / custom 透传 model id；其余 6 个写死 DEFAULT_MODEL（claude.rs 是 Anthropic Messages API 异类）。详见 memory `project_lotus_llm_routing.md`
7. **AsyncAgentTaskStore lifecycle**：`update_state(Completed/Failed/Killed)` **不删除 entry**，parent 完成后还能 task_output 查询（P6.1 设计 + P8.1 依赖）
8. **TaskNotificationQueue drain 语义**：drain 是 capture-and-re-enqueue（P7.2-fix），失败/cancel/Err 路径会把 drained 列表重新入队避免永久丢失（10 个失败分支已 trace）
9. **fail-closed 一致**：plugin/registry.rs 和 transport/tauri_commands/chat.rs 缺 app state 时都 `log::error!` + 不注册/不接 queue（不 panic，不静默 fresh-instance）

---

## 测试覆盖（截至 commit `753a4d1`）

| 模块 | 测试数 |
|---|---|
| `runtime::agent` lib 单测 | 28 (含 output_writer 7) |
| `runtime::tools::builtin::task_output` 单测 | 7 |
| `tests/agent_*` 集成测试 | 12 |
| `tests/spawn_subagent_*` | 24 |
| `tests/task_notification_injection_test` | 4 |
| `tests/e2e_spawn_subagent_explore` | 3 |
| `tests/worker_runtime_*` | 4 |
| `tests/review_*`（部分） | 已修过 |
| **合计** | **80+ 单测/集成测试，全部 PASS** |

预先存在的 `storage::file_store::messages` 5 个失败与 Mode B 无关，独立 ticket 跟踪。

---

## 新会话恢复指令

新会话第一条建议：

```
继续 lotus-app Mode B subagent 大计划（M2 收尾 + M3）。

读这两份文档恢复上下文：
- docs/superpowers/plans/2026-04-30-lotus-subagent-mode-b-master-plan.md
- docs/superpowers/plans/2026-04-30-mode-b-progress-handoff.md

跑 git log --oneline d58b3e8..HEAD 应有 26 个 commit，最新 753a4d1。
TaskList 看待办；当前可立刻做的是 handoff 文档里的「片 1（P8.1c）」「片 2（P8.1d）」「片 3（P8.1e）」三片中任一。

执行参数：当前 worktree 直接做、subagent-driven、串行不并行、reviewer=opus、implementer 按复杂度选模型。
```

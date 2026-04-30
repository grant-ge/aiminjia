# Mode B Subagent 实施进度（会话切换交接）

> **当前会话**：2026-04-30
> **branch**: main
> **基线 commit**: `d58b3e8` (Merge branch 'pzc' into main)
> **最新 commit**: `13d4e12` (P5.1)
>
> **大计划**: `docs/superpowers/plans/2026-04-30-lotus-subagent-mode-b-master-plan.md`
> **对标文档**: `docs/superpowers/plans/2026-04-30-subagent-benchmark-vs-claude-code-best.md` (v2)

---

## 已完成 commit（按时间顺序）

| commit | task | 说明 |
|---|---|---|
| `900b26e` | P0.2 | UserScopedPaths::agents_dir() |
| `3c594e8` | P7.1 | TaskNotificationQueue + XML builder |
| `f85ea82` | P0.1 | AgentDefinition: + disallowed_tools / permission_mode / background_default |
| `d7d31b8` | P4.1 | tool_whitelist 三层（ALL_AGENT_DISALLOWED + ASYNC_AGENT_ALLOWED + recursive guard） |
| `f821065` | P4.1-fix | mod.rs 注册 tool_whitelist |
| `42ac4c9` | P0.0 | baseline: 修 thinking / dingtalk_bridge 字段缺失（src + 12 tests） |
| `3d058cf` | P1.1 | markdown frontmatter loader (gray_matter + serde_yaml) |
| `5232fe3` | P1.2 | three-tier registry merge (builtin < user < project) |
| `565c6ee` | P1.3 | wire Arc<AgentRegistry> into Tauri startup + list_agents handler |
| `60b5514` | P2.1 | SubAgentConfig: + model_override / agent_name / disallowed_tools |
| `e403dd8` | P1.3-fix | registry_loader 改 infallible + 删 tautological 测试 |
| `172e245` | P2.2 | effective_settings_for_subagent 透传 model_override |
| `8793691` | P2.3 | SpawnSubagentRuntimeTool + DefaultSpawnSubagentLauncher（sync only） |
| `c2d9775` | P2.4 | spawn_subagent 加入 DAILY_ALLOWED_TOOLS |
| `5dc0ae8` | P4.2 | worker_runtime 接通三层白名单 + 删 browse_data 旧守卫 |
| `13d4e12` | P5.1 | dispatcher 并行 dispatch 验证测试（无代码改动，证明已 work） |

---

## 待办 task 列表（恢复用 TaskList 看，下面是简表）

### 进行中
- **#10 P9.1 内置 general-purpose / explore agent** — implementer prompt 已写，但用户中断让存进度，**未派 subagent**。下次直接派。

### 未开始（按推荐顺序）
1. **P9.1** 内置 general-purpose / explore（先做完，sync 路径就闭环了）
2. **P9.2** browse_data 兼容包装（让旧 browse_data 走通用 spawn_subagent 通道）
3. **P10.1** 端到端 sync e2e 测试
4. **【M1 里程碑达成 — sync subagent 全功能可用】**
5. **P6.1** AsyncAgentTaskStore（async 链路开始）
6. **P6.2** launch_async + tokio::spawn 后台 lifecycle
7. **P7.2** chat_turn_driver 注入 task notification
8. **P8.1** task_output tool + output_writer
9. **P10.2** 端到端 async e2e 测试
10. **【M2 里程碑达成 — async + notification 全功能】**
11. **P10.3** review_ 架构约束回归
12. **【M3 里程碑达成 — 测试齐全】**

### Follow-up（review 发现的 minor，未阻塞）
- **#27** ✅ 已完成（P1.3-fix，registry loader infallible 化）
- **#34 P2.2-fix**: model_override 在 cloud/custom 路由模式下失效。建议方案 A：在 SubAgentConfig.model_override doc 加一行"仅 direct-provider 路由生效"。或扩展 effective_settings_for_subagent 同步处理 cloud_model
- **#39 P2.3-fix**: (1) plugin/registry.rs:893-895 silent fallback 加 log::warn (2) async placeholder JSON echo parent_tool_use_id
- **#43 P4.2-fix**: 在 internal_system.rs:368 加 SECURITY 注释标记"这是 browse_data 递归屏障 — 重构必须保证 list 不含 browse_data，否则需把 browse_data 加进 ALL_AGENT_DISALLOWED"

---

## 执行参数（已约定）

| 维度 | 设置 |
|---|---|
| 分支 / worktree | 当前 worktree 直接做（`/Users/a20250311/.codex/worktrees/4dc8/lotus-app`） |
| 执行方式 | Subagent-Driven（每 task 派 fresh implementer subagent） |
| **并行规则** | **串行**（不再并行 implementer，避免 P0 阶段那种 git reset 互相污染） |
| 反馈节奏 | 全部跑完一次性给（M1+M2+M3） |
| Implementer 模型 | 按复杂度选 haiku/sonnet（机械活 haiku，多文件 TDD sonnet） |
| **Reviewer 模型** | **opus（用户明确要求所有 review 至少 sonnet，但选择 opus）** |
| Review 7 维度 | 每次 opus review 强制 7 维度 checklist：直接 / 横向 / 纵向 / 时间 / 失败 / 安全 / 计划对齐 |

---

## 重要架构事实（已验证，记账）

1. **LlmGateway 是 stateless w.r.t. model**：model 信息走 `AppSettings.primary_model`，不在 LlmGateway 里。这意味着 P2.2 用 `effective_settings_for_subagent(base, override)` clone-and-mutate 模式，**不需要 Arc<LlmGateway>**。

2. **AgentRegistry 注入位置**：lib.rs `app.manage(Arc<AgentRegistry>)` 在 setup 阶段注册一次，registry 由 builtin + user-scope `~/.renlijia/users/<scope>/agents/*.md` 合并而成。**不会按用户切换刷新**——这是已知限制（架构调研报告 Q3 同问题）。

3. **runtime/ 层纯度**：`runtime/tools/builtin/spawn_subagent.rs` 不导入 LlmGateway / SubAgentConfig / run_sub_agent / tauri::* — DefaultSpawnSubagentLauncher 在 `llm/tool_executor/spawn_subagent.rs`，通过 trait 注入。这是 BrowseDataLauncher 的标准 launcher 模式。

4. **三层工具白名单**（resolve_agent_tools）：
   - def_allowed (config.allowed_tools)
   - def_disallowed (config.disallowed_tools)
   - ALL_AGENT_DISALLOWED: ask_user_question, exit_plan_mode, enter_plan_mode
   - is_async=true → ASYNC_AGENT_ALLOWED 子集
   - allow_recursive_spawn=false（默认）→ 删除 spawn_subagent

5. **browse_data 递归保护现状**：旧 `run_sub_agent` 中 `if allowed_tools.contains("browse_data") return Err` 已删。新保护 = `internal_system.rs:368` 硬编码 allowed_tools 列表（不含 browse_data）。**这是隐式安全边界**——见 #43 follow-up。

6. **架构调研报告 4 个开放问题**：
   - Q1 (task-notification 注入点) — 待 P7.2 决策
   - Q2 (gateway ownership) — P2.2 已绕过（stateless model 让 Arc 不必要）
   - Q3 (AsyncAgentTaskStore 归属) — 待 P6.1 决策（推荐 app-level Arc）
   - Q4 (model 不存在的错误路径) — fast-fail 在 gateway 即可，不在 P2.x

---

## 测试覆盖（当前累计）

| 测试 | 用途 | 数量 |
|---|---|---|
| `runtime::agent::definition` (lib) | AgentDefinition 字段 | 2 |
| `runtime::agent::tool_whitelist` (lib) | 三层白名单 | 8 |
| `runtime::agent::task_notification` (lib) | XML + queue | 5 |
| `storage::user_scoped_paths` (lib) | agents_dir 等路径 | 2 |
| `llm::sub_agent` (lib) | SubAgentConfig 字段 | 3 |
| `runtime::agent` (lib 全) | (含上面部分) | 21 |
| `tests/agent_markdown_loader_test` | YAML frontmatter | 5 |
| `tests/agent_registry_merge_test` | 三层 merge | 5 |
| `tests/agent_registry_runtime_wiring_test` | lib.rs wiring 模式 | 2 |
| `tests/spawn_subagent_model_override_test` | effective_settings | 3 |
| `tests/spawn_subagent_tool_basic_test` | RuntimeTool 接口 | 11 |
| `tests/worker_runtime_whitelist_integration_test` | 三层 filter | 4 |
| `tests/spawn_subagent_parallel_dispatch_test` | 并行 dispatch | 3 |
| `tests/review_worker_run_config_permission_mode_test` | (修过) | 2 |
| `tests/tool_catalog_contract_test` | catalog (修过) | 20 |

**所有测试 PASS**。`cargo build --tests` 0 errors。

---

## 新会话恢复指令

新会话第一条消息建议：

```
继续 lotus-app Mode B subagent 大计划。

读这两份文档恢复上下文：
1. /Users/a20250311/.codex/worktrees/4dc8/lotus-app/docs/superpowers/plans/2026-04-30-lotus-subagent-mode-b-master-plan.md
2. /Users/a20250311/.codex/worktrees/4dc8/lotus-app/docs/superpowers/plans/2026-04-30-mode-b-progress-handoff.md

然后跑 git log --oneline d58b3e8..HEAD 确认进度（应有 16 个 commit），
TaskList 看待办，从 P9.1 继续。

执行参数：当前 worktree 直接做、subagent-driven、串行不并行、reviewer=opus、implementer 按复杂度选模型。
```

新会话不需要：
- 重新派 lotus 架构调研
- 重新对标 claude-code-best
- 重新确认 Q1-Q4 开放问题

新会话要做的：
- 从 P9.1 继续（implementer prompt 在大计划文档 §12 已有，可参考；上一次会话已写了详细 prompt 但用户中断没派出去，新会话用类似 prompt 直接派 sonnet 即可）
- 走完 M1 → M2 → M3
- 收尾时把 4 个 follow-up（#27 已完成，#34 / #39 / #43 待做）一起处理

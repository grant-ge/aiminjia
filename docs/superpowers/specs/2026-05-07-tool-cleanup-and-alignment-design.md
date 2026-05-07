# Lotus 工具系统清理与对标设计

日期：2026-05-07
状态：实施中（Phase 1 ✅ / Phase 2 ✅ / Phase 3 ✅ / Phase 4 ✅ / Phase 5 进行中：TaskGet 实施中，TaskStop 推迟，SKILL.md 重写推迟）
基线：`docs/2026-05-07-tool-inventory-and-claude-code-best-comparison.md`

---

## 0bis. 当前实施进度（断点续做用，最后更新 2026-05-07）

### 已完成的 commit（按时间顺序）

| Commit | 内容 |
|---|---|
| `4c288a6` | Phase 1：4 个运行时 bug 修复 + 设计/调研文档 |
| `56bd875` | 数字员工 5 个内置模板 toolWhitelist 重写 |
| `41ffbae` | Phase 2A + 2B：删 list_directory + 7 僵尸 + 1 孤儿（约 -1942 行） |
| `3b724b2` | Phase 3 主体（WIP）：删 12 业务工具 + Python 整目录 + browse_data_agent（约 -19056 行） |
| `7b7448c` | Phase 3 收尾：8 处残留清理（-203 行） |
| `cdcc375` | Phase 4：13 对工具硬改名（81 文件，+400/-400） |

### Phase 5 范围调整（基于实施时发现的差距）

**做**：
- §7.1 新增 TaskGet 工具（read-only，依赖已有 task store，零阻塞）

**推迟**：
- §7.2 新增 TaskStop —— **见 §7.2 现状记录**：CancellationToken 类型已可用，但 `agent_runtime.cancel_run` 没真正调 `.cancel()`，且没有 `task_id -> RunId` 映射，前置缺三块
- §9.2 30+ SKILL.md 正文重写 —— A 类（24/27 个 SKILL）依赖被删工具的产品流程，单纯 sed 改名不足以恢复语义；正文逻辑重写工作量超出本期负担。改名 sed 脚本本身保留在 §9.2 待用

### 实施过程中的关键经验（给下次会话）

- **prompt 太长 / 改动太大时 implementer 会 stall**：拆成多个 batch（每个 batch 改 2-3 个文件）效果最好
- **subagent stall 不一定是失败**：watchdog 超时前往往主体已做完，stop 后看 git diff 经常发现工作完成了，只是没报告
- **跑 cargo build 验证比让 agent 自己跑更可靠**：每个 implementer 完成后主线程独立 build 一次确认
- **review_ 测试中 `review_send_message_clears_gateway_busy_after_runtime_returns` 和 `review_sub_agent_should_not_hardcode_foreground_child_runs` 是 pre-existing 失败**：用 `git stash` 验证过，跟本期改动无关，可以忽略
- **Phase 4 改名能并行**：按"src 源码 vs tests 目录"切两个 subagent 完全文件不重叠，可真并行；同一文件 13 对改名内部不能并行（共享 catalog 数组）
- **SKILL.md 推迟决策**：A 类 SKILL（24/27 个）正文里写"用 execute_python 计算..."这种产品流程描述，工具被删后正文整段失语义；机械 sed 改名解决不了这种"语义性失效"，必须人工逐个 SKILL 重写产品流程，工作量与产品定位最终决策耦合（哪些场景留 / 哪些下线），不适合在工具清理这一期里捎带

---

## 0. 战略转向背景

本次调整不是单纯的"清理"，而是产品定位调整：

- **从前**：企业 AI 工作台 + 数字员工（重业务工具：Python 沙箱、Excel/PDF 解析、HTML 报告生成、浏览器自动化）
- **此后**：以通用工具 + SKILL 为核心的轻量 AI 助手（对齐 claude-code-best 工具集 + Lotus 的 SKILL/数字员工外壳）

核心动作：删 22 个工具 + 整个 Python 沙箱 + 改名 13 对 + 新增 2 个 + 修 4 个 bug。

---

## 1. 改动总览（一张表）

| # | 动作 | 对象 | 工程量 |
|---|---|---|---|
| **A** | 删除（catalog 死代码） | 7 个僵尸：`export_data` / `generate_slides` / `hypothesis_test` / `detect_anomalies` / `plan_update` / `progress_update` / `save_analysis_note` | 中 |
| **A+** | 删除（孤儿 handler） | `update_progress`（progress.rs 整个文件，catalog 里根本没注册） | 小 |
| **B** | 删除（对齐 best） | `list_directory`（best 没独立 LS） | 中 |
| **C** | 删除（业务工具退场） | 12 个：`execute_python` / `load_file` / `generate_report` / `generate_chart` / `browse_navigate` / `read_page_content` / `page_execute_js` / `extract_table_data` / `extract_with_pagination` / `browse_and_extract` / `browse_data` / `get_file_info` | 大 |
| **C+** | 删除（基础设施） | 整个 `src-tauri/src/python/` 目录（约 4000+ 行）、`browse_data_agent` 内置 agent | 大 |
| **D** | 重命名 | 13 对：见 §6 | 大 |
| **E** | 新增 | `TaskGet` / `TaskStop` | 中 |
| **F** | 修 bug | 4 处：见 §8 | 小 |
| **G** | 适配 | 5 个数字员工模板 toolWhitelist（已完成）+ 30+ SKILL.md 重写 | 中 |
| **H** | 适配 | 前端 i18n / 测试 fixture | 小 |

**最终静态工具集**：从 37 → **17 个**。

```
通用 16:  Read Write Edit Glob Grep Bash PowerShell
          WebSearch Agent TaskOutput TaskCreate TaskGet
          TaskList TaskUpdate TaskStop AskUserQuestion
记忆 + Skill 3:
          WriteMemory SearchMemory Skill
MCP 动态: mcp__<server>__<tool>
```

---

## 2. 目标与非目标

### 目标

1. 把 Lotus 工具集精简对齐到 claude-code-best 通用工具集 + 最小必要的 Lotus 自有能力（记忆、Skill 加载）。
2. 删除已无产品支撑的业务工具（Python 沙箱、Excel/PDF 解析、报告/图表生成、浏览器自动化）及其全部依赖代码。
3. 修复 4 个已确认运行时 bug。
4. 命名风格统一 PascalCase（不留 snake_case alias）。
5. 数字员工系统保留外壳，工具白名单切换到通用工具。

### 非目标

- 不补 `WebFetch` / `PlanMode` / `LSP` / `Cron*` / `Sleep` / `Team*`（YAGNI）。
- 不重构 RuntimeTool trait / 三段式注册链路。
- 不动 docs/ 历史文档的旧名（按时间快照保留）。
- 不发版（具体由用户后续决定）。
- 不动 RuntimeTool 内部 Rust struct 名（如 `BashTool` 仍叫 `BashTool`，只改运行时字符串 ID）。

---

## 3. 删除清单 A — 7 个僵尸 + 1 个孤儿

> 判定：catalog 有 entry，但全局 + request-scoped 两边都不注册，LLM 调用必报 unknown。

### 3.1 删除工具 + 位置

| 工具 | catalog 行 | handler 文件 | retired 断言行 |
|---|---|---|---|
| `export_data` | catalog.rs:555-575 | tool_executor/export.rs（整文件） | prompts.rs:727 |
| `generate_slides` | catalog.rs:648-664 | tool_executor/slides.rs（整文件） | — |
| `hypothesis_test` | catalog.rs:747-761 | tool_executor/stats.rs::handle_hypothesis_test | prompts.rs:724 |
| `detect_anomalies` | catalog.rs:762-779 | tool_executor/stats.rs::handle_detect_anomalies | prompts.rs:725 |
| `plan_update` | catalog.rs:687-694（for 循环） | 无 | prompts.rs:721 |
| `progress_update` | catalog.rs:687-694（同上） | 无 | prompts.rs:722 |
| `save_analysis_note` | catalog.rs:687-694（同上） | tool_executor/notes.rs（整文件） | prompts.rs:723 |
| `update_progress`（孤儿，A+） | **catalog 中无** | tool_executor/progress.rs（整文件） | — |

### 3.2 完整删除点

#### catalog.rs

- 删 entry：行 555-575 / 648-664 / 747-761 / 762-779
- 删 for 循环：行 686-694
- 修 description 内嵌引用：行 451（load_file desc 中提到 `list_directory / search_files / read_workspace_file` —— 此 entry 整个会在 §5 删除）
- 修 `DAILY_ALLOWED_TOOLS` 上方注释（行 888-898）整段重写

#### llm/tool_executor/

整文件删除：
- `export.rs`、`slides.rs`、`stats.rs`、`notes.rs`、`progress.rs`

`mod.rs` 删除以下行：
- 行 56：`pub(crate) use export::handle_export_data;`
- 行 66：`pub(crate) use notes::handle_save_analysis_note;`
- 行 67：`pub(crate) use progress::handle_update_progress;`
- 行 77：`pub(crate) use slides::handle_generate_slides;`
- 行 79-80：`pub(crate) use stats::handle_detect_anomalies;` / `handle_hypothesis_test`
- 对应 `mod` 声明

修 mod.rs 行 40 注释中的 `export_data` 提及。

#### llm/prompts.rs

行 710-727 retired tool 断言列表：删除 `plan_update` / `progress_update` / `save_analysis_note` / `hypothesis_test` / `detect_anomalies` / `slides_gen` / `export_data` 七项。保留 `save_memory` / `load_core_memory` / `distill_memories`。

#### llm/prompt_guard.rs

- 行 13、94、152-154：`save_analysis_note` / `_export_detail` / `前序分析记录` 注释和测试 fixture 改写或删除

#### commands/chat.rs

- 行 539-540：`FILE_GEN_TOOLS` 中 `"export_data"` / `"generate_slides"` 删除（整个常量在 §4 因为 `generate_report` / `generate_chart` 也删了，最终整个常量删除）
- 行 601-604、607-610、633-640：相关测试用例删除

---

## 4. 删除清单 C — 12 个业务工具 + Python 环境 + 浏览器 agent

### 4.1 业务工具（12 个）

| 工具 | catalog 行 | 主实现位置 |
|---|---|---|
| `execute_python` | 443 | runtime/tools/builtin/python.rs + python_execution.rs |
| `load_file` | 337 | runtime/tools/builtin/file.rs |
| `generate_report` | 514 | runtime/tools/builtin/report.rs + report_capability.rs |
| `generate_chart` | 537 | runtime/tools/builtin/chart.rs + chart_capability.rs |
| `browse_navigate` | 375 | runtime/tools/builtin/browser.rs::BrowseNavigateRuntimeTool |
| `read_page_content` | 388 | runtime/tools/builtin/browser.rs::ReadPageContentRuntimeTool |
| `page_execute_js` | 401 | runtime/tools/builtin/browser.rs::PageExecuteJsRuntimeTool |
| `extract_table_data` | 414 | runtime/tools/builtin/browser.rs::ExtractTableDataRuntimeTool |
| `extract_with_pagination` | 428 | runtime/tools/builtin/browser.rs::ExtractWithPaginationRuntimeTool |
| `browse_and_extract` | 495 | runtime/tools/builtin/browser.rs::BrowseAndExtractRuntimeTool |
| `browse_data` | 472 | runtime/tools/builtin/browse_data.rs |
| `get_file_info` | 187 | runtime/tools/builtin/workspace.rs::GetFileInfoRuntimeTool |

### 4.2 删除点

#### catalog.rs

删除上述 12 个 entry。同时：
- 删除 `DAILY_ALLOWED_TOOLS` 中 `"get_file_info"`（行 909）
- `WORKSPACE_TOOL_NAMES`（chat_runtime_impl.rs:19-28）删 `"get_file_info"`

#### runtime/tools/builtin/

整文件删除：
- `browser.rs`（7 个浏览器工具实现）
- `browse_data.rs`
- `file.rs`（load_file）
- `python.rs`、`python_execution.rs`
- `report.rs`、`report_capability.rs`
- `chart.rs`、`chart_capability.rs`
- `network.rs` 中 `WebSearchRuntimeTool` 保留（这是 web_search 改名 WebSearch 的实现），其余如有可清

`workspace.rs` 中删除：
- `GetFileInfoRuntimeTool` struct + impl

`mod.rs`（builtin 目录）：删除以上模块声明。

#### plugin/builtin/tools/mod.rs

- 删除 `GetFileInfoRuntimeTool` 注册（行 41）
- use 语句行 24-30 中删除 `GetFileInfoRuntimeTool`

#### plugin/registry.rs

`REQUEST_SCOPED_RUNTIME_TOOL_NAMES`（行 128-145）删除：
- `web_search` 不删（改名 WebSearch）
- 删除：`browse_navigate` / `read_page_content` / `page_execute_js` / `extract_table_data` / `extract_with_pagination` / `browse_and_extract` / `load_file` / `browse_data` / `execute_python` / `generate_report` / `generate_chart`

> 保留：`spawn_subagent`（→ Agent）、`write_memory`（→ WriteMemory）、`search_memory`（→ SearchMemory）、`load_skill`（→ Skill）

`try_build_request_scoped_tool` match arm（行 780-1085）删除上述 11 个 arm 及其实现。

依赖字段连带可删（如果只为这些工具服务）：
- `RequestScopedRuntimeDeps` 中 `python_runtime` / `connector_engine` / `file_manager` / `tavily_api_key` / `bocha_api_key`（WebSearch 还要 → 保留 search 相关字段）等

需要 grep 确认无其他调用方再删。

#### llm/tool_executor/

整文件删除：
- `report.rs`、`chart.rs`、`python.rs`
- `file_load.rs`
- `internal_system.rs`（含 7 个浏览器 handler）
- `search.rs` ⚠️ 保留（WebSearch 仍用 `execute_web_search_core`）

`mod.rs` 删除对应 `mod` / `pub(crate) use` 行。

### 4.3 Python 环境（C+）

整目录删除：`src-tauri/src/python/`
- `sandbox.rs`（约 1000 行）
- `runner.rs`
- `analysis_utils.rs`（约 2500 行）
- 该目录下所有其他文件

`src-tauri/src/lib.rs` 中 `mod python;` 删除。

#### RuntimeManager

`src-tauri/src/runtime/runtime_manager.rs` 中 Python 二进制下载/解压/路径解析逻辑全部删除。具体行号要 grep 后确定（保守估计 200-500 行）。

CLAUDE.md 中"Python 沙箱"小节删除。

### 4.4 browse_data_agent

文件 `runtime/agent/builtin/browse_data_agent.rs` 整个删除（它的 6 个 allowed_tools 全是浏览器工具）。

`runtime/agent/builtin/mod.rs` / `runtime/agent/registry.rs` 中相关引用同步删除。

### 4.5 钉钉工具族确认

`llm/tool_executor/dingtalk.rs` 中有 18 个钉钉工具（AI 表格/通讯录/群聊/日历/待办/审批等）。本期不动——这些走的是不同路径（钉钉 SKILL + Bash dws CLI 调用），不是 LLM 直接工具，但代码可能与 PluginContext 耦合。**需后续确认是否还活着**，本期保留。

---

## 5. 删除清单 B — list_directory

### 5.1 理由

- claude-code-best 没有独立 LS（由 BashTool 内部识别 `ls` 命令）
- `Bash ls` / `Glob '*'` 完全可替代

### 5.2 完整删除点

| 文件 | 改动 |
|---|---|
| `runtime/tools/builtin/workspace.rs` | 删除 `ListDirectoryRuntimeTool` struct + impl（行 123-160 区段） |
| `plugin/builtin/tools/mod.rs:28,32` | 删除 use + register_runtime |
| `runtime/tools/catalog.rs:138-152` | 删除 entry |
| `runtime/tools/catalog.rs:907` | `DAILY_ALLOWED_TOOLS` 删 `"list_directory"` |
| `transport/tauri_commands/chat/chat_runtime_impl.rs:20,143,209` | `WORKSPACE_TOOL_NAMES` 删；附件提示文本改写；其他引用清理 |
| `runtime/agent/builtin/explore.rs:13,46,67` | explore agent allowed_tools + 注释 |
| `runtime/agent/tool_whitelist.rs:23` | 常量数组删 |
| `llm/providers/openai.rs:1553,1566` | 测试 fixture 改名 |
| `llm/tools.rs:106,108` | 测试用例改名/删除 |
| `runtime/query_engine.rs:597` | 注释改写 |
| `tests/builtin_runtime_registration_test.rs:747-751` | 删除相关 |
| `tests/runtime_tool_registry_production_path_test.rs:179-183` | 同上 |
| `tests/tool_dispatcher_test.rs:19-48` | 4 处 |
| `tests/tool_runtime_integration_test.rs:32-43` | 同上 |
| `tests/runtime_tool_registry_test.rs:2-73` | 4 处 |

---

## 6. 重命名清单 D — 13 对

### 6.1 改名映射

| 旧名 | 新名 | 来源 |
|---|---|---|
| `read_workspace_file` | `Read` | best 同名 |
| `write_file` | `Write` | best 同名 |
| `edit_file` | `Edit` | best 同名 |
| `search_files` | `Glob` | best 同名 |
| `grep_content` | `Grep` | best 同名 |
| `bash` | `Bash` | best 同名 |
| `powershell` | `PowerShell` | best 同名 |
| `web_search` | `WebSearch` | best 同名 |
| `spawn_subagent` | `Agent` | best 主名（不引入 `Task` alias） |
| `task_output` | `TaskOutput` | best 同名（合并修 §8.1 bug） |
| `write_memory` | `WriteMemory` | PascalCase 风格统一 |
| `search_memory` | `SearchMemory` | 同上 |
| `load_skill` | `Skill` | best 同名 |

### 6.2 改名落地点

#### catalog.rs

每个工具的 entry：`ToolDefinition::new("旧名", ...)` 改新名。

description 内嵌跨引用同步改写：
- 行 217（write_file desc 提到 `read_workspace_file` / `edit_file`）
- 行 239（edit_file desc 提到 `read_workspace_file` / `write_file`）
- 行 581 / 607 / 622 / 624（spawn_subagent 与 task_output 互引）

`DAILY_ALLOWED_TOOLS`（行 900-920）所有旧名改新名。最终内容：

```rust
pub const DAILY_ALLOWED_TOOLS: &[&str] = &[
    // Shell：每平台只注册其一，过滤层会自动隐藏不可达的
    "Bash",
    "PowerShell",
    "Read",
    "Write",
    "Edit",
    "Glob",
    "Grep",
    "WriteMemory",
    "SearchMemory",
    "Agent",
    "TaskOutput",
    "Skill",
    "AskUserQuestion",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskUpdate",
    "TaskStop",
    "WebSearch",
];
```

#### RuntimeTool struct ID

每个 `impl RuntimeTool::definition()` 中 `ToolDefinition::new(...)` 字符串改新名。涉及文件：

- `runtime/tools/builtin/workspace.rs`（Read / Write / Edit / Glob）
- `runtime/tools/builtin/grep.rs`（Grep）
- `runtime/tools/builtin/bash.rs`（Bash）
- `runtime/tools/builtin/powershell.rs`（PowerShell）
- `runtime/tools/builtin/network.rs`（WebSearch）
- `runtime/tools/builtin/spawn_subagent.rs`（Agent）
- `runtime/tools/builtin/task_output.rs`（TaskOutput）
- `runtime/tools/builtin/memory.rs`（WriteMemory / SearchMemory）
- `runtime/tools/builtin/load_skill.rs`（Skill）

> Rust struct 名（`ReadWorkspaceFileRuntimeTool` / `WriteMemoryRuntimeTool` 等）保留，**只改字符串 ID**。

#### plugin/registry.rs

`REQUEST_SCOPED_RUNTIME_TOOL_NAMES`（删完业务工具后剩下）：

```rust
const REQUEST_SCOPED_RUNTIME_TOOL_NAMES: &[&str] = &[
    "WebSearch",
    "Agent",
    "WriteMemory",
    "SearchMemory",
    "Skill",
    "TaskOutput",  // 修 bug §8.1 加进来
    "TaskStop",    // 新增 §7
];
```

工厂 match arm 全部改用新名。

#### Agent allowed_tools

| 文件 | 改动 |
|---|---|
| `runtime/agent/builtin/explore.rs:9-15` | 改为 `["Read", "Grep", "Glob", "WebSearch"]`（list_directory 已删） |
| `runtime/agent/builtin/daily_assistant_agent.rs` | 引用 DAILY_ALLOWED_TOOLS，自动跟随 |
| `runtime/agent/builtin/general_purpose.rs` | 不动（空 allowed_tools 表全集） |
| `runtime/agent/builtin/browse_data_agent.rs` | 已 §4.4 整文件删除 |
| `runtime/agent/tool_whitelist.rs` | 行 23 + 其他出现处全改 |

---

## 7. 新增清单 E — TaskGet / TaskStop

### 7.1 TaskGet

**用途**：取单条任务详情（含 metadata、blocks/blockedBy）。

**实现**：
- 新文件：`src-tauri/src/runtime/tools/builtin/task_get.rs`（参考 `task_tools.rs::TaskListRuntimeTool`）
- catalog entry：插在 `TaskList` 后
- 全局注册：`plugin/builtin/tools/mod.rs` 加 `register_runtime(Arc::new(TaskGetRuntimeTool))`
- DAILY_ALLOWED_TOOLS 加 `"TaskGet"`

**Schema**：

```json
{
  "type": "object",
  "required": ["taskId"],
  "properties": { "taskId": { "type": "string" } }
}
```

**返回**：完整 Task JSON。

### 7.2 TaskStop —— 推迟到下一期

**结论（2026-05-07 实施时确认）**：缺三个前置条件，本期不实施。

**实际现状（grep 后）**：
- `runtime/cancellation::CancellationToken` 类型完整可用，含 `BackgroundStop` reason 和 parent/child 级联取消（`runtime/cancellation.rs`）。
- `worker_runtime.rs:73` 的 `WorkerConfig` 含 `cancel_token: Option<CancellationToken>`，token 已往下传。
- **缺口 1**：`agent_runtime::cancel_run(child_run_id)`（agent_runtime.rs:75-83）只更新 `AgentInvocationStore` 的状态字段（Cancelled），**没有调用对应 worker token 的 `.cancel()`** —— 也就是 worker 不会真停。
- **缺口 2**：没有 `task_id -> CancellationToken` 的全局 registry。`TaskCreate` 创建的 task 与 `spawn_subagent` 启动的 child run 是两套 ID（taskId vs RunId/AgentId），TaskStop 接哪一头都需要新的关联存储。
- **缺口 3**：���步子 agent（`spawn_subagent run_in_background=true`）的 launcher 把 `agent_id` 返回给 LLM，但没有把 `agent_id ↔ task_id` 的双向映射落盘 —— TaskStop 收到 task_id 也找不到对应运行体。

**下一期需要先做**（不在本设计范围）：
1. 给 `agent_runtime.cancel_run` 加上真正的 token-stop 路径（保留 invocation_store 状态写入，再调被取消运行体的 `.cancel_with_reason(BackgroundStop)`）。这意味着 invocation_store 要持有 token Arc 或 worker_runtime 维护一个 `RunId -> CancellationToken` 的 Mutex<HashMap>。
2. 把 `spawn_subagent` async path 输出的 metadata 里加 task_id（从 ToolExecutionContext 的 caller chain 提取），并把 `task_id -> RunId` 写到 task store 的 metadata 字段上。
3. `TaskStop` 工具的实现（schema / catalog / register / disallow 列表）见下文存档，等前置补齐后直接用。

**已存档的 TaskStop 设计（前置补齐后实施用）**：

- 新文件：`src-tauri/src/runtime/tools/builtin/task_stop.rs`
- 路径：request-scoped（依赖 app state 拿 agent_runtime / worker_runtime registry）
- 添加：catalog entry + `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` + `try_build_request_scoped_tool` match arm
- DAILY_ALLOWED_TOOLS 加 `"TaskStop"`
- `ALL_AGENT_DISALLOWED` 加 `"TaskStop"`（§8.2）

**Schema**：

```json
{
  "type": "object",
  "required": ["task_id"],
  "properties": { "task_id": { "type": "string" } }
}
```

---

## 8. Bug 修复清单 F — 4 处

### 8.1 task_output 名单漏列（合并改名）

`plugin/registry.rs:128-145` 的 `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 加 `"TaskOutput"`。
工厂 match arm（registry.rs:968 `"task_output" =>`）改 `"TaskOutput" =>`。

### 8.2 ALL_AGENT_DISALLOWED 大小写错配 + 缺 Agent

`runtime/agent/tool_whitelist.rs:9-13`：

```rust
const ALL_AGENT_DISALLOWED: &[&str] = &[
    "AskUserQuestion",
    "Agent",       // 防止子 agent 递归 spawn
    "TaskStop",    // 子 agent 不应有终止能力
];
```

新增防回归测试：

```rust
#[test]
fn all_agent_disallowed_names_match_catalog_exactly() {
    use crate::runtime::tools::catalog::TOOL_CATALOG;
    for name in ALL_AGENT_DISALLOWED {
        assert!(
            TOOL_CATALOG.get(name).is_some(),
            "ALL_AGENT_DISALLOWED contains '{}' which is not in TOOL_CATALOG",
            name
        );
    }
}
```

### 8.3 WORKSPACE_TOOL_NAMES 漏 PowerShell

`transport/tauri_commands/chat/chat_runtime_impl.rs:19-28`（同时应用改名 + 删除）：

```rust
const WORKSPACE_TOOL_NAMES: &[&str] = &[
    "Read",
    "Glob",
    "Write",
    "Edit",
    "Bash",
    "PowerShell",  // 新增
    "Grep",
];
```

### 8.4 DAILY_ALLOWED_TOOLS 平台冲突注释

加注释明确 Bash/PowerShell 平台二选一（§6.2 已含）。

---

## 9. 适配清单 G — 数字员工 + SKILL.md

### 9.1 数字员工 templates.ts ✅ 已完成

`src/features/employees/templates.ts` 5 个内置员工 toolWhitelist 已重写：

| 员工 | 角色 | 新 toolWhitelist |
|---|---|---|
| 小研 | 行业/竞品调研 | `WebSearch` `WriteMemory` `SearchMemory` `Skill` `Read` `Write` `Edit` `Bash` `Grep` `Glob` |
| 小法 | 合同审阅 | `Read` `Grep` `Glob` `Edit` `Write` `Skill` `WriteMemory` `SearchMemory` |
| 小算 | 数据分析 | `Read` `Write` `Edit` `Bash` `Grep` `Glob` `Skill` `WriteMemory` `SearchMemory` |
| 小销 | 客户跟进 | `Bash` `WebSearch` `WriteMemory` `SearchMemory` `Skill` `Read` `Write` `Edit` |
| 小钉 | 钉办助理 | `Bash` `Skill` `Read` `Write` `Edit` `WriteMemory` `SearchMemory` |

**附带修复 4 个历史 bug**（templates.ts 中 `memory_save` / `memory_search` / `read_file` / `analysis_note` 是 catalog 里根本不存在的幻影工具名）。

### 9.2 SKILL.md 重写策略

`~/.renlijia/skills/` 下 30+ 个 SKILL.md 需要分批重写，按"是否涉及被删工具"分组：

#### A 类：被删工具是核心依赖（必须重写或删除）

涉及 `execute_python` / `load_file` / `generate_report` / `generate_chart` 的 SKILL：约 25 个，含：
- 数据分析类：`comp-analysis-v2` / `recruit-funnel` / `engagement-survey` / `talent-9box` / `salary-benchmarking` / `customer-segmentation` / `user-behavior` / `sales-analysis` / `finance-analysis` / `multi-file-handler` / `survey-analysis` / `budget-analysis` / `ops-analysis`
- 报告类：`biz-writing` / `biz-proposal` / `pa-maturity` / `okr-coach` / `org-diagnosis` / `policy-compliance-audit` / `labor-compliance` / `perf-system-design`
- 浏览器类：`competitive-intelligence` / `sales-followup-rules`

**重写原则**：
- `execute_python` 改 `Bash`：`"用 execute_python 计算..." → "用 Bash 调用 python 计算..."`（前提：用户机器有 python，无沙箱保护）
- `load_file` 改 `Read`：仅文本类（CSV/JSON/MD）可读，**Excel/PDF 不再支持**——SKILL 中明确标注"请先把 Excel 转 CSV、PDF 转 txt"
- `generate_report` 改 LLM 直接生成 markdown + `Write` 落盘
- `generate_chart` 改"输出 markdown 表格 + 文字描述"
- 浏览器抓取改 `WebSearch` + LLM 总结（精度大幅下降，要在 SKILL 顶部写明）

#### B 类：仅 allowed_tools 字段提及被删工具（轻量改）

约 5 个 SKILL：仅在 frontmatter `allowed_tools` 列了被删工具，正文不依赖。改 `allowed_tools` 数组。

#### C 类：完全不依赖被删工具

不动。

#### 迁移脚本

`scripts/migrate-skill-tool-names.sh`（仅做名字替换，**正文重写需人工**）：

```bash
#!/bin/bash
SKILLS=~/.renlijia/skills
[ -d "$SKILLS" ] || { echo "skills dir not found"; exit 0; }

for skill in $SKILLS/*/SKILL.md; do
  [ -f "$skill" ] || continue
  cp "$skill" "$skill.bak.20260507"
  sed -i '' \
    -e 's/\bweb_search\b/WebSearch/g' \
    -e 's/\bedit_file\b/Edit/g' \
    -e 's/\bgrep_content\b/Grep/g' \
    -e 's/\bsearch_files\b/Glob/g' \
    -e 's/\bread_workspace_file\b/Read/g' \
    -e 's/\bwrite_file\b/Write/g' \
    -e 's/\bspawn_subagent\b/Agent/g' \
    -e 's/\btask_output\b/TaskOutput/g' \
    -e 's/\bwrite_memory\b/WriteMemory/g' \
    -e 's/\bsearch_memory\b/SearchMemory/g' \
    -e 's/\bload_skill\b/Skill/g' \
    -e 's/`bash`/`Bash`/g' \
    -e 's/`powershell`/`PowerShell`/g' \
    "$skill"
done

echo "Tool name aliases applied. Manual review still needed for:"
echo "  - 'execute_python' references → rewrite to use Bash + python"
echo "  - 'load_file' references → rewrite to use Read or remove Excel/PDF support"
echo "  - 'generate_report' / 'generate_chart' references → rewrite to LLM markdown"
echo "  - 'browse_*' references → rewrite to WebSearch or remove"
echo "Backups: $SKILLS/*/SKILL.md.bak.20260507"
```

**重要**：`execute_python` / `load_file` / `generate_report` / `generate_chart` / `browse_*` 不能用 sed 简单替换——必须人工逐个 SKILL 重写正文逻辑。

---

## 10. 适配清单 H — 前端 + 测试 fixture

### 10.0 前端任务监控代码无需改动

`src/hooks/useStreaming.ts:96-105` / `462`、`src/components/chat/RightPanel.tsx::TaskItem`、`src/stores/streamingStore.ts::ConversationTaskState`、`src/components/rich-content/SubAgentTranscriptViewer.tsx`、`src/lib/tauri.ts:44 (TASK_STATUS_CHANGED)`：

这些代码处理的工具名是 `'TaskCreate'`（PascalCase 已就绪），订阅的是 `task:status-changed` RuntimeEvent（不是工具名），新增的 `TaskGet` / `TaskStop` 都不需要前端解析返回内容（状态变化由 RuntimeEvent 自动驱动）——**任务监控相关前端代码本期完全不动**。

### 10.1 前端 i18n

`src/i18n/zh-CN.json` + `src/i18n/en-US.json` 中 `streaming.tools.*`：

**删除（被删工具的中文文案）**：
- `execute_python`
- `load_file`
- `save_analysis_note`
- `update_progress`
- `generate_report`
- `export_data`
- `hypothesis_test`
- `detect_anomalies`
- `generate_chart`

**改名**：
- `web_search` → `WebSearch`

最终 `streaming.tools` 大概只剩 `WebSearch` 一项（其他工具走 fallback 显示工具名本身）。可考虑加新映射（如 `Bash`/`Read`/`Write`/`Edit`/`WebSearch` 等），但本期可不补，让 fallback 直接显示英文工具名。

### 10.2 前端测试 fixture

| 文件 | 改动 |
|---|---|
| `src/components/rich-content/ExecutionTraceCard.test.tsx:52` | `read_workspace_file` → `Read` |
| `src/components/chat-scene/__tests__/ToolGroupCard.test.tsx` | 8 处 `read_workspace_file` → `Read` |

### 10.3 类型注释

`src/types/message.ts:260` 注释中"如 'browse_navigate'"改成"如 'Bash'"或删除示例。

### 10.4 后端测试 fixture（约 30 个文件）

按引用面扫描结果。建议批量 sed + 跑测试验证：

```bash
cd src-tauri
find tests/ -name '*.rs' -exec sed -i '' \
  -e 's/"read_workspace_file"/"Read"/g' \
  -e 's/"write_file"/"Write"/g' \
  -e 's/"edit_file"/"Edit"/g' \
  -e 's/"search_files"/"Glob"/g' \
  -e 's/"grep_content"/"Grep"/g' \
  -e 's/"web_search"/"WebSearch"/g' \
  -e 's/"spawn_subagent"/"Agent"/g' \
  -e 's/"task_output"/"TaskOutput"/g' \
  -e 's/"write_memory"/"WriteMemory"/g' \
  -e 's/"search_memory"/"SearchMemory"/g' \
  -e 's/"load_skill"/"Skill"/g' \
  {} \;
# bash / powershell 单独人工审查（避免误改无关字符串）
```

涉及被删工具的测试（execute_python / load_file / generate_* / browse_*）整个删除，包括：
- `tests/` 下所有 `*python*.rs` / `*browser*.rs` / `*browse_data*.rs` / `*report*.rs` / `*chart*.rs` 等

---

## 11. 实施顺序（5 阶段，每阶段独立可上线/可回滚）

### 阶段 1：bug 修复（半天）

只改不删，**用旧名**（保险起见，先验证 bug 修复路径）：

- §8.1 `task_output` 加进名单
- §8.2 `ALL_AGENT_DISALLOWED` 修正
- §8.3 `WORKSPACE_TOOL_NAMES` 加 `powershell`
- §8.4 注释

**验证**：`cargo test review_` 全过；手动跑异步 agent 读 transcript。

### 阶段 2：删除死代码（1 天）

- §3 7 个僵尸 + 1 个孤儿
- §5 `list_directory`

**验证**：`cargo build`、`cargo test --test tool_catalog_contract_test`、`pnpm test`、`pnpm lint`、`pnpm tauri:dev` 启动后人工跑一下普通对话。

### 阶段 3：删除业务工具与 Python 环境（2-3 天）

- §4.1 12 个业务工具（catalog + builtin/ + tool_executor/ + registry 工厂分支）
- §4.3 整个 `src-tauri/src/python/` 目录
- §4.4 browse_data_agent
- §4.2 内 `RequestScopedRuntimeDeps` 字段精简
- §4.2 `RuntimeManager` 中 Python 资源逻辑
- 相关测试文件批量删除

**验证**：`cargo build`（确保无遗漏的死引用）、`cargo test`、CLAUDE.md 中"Python 沙箱"小节同步删除。

### 阶段 4：硬改名（2-3 天）

按工具一个一个改，每改完跑测试。建议顺序（按引用面从小到大）：

1. `powershell` → `PowerShell`（43 处）
2. `web_search` → `WebSearch`（120 处，含 SKILL.md 17 处）
3. `grep_content` → `Grep`（60 处）
4. `search_files` → `Glob`（79 处）
5. `edit_file` → `Edit`（55 处）
6. `write_file` → `Write`（108 处）
7. `read_workspace_file` → `Read`（125 处）
8. `bash` → `Bash`（160 处）
9. `spawn_subagent` → `Agent`（177 处）
10. `task_output` → `TaskOutput`（合并 §8.1 一起）
11. `write_memory` → `WriteMemory`（约 50 处）
12. `search_memory` → `SearchMemory`（约 50 处）
13. `load_skill` → `Skill`（约 100 处）

每个工具单 commit。

### 阶段 5：新增 + SKILL.md 重写（1-2 天）

- §7.1 TaskGet
- §7.2 TaskStop（前置确认 cancellation token 接通）
- §9.2 SKILL.md 批量改名（脚本） + 人工重写正文逻辑

---

## 12. 验证清单

### 自动化（每阶段必跑）

- [ ] `cd src-tauri && cargo build`
- [ ] `cd src-tauri && cargo test review_ --tests --no-fail-fast`
- [ ] `cd src-tauri && cargo test --test tool_catalog_contract_test`
- [ ] `cd src-tauri && cargo test --test tool_schema_single_source_test`
- [ ] `cd src-tauri && cargo test --test builtin_runtime_registration_test`
- [ ] `pnpm test`
- [ ] `pnpm lint`

### 手动（阶段 4-5 完成后）

- [ ] `pnpm tauri:dev` 启动
- [ ] daily_assistant_agent：`Read` / `Write` / `Edit` / `Glob` / `Grep` / `Bash` / `WriteMemory` / `SearchMemory` / `Skill` 各调用一次
- [ ] 异步 agent：`Agent(run_in_background=true)` → `TaskOutput` → `TaskGet` → `TaskStop`
- [ ] Windows：无授权目录时 LLM schema 列表无 `Bash` / `PowerShell`
- [ ] 5 个内置员工分别派活：每个员工的 toolWhitelist 中工具调用都不报 unknown
- [ ] 触发若干 SKILL（competitive-intelligence / contract-review 等），确认正文中工具引用解析正常

---

## 13. 风险与回滚

| 风险 | 触发 | 缓解 |
|---|---|---|
| 阶段 3 删 Python 涉及大面积 import 链断裂 | RuntimeManager / RequestScopedRuntimeDeps 之外有遗漏调用方 | 每删一个文件先 `cargo build`；逐文件单 commit |
| SKILL.md 自动迁移误改 markdown 代码块 | sed 全局替换 | `.bak.20260507` 备份恢复；脚本不动 ```python 这种代码块 fence |
| 阶段 5 SKILL.md 正文人工重写工作量超预期 | 30+ 个 SKILL 需要逐个重写产品逻辑 | 不阻塞主路径，可分批发；本期先发"alias 改名"，正文重写延后 |
| `TaskStop` cancellation 未接通 worker | worker_runtime 不响应 token | 推迟到下一期，本期只发 D.1 TaskGet |
| 数字员工删了核心工具后，原本能跑的场景跑不动 | 用户已雇佣的"小算"突然不能分析 Excel | Release notes 明确告知能力变更；EmployeeDrawer UI 显示"已升级"提示 |
| docs/ 历史文档大量旧名引用引起读者困惑 | 不在改名范围 | 顶部加版本注释，指向新设计 |

每阶段独立 commit，需要回滚 `git revert` 该阶段的 commit 范围。

---

## 14. 不做的事（明确记录）

- 不引入命名 alias（用户选硬改）
- 不补 `WebFetch` / `PlanMode` / `LSP` / `Cron*` / `Sleep` / `Team*`
- 不重构 RuntimeTool trait
- 不动 docs/ 历史文档（按时间快照保留）
- 不发版（具体由用户后续决定）
- 不改 RuntimeTool Rust struct 名
- 不删钉钉工具族（dingtalk handlers，本期保留待后续评估）
- 不重写 RuntimeManager 的非 Python 资源逻辑

---

## 15. 决策溯源

本设计基于 2026-05-07 brainstorming 多轮决策：

1. 调整目标：清理 + 命名 + 补关键工具
2. 重命名范围：硬改（不留 alias）
3. PlanMode：不补
4. 新增工具：`Agent` 改名 + `TaskStop` + `TaskGet`
5. `list_directory`：claude-code-best 没有则删
6. **业务工具去留**：`execute_python` / `load_file` / `generate_report` / `generate_chart` / 7 浏览器 / `browse_data` / `get_file_info` 全删；`write_memory` / `search_memory` / `load_skill` 保留并改名
7. **Python 环境**：全删（`src-tauri/src/python/`）
8. **SKILL.md**：保留但重写
9. **5 个内置员工**：toolWhitelist 重写为通用工具集（已完成）

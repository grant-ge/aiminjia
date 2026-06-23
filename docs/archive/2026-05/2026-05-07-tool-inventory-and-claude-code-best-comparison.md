# Lotus App 工具清单与 claude-code-best 对标（事实核对版）

日期：2026-05-07
仓库：`/Users/a20250311/.codex/worktrees/c3d4/lotus-app`
对标仓库：`/Users/a20250311/github/claude-code-best`
基线：本文档基于 4 个并行子 agent 在源码上的精确事实核对（含行号），替代 codex 早先生成的初版。

---

## 30 秒结论

Lotus 工具系统**整体架构与 claude-code-best 是同构的**（catalog → registry → request-scoped 三段式），但在**清单管理和命名上有 5 类问题**：

1. **7 个僵尸工具污染 catalog**：LLM 看得到、调了必报错（`export_data` / `generate_slides` / `hypothesis_test` / `detect_anomalies` / `plan_update` / `progress_update` / `save_analysis_note`）。
2. **3 个确定的运行时 bug**：`task_output` 工厂分支永不可达；子 agent 系统级禁止全部失效（大小写错配）；Windows 上 PowerShell 无授权目录时未隐藏。
3. **1 个跨平台数据不一致**：`bash` / `powershell` 同时在 daily 名单但每平台只一个可达。
4. **命名风格漂移**：与 claude-code-best 大量同义工具名字不同（`read_workspace_file` vs `Read` 等 9 对）。
5. **缺 3 个有用工具**：`TaskGet` / `TaskStop` / `WebFetch`。

调整方向：删僵尸 + 修 bug + 硬改名对齐 + 补 `TaskGet` / `TaskStop`。详见配套 design：`docs/superpowers/specs/2026-05-07-tool-cleanup-and-alignment-design.md`。

---

## 摘要

Lotus 当前工具系统存在 **5 类问题**，按影响优先级排序：

| 类别 | 数量 | 性质 |
|---|---|---|
| 僵尸工具：catalog 有 entry，但全局/请求级两边都不注册，LLM 调用必报 unknown | 7 | 死代码 |
| 代码 bug：task_output 工厂分支已写好（registry.rs:968），但漏加进 `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 名单 → 永不调到 | 1 | 阻塞 |
| 代码 bug：`ALL_AGENT_DISALLOWED` 三个名字大小写错配，子 agent 系统级禁止从未生效 → 子 agent 可递归 spawn Agent | 1 | 安全/语义 |
| 数据不一致：`bash` / `powershell` 同时在 `DAILY_ALLOWED_TOOLS` 出现但每平台只注册一个；`WORKSPACE_TOOL_NAMES` 漏 `powershell` | 2 | 平台条件 |
| 命名漂移：与 claude-code-best 命名风格分歧（snake_case vs PascalCase）；缺少 `TaskGet` / `TaskStop` / `WebFetch` 等常用工具 | — | 对标差距 |

**主要数字**：

- TOOL_CATALOG 静态条目：**37 个**（含 for 循环注册的 3 个 stub）
- 全局 `register_runtime` 工具：**13 个**（其中 `bash`/`powershell` 平台条件二选一）
- `REQUEST_SCOPED_RUNTIME_TOOL_NAMES`：**16 个**
- `try_build_request_scoped_tool` 具名分支：**17 个**（包含未列入名单的 `task_output` 死分支）
- 真僵尸（catalog 有但两边都没注册）：**7 个**

---

## 1. 判断口径

| 状态 | 含义 |
|---|---|
| **真注入** | catalog 有 entry + 有 RuntimeTool 实现 + 有可达注册路径（全局或请求级名单），LLM schema 可见且执行端能调到 |
| **平台条件** | 受 `cfg(unix)` / `cfg(windows)` 影响，注册仅在该平台生效 |
| **stub 可见** | 工具对 LLM 可见，但运行时依赖不可用时返回 `ToolError::ExecutionFailed`（如 `execute_python` 在 Python runtime 启动失败时） |
| **名单虚指** | 名单里有名字但 catalog/工厂均无对应实现 |
| **代码死分支** | 工厂里有 match arm，但因不在名单里永不调到 |
| **catalog 僵尸** | catalog 有 entry，但全局注册 ∪ 请求级名单都没有 → LLM 看得到但调用必报 unknown tool |

---

## 2. Lotus 工具主链路

### 2.1 元数据真相源

- `src-tauri/src/runtime/tools/catalog.rs`：`TOOL_CATALOG`（`build_default_catalog()`）是唯一权威 schema 来源。
- `src-tauri/src/llm/tools.rs`：兼容层，把 `TOOL_CATALOG` 转成旧调用方需要的 `Vec<ToolDefinition>`。

### 2.2 注册路径

```text
启动时（src-tauri/src/lib.rs）
  → plugin::builtin::tools::register_builtin_tools()
  → ToolRegistry::register_runtime()
  → runtime_tools HashMap + TOOL_CATALOG

请求时（registry.rs:307–425）
  → ToolRegistry::to_runtime_dispatcher(request_scoped)
  → 复制全局 runtime_tools
  → 遍历 REQUEST_SCOPED_RUNTIME_TOOL_NAMES
  → try_build_request_scoped_tool() 构造 RuntimeTool
  → 装入本轮 ToolDispatcher

MCP 连接时
  → McpServerManager::connect/refresh
  → ToolRegistry::register_mcp_server()
  → register_runtime(McpRuntimeTool) + TOOL_CATALOG.register_entry()
```

### 2.3 LLM 可见工具最终决定路径

```text
RuntimeChatTurnDriver::build_turn_config
  → executor.get_tool_defs()
  → executor.load_turn_config_overrides()
    → chat_runtime_impl::build_visible_tool_defs()
      → has_authorized_workspace ?
        true  → ToolFilter::All
        false → ToolFilter::Exclude(WORKSPACE_TOOL_NAMES)
      → 再用 allowed_tools（来自 employee whitelist）做白名单过滤
  → TurnConfig.tool_defs = overrides.tool_defs.unwrap_or(tool_defs)
  → LlmStepInput.tool_defs → provider API tools
```

注：`chat.rs:1262` 还有一条 `get_tool_defs()` 用 `ToolFilter::Only(DAILY_ALLOWED_TOOLS)` 的平行路径，仅 daily_assistant_agent 走它；不参与普通对话路径。

---

## 3. 工具完整清单（按状态分组）

### 3.1 全局 RuntimeTool（启动时无条件注册，13 个）

来源：`src-tauri/src/plugin/builtin/tools/mod.rs:17-66`。

| 工具名 | 注册行 | catalog 行 | 说明 |
|---|---|---|---|
| `list_directory` | mod.rs:32 | 138 | 列目录 |
| `read_workspace_file` | mod.rs:35 | 153 | 读文件 |
| `search_files` | mod.rs:38 | 169 | glob 找文件名 |
| `get_file_info` | mod.rs:41 | 186 | 元数据 |
| `write_file` | mod.rs:44 | 200 | 全文覆盖写 |
| `edit_file` | mod.rs:47 | 221 | 字符串替换 |
| `grep_content` | mod.rs:53 | 300 | 内容正则 |
| `AskUserQuestion` | mod.rs:55 | 698 | 多选问询 |
| `TaskCreate` | mod.rs:58 | 781 | 建任务 |
| `TaskUpdate` | mod.rs:61 | 798 | 改任务 |
| `TaskList` | mod.rs:64 | 821 | 列任务 |
| `bash` | mod.rs:50 `cfg(not(windows))` | 243 | shell |
| `powershell` | mod.rs:52 `cfg(windows)` | 268 | PowerShell |

### 3.2 请求级 RuntimeTool（按请求构造，16 个 + 1 个死分支）

`REQUEST_SCOPED_RUNTIME_TOOL_NAMES`（registry.rs:128-145）：

```rust
"web_search", "browse_navigate", "read_page_content", "page_execute_js",
"extract_table_data", "extract_with_pagination", "browse_and_extract",
"load_file", "browse_data", "spawn_subagent", "execute_python",
"generate_report", "generate_chart", "write_memory", "search_memory",
"load_skill",
```

`try_build_request_scoped_tool` match arm（registry.rs:780-1085）共 17 个具名分支：以上 16 个 + **`task_output` (registry.rs:968)**。

| 工具 | 名单行 | 工厂分支行 | RuntimeTool 实现 | 关键依赖 |
|---|---|---|---|---|
| `web_search` | 129 | 788 | builtin/network.rs:36 | `tavily_api_key` / `bocha_api_key` / `auth_manager` |
| `browse_navigate` | 130 | 797 | builtin/browser.rs:72 | `connector_engine` / `file_manager` |
| `read_page_content` | 131 | 810 | builtin/browser.rs:160 | 同上 |
| `page_execute_js` | 132 | 823 | builtin/browser.rs:271 | 同上 |
| `extract_table_data` | 133 | 836 | builtin/browser.rs:609 | 同上 |
| `extract_with_pagination` | 134 | 849 | builtin/browser.rs:757 | 同上 |
| `browse_and_extract` | 135 | 863 | builtin/browser.rs:334 | 同上 |
| `load_file` | 136 | 876 | builtin/file.rs:20 | 无 |
| `browse_data` | 137 | 877 | builtin/browse_data.rs:68 | `RequestScopedRuntimeDeps` 全套 |
| `spawn_subagent` | 138 | 884 | builtin/spawn_subagent.rs | `app_handle` + 4 个 app state Arc |
| `execute_python` | 139 | 993 | builtin/python.rs:36 | `python_runtime` 失败则 stub |
| `generate_report` | 140 | 1023 | builtin/report.rs:15 | `python_runtime` + `auth_manager` |
| `generate_chart` | 141 | 1044 | builtin/chart.rs:15 | `python_runtime` |
| `write_memory` | 142 | 1063 | builtin/memory.rs | `storage.base_dir()` |
| `search_memory` | 143 | 1070 | builtin/memory.rs | 同上 |
| `load_skill` | 144 | 1077 | builtin/load_skill.rs | `skill_registry` |
| **`task_output`** ⚠️ | **缺失** | **968** | **builtin/task_output.rs:20** | **app_handle + UserScopedPathResolver** |

⚠️ **bug：`task_output` 在工厂里有 match arm（registry.rs:968），但名字没加进 `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` → 该分支永远不会被触发。**

### 3.3 MCP 动态工具

- 命名规则：`mcp__<server_name>__<tool_name>`
- 注册时机：`McpServerManager::connect/refresh` → `ToolRegistry::register_mcp_server()` → 进入 runtime_tools + TOOL_CATALOG
- 清理：disconnect / refresh / unregister 时同步移除

### 3.4 catalog 僵尸（7 个）

以下工具：catalog 有 entry，但全局注册 ∪ 请求级名单都没有，且旧 `tool_executor/` handler 已无任何调用方（`registry.execute()` 已 `#[deprecated]`）。

| 工具 | catalog 行 | 旧 handler 残留 | 状态 |
|---|---|---|---|
| `export_data` | 555 | tool_executor/export.rs:36 | 完全死路 |
| `generate_slides` | 648 | tool_executor/slides.rs:32 | 完全死路 |
| `hypothesis_test` | 747 | tool_executor/stats.rs:29 | 完全死路；prompt_guard.rs:724 已断言 retired |
| `detect_anomalies` | 762 | tool_executor/stats.rs:61 | 同上，prompts.rs:725 retired |
| `plan_update` | 687 (for 循环) | 无（仅 catalog stub） | 完全死路 |
| `progress_update` | 687 (for 循环) | 无 | 完全死路 |
| `save_analysis_note` | 687 (for 循环) | tool_executor/notes.rs:11 | 完全死路 |

---

## 4. DAILY_ALLOWED_TOOLS 与 WORKSPACE_TOOL_NAMES

### 4.1 DAILY_ALLOWED_TOOLS（catalog.rs:900-920）

18 个工具：`bash`, `powershell`, `read_workspace_file`, `write_file`, `edit_file`, `list_directory`, `search_files`, `get_file_info`, `grep_content`, `write_memory`, `search_memory`, `spawn_subagent`, `task_output`, `load_skill`, `AskUserQuestion`, `TaskCreate`, `TaskUpdate`, `TaskList`。

⚠️ 包含 `task_output` 但因 3.2 中 bug，daily 模式实际拿不到该 RuntimeTool。
⚠️ 同时含 `bash` 和 `powershell` —— 在 Unix 上 `powershell` 不可达；在 Windows 上 `bash` 不可达。

### 4.2 WORKSPACE_TOOL_NAMES（chat_runtime_impl.rs:19-28）

8 个工具，无授权 workspace 时被 `ToolFilter::Exclude` 隐藏：`list_directory`, `read_workspace_file`, `search_files`, `get_file_info`, `write_file`, `edit_file`, `bash`, `grep_content`。

⚠️ **漏列 `powershell`**：Windows 上无授权目录时，`powershell` 不会被排除；同时 `bash` 在 Windows 上根本没注册，被排除的是不存在的工具名。

---

## 5. 内置 Agent 的 allowed_tools

| Agent | allowed_tools | 来源 |
|---|---|---|
| `daily_assistant_agent` | DAILY_ALLOWED_TOOLS（18 个） | builtin/daily_assistant_agent.rs:8 |
| `explore` | `read_workspace_file`, `grep_content`, `search_files`, `list_directory`, `web_search` | builtin/explore.rs:9-15 |
| `browse_data_agent` | `browse_and_extract`, `browse_navigate`, `read_page_content`, `page_execute_js`, `extract_table_data`, `extract_with_pagination` | builtin/browse_data_agent.rs:7-14 |
| `general-purpose` | `vec![]` 表示全集，受 `ALL_AGENT_DISALLOWED` 过滤 | builtin/general_purpose.rs:9 |

### 5.1 ALL_AGENT_DISALLOWED bug

`tool_whitelist.rs:9-13` 当前内容：

```rust
"ask_user_question", "exit_plan_mode", "enter_plan_mode"
```

⚠️ **这里有 3 个错误**：

1. `ask_user_question` 与 catalog 实际工具名 `AskUserQuestion` 大小写不一致 → **过滤永不命中**。
2. `exit_plan_mode` / `enter_plan_mode` 在 Lotus 的 catalog 里**根本不存在**（Lotus 没有 PlanMode）→ 这两条规则纯属占位无效。
3. 无 `Agent` / `spawn_subagent` 禁止 → **子 agent 可以无限递归 spawn 子 agent**（claude-code-best 默认禁止递归 spawn）。

整体效果：当前子 agent 的"系统级禁止"实际上一条都没生效。

---

## 6. claude-code-best 工具系统

### 6.1 主链路

- 来源：`src/tools.ts`
- `getAllBaseTools()` (L191)：所有内置工具源头，由编译标志/feature gate 决定包含哪些。
- `getTools(permissionContext)` (L269)：从 baseTools 移除特殊工具（MCP 资源、SyntheticOutput），过滤 deny rule，过滤 `isEnabled()`。`CLAUDE_CODE_SIMPLE=1` 时只剩 Bash/Read/Edit。
- `assembleToolPool(permissionContext, mcpTools)` (L343)：内置 + MCP（按 deny rule 过滤）合并，按字母排序，内置工具在 MCP 前，同名时内置优先。
- 工具的 description 来自 `await tool.prompt({...})`（utils/api.ts:171），inputSchema 来自 `zodToJsonSchema(tool.inputSchema)` 或直接 `tool.inputJSONSchema`。

### 6.2 默认开启的核心工具（25 个 + cron 4 个）

| 工具 | 文件 | 说明 |
|---|---|---|
| `Agent`（别名 `Task`） | tools/AgentTool/AgentTool.tsx | spawn 子 agent |
| `AskUserQuestion` | AskUserQuestionTool/ | 多选问询 |
| `Bash` | BashTool/BashTool.tsx | shell；ls 由内部 `BASH_LIST_COMMANDS` 识别 |
| `PowerShell` | PowerShellTool/ | Windows 限定 |
| `Read` | FileReadTool/ | 读文件 |
| `Write` | FileWriteTool/ | 写文件 |
| `Edit` | FileEditTool/ | 精确替换；要求先 Read |
| `Glob` | GlobTool/ | 文件名 glob |
| `Grep` | GrepTool/ | 内容 ripgrep |
| `WebFetch` | WebFetchTool/ | 抓 URL |
| `WebSearch` | WebSearchTool/ | 搜索（auto-allow） |
| `NotebookEdit` | NotebookEditTool/ | Jupyter cell |
| `Skill` | SkillTool/ | SKILL.md 加载 |
| `EnterPlanMode` / `ExitPlanMode` | EnterPlanModeTool/ + ExitPlanModeV2Tool | 规划模式 |
| `EnterWorktree` / `ExitWorktree` | (worktree feature) | worktree 切换 |
| `TaskCreate` / `TaskGet` / `TaskList` / `TaskUpdate` | TaskCreateTool/ 等 | TodoV2 任务（受 `isTodoV2Enabled()` 控制） |
| `TaskOutput` | TaskOutputTool/ | 读异步子任务输出 |
| `TaskStop` | TaskStopTool/ | 终止异步子任务 |
| `TodoWrite` | TodoWriteTool/ | 旧 todo（与 Task* 共存） |
| `SendMessage` | SendMessageTool/ | 跨 agent 通信 |
| `CronCreate` / `CronDelete` / `CronList` | ScheduleCronTool/ | 定时任务 |
| `TeamCreate` / `TeamDelete` | TeamCreateTool/ + TeamDeleteTool | 团队（受 `isAgentSwarmsEnabled()`） |
| `Sleep` | SleepTool/ | 等待 |
| `LSP` | LSPTool/ | LSP 集成（`ENABLE_LSP_TOOL=1`） |
| `ToolSearch` | ToolSearchTool/ | 延迟加载工具搜索 |

> 注意：claude-code-best **没有独立 `LS` 工具**。`ls` 由 BashTool 内部识别为 `isList: true`。

### 6.3 子 Agent 工具过滤

`ALL_AGENT_DISALLOWED_TOOLS`（src/constants/tools.ts:36）：
`TaskOutput`, `ExitPlanMode`, `EnterPlanMode`, `Agent`, `AskUserQuestion`, `TaskStop`, `Workflow`。

- 默认禁止递归 spawn（`Agent` 在禁止列表）。`USER_TYPE === 'ant'` 时放开。
- Teammate 绝对禁止 spawn 其他 teammate（`AgentTool.tsx:425`）。
- `explore` agent 用 `tools: ['*']` + `disallowedTools: [Agent, ExitPlanMode, FileEdit, FileWrite, NotebookEdit]`，read-only。
- `general-purpose` agent 用 `tools: ['*']` + 无 disallowedTools，受 `filterToolsForAgent` 过滤。
- `ASYNC_AGENT_ALLOWED_TOOLS`（src/constants/tools.ts:55）：异步 agent 仅允许 Read/Write/Edit/Bash/Glob/Grep/WebFetch/WebSearch/Skill/NotebookEdit/TodoWrite/ToolSearch/EnterWorktree/ExitWorktree + SHELL_TOOL_NAMES + SyntheticOutput。

### 6.4 MCP 命名规则

`mcp__${normalizeNameForMCP(serverName)}__${toolName}`（src/services/mcp/utils.ts:40）。
环境变量 `CLAUDE_AGENT_SDK_MCP_NO_PREFIX=1` 时去掉 `mcp__` 前缀。

---

## 7. Lotus 与 claude-code-best 工具对照

### 7.1 同义工具（命名差异）

| Lotus | claude-code-best | 备注 |
|---|---|---|
| `read_workspace_file` | `Read` | 语义等价 |
| `write_file` | `Write` | 语义等价 |
| `edit_file` | `Edit` | 语义等价；都要求先 Read |
| `search_files` | `Glob` | Lotus 名字误导（"search files" 听起来像内容搜） |
| `grep_content` | `Grep` | 语义等价 |
| `bash` / `powershell` | `Bash` / `PowerShell` | 仅大小写差异 |
| `web_search` | `WebSearch` | 语义等价 |
| `spawn_subagent` | `Agent`（别名 `Task`） | 语义等价 |
| `list_directory` | （无独立工具，由 Bash `ls` 处理） | claude-code-best 不单设此工具 |
| `task_output` | `TaskOutput` | 语义等价；Lotus 当前因 bug 不可用 |
| `load_skill` | `Skill` | 语义等价 |
| `AskUserQuestion` | `AskUserQuestion` | 完全一致 |
| `TaskCreate` / `TaskUpdate` / `TaskList` | 同名 | 完全一致 |

### 7.2 Lotus 有但 claude-code-best 没有

业务工具，应保留：

- `load_file`：把上传的 CSV/Excel/PDF 加载成 Python 变量（_df/_text）
- `execute_python`：受沙箱约束的 Python 解释器
- `generate_report`：HTML/Markdown/PDF/DOCX 报告生成
- `generate_chart`：交互式图表
- `browse_navigate` / `read_page_content` / `page_execute_js` / `extract_table_data` / `extract_with_pagination` / `browse_and_extract` / `browse_data`：浏览器自动化与企业内系统数据抽取
- `write_memory` / `search_memory`：本地记忆库

### 7.3 claude-code-best 有但 Lotus 没有

| 工具 | 用途 | Lotus 缺失影响 |
|---|---|---|
| `WebFetch` | 抓单个 URL 内容 | 当前必须走 browse_navigate + read_page_content 两步，重 |
| `TaskGet` | 单条任务详情 | 用 TaskList 全列再过滤，浪费 token |
| `TaskStop` | 终止异步子任务 | 异步子 agent 启动后无法取消 |
| `EnterPlanMode` / `ExitPlanMode` | 计划模式 | 依赖前端配套 UI，本期不补 |
| `NotebookEdit` | Jupyter cell 编辑 | Lotus 无 .ipynb 场景 |
| `SendMessage` / `TeamCreate` / `TeamDelete` | 跨 agent / 团队 | Lotus 无对应模型 |
| `CronCreate/Delete/List` / `Sleep` | 定时与等待 | Lotus 调度走后端 cron，不走 LLM 工具 |
| `LSP` / `ToolSearch` / `EnterWorktree` / `ExitWorktree` | 代码场景 | Lotus 主要场景非 IDE |

---

## 8. 改名/调整工程量评估

### 8.1 关键事实

**prompt 文件零硬编码**：`base.md` / `daily.md` 中**没有任何工具名字符串字面量**。LLM 看到的工具名通过 schema 自动注入（`provider API tools`）。这意味着改名不需要改 prompt 主体内容。

**catalog description JSON 内有交叉引用**：`catalog.rs` 中有以下工具间互相提及（改名时必须同步改 description 文字）：

- `write_file` description 提到 `read_workspace_file`、`edit_file`（行 217）
- `edit_file` description 提到 `read_workspace_file`、`write_file`（行 239）
- `load_file` description 提到 `list_directory` / `search_files` / `read_workspace_file`（行 451）
- `spawn_subagent` description 提到 `task_output`（行 581/607/622/624）

### 8.2 引用面汇总

| 旧名 | Rust 源码 | Rust 测试 | 前端 TS | SKILL.md | docs/ | 总计 |
|---|---|---|---|---|---|---|
| `read_workspace_file` | 20/8 | 14/8 | 9/2（测试） | 0 | 82 | 125 |
| `write_file` | 13/5 | 27/13 | 0 | 0 | 68 | 108 |
| `edit_file` | 10/5 | 6/5 | 1（templates.ts） | 1（contract-review） | 37 | 55 |
| `search_files` | 10/5 | 11/8 | 0 | 0 | 58 | 79 |
| `grep_content` | 12/5 | 8/5 | 1（templates.ts） | 1（contract-review） | 38 | 60 |
| `list_directory` | 15/7 | 24/12 | 0 | 0 | 81 | 120 |
| `bash` | 18/7（含 1 系统调用） | 36/15 | 2（templates.ts） | 5（4 个 skill） | 99 | 160 |
| `powershell` | 12/3（含 2 探测） | 2/2 | 0 | 0 | 29 | 43 |
| `web_search` | 17/8 | 16/9 | 1（templates.ts） | 17（7 个 skill） | 68 | 119 |
| `spawn_subagent` | 21/5 | 8/4 | 0 | 0 | 148 | 177 |

### 8.3 关键风险点

- **生产代码硬编码**：`src/features/employees/templates.ts` 中 5 处工具名（`bash` x 2 / `edit_file` / `grep_content` / `web_search`）。改名必须前后端同步。
- **SKILL.md 改名风险**：`web_search` 影响 7 个 skill 17 处；`bash` 影响 4 个 skill 5 处。这些文件在 `~/.renlijia/skills/` 运行时目录，不在 git src 内，需要单独迁移脚本。`contract-review/SKILL.md` 的 `allowed_tools` 字段引用了 `edit_file` / `grep_content`。
- **review_*.rs 测试**：架构约束回归测试（`review_permission_*` 系列、`review_schema_registry_consistency_test`、`review_tool_pool_ordering_test`）会全数受影响。
- **测试文件名包含旧名**：`spawn_subagent_*.rs` 系列、`builtin_runtime_registration_test.rs`、`primitive_tools_migration_test.rs`、`workspace_first_agent_golden_path_test.rs` 等。

---

## 9. 已确认 bug 清单（按修复优先级）

| # | bug | 位置 | 性质 | 修复成本 |
|---|---|---|---|---|
| 1 | `task_output` 工厂分支永不可达 | registry.rs:128-145 缺 `task_output` | 阻塞，daily 模式异步子任务读输出失效 | 1 行 |
| 2 | `ALL_AGENT_DISALLOWED` 大小写错配 + 占位无效项 | tool_whitelist.rs:9-13 | 子 agent 系统级禁止全部失效，可递归 spawn | 几行 |
| 3 | `WORKSPACE_TOOL_NAMES` 漏 `powershell` | chat_runtime_impl.rs:19-28 | Windows 无授权目录时 PowerShell 误暴露 | 1 行 |
| 4 | `DAILY_ALLOWED_TOOLS` 同时含 `bash` + `powershell` 但每平台只一个可达 | catalog.rs:902-903 | 每平台都有一个工具不可调用 | 改注释或拆 cfg |
| 5 | 7 个僵尸工具污染 catalog | catalog.rs:555/648/687/747/762 + tool_executor/ | LLM 看到调不动；schema 噪音 | 中等（涉及 prompt_guard 和 retired 断言） |
| 6 | catalog description 文字内嵌旧工具名引用 | catalog.rs:217/239/451/581 等 | 改名时若漏改这些文字，LLM 收到的说明会包含旧名 | 改名时同步处理 |

---

## 10. 证据索引

### Lotus

- `src-tauri/src/runtime/tools/catalog.rs`
- `src-tauri/src/plugin/registry.rs`
- `src-tauri/src/plugin/builtin/tools/mod.rs`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- `src-tauri/src/transport/tauri_commands/chat.rs`
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
- `src-tauri/src/runtime/agent/tool_whitelist.rs`
- `src-tauri/src/runtime/agent/builtin/*.rs`
- `src-tauri/src/runtime/tools/builtin/*.rs`
- `src-tauri/src/llm/tool_executor/*.rs`（旧 handler，已无调用）

### claude-code-best

- `src/Tool.ts` / `src/tools.ts` / `src/utils/api.ts`
- `src/constants/tools.ts`（ALL_AGENT_DISALLOWED_TOOLS / ASYNC_AGENT_ALLOWED_TOOLS）
- `src/tools/AgentTool/agentToolUtils.ts`（filterToolsForAgent / resolveAgentTools）
- `src/tools/AgentTool/builtInAgents.ts`
- `src/services/mcp/utils.ts`（MCP 命名）

---

## 附录：与初版 md 的差异

对 codex 早先生成的版本做以下纠正：

1. ❌ 初版："`generate_report` / `generate_chart` 是 catalog 僵尸候选" → ✅ 实际有完整 RuntimeTool 实现（builtin/report.rs、builtin/chart.rs），工厂分支也在，正常工作。
2. ❌ 初版："`task_output` 没有 RuntimeTool 实现" → ✅ 实际有实现（builtin/task_output.rs:20）和工厂分支（registry.rs:968），漏的是名单。
3. ❌ 初版："系统级禁止 ask_user_question / enter_plan_mode / exit_plan_mode" → ✅ 这三条规则实际从未生效（大小写错配 + 不存在的工具名）。
4. ❌ 初版："`spawn_subagent` 默认不允许递归" → ✅ 实际允许递归（`ALL_AGENT_DISALLOWED` 不含 `spawn_subagent` 也不含 `Agent`）。
5. ❌ 初版："prompt 中需修改工具名引用" → ✅ prompt 文件零工具名硬编码，主要修改面在 catalog description 文字交叉引用、测试 fixture、docs。

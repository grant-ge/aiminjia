# Managed Runtime Process Env 改造方案

> 日期：2026-06-30
> 范围：AIjia 桌面端本地子进程 runtime 环境注入
> 状态：已实施

## 最终口径

AIjia 不新增工具级 `runtime_env` 参数。用户只有一个全局开关：

- 开启：本地命令默认优先使用 AIjia 自带 Node / Python / uv。
- 关闭：本地命令默认使用系统环境，不注入 AIjia 自带 Runtime。

用户临时说“这次用系统自带 Node / Python / dws”时，Agent 不传额外工具参数，而是使用动态上下文里探测到的系统绝对路径，或先运行系统路径探测命令。系统环境不可用时直接说明不可用，不静默回落到 AIjia 自带 Runtime。

## WorkBuddy 对照

WorkBuddy 的核心不是在 Bash 工具上增加参数，而是在启动 sidecar / agent / MCP 子进程前构造环境变量。

WorkBuddy 常见形态：

```text
WORKBUDDY_EXTRA_PATHS=<managed node dir>;<connector cli dir>
PATH=<managed path>;<original path>
npm_config_prefix=<managed node prefix>
npm_config_cache=<managed npm cache>
```

系统环境出口主要靠配置级开关，或让模型使用系统探测到的绝对路径。AIjia 采用同一类产品语义，但不改成 WorkBuddy 的进程树架构；AIjia 的真实 spawn 边界仍是 Bash / PowerShell / Skill shell / MCP stdio 各入口。

## 动态上下文文案

动态上下文固定展示：

```text
[当前环境]
工作目录: <workspace>
Platform: windows|darwin|linux
系统环境检测（未注入 AIjia 自带 Runtime）:
- node: <system node path> 或 未发现
- npm: <system npm path> 或 未发现
- npx: <system npx path> 或 未发现
- python: <system python path> 或 未发现
- uv: <system uv path> 或 未发现
- uvx: <system uvx path> 或 未发现
```

开关开启且 managed runtime 可用时，额外展示：

```text
AIjia 自带运行环境：已开启（默认优先）
Runtime 当前目录: <runtime_root>
Python: <managed python>
Node: <managed node>
npm: <managed npm>
npx: <managed npx>
uv: <managed uv>
uvx: <managed uvx>
Node 全局包目录: <node_modules>
Node 命令目录: <cli_dir>

规则:
1. Bash / PowerShell / Skill / MCP 本地子进程默认会把 AIjia 自带 Runtime 放到 PATH 前面。
2. 普通任务直接使用裸 node / npm / npx / python / python3 / uv / uvx。
3. 工具没有 runtime_env 参数，不要传这个字段。
4. 用户明确要求系统 Node / Python / npm / uv 时，使用上方“系统环境检测”里的系统绝对路径。
```

开关关闭时展示：

```text
AIjia 自带运行环境：已关闭（默认使用系统环境）
规则:
1. 本地 Bash / PowerShell / Skill / MCP 子进程不会注入 AIjia 自带 Runtime。
2. 裸 node / npm / npx / python / python3 / uv / uvx 来自系统 PATH。
3. 工具没有 runtime_env 参数，不要传这个字段。
```

macOS 额外通过 login shell 补充 `command -v` 探测，避免 GUI 应用 PATH 和用户终端 PATH 不一致。Windows 使用 `where.exe` 探测；非 Windows 使用 `which -a`，并过滤掉 AIjia managed runtime root 下的路径。

## Env Patch 构造层

新增统一构造层 `ManagedRuntimeProcessEnv`，只负责从 `WorkspaceDependencies` / `RuntimeResolver` 产出 env patch。

注入内容：

```text
PATH=<node dir>;<python dir>;<uv dir>;<原 PATH>
NODE_PATH=<managed node_modules>
npm_config_prefix=<managed node prefix>
npm_config_cache=<managed node prefix>/.npm-cache
```

注意：

- 这是子进程 env patch，不修改用户系统 PATH。
- Windows / macOS / Linux 使用当前平台的路径分隔符。
- PATH 前缀去重，避免重复堆叠 managed runtime 目录。
- Windows 的 npm global shim 目录按 node prefix 形态处理。

## 全局开关

新增设置字段：

```text
managedRuntimeEnabled: boolean
```

默认值：`true`。

设置页 Runtime 面板新增开关“优先使用 AIjia 自带运行环境”。更新设置时：

1. 写入本地 settings。
2. 同步刷新内存态 `ManagedRuntimePreference`。
3. 后续 Bash / PowerShell / Skill shell / MCP stdio spawn 读取最新内存态。

已经启动的 MCP stdio 进程无法被热修改环境变量；开关变化对后续连接 / 重连 / 新启动的 MCP 进程生效。

## 接入点

### Bash / PowerShell

工具 schema 不暴露 `runtime_env`。

执行前按 `ToolExecutionContext.managed_runtime_enabled` 判断：

- `true`：调用 `ManagedRuntimeProcessEnv::from_resolver(...)` 并 apply 到子进程。
- `false`：跳过 managed runtime env 注入。

### Skill `!cmd`

`SkillSubstitutionContext` 携带 `managed_runtime_enabled` 布尔快照。

内联 shell 块执行前复用同一套 `ManagedRuntimeProcessEnv` 注入逻辑。这样技能里描述“缺 dws 就安装”时，默认安装到 AIjia 自带环境；用户明确要求系统 dws 时，Agent 需要用动态上下文里的系统路径或系统探测命令，而不是靠工具参数切换。

### MCP stdio

`McpServerConfig` 不包含 `runtime_env` 字段。

`StdioMcpConnection` 持有 `Arc<ManagedRuntimePreference>`：

- 开关开启：连接时注入 managed runtime env。
- 开关关闭：连接时不注入。
- 旧 JSON 配置里残留 `runtime_env` 字段时，serde 按未知字段忽略，避免历史配置坏档。

### QueryEngine / 子 Agent

`QueryEngine` 和 `ToolExecutionContext` 都携带 `managed_runtime_enabled`。

主对话、request-scoped dispatcher、子 Agent、teammate idle runtime 都从当前 settings / runtime deps 传递同一个布尔快照，保证同一轮工具调用口径一致。

## 意图测试口径

先改 Runtime / 技能意图，再改技术实现。

Runtime task 覆盖：

- 默认裸命令命中 AIjia 自带 Runtime。
- 关闭开关时默认系统环境。
- 用户明确要求系统环境时不出现 `toolCalls[].arguments.runtime_env` 字段。
- 系统 Node / Python 不可用时说明不可用，不回落到 AIjia Runtime。
- 第三方 Python / Node 包安装后新对话复用，不重复安装。
- dws 等技能 CLI 的“默认补齐”归技能 task 覆盖。

Skill task 覆盖：

- dws 缺失时默认补到 AIjia 自带环境。
- 用户明确指定系统 dws 时使用系统路径 / 系统 PATH 探测，不安装、不回落、不传 `runtime_env`。

## 风险与边界

1. PATH 优先级变化会让裸 `node` / `python` 默认命中 AIjia Runtime，这是预期行为；系统请求必须走动态上下文系统路径。
2. macOS GUI PATH 可能缺少用户 shell 初始化结果，所以需要 login shell 探测补充。
3. Windows 上 Python 命令可能是 `python` 或 `py`；动态上下文需要把探测结果完整给 Agent。
4. 已连接 MCP 进程不会因开关切换自动改变 env；需要重连后生效。
5. HookRunner 暂不纳入本次默认注入，它更接近用户自定义系统命令。

## 验证口径

必须验证：

- settings 默认值和持久化读取。
- Bash / PowerShell schema 不再暴露 `runtime_env`。
- Bash / PowerShell 开关开启时注入 managed env，关闭时跳过。
- Skill shell 复用同一注入逻辑。
- MCP stdio 使用全局 preference，旧配置字段被忽略。
- 动态上下文同时包含系统探测和 managed runtime 策略提示。
- 前端 Runtime 设置开关能写入 settings 并刷新内存配置。

# Subagent 系统对标报告 v2：lotus-app vs claude-code-best（按三模式拆分）

> **版本**：v2 (2026-04-30)，重写于 v1 之后。
>
> **重写动机**：v1 把 claude-code-best 的 Agent 能力挤在一条链路里描述，遗漏了"主会话 agent profile / 普通 subagent / Agent Teams"三种模式的本质差异，导致后续 lotus 的 `send_message(agent_name)`、`spawn_subagent`、`team.rs` 容易被设计成混合模型。本版本以三模式分别独立成节为骨架，并补齐 v1 review 中标出的 9 处空白：CLI/flag/main-thread agent 细节、send_message 双路由、Resume 链路、TaskOutput/notification、权限差异、按模式分开的存储路径矩阵。
>
> **目的**：作为后续"Agent 能力大计划"的事实基础。先把模式边界划清楚，再设计实现，避免 lotus 把三种模式糅在一个混合实现里。
>
> **方法**：三轮调研，覆盖 lotus 现状 + claude-code-best 三模式 + 跨模式专题。

---

## 0. 三种模式定义（Mental Model）

不同视角下 "agent" 是不同的东西。先把术语固定下来，避免后文歧义：

| 模�� | 角色 | 进程位置 | LLM loop |
|---|---|---|---|
| **A. 主会话 Agent Profile** | 决定**当前会话**用哪个 system prompt / tools / model | 主进程，无独立 loop | 主 query loop 直接消费 |
| **B. 普通 Subagent (sync/async)** | 父 agent 派出去做一件事，结果回传 | 主进程内 detached promise | 独立 LLM loop，有 child run |
| **C. Agent Teams / Swarm** | 一组并行运行的 teammate，可互发消息 | 独立 OS 进程（pane）或 ALS 隔离 | 每个 teammate 独立 loop + mailbox |

**lotus-app 当前**：A 部分有（`ChatRequest.agent_name`）、B 部分有（业务专用 `browse_data`）、C 几乎没有（仅 `team.rs` 占位结构体）。
**claude-code-best**：三种模式全部完整实现。

---

## 一、lotus-app 现状（按三模式重组）

### 1.A 主会话 Agent Profile

**入口**
- `transport/tauri_commands/chat.rs:1988` `send_message` Tauri command 接受 `agent_name: Option<String>` 参数
- 透传到 `runtime/chat/chat_turn_driver.rs:101` `ChatTurnRequest.agent_name`
- 仅本轮请求级，不持久化到 conversation 元数据

**定义来源**
- 仅 `AgentRegistry::with_builtins()` 硬编码（同 1.B）
- 无 main-thread vs subagent 区分：当前是同一份 `AgentDefinition` 想用哪儿用哪儿

**生命周期**
- 仅本 turn 生效，下轮如不传 `agent_name` 则回到默认
- 不写 session metadata，不影响 `--resume`（lotus 也无 resume 概念）

**system prompt / tools 注入**
- 当前实际上**未消费** `agent_name`：grep 结果显示 `ChatTurnRequest.agent_name` 设值后没有读者。属于"参数已穿但功能未接通"状态

**与 subagent 边界**
- 设计上想共用 `AgentDefinition`，但实际接通的只有 subagent 路径

**结论**：1.A **结构占位完成、功能未接通**。

---

### 1.B 普通 Subagent

**入口工具**
- 唯一入口：业务工具 `browse_data`（`plugin/builtin/tools/browse_data.rs:31` + `runtime/tools/builtin/browse_data.rs`）
- 无通用 `spawn_subagent` / `Agent` / `Task` 工具
- LLM 不能主动决定派子 agent，只能通过 browse_data 间接触发

**定义**（见 v1 §1.1）
```rust
AgentDefinition { name, description, allowed_tools, max_iterations,
                  model: AgentModel::{Inherit|Fixed}, system_prompt, source }
```

**SubAgentConfig**（`llm/sub_agent.rs:85`）
```
task, system_prompt, allowed_tools, max_iterations, dynamic_context,
conversation_id, parent_run_id, background, app_handle, cancel_token, permission_mode
// 缺：model_override, isolation, agent_name, team_name
```

**调用链路**
```
父 LLM tool_use("browse_data")
  → execute_browse_data → SubAgentConfig
    → run_sub_agent (递归守卫: allowed_tools 不能含 browse_data)
      → SubagentWorkerRuntime::run
        → agent_runtime.spawn_child_run() → AgentInvocationRecord
        → loop { gateway.stream_message; ToolRoundDriver.execute_round } 直至上限
        → SubAgentResultEnvelope 回传父
```

**模型指定**
- `AgentModel::Fixed(String)` 字段存在但无消费者；`worker_runtime.rs:63-75` 直接克隆父 `LlmGateway` → **永远复用父模型**

**工具白名单**
- 单层 filter（`worker_runtime.rs:99-103`）：从 registry 全集按 `allowed_tools` 过滤
- 无系统级 disallowed list、无 async 专属 allowed list、无递归 spawn 通用控制（仅 browse_data 单点硬编码）

**异步 / 后台**
- `SubAgentConfig.background: bool` + `RuntimeEvent::AgentIdle`
- 无 sync→async 动态迁移
- 无 named async agent 概念（无 `agentNameRegistry` 等价物）

**结果回传**
- `SubAgentResultEnvelope` 序列化为 `subagent-envelope:v1:<json>` 前缀字符串，存入 `AgentInvocationRecord.summary_or_output_ref`
- **无 TaskOutput tool 等价物**，父无法增量读取后台 agent 的 progress
- **无 `<task-notification>` 等价物**，父上下文不会被自动注入"子 agent 完成"信号

**Resume**
- `AgentRuntime::resume_child_run` 存在，但**不等价**于"恢复后台 LLM loop"——只是重新读出 invocation 元数据。无 pendingMessages、无 sidechain 重建、无后台 LLM loop 唤醒

**存储**
- invocation：`~/.renlijia/agent_invocations.json`（JSON 数组）
- transcript：`~/.renlijia/subagent_transcripts/<sanitized-child-run-id>.json`（一文件一条）

---

### 1.C Agent Teams

**几乎不存在**。
- `runtime/agent/team.rs` 只有：
  ```rust
  pub struct TeamContext { team_id: String, agent_ids: Vec<AgentId> }
  ```
- 无 TeamFile 持久化、无 mailbox、无 backend、无 SendMessage 路由、无 idle/shutdown 协议
- LLM 无 `TeamCreate` / `SendMessage` 工具

---

## 二、claude-code-best 实现（三模式分别完整描述）

### 2.A 主会话 Agent Profile (main-thread agent)

#### 入口
- CLI `--agent <name>`：选择已加载的 agent
- CLI `--agents <json>`：注入 inline JSON 定义为 `flagSettings` 来源
- `getInitialSettings().agent`：会话恢复时重读
- `/agents` 命令：运行时切换（`AgentsMenu.tsx`）

#### 定义来源（六层合并）
`getAgentDefinitionsWithOverrides(cwd)` (`loadAgentsDir.ts:296`，按 cwd memoize)：

```
builtIn < plugin < userSettings < projectSettings < flagSettings < policySettings
```

后者覆盖前者同名 agentType（`loadAgentsDir.ts:203-220`）。

具体路径：
| 来源 | 路径 |
|---|---|
| builtIn | 代码中 `getBuiltInAgents()`：general-purpose / Explore / Plan / verification … |
| plugin | `loadPluginAgents()` |
| userSettings | `~/.claude/agents/*.md`（可被 `CLAUDE_CONFIG_DIR` 覆盖） |
| projectSettings | `<project>/.claude/agents/*.md`（cwd 向上至 git root，每层独立） |
| flagSettings | `--agents <json>` 解析（`parseAgentsFromJson()` `loadAgentsDir.ts:521`） |
| policySettings | 管理员策略目录 |

禁用开关：
- `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS`：仅 `getIsNonInteractiveSession()` 时清空内置（`builtInAgents.ts:26-29`）
- `CLAUDE_CODE_SIMPLE=1`：跳过 custom/plugin，只留 builtIn

#### 创建/选择 → 切换
1. `--agent` flag 或 `settings.agent` → `main.tsx:3119-3126` 在 `activeAgents` 中查找
2. `/agents` → `setMainThreadAgentDefinition()` 更新 REPL state，下一 turn 生效
3. `--agents <json>` → 注入 flagSettings 后参与合并

#### `initialPrompt` 使用时机
`main.tsx:3176-3184`：在 `inputPrompt` 提交前拼接到最前面：
- `inputPrompt` 是字符串：`${initialPrompt}\n\n${inputPrompt}`
- 否则替换为 `initialPrompt`

#### 生命周期
- `mainThreadAgentDefinition` 存活整个 REPL session，`useState` 保存
- `/clear` 不重置 agent
- `/compact` 显式传 `mainThreadAgentDefinition: undefined`（`compact.ts:270`）
- 持久化：`saveAgentSetting(agentType)` 写入 transcript metadata，`--resume` 时恢复

#### system prompt 与 tools 注入
- `buildEffectiveSystemPrompt()` (`systemPrompt.ts:42`) 每个 turn 实时构建
- 优先级：override > coordinator > **agent** > custom > default，最后追加 appendSystemPrompt
- proactive/KAIROS 模式下：agent prompt **追加**到 default（不替换）
- tools：`resolveAgentTools(mainDef, mergedTools, false, true)` (`REPL.tsx:1044`)

#### 与 subagent 的边界
**同一份 .md 文件可既作 main 也作 sub**——通过 `--agent`/`/agents` 选作主，通过 `AgentTool.subagent_type` 选作子；两路独立过滤 tools，互不干扰。

---

### 2.B 普通 Subagent (sync + async LocalAgentTask)

#### 入口
`AgentTool.tsx::call()`。走 async 路径的条件（任一）：
- `run_in_background=true`
- agent definition `background: true`
- coordinator mode 启用
- `isForkSubagentEnabled()`
- `assistantForceAsync`（KAIROS）
- proactive 激活

否则走 sync 路径。

#### 定义来源
共用 §2.A 的 `activeAgents`，通过 `subagent_type` match。fork 路径（省略 `subagent_type`）使用合成 `FORK_AGENT` (`forkSubagent.ts:60`)。

#### 创建（async 路径）
1. `selectedAgent` 选定后 → `registerAsyncAgent()` (`LocalAgentTask.tsx:574`) 创建 `LocalAgentTaskState`，注册到 `AppState.tasks`
2. `initTaskOutputAsSymlink(agentId, transcriptPath)` (`diskOutput.ts:427`) 建立 `<sessionTempDir>/tasks/<agentId>.output → transcript JSONL` 的 symlink
3. 若传入 `name`：`agentNameRegistry.set(name, agentId)` (`AgentTool.tsx:974`) — **普通 async agent 也能命名，不仅是 team teammate**
4. `void runAsyncAgentLifecycle(...)` detached promise 启动

#### 生命周期状态机
```
running ──┬──→ completed (completeAsyncAgent)
          ├──→ failed   (failAsyncAgent)
          └──→ killed   (killAsyncAgent → AbortController)

isBackgrounded: bool   — 是否在后台
retain: bool           — UI 是否持有（阻 evict）
diskLoaded: bool       — 已从 JSONL bootstrap
evictAfter: ts         — 完成后 PANEL_GRACE_MS 后 GC
```

#### 退出条件
1. LLM 停止（query 返回 `done`）
2. `maxTurns` 达上限（`runAgent.ts:757`）
3. `AbortController.abort()`（用户 kill / 父链传播）
4. 未捕获异常

#### 消息路由：父 → async agent
**SendMessage 双路由 dispatch（`SendMessageTool.ts:800-873`）**：

```
SendMessage({ to, message })
  → agentNameRegistry.get(to)
       ├─ 命中 (named async agent)
       │    ├─ task running        → queuePendingMessage(taskId, message)
       │    ├─ task stopped (state 在但非 running)
       │    │                      → resumeAgentBackground(...)
       │    └─ task evicted (state 不在)
       │                           → resumeAgentBackground(...) 从磁盘 transcript 恢复
       └─ 未命中
            → handleMessage()  → writeToMailbox()  (走模式 C)
```

**唤醒机制（不是事件驱动）**：running async agent 的 LLM loop 在每个 tool round 调用 `getAttachmentMessages()` → `getAgentPendingMessageAttachments()` → `drainPendingMessages()` (`attachments.ts:1091`)，把队列中的 string 转成 `queued_command` attachment 注入下一个 user message。

#### 父观察 async agent：TaskOutput tool
- `TaskOutput(taskId, offset?)` → `getTaskOutputDelta(taskId, offset)` 读 output 文件增量
- output 文件是 transcript JSONL 的 **symlink**，非独立文件

#### `<task-notification>` 投递（注入父上下文）
完成时 `enqueueAgentNotification()` → `enqueuePendingNotification(...)` → 内存 `messageQueueManager` → `query.ts:1578-1636` 在每个 tool round 的 `getAttachmentMessages()` drain。

XML 结构（`LocalAgentTask.tsx:338-345`）：
```xml
<task-notification>
  <task-id>{agentId}</task-id>
  <tool-use-id>{parentToolUseId}</tool-use-id>      <!-- async only -->
  <output-file>...</output-file>
  <status>completed|failed|killed</status>
  <summary>...</summary>
  <result>{finalMessage}</result>                    <!-- optional -->
  <usage><total_tokens>N</total_tokens>...</usage>   <!-- optional -->
  <worktree>...</worktree>                            <!-- worktree 隔离才有 -->
</task-notification>
```

只向当前 agentId 匹配的 subagent 投递；主线程消费所有 task-notification。

#### Resume 链路（关键）
`resumeAgentBackground()` (`resumeAgent.ts:42`)：
1. 并行读 `getAgentTranscript(agentId)` + `readAgentMetadata(agentId)`
2. 三重过滤 transcript：whitespace-only assistant → orphaned thinking-only → unresolved tool uses
3. `reconstructForSubagentResume()` 从父 `contentReplacementState` 重建 replacement 映射（保 prompt cache 命中）
4. 从 meta 读 `agentType`，在 `activeAgents` 中找 definition；fork agent 特殊处理：重建父 `renderedSystemPrompt`
5. `registerAsyncAgent()` 重新注册（保留 agentNameRegistry 原条目）
6. `runAsyncAgentLifecycle({ promptMessages: [...resumed, createUserMessage(prompt)] })`

sidechain JSONL 链路：通过 `parentUuid` 构链，`buildConversationChain(messages, leafMessage)` 从 leaf 上溯重建（`sessionStorage.ts:4191-4237`），过滤 `agentId` 匹配的消息。

#### Retain / Evict
- 完成或 killed：若 `retain=false` → `evictAfter = now + PANEL_GRACE_MS`；若 `retain=true`（UI 持有） → 不设
- `evictTaskOutput()` 仅 flush + 清内存 map，**不删磁盘**（`diskOutput.ts:288`）
- 因此后台 agent transcript 长期保留在磁盘，可重 resume

---

### 2.C Agent Teams / Swarm

#### 入口
- `TeamCreate` tool（`teamHelpers.ts`）创建 team
- `AgentTool` 传 `team_name + name` 触发 `spawnTeammate()`（`AgentTool.tsx:449`）
- `TeamDelete` tool 清理

#### TeamFile（持久化）
路径：`~/.claude/teams/<teamName>/config.json`（`getTeamFilePath()`）

格式（`teamHelpers.ts:64`）：
```ts
TeamFile {
  name: string,
  leadAgentId: string,
  leadSessionId?: string,
  members: [{
    agentId, name, agentType?,
    tmuxPaneId, cwd, backendType,
    subscriptions, isActive?, mode?
  }]
}
```

#### Backend：两种执行体
**PaneBackendExecutor**（tmux/iTerm2，独立 OS 进程）：
- `PaneBackendExecutor.spawn()` (`PaneBackendExecutor.ts:114-158`) 创建 pane
- 发送命令：`claude --agent-id <id> --agent-name <name> --team-name <team> --agent-color <color>`
- 然后 `writeToMailbox(name, {from:'team-lead', text: prompt}, teamName)` 投初始任务

**InProcessTeammateTask**（`--in-process-teammates`，同进程）：
- AsyncLocalStorage 隔离运行
- 无 pane，注册 `InProcessTeammateTaskState`

#### 消息路由：team mailbox
- 路径：`~/.claude/teams/<teamName>/inboxes/<agentName>.json`（JSON 数组，proper-lockfile 锁）
- teammate 通过 `useInboxPoller` 定期拉取，未读消息包装成 `<teammate_message teammate_id="from">text</teammate_message>` XML 注入 LLM context
- 协议消息类型：`task_assignment`、`approval`、`idle_notification`、`shutdown_request`、`shutdown_approved`、`permission_request`、`permission_response`

#### 生命周期
1. **spawned**：TeamFile 写入 member
2. **running**：teammate 轮询 inbox 处理 `<teammate_message>`
3. **idle**：teammate 通过 Stop hook 发 `idle_notification` JSON 到 team-lead inbox
4. **shutdown_request** 收到 → 回 `shutdown_approved` → exit
5. **removed**：leader 收到 approved 后从 TeamFile 移除 member，kill pane

#### 退出条件
- 主动：leader 发 `shutdown_request` → teammate 批准 → `gracefulShutdown(0)` 或 abort in-process controller
- 被动：leader 进程退出 → `registerCleanup()` 中 `PaneBackendExecutor.kill()` 关闭所有 pane
- `TeamDelete`：清理 TeamFile 和 inbox 目录

#### 边界（递归保护）
- Teammate 不能再 spawn teammate（`AgentTool.tsx:423-427`）
- In-process teammate 不能 spawn background agent（`AgentTool.tsx:431-435`）

---

## 三、跨模式差异表

| 维度 | A 主会话 | B Subagent | C Teammate |
|---|---|---|---|
| 进程隔离 | 无 | 无（detached promise） | pane=独立进程；in-process=ALS |
| AbortController | 共享主 loop | 独立（不链父，async） | pane=SIGTERM；in-process=独立 |
| LLM loop | 主 query loop | 独立 child loop | 每 teammate 独立 loop |
| System prompt | `buildEffectiveSystemPrompt` | `getAgentSystemPrompt` | 继承 parent 或 def |
| Tools 过滤 | resolve(mainDef, …, false, true) | resolve(agentDef, workerTools, isAsync) | 各自独立 |
| 消息传递 | 直接 user turn | pendingMessages + drain | mailbox JSON 文件 |
| 命名注册 | `settings.agent` | `agentNameRegistry` Map（内存） | TeamFile.members（磁盘） |
| 退出通知 | 无（等用户） | `<task-notification>` XML | `idle_notification` JSON |
| 持久化 | session transcript | sidechain JSONL + .meta.json | TeamFile + inbox JSON |
| 权限提示 | 正常 | 默认 auto-deny；bubble 例外 | in-process=显示；pane=mailbox 协议 |

---

## 四、专题对标（按模式拆开）

### 4.1 Agent Loader 全集（仅 A/B 共用，C 不走 loader）

`getAgentDefinitionsWithOverrides(cwd)` 统一加载，A 选其一作主、B 按 subagent_type 选其一作子。

完整来源链：
```
flag agents (--agents JSON inline)
  > policySettings (~/.claude/policies/agents/*.md or managed dir)
  > projectSettings (<root>/.claude/agents/*.md, 多层遍历)
  > userSettings (~/.claude/agents/*.md, CLAUDE_CONFIG_DIR 可改)
  > pluginAgents (loadPluginAgents)
  > builtIn (硬编码代码)
```

`/agents` 命令：默认在 user 级创建/编辑（`~/.claude/agents/<name>.md`）。

**禁用 flag 作用范围**：
- `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS` 仅 non-interactive 时生效
- `CLAUDE_CODE_SIMPLE=1` interactive 也生效，跳过 plugin/custom

**lotus 现状对比**：仅 builtIn，无任何文件加载/flag/CLI 入口。

---

### 4.2 send_message(agent_name) 双路由

| 路由 | 触发条件 | 行为 | lotus 当前 |
|---|---|---|---|
| **主会话切 agent** | 不是 send_message，是 `--agent` / `/agents` | 切换 mainThreadAgentDefinition，下轮 turn 生效 | ChatRequest.agent_name 已穿但未消费 |
| **send_message → named async** | `agentNameRegistry` 命中 | running→queue；stopped→resumeAgentBackground；evicted→从磁盘 resume | 完全无 |
| **send_message → team mailbox** | 上面未命中 | writeToMailbox 写文件 + teammate 拉取 | 完全无 |

**关键澄清**：`agent_name` 在 lotus 当前的 ChatRequest 上**指主会话切 agent（模式 A）**，与 claude-code-best 的 SendMessage(to=name) 不是一回事。后者要求 named async 已经存在，前者是声明本轮主 agent profile。

设计 lotus 时这两条路必须分开：
- **模式 A 入口**：`ChatRequest.agent_name` → 选主 agent definition（已有参数，待消费）
- **模式 B/C 入口**：未来的 `send_message_to_agent(name, message)` tool / SDK → 双路由 dispatch

---

### 4.3 Resume / Continue 链路（仅模式 B）

模式 A 的 resume = REPL `--resume`，从 transcript metadata 读回 `agent` 字段，重建 mainThreadAgentDefinition。
模式 C 的 resume = teammate 重新启动 = 重新发命令 + 读 inbox。
模式 B 的 resume = 关键，最复杂：

```
入口  : resumeAgentBackground(agentId, prompt)
读盘  : getAgentTranscript(agentId)  +  readAgentMetadata(agentId)
       ↓
过滤  : filterWhitespaceOnlyAssistant
       → filterOrphanedThinkingOnly
       → filterUnresolvedToolUses
       ↓
重建  : reconstructForSubagentResume   ← contentReplacementState 还原
prompt cache 命中
       ↓
找定义: meta.agentType → activeAgents.find()
       fork 特例: 重建父 renderedSystemPrompt
       ↓
注册  : registerAsyncAgent (保留 agentNameRegistry 旧条目)
       ↓
启动  : runAsyncAgentLifecycle({ promptMessages: [...resumed, user(prompt)] })
```

**lotus 现状对比**：
- `AgentRuntime::resume_child_run` 仅读 invocation 元数据，**不重建 LLM loop**
- 无 sidechain transcript 重建
- 无 contentReplacementState 等价机制
- 无 pendingMessages 队列

---

### 4.4 TaskOutput / `<task-notification>`（仅模式 B）

| 能力 | claude-code-best | lotus 现状 |
|---|---|---|
| TaskOutput tool | `getTaskOutputDelta(taskId, offset)` 读 symlink 文件 | 无 |
| Output 文件 | `<projectTempDir>/<sessionId>/tasks/<agentId>.output` → transcript JSONL symlink | 无 |
| sessionId 稳定性 | 进程首次调用时 capture，跨 `/clear` 不变（`diskOutput.ts:49-55`） | N/A |
| `<task-notification>` XML | `enqueuePendingNotification` → `query.ts:1578-1636` drain → 注入父 user turn | 无（仅 RuntimeEvent::AgentIdle 给前端） |
| Retain/Evict | `retain=false` 完成后 `PANEL_GRACE_MS` 设 evict；UI 持有阻 evict；`evictTaskOutput` 不删磁盘 | 无 |
| 完成后 progress | symlink 到 transcript，TaskOutput 持续可读 | 无 |

---

### 4.5 权限 / AskRequired 差异（按模式）

| 模式 | shouldAvoidPrompts | AskRequired 处理 | lotus 现状 |
|---|---|---|---|
| A 主会话 | false | 终端弹对话框，同步阻塞 | ✅ AskRequired 冒泡到前端 |
| B sync subagent | false | 父终端显示，同步阻塞 | ✅ 沿用主链路 |
| B async subagent (默认) | **true** | **auto-deny 所有需要确认的工具** | 无（lotus 无 async） |
| B async subagent (`permissionMode='bubble'`) | false | `awaitAutomatedChecksBeforeDialog=true`，等 classifier/hooks 后弹 | 无 |
| C in-process teammate | true（等同 bubble） | `canShowPermissionPrompts=true` | 无 |
| C pane teammate | 独立进程 | mailbox `permission_request`/`permission_response` JSON 协议 | 无 |

`AgentTool` 的 `permissionMode` 入参可强制 bubble。

---

### 4.6 存储路径矩阵（按模式分开）

| 内容 | claude-code-best 路径 | lotus 当前 |
|---|---|---|
| **A 主会话 transcript** | `~/.claude/projects/<hash>/<sessionId>.jsonl` | `~/.renlijia/conversations/<id>/messages.N.jsonl` |
| **A agent setting** | transcript metadata 字段 `agent` | ChatRequest.agent_name（不持久化） |
| **B sync subagent transcript** | `…/<sessionId>/subagents/agent-<agentId>.jsonl` | `~/.renlijia/subagent_transcripts/<run-id>.json` |
| **B async subagent transcript** | 同上（`getAgentTranscriptPath()` `sessionStorage.ts:247-258`） | 无（无 async） |
| **B agent metadata sidecar** | `…/subagents/agent-<agentId>.meta.json` | 仅 `agent_invocations.json` 单文件 |
| **B output file (symlink)** | `<projectTempDir>/<sessionId>/tasks/<agentId>.output` | 无 |
| **B workflow subagent transcript** | `…/subagents/workflows/<runId>/agent-<agentId>.jsonl` | 无 |
| **C TeamFile** | `~/.claude/teams/<teamName>/config.json` | 无 |
| **C team mailbox** | `~/.claude/teams/<teamName>/inboxes/<agentName>.json` | 无 |
| **B/C remote agent metadata** | `…/<sessionId>/remote-agents/remote-agent-<taskId>.meta.json` | 无 |
| **B/C worktree** | `.claude/worktrees/agent-<agentId[:8]>/` | `worktree.rs` 文件存在但未对接 |

---

## 五、差距矩阵（按模式分组）

### 5.A 模式 A — 主会话 Agent Profile

| 差距 | 严重度 | 说明 |
|---|---|---|
| ChatRequest.agent_name 未消费 | **高** | 参数已穿，但 chat_turn_driver 没读，system prompt/tools 未切换 |
| 无 main-agent loader | **高** | 仅 builtin 硬编码 |
| 无 `/agents` 等价命令 | 中 | 用户只能改代码 |
| 无 `--agent` CLI flag | 中 | Tauri 启动无 CLI 概念 |
| 无 `initialPrompt` 注入 | 中 | 无法在主 agent 切换时自动塞首 prompt |
| 无持久化到 conversation 元数据 | 中 | 切了下次打开还原 |
| 无 main vs sub 复用同 definition 的能力 | 中 | 当前 `AgentDefinition` 是平的 |

### 5.B 模式 B — 普通 Subagent

| 差距 | 严重度 | 说明 |
|---|---|---|
| 无通用 `Agent`/`Task` tool | **高** | LLM 无法主动派 subagent |
| Tool schema 字段缺失 | **高** | 缺 subagent_type/model/run_in_background/isolation/cwd/mode/team_name |
| 无 fork 模式 | 中 | 省略 subagent_type 时继承父对话 |
| 模型运行时指定缺失 | **高** | 只能 inherit，AgentModel::Fixed 无消费者 |
| 无 named async agent 注册 | **高** | 无 agentNameRegistry，没法 send_message |
| 无 sync→async 动态迁移 | 中 | 前台变后台 |
| 无 TaskOutput tool / output symlink | **高** | 父无法增量观察后台 |
| 无 `<task-notification>` 注入 | **高** | 父上下文不知道后台完成 |
| 无 resumeAgentBackground | **高** | 仅元数据 resume，非 LLM loop 唤醒 |
| 无 pendingMessages 队列 | **高** | resume 唤醒缺数据通道 |
| 无 sidechain transcript .meta sidecar | 中 | resume 无 agentType 锚点 |
| 无 retain/evict | 中 | 后台 task GC |
| 无 worktree 隔离接通 | 中 | worktree.rs 占位 |
| 无 permissionMode='bubble' 三档 | 中 | async 默认 auto-deny 缺 |
| 工具白名单单层 | 中 | 缺 ALL_AGENT_DISALLOWED + ASYNC_AGENT_ALLOWED |
| 递归 spawn 控制 | 中 | 仅 browse_data 单点硬编码 |
| 同回合并行 dispatch | 中 | 调用方负责，dispatcher 无并发 |

### 5.C 模式 C — Agent Teams

| 差距 | 严重度 | 说明 |
|---|---|---|
| 几乎完全缺失 | — | 仅 `TeamContext` 占位结构 |
| 无 TeamFile 持久化 | 高 | 无 config.json |
| 无 mailbox 协议 | 高 | 无 inboxes 目录、无 8 类协议消息 |
| 无 backend 抽象 | 高 | pane / in-process / remote |
| 无 idle/shutdown 流程 | 高 | teammate 终止生命周期 |
| 无 SendMessage 双路由 | 高 | named async + team mailbox |

---

## 六、设计原则（避免设计成混合模型）

写后续实施方案时遵守：

1. **三模式分离**：模式 A 改 `ChatTurnDriver` 不动 `runtime/agent/`；模式 B 改 `runtime/agent/` + 新增 spawn_subagent tool；模式 C 改 `team.rs` + 新增 TeamCreate/SendMessage tool。三者各自有独立入口、独立存储、独立生命周期。
2. **共享只在 AgentDefinition 层**：A/B 共用 `AgentDefinition` 与 loader（C 用 TeamFile 不复用 AgentDefinition）。
3. **send_message 不等于切主 agent**：lotus 现有 ChatRequest.agent_name 是模式 A 入口，未来的 `send_message(to=name)` 是模式 B/C 入口，两者不复用代码路径。
4. **Resume 三模式各异**：A=transcript metadata + REPL；B=sidechain + pendingMessages；C=teammate respawn + inbox。不试图抽象成同一个 `resume_*`。
5. **存储路径按模式独立目录**：`~/.renlijia/conversations/`（A）、`~/.renlijia/subagent_transcripts/`+sidecar+output（B）、`~/.renlijia/teams/<name>/`（C）。

---

## 七、关键文件索引（claude-code-best 侧）

| 主题 | 文件 |
|---|---|
| AgentDefinition + loader | `src/tools/AgentTool/loadAgentsDir.ts` |
| 内置 agent 定义 | `src/tools/AgentTool/builtInAgents.ts` |
| AgentTool.call() | `src/tools/AgentTool/AgentTool.tsx` |
| runAgent 执行核心 | `src/tools/AgentTool/runAgent.ts` |
| LocalAgentTask 状态机 | `src/tasks/LocalAgentTask/LocalAgentTask.tsx` |
| Resume 链路 | `src/tools/AgentTool/resumeAgent.ts` |
| SendMessage 双路由 | `src/tools/SendMessageTool/SendMessageTool.ts:800-873` |
| Mailbox 协议 | `src/utils/teammateMailbox.ts` |
| TeamFile + helpers | `src/utils/swarm/teamHelpers.ts` |
| Pane backend | `src/utils/swarm/backends/PaneBackendExecutor.ts` |
| Output disk symlink/evict | `src/utils/task/diskOutput.ts` |
| sessionStorage（transcript 路径） | `src/utils/sessionStorage.ts` |
| pendingMessages drain | `src/utils/attachments.ts:1086-1102` |
| Main thread agent (CLI/REPL) | `src/main.tsx:3088-3200` |
| systemPrompt 组装 | `src/utils/systemPrompt.ts:42` |
| `/agents` UI | `src/components/AgentsMenu.tsx` |

---

**下一步**：基于本对标 v2，撰写《lotus-app Agent 能力综合实施大计划》（一份大计划，按模式 A→B→C 分阶段实现，每阶段都能独立形成可工作软件），不再 P0/P1/P2 平铺。文件名：`2026-04-30-agent-capability-master-plan.md`（待写）。

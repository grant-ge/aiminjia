---
title: Lotus Team Runtime (LTR) 架构方案
status: draft-v4
date: 2026-05-11
scope: Phase 0/1/2 — 能力面 1:1 对齐 claude-code-best Agent Teams,实现层用 Rust 原生优化
supersedes: docs/2026-05-09-agent-teams-architecture-research.md(已删除,决策依据吸收进本文档 §A 附录)
v3-to-v4-changelog: 经过 G1-G22 决策讨论,主要变更:删除 WorkerComplete / ArtifactRef / artifacts 目录约束 / worktree 隔离;广播 `to:'*'` + plan_approval_* 全部 MVP 做(对齐 claude-code-best);TEAMMATE_ADDENDUM 中文化;Lead 不叠 LEAD_ADDENDUM;新增 Lead idle 触发机制 / Teammate 启动扫 unclaimed / 收件人不存在错误 / blocks 环检测;`is_async` flag 仅 Teammate / async subagent 为 true;Employee 加载机制由独立工作流负责,LTR 只声明只读查询接口
for-agentic-workers: 本文档只描述**架构能力**,不包含实施细节(代码草稿、Step-by-Step、具体文件路径)。实施前需要另起"实施细节"工单。
---

# Lotus Team Runtime (LTR) 架构方案

> **这份文档回答三件事**:
> 1. **做完之后用户能干什么**(§1 Executive Summary)
> 2. **要对齐 claude-code-best Agent Teams 的哪些能力**(§3 能力面清单)
> 3. **架构形状是什么样**(§4 实体模型、§5 通信、§6 生命周期、§7 取消传播)
>
> **这份文档不回答**:代码怎么写、文件放哪、测试怎么跑、每天干啥。
> 这些等架构评审通过后另起文档。

---

## 1. Executive Summary — 做完之后用户能做什么

### 1.1 一句话能力

> **任何数字员工面对复杂任务时,可自动派出 2-4 个常驻的助手员工一起干活;助手之间能直接交流协作;Lead 优雅关闭或强制关闭助手;Lead 最终综合产出答案。**

### 1.2 使用前 vs 使用后

#### 使用前(当前)

> 用户:"小研,帮我做一份 X 公司竞品调研,覆盖产品、技术、商业、团队四个维度。"
>
> 小研:(单线程串行)
> - 产品 5min → 技术 5min → 商业 5min → 团队 5min → 综合 1min
> - **总耗时 21 分钟**,用户只能干等
> - 上下文越攒越满,后面分析质量下降

#### 使用后(本方案完成后)

> 用户:"小研,帮我做一份 X 公司竞品调研,覆盖产品、���术、商业、团队四个维度。"
>
> 小研:**任务复杂,开 Team 派 4 个助手**
> - 建 Team → 派小研·产品 / 小研·技术 / 小研·商业 / 小研·团队(4 个 Teammate,都是数字员工,常驻 idle)
> - 4 个 Teammate 并行调研,各自独立 LLM 上下文,互不污染
> - 小研·商业 → SendMessage 小研·技术:"他们用了 Stripe,确认下技术架构?"——Teammate 直接对话不绕 Lead
> - 5 分钟后第一个产出 → task-notification 自动注入小研下一 turn
> - 所有产出回来 → 小研综合 → 交付答案 → 小研发 `shutdown_request` 给 4 个 Teammate → 各自 LLM 决定批准关闭
> - **总耗时 6-7 分钟**,小研上下文只有"派工 + 综合",清爽

### 1.3 关键能力清单(实施完成后可用的 9 项)

| # | 能力 | 触发方式 |
|---|---|---|
| 1 | **Lead 派多 Teammate 并行执行** | LLM 在一个 turn 内调多次 `Agent(name=..., team_name=..., employee_id=..., prompt=...)`,各 Teammate 独立 transcript |
| 2 | **Teammate 常驻 idle**(不跑完就死) | spawn 后进 idle loop,持续可接新任务、新消息,直到显式关闭 |
| 3 | **Teammate 间点对点通信 + 广播** | `SendMessage(to=name)` 点对点;`SendMessage(to="*")` Team 内广播(排除发送者) |
| 4 | **共享 Task list 协作** | Lead 写 task 到共享列表,任意 Teammate 看到 unclaimed 可自主 claim → 完成回写 |
| 5 | **结构化关闭协议** | Lead `SendMessage(shutdown_request)` → Teammate LLM 决定批准 → 优雅退出 |
| 6 | **强制关闭兜底** | Lead `TaskStop(teammate_id)` 直接 abort,跳过 LLM 决策 |
| 7 | **取消传播无泄漏** | 用户中断主会话,所有 Team 成员 3s 内 Cancelled;MCP 子进程跟着关 |
| 8 | **异步 Employee 不被误杀** | Employee cron 触发的长任务用独立 token,主会话中断不影响 |
| 9 | **Lead 消息驱动续 turn** | Teammate 给 Lead 发消息后,Lead 当前 turn 结束自动起下一 turn 处理(不依赖用户输入) |

### 1.4 不变的事

| 维度 | 是否变化 |
|---|---|
| `EmployeeRecord` 核心数据模型 | **不动**(只在 tool_whitelist 层面要求含 3 个强制协作工具) |
| 现有派活 API(`employee_trigger`) | **不动** |
| 现有 conversation 文件结构 | **不动**(目录下只加 `team.json` / `tasks/` / `inboxes/` / `teammates/` / `subagents/`) |
| 现有 Tauri 事件协议 | **不动**(扩 3 个新 RuntimeEventKind 后向兼容) |
| 用户主会话单 agent 行为 | **不动**(Team 是 Lead 自主决策的协作模式,非强制) |
| Lead 自身的 system prompt 结构 | **不动**(不叠 LEAD_ADDENDUM,所有协作引导由工具 description 承载) |
| Teammate 文件写入位置 | **不限制**(无强制 artifacts/ 目录约束,与 Lead 共享 cwd / workspace,受 permission 管控) |

### 1.5 完成标志(客观可验证)

3 条端到端验收通过即视为完成:

1. **并行验收**:用户对小研说"用 Team 模式调研 X",能看到 `~/.renlijia/users/{scope_key}/conversations/{id}/team.json` 有 1 Lead + 2-4 Teammate,对应数量的 transcript 文件并行写入,墙钟时间 < 串行 1/2。
2. **协作验收**:(a) Teammate_A → Teammate_B 的 SendMessage 收发成功;(b) 广播 `to:"*"` 团队内所有人(发送者除外)收到;(c) 共享 Task list claim 机制:两个 Teammate 抢同一 task,一个拿到一个收到"already claimed";(d) Lead 发 shutdown_request,Teammate 回 shutdown_response(approve) → Lead 调 TaskStop 退出。
3. **取消验收**:Teammate 跑到一半用户按"中断",所有同步 Teammate 在 3s 内进入 cancelled 态,MCP 子进程关闭;Employee cron 触发的异步 Teammate 不受影响。

---

## 2. Scope 与非目标

### 2.1 本方案覆盖(Phase 0 + 1 + 2)

| Phase | 主题 | 对应能力(§1.3) |
|---|---|---|
| **P0** | MCP 迁新 ToolDispatcher + cancel 挂钩 | 支撑能力 7 |
| **P0** | `compact_summary` 迁 RuntimeLlmExecutor trait | Teammate 长 transcript 时的 compaction |
| **P1** | Team 实体 + 持久化(`team.json` / `tasks/` / `inboxes/` / `teammates/`) | 能力 1 |
| **P1** | AgentNameRegistry(按 SessionId 分区) | 能力 3 的按名寻址基础 |
| **P1** | **常驻 idle Teammate 循环**(wakeup = pendingMessages / unclaimed task / cancellation) | 能力 2 |
| **P1** | **共享 Task list + Rust Mutex claim 协议 + blocks 环检测** | 能力 4 |
| **P1** | Team 工具(TeamCreate / TeamDelete / TaskCreate / TaskUpdate / TaskList / TaskGet / TaskStop) | 能力 1 / 4 / 6 |
| **P1** | `Agent(name, team_name, employee_id, prompt)` 扩展 —— 派 Teammate 走 EmployeeRecord 模板 | 能力 1 |
| **P1** | **Lead idle 触发机制**(turn 结束自检 pending + 消息入队 kick) | 能力 9 |
| **P2** | **SendMessage 结构化消息**(text / shutdown_request / shutdown_response / plan_approval_response,全部 MVP 做) | 能力 3 / 5 |
| **P2** | **SendMessage 广播 `to:"*"`**(Team 内排除发送者) | 能力 3 |
| **P2** | **LLM 决策 shutdown 协议** | 能力 5 |
| **P2** | `TaskStop` 强制关闭路径 | 能力 6 |
| **P2** | pending_messages 工具层接通 + turn 起手 drain inject | 能力 3 / 9 |
| **P2** | `is_async` flag + permission auto-deny(Teammate / async subagent) | 能力 8 |

### 2.2 本方案**不做**(明确划线)

1. 不做 Phase 3(前端 Team Room 视图)—— 后端跑通后再做
2. 不做 Phase 4(Trace 视图、Teammate template 用户自定义)
3. 不做 Phase 5(A2A adapter / Remote teammate)
4. 不做共享 transcript GroupChat 模式(保留每 Member 独立 transcript)
5. 不做 Teammate 递归 spawn(Teammate 不能 spawn 子 Teammate)
6. 不做 fork subagent prompt cache
7. 不做崩溃后自动恢复 Teammate(对齐 claude-code-best,磁盘文件保留但不恢复 in-memory runner)
8. 不做 idle timeout 自动关闭(对齐 claude-code-best,只支持 shutdown_request + TaskStop 两种关闭)
9. 不做 Coordinator Mode 概念(用 EmployeeRecord.tool_whitelist 精确控权替代)
10. 不引入"Worker"新实体(Worker = 被 Lead 派的 Employee 的一种角色)
11. 不动 `EmployeeRecord` 核心数据模型
12. 不引入跨进程通信(UDS / bridge;claude-code-best 支持但 lotus 单进程用不上)
13. 不引入新外部依赖(crate / 外部服务)
14. **不引入 `WorkerComplete` 工具**(Teammate 收尾用 SendMessage + TaskUpdate(status=completed),结构化数据走 task metadata)
15. **不引入 ArtifactRef / artifacts/ 目录约束**(对齐 claude-code-best,文件系统就是 SoT,Teammate 与 Lead 共享 cwd)
16. **不引入 `isolation: worktree` 隔离**(lotus 用户是办公人员,无 git worktree 概念)
17. **不引入 LEAD_ADDENDUM**(Lead 的协作引导全部由工具 description 承载)
18. **不实现 EmployeeRecord 的加载/磁盘扫描/热重载机制**(由独立工作流负责;LTR 只声明只读查询接口)

### 2.3 红线(违反即回滚)

- ✋ 不在 `runtime/` 模块下 `use tauri::*`
- ✋ 不让 LTR 引入 `LlmGateway`/`AuthManager` 进 `CapabilityContext`
- ✋ 不创建独立 conversation 给 Team(SessionId 1:1 必须保持)
- ✋ 不让 `AgentNameRegistry` 跨 SessionId 查询
- ✋ 不引入 controller loop / reconcile interval
- ✋ 不在 LTR 内实现 EmployeeRecord 的加载逻辑(只调用 `EmployeeStore` 提供的只读接口)

---

## 3. 对齐 claude-code-best Agent Teams 的能力面(权威清单)

本方案**能力面 1:1 对齐**(B 方案),**实现层用 Rust 原生优化**。下表列出所有能力,并标明 lotus 的对应。

### 3.A Team 管理(5 项,100% 对齐)

| # | 能力 | claude-code-best 来源 | lotus 对应 |
|---|---|---|---|
| A1 | TeamCreate(name, description?, agent_type?) | `TeamCreateTool.ts:37-48` | 同,单例约束一致;**`agent_type` 入参保留以对齐 claude-code-best 入参 schema,但 LTR 内部不读**(Lead 实际身份永远从派活的 Employee 取,该字段仅记入 team.json 作审计) |
| A2 | TeamDelete() 空入参 | `TeamDeleteTool.ts:21` | 同 |
| A3 | session 结束自动清理 | `cleanupSessionTeams()` | 同,挂到 `SessionRuntime::cancel_session`;4 个明确触发点见 §7.3 |
| A4 | TeamFile 持久化 | `~/.claude/teams/{name}/config.json` | `~/.renlijia/users/{scope_key}/conversations/{id}/team.json` |
| A5 | 前端 TeamStatus / TeamList UI | React 组件 | Phase 3 再做,**后端不暴露为 LLM 工具**(Lead 想看队友直接 Read `team.json`,对齐 claude-code-best 设计) |

### 3.B Teammate 生命周期(8 项,100% 对齐能力,实现层用 Rust)

| # | 能力 | claude-code-best 来源 | lotus 对应 |
|---|---|---|---|
| B1 | spawn teammate | `AgentTool.tsx:439`(when team_name && name)→ `spawnTeammate()` 共享函数(`shared/spawnMultiAgent.ts:1088`)→ `inProcessRunner.ts` | 扩展 `Agent` 工具入参加 `name` + `team_name` + `employee_id` → 走 `spawn_teammate` 共享函数 |
| B2 | **常驻 idle loop** | 每次 `runAgent()` 完成后进 `waitForNextPromptOrShutdown()` 500ms 文件轮询(`inProcessRunner.ts:686-689`) | **Rust 原生 tokio `select!` 零延迟 wakeup**(替代 500ms 文件轮询) |
| B3 | 三种 wakeup:in-mem pendingMessages / 磁盘 mailbox / unclaimed task | `inProcessRunner.ts:689-868` | tokio mpsc channel + Task list change notifier(`tokio::sync::watch`) |
| B4 | **LLM 决策 shutdown** | shutdown_request → LLM → approve → abort | 同,靠 `SendMessage(shutdown_request)` 协议 |
| B5 | AbortController 强制中止 | abort.signal | `CancellationToken` 已有 |
| B6 | 重启不恢复(设计选择) | 磁盘文件保留,内存状态丢 | 同,Team 是临时编排;磁盘 `team.json` / `tasks/` / `inboxes/` / `teammates/*.jsonl` 保留作审计(MVP 不做历史 Team 视图) |
| B7 | system prompt 三层叠加(仅 Teammate) | base + `TEAMMATE_ADDENDUM` + agent 定义(`inProcessRunner.ts:925-970`) | **Teammate**:base + `TEAMMATE_ADDENDUM` + EmployeeRecord(由 `employee_id` 引用);**Lead**:不叠 addendum,与普通 chat session 同构,协作引导由工具 description 承载(见 §3.B.1) |
| B8 | **强制注入协作工具集** | 硬编码注入(`inProcessRunner.ts:983-994`) | lotus:**分级强制**——`SendMessage` / `TaskList` / `TaskGet` 强制必含(配置校验);其余 6 个协作工具(`TaskCreate` / `TaskUpdate` / `TaskStop` / `TeamCreate` / `TeamDelete` / `Agent`)默认勾选可去(Employee 配置时可关闭) |

#### 3.B.1 Lead 与 Teammate 的 prompt 差异(决策要点)

| 对象 | system prompt 结构 | 协作引导来源 |
|---|---|---|
| **Lead**(被派活的 Employee 实例) | `base + EmployeeRecord.system_prompt`,**与普通 chat session 完全同构** | `TeamCreate` / `Agent` / `SendMessage` / `Task*` 等工具自己的 description(每个工具 description 包含完整 workflow 指引,对齐 claude-code-best `TeamCreateTool/prompt.ts` 113 行长 prompt) |
| **Teammate**(Lead 派出的常驻助手) | `base + TEAMMATE_ADDENDUM + EmployeeRecord.system_prompt` | addendum 说明协作通信规则;第一 turn 前注入 `team_context` attachment 指向 `team.json` 让 Teammate 自主读队友名单(见 §5.7) |

**关键原则**(对齐 claude-code-best):
- 不判断"当前是不是 Lead"、不做 prompt 动态切换 —— 所有 Employee 看到的工具 prompt 一致
- Lead 是否进入"Team 模式"由 LLM **自主决定**(建 Team 就进入,不建就是普通 chat);无专用"Lead 模式"标志

### 3.C 通信(9 项能力,MVP 做 7 项 C1-C4 + C7-C9,跨进程 C5/C6 不做)

| # | 能力 | claude-code-best 来源 | lotus 对应 |
|---|---|---|---|
| C1 | SendMessage(to, message, summary?) | `SendMessageTool.ts` | 同 |
| C2 | **message 是 discriminated union**:text / shutdown_request / shutdown_response / plan_approval_response | 同 | **MVP 全做**(对齐 claude-code-best) |
| C3 | to: 任意 teammate name | `agentNameRegistry` | 同,用 `AgentNameRegistry` 按 SessionId 分区;**收件人不存在时返回明确错误并列出当前成员名单**(优于 claude-code-best 不做校验的设计) |
| C4 | to: `'*'` 广播 | 支持 | **MVP 做**(对齐 claude-code-best);展开成 N 条单发,排除发送者,各自进对方 pending channel + 磁盘 inbox |
| C5 | to: `'uds:...'` 跨进程 | 支持 | **不做**(lotus 单进程) |
| C6 | to: `'bridge:...'` 跨机器 | 支持 | **不做**(lotus 单进程) |
| C7 | mailbox 存储 | 磁盘 JSON 文件是 SoT(`teammateMailbox.ts:60` `~/.claude/teams/{name}/inboxes/{agent}.json`)+ proper-lockfile | **tokio mpsc channel 是 SoT**(零延迟);`conversations/{id}/inboxes/{name}.json` 仅作开发期审计/排错备份(append-only,不用于状态恢复;LLM / 前端均不直接读) |
| C8 | inbox 是 Leader 侧 UI 内存镜像 | `AppState.inbox`(`AppStateStore.ts:351-361`) | 同,Phase 3 前端对接 |
| C9 | 标记已读(markMessageAsReadByIndex) | 磁盘读后改 read 字段 | channel 自然消费,无需显式标记;磁盘备份不动(append-only 审计) |

### 3.D 共享 Task list 协议(10 项,9 项对齐 + 1 项简化)

| # | 能力 | claude-code-best 来源 | lotus 对应 |
|---|---|---|---|
| D1 | TaskCreate(subject, description, activeForm?, metadata?) | `tasks.ts` | 同 |
| D2 | TaskUpdate(..., owner?, addBlocks?, addBlockedBy?, status?) | 同,status=deleted 软删 | 同 |
| D3 | TaskList(支持过滤) | 同 | 同 |
| D4 | TaskGet(taskId) | 同 | 同 |
| D5 | TaskStop(taskId) | `TaskStopTool.ts` | 同 |
| D6 | **claim 机制** | `claimTask` proper-lockfile retries:30(`tasks.ts:541-612`),`.lock` 文件(`tasks.ts:504-506`) | **tokio Mutex 同进程锁,纳秒级**;**不创建 `.lock` 文件** |
| D7 | 高水位线防 ID 复用 | `.highwatermark` 文件(`tasks.ts:111`) | **不创建 `.highwatermark`,task_id = uuid v4 字符串** |
| D8 | blocks/blockedBy 依赖图 | 完成自动解锁 | 同;**`TaskUpdate(addBlocks/addBlockedBy)` 写入时做 DFS 环检测,发现环拒绝写入并返回 `Cycle detected: A → B → A`** |
| D9 | owner 字段 | 同 | 同 |
| D10 | **每 task 一个文件** | `~/.claude/tasks/{taskListId}/{id}.json`(顶级独立目录) | `~/.renlijia/users/{scope_key}/conversations/{conv_id}/tasks/{task_id}.json`(收进 conversation 目录,详见 §8.4) |

### 3.E Coordinator Mode(**删除,不对齐**)

claude-code-best 用环境变量 `CLAUDE_CODE_COORDINATOR_MODE=1` 在 session 级收窄工具白名单,目的是防止 Lead 自己插手动手活。

**lotus 不引入此概念**,用现有 `EmployeeRecord.tool_whitelist` 精确控制每个 Employee 能用什么工具,产品上更清晰:

- 想让某员工"只指挥不动手" → 配置时不勾 Bash/Edit/Write
- 想让某员工"能指挥能动手" → 全勾
- Lead 是哪个 Employee 就用谁的工具集,**不需要运行时 mode 切换**

### 3.F 子代理 AgentTool(扩展,完整对齐 claude-code-best 设计)

**claude-code-best 真实设计**(代码事实):
- `AgentTool` 入参 schema(`AgentTool.tsx:163-268`)同时支持普通 subagent 和 Teammate 两种用途
- 关键判断逻辑(`AgentTool.tsx:439`):`if (team_name && name)` → 走 `spawnTeammate()` 共享函数(`shared/spawnMultiAgent.ts:1088`),否则走普通 subagent 路径
- `runAgent()` 是共享底层(`runAgent.ts:248`),`AgentTool` 和 `inProcessRunner` 都调用它
- Teammate 的"常驻 idle"是 **runtime 行为**,不是工具入参——`inProcessRunner.ts:689` 在 `runAgent()` 返回后进 `waitForNextPromptOrShutdown()` 轮询保活
- 历史上有过独立的 `TeammateTool`,**已废弃合并**(`spawnMultiAgent.ts:3` 注释为证)

**lotus 对齐设计**:扩展现有 `SpawnSubagentRuntimeTool`(注册名 `Agent`),不新建独立 Teammate 工具。

入参变化(在现有基础上加三个可选字段):

| 字段 | 必填? | 含义 |
|---|---|---|
| `description` | ✓ | 任务描述(已有) |
| `prompt` | ✓ | 给子 agent 的 prompt(已有) |
| `model` | 可选 | sonnet/opus/haiku 覆盖(已有) |
| `run_in_background` | 可选 | 异步 subagent(已有,但派 Teammate 时此字段无效——Teammate 总是异步常驻) |
| **`employee_id`** | 可选 | 引用哪个 EmployeeRecord 模板物化(对齐 claude-code-best `subagent_type`) |
| **`name`** | 派 Teammate 必填 | Teammate 在 Team 内的名字,SendMessage 按它寻址 |
| **`team_name`** | 派 Teammate 必填 | 哪个 Team(必须等于当前 SessionId 的 team_id) |
| `mode` | 可选 | permission mode 覆盖 |

判断逻辑(对齐 claude-code-best `AgentTool.tsx:439`):

```
if (team_name && name) {
  → 派 Teammate(常驻 idle)
  → 走共享函数 spawn_teammate()
  → transcript 写到 conversations/{conv_id}/teammates/agent-{agent_id}.jsonl
  → meta.json 含 kind=teammate, team_id, agent_name, ...
  → 加入 Team.members,注册 AgentNameRegistry
  → 启动 idle loop,Lead 后续可 SendMessage(to=name) 续传
} else {
  → 普通 subagent(现有行为,跑完即返)
  → transcript 写到 conversations/{conv_id}/subagents/agent-{agent_id}.jsonl
  → meta.json 含 kind=subagent
  → 不加入 Team
  → run_in_background=true 时返回 agent_id 让父 agent 用 TaskOutput 轮询
}
```

**调用示例**:

```
# 派 Teammate(常驻 idle)
Agent(
  description="调研 X 公司商业模式",
  prompt="...",
  name="商业研究员",
  team_name="x-company-research",   # 必须 == 当前 conv_id
  employee_id="xiaoyan"             # 用小研模板物化
)
→ Teammate 启动,加入 Team,Lead 后续 SendMessage(to="商业研究员", ...)

# 派普通 subagent(跑完即返)
Agent(
  description="读一下 README",
  prompt="..."
)
→ 现有行为,不进 Team
```

#### 3.F.1 Lead 如何知道有哪些 `employee_id` 可派(对齐 claude-code-best `prompt.ts:194-212`)

**机制**:`Agent` 工具的 description 里**动态拼接**当前所有可派 Employee 清单,LLM 在 tools schema 里读到工具时就看到了。

每行格式(对齐 claude-code-best `formatAgentLine` `prompt.ts:43-46`):

```
- {employee_id}: {description} (Tools: {tools 摘要})
```

`Agent` 工具 description 渲染示意:

```
Launch a new agent to handle complex, multi-step tasks autonomously.

Available employee types and the tools they have access to:
- xiaoyan: 竞品/市场研究助理 (Tools: WebFetch, Read, SendMessage, ...)
- xiaoxiao: 销售跟进助理 (Tools: ...)
- ...

When using the Agent tool, specify an employee_id parameter to select which employee to use...
```

**数据来源约定**:`Agent` 工具构造 description 时,通过 `EmployeeStore` 暴露的查询接口拿到全部可派 Employee 列表。**本方案不规定 EmployeeStore 从何处加载、加载几个来源、何时加载** —— 这些由独立的 Employee 加载工作流负责。LTR 只依赖一组**只读查询接口**:

| 接口(语义,签名不强约束) | LTR 用途 |
|---|---|
| `list_employees() -> Vec<EmployeeRecord>` | 渲染 `Agent` 工具 description 的可派清单 |
| `get_employee(employee_id) -> Option<EmployeeRecord>` | 派 Teammate 时按 `employee_id` 物化,读 `tool_whitelist` / `system_prompt` / `default_skill_id` 等字段 |

**LTR 需要从 `EmployeeRecord` 读取的字段**(其他字段不关心):

- `employee_id`(主键,= `Agent.employee_id` 参数)
- `description` / `when_to_use`(渲染清单时给 LLM 看的"何时用")
- `tool_whitelist`(派 Teammate 时强制注入的工具集,**必须含 `SendMessage`**)
- `system_prompt` 或等价的 prompt body(派 Teammate 时叠加为系统提示)
- `default_skill_id` / `requires_attachment` 等业务字段(Teammate 物化时透传)

**加载时机的隐含要求**:LTR 不要求 EmployeeStore 提供 watch / hot reload 能力,但要求每次构造 `Agent` 工具 description 时调用 `list_employees()` 拿到当时的快照(可以是 memoized 结果,新增/修改 employee 后由加载方负责使缓存失效)。

### 3.G Plan 协议

| # | 能力 | 处置 |
|---|---|---|
| G1 | plan_approval_request 结构化消息 | **MVP 做**(对齐 claude-code-best,structured message union 全做) |
| G2 | plan_approval_response 回执 | **MVP 做** |
| G3 | `shouldAvoidPermissionPrompts`(async agent 自动 deny) | **MVP 做**;判定依据是 `is_async` flag(仅 Teammate / `run_in_background=true` subagent 为 true,Lead 永远 false 含 cron 触发的 Lead);见 §7.4

---

## 4. 架构实体模型

### 4.1 核心实体(精简版,对齐 claude-code-best TeamFile)

```
Team(SessionId 范围内单例)
  ├─ team_id        (内部 = SessionId)
  ├─ session_id
  ├─ lead_agent_id  (即派活那个 Employee 的实例)
  ├─ members[]      (含 Lead 自己)
  ├─ created_at
  └─ closed_at

Member(每个 Team 成员)
  ├─ agent_id       (本次运行实例 ID)
  ├─ name           (AgentNameRegistry 注册名,SendMessage 按它寻址)
  ├─ employee_id    (从哪个 Employee 模板物化的;Lead 的就是派活那个)
  ├─ role           (Lead | Teammate)
  ├─ last_active_at (turn 起/止时更新,供 UI 派生 phase)
  └─ cancellation_token

AgentTask(共享 Task list,存磁盘 + 内存镜像)
  ├─ task_id        (uuid v4 字符串)
  ├─ subject / description / activeForm
  ├─ owner          (谁 claim 了,None 表示 unclaimed)
  ├─ status         (pending | in_progress | completed | deleted)
  ├─ blocks / blocked_by   (写入时 DFS 环检测)
  └─ metadata       (Teammate 收尾结构化结果可放此处,例如 findings)

PendingMessage(SendMessage 队列的基本单位)
  ├─ from_agent_id
  ├─ from_name
  ├─ source         (Lead | Teammate | User | System)
  ├─ message        (StructuredMessage:text / shutdown_request / shutdown_response / plan_approval_request / plan_approval_response)
  ├─ summary        (UI 用,不进 LLM)
  └─ timestamp
```

**关键:不引入以下字段/枚举**(相比 v2 方案):

- ❌ `TeamMode`(删;只有 Coordinator 模式,不做 mode 枚举)
- ❌ `Lifecycle { Active, Stopped }`(删;Teammate 是否活着由 `cancellation_token.is_cancelled()` 判断)
- ❌ `Phase { Pending, Running, Idle, Completed, Failed, Cancelled }`(删;用 `CancellationToken` 状态 + `last_active_at` 只在前端展示时派生)
- ❌ `infrastructure_ready`(删;HiClaw 一次性翻转在桌面单进程用不上)
- ❌ `observed_generation`(删;K8s 术语)
- ❌ `desired_lifecycle`(删)
- ❌ `ArtifactRef`(删;对齐 claude-code-best 文件系统 SoT,Teammate 写文件无目录强制,Lead 用 Read/Glob 查产物)
- ❌ `WorkerComplete`(删;Teammate 收尾用 SendMessage + TaskUpdate(status=completed),结构化数据走 task metadata)

### 4.2 StructuredMessage(SendMessage 的 message 字段)

```
message: union {
  text:                      string                           // 普通文本,最常用
  shutdown_request:          { reason? }                      // Lead 请 Teammate 优雅关闭
  shutdown_response:         { approve, reason? }             // Teammate LLM 的决定
  plan_approval_request:     { plan, reason? }                // 请求计划批准
  plan_approval_response:    { approve, reason? }             // 计划批准回执
}
```

**所有 union 类型 MVP 全做**(对齐 claude-code-best `SendMessageTool.ts` schema)。

### 4.3 MessageSource(注入 LLM 时的来源标记)

```
enum MessageSource { Lead, Teammate, User, System }
```

Teammate 收到消息注入 LLM 为 user turn 时带前缀:`[来自 lead "name"]` / `[来自 teammate "name"]` / ...,让 LLM 知道发起人权重。

### 4.4 ID 拓扑(复用现有,不新增)

```
SessionId (= conversation_id)
  │
  ├─ TeamId (= SessionId.to_string(),不是新 ID 层)
  │   ├─ Lead Member                → Lead's RunId → ToolCallId[]
  │   └─ Teammate Members (≤ 4)     → Teammate's RunId → ToolCallId[]
  │
  ├─ AgentNameRegistry(按 SessionId 分区 HashMap<name, AgentId>)
  │
  └─ SharedTaskList(按 SessionId 分区,tokio Mutex 保护)
```

**强制不变量**:
- 一个 Session 最多一个 Team
- Team 成员数 ≤ 5(1 Lead + **硬限** 4 Teammate;超出抛 `MaxTeammateLimitReached`)
- 同一 Employee MVP 限制只能在 1 个 Team 中活跃(跨 Session 不同对话无限制)
- `AgentNameRegistry` 跨 Session 查询必须返回 None

---

## 5. 通信模型

### 5.1 五条消息路径

```
┌────────────────────────────────────────────────────────────────┐
│ 1. 用户 → Lead:主会话 chat(现有机制,不变)                      │
│ 2. Lead → Teammate:                                             │
│    (a) Agent(name, team_name, employee_id, prompt) — 派 Teammate │
│    (b) SendMessage(to=teammate_name, message=text) — 续传指令    │
│    (c) SendMessage(to=teammate_name, message=shutdown_request)   │
│ 3. Teammate → Lead:                                             │
│    (a) SendMessage(to=lead_name, message=text) — 回报/提问        │
│    (b) SendMessage(to=lead_name, message=shutdown_response)      │
│    (c) TaskUpdate(status=completed, metadata={...}) — 任务收尾  │
│ 4. Teammate → Teammate:                                          │
│    SendMessage(to=other_teammate, message=text) — 同侪协作       │
│ 5. SendMessage(to="*") 广播:                                    │
│    展开成 N 条单发到 Team 内所有 Member(排除发送者)             │
│ 6. 共享 Task list:                                              │
│    - Lead / Teammate:TaskCreate / TaskUpdate                     │
��    - Teammate:TaskList → claim unclaimed → TaskUpdate(done)      │
│    - TaskUpdate(owner=...) Lead push 与 Teammate self-claim 共用 │
│      工具,先到先得;后写者收到 AlreadyClaimed 错误               │
└────────────────────────────────────────────────────────────────┘
```

### 5.2 MVP 边界(允许 / 拒绝)

| 发起 → 目标 | MVP 允许? | 理由 |
|---|---|---|
| Lead → 同 Team Teammate | ✓ | 派工 / 续传 / shutdown |
| Teammate → 同 Team Lead | ✓ | 回报 / 提问 / shutdown_response |
| Teammate → 同 Team 另一 Teammate | ✓ | 对齐 swarm,不走 Lead 中转 |
| 任意 → 同 Team Broadcast `'*'` | ✓ | 对齐 claude-code-best,排除发送者 |
| 任意 → 跨 Session | ✗ | AgentNameRegistry 按 SessionId 分区 |
| 任意 → 跨进程 / 跨机器 | ✗ | 不做(lotus 单进程) |
| 收件人不存在 | 返回错误 + 列出当前成员名单 | 优于 claude-code-best 不校验设计 |

### 5.3 结构化 Shutdown 协议(对齐 claude-code-best)

```
[优雅关闭路径]
Lead LLM 决定关 Teammate_A
  → Lead: SendMessage(to="teammate_a", message={type:"shutdown_request", reason:"任务已完成"})
  → Teammate_A 下一 turn drain pending 收到 shutdown_request
  → Teammate_A LLM 评估:
      - 手头有未完成 task?→ 回 shutdown_response(approve=false, reason="还有 X 没做完")
      - 可以走 → 回 shutdown_response(approve=true)
  → Lead 下一 turn 看到 approve=true(由 §5.6 Lead idle 触发机制保证)
  → Lead 调 TaskStop(teammate_a_id) 正式触发 AbortController
  → Teammate_A 的 idle loop 被 abort,清理 + 退出

[强制关闭路径]
Lead 绕过 LLM 决策
  → Lead: TaskStop(teammate_a_id)
  → 直接 cancel Teammate_A 的 CancellationToken
  → Teammate_A 立即停(idle loop 下次 select! 命中 cancel 分支)

[不做自动兜底]
- Lead LLM 收到 approve=true 后漏调 TaskStop:不做 runtime 自动兜底,完全靠 LLM 协议自洽
- 兜底交给 session 结束 cleanup(§7.3 4 个触发点)
```

### 5.4 wakeup 三条件(Teammate idle loop)

对齐 claude-code-best `waitForNextPromptOrShutdown`,但实现用 tokio 原生:

```
Teammate spawn → 跑初始 prompt turn → 进 idle loop
  ↓
进 select! 前必做一次:扫 task_list.list_unclaimed()
  - 非空 → 处理(交给 LLM 决策 claim 哪个)起新 turn
  - 空   → 进入 select! 等
  ↓
loop {
  select! {
    msg = pending_messages.recv() => handle_message(msg)        // SendMessage 进来
    task_change = task_list_watch.changed() => check_unclaimed()  // task 列表变化
    _ = cancellation_token.cancelled() => break                  // 被 abort
  }
}
```

**Teammate 启动扫一次的原因**:tokio watch channel 只通知"变化",不重放历史;若 Lead 先 TaskCreate 后 spawn Teammate,Teammate 启动那一刻已有的 unclaimed task 不会触发 watch,必须主动扫一次。

**相比 claude-code-best 的优化**:
- 不用 500ms 文件轮询,tokio channel/watch 是**零延迟**的事件驱动
- 不用 proper-lockfile 文件锁,tokio Mutex 是**纳秒级**同进程锁

### 5.5 共享 Task list claim 协议

```
TaskUpdate(owner=X) 是 Lead push 与 Teammate self-claim **共用工具**:
  - Lead 调 TaskUpdate(task_id=X, owner="researcher") → 指派给 researcher
  - Teammate 调 TaskUpdate(task_id=X, owner=self_name) → 自己抢

Teammate 看到 unclaimed task:
  1. TaskList → 看到 task_id=X, owner=None
  2. 决定 claim
  3. 调 TaskUpdate(task_id=X, owner=self_name, status=in_progress)
         ↓
     runtime 层:
         tokio Mutex.lock() [per SharedTaskList]
         re-read task state(防止并发)
         if task.owner != None && task.owner != self_agent_id:
             return AlreadyClaimed
         task.owner = self
         task.status = in_progress
         persist to disk
         notify task_list_watch
         unlock
```

**并发安全性**:
- tokio Mutex 保证同进程内原子
- persist 用 tmp+rename 原子写(对齐现有 `runtime::employee::store::write_atomic`)
- unclaimed task 被多个 Teammate 抢时,**先到先得**,其余收到 `AlreadyClaimed` 错误
- Lead 想强制改派 → 先 `TaskUpdate(task_id=X, owner=None)` 释放,再 `TaskUpdate(owner=B)` 指派(两步显式)
- `TaskUpdate(addBlocks/addBlockedBy)` 写入时做 DFS 环检测,发现环拒绝写入,返回 `Cycle detected: A → B → A`

### 5.6 Lead idle 触发机制(对齐 claude-code-best Lead 自动续 turn)

Lead 是普通 chat session(不是常驻 idle 进程),turn 结束后默认停在 idle 等下一个外部 invoke。但 Teammate 给 Lead 发消息时,需要让 Lead 自动续 turn 处理。**采用 A+C 双保险机制**:

```
[A: turn 结束自检]
Lead 当前 turn 跑完 → driver 检查 pending_messages 非空
  → 立刻起新 turn,把 pending drain 作为 user message 注入
  → 新 turn 跑完再次检查,空就停,非空再起一轮

[C: 消息入队 kick]
任意 Teammate 调 SendMessage(to=lead, ...) 入队后
  → 检查 Lead 当前状态:
      - 正在跑 turn → 不动作(turn 结束时 A 兜底)
      - idle(turn 已结束)→ 触发新 turn 起手 drain
```

**两条路径互补**:
- 仅 A:Lead 已经 idle 之后才到的消息会漏接(turn 早跑完了,driver 没机会自检)
- 仅 C:Lead 正在 turn 中的入队消息会漏接(kick 失败因为已在跑)
- A + C:任何时刻消息进 pending,Lead 都被触发跑下一轮

**Teammate 也用同一个 idle-loop pattern**(§5.4),只是 Teammate 没有"用户/cron 外部 invoke"的 wakeup,而 Lead 有。

### 5.7 task-notification 与 user message 投递规则

Teammate 给 Lead 投递事件时按以下规则注入 Lead pending:

| 事件 | 注入形态 |
|---|---|
| Teammate `SendMessage(to=lead, message=text)` | user message,前缀 `[来自 teammate "{name}"] {text}`(对齐 §4.3 MessageSource) |
| Teammate `SendMessage(to=lead, message=shutdown_response/...)` 等结构化消息 | user message,内容标注消息类型 |
| Teammate `TaskUpdate(status=completed/failed/cancelled)` 状态变化 | `<task-notification>` XML 包装,含 `task_id` / `status` / `summary` / `metadata` |
| Teammate 进入 idle 等下一任务 | **不通知**(对齐 claude-code-best,idle 是常态) |
| Teammate spawn 后初始 prompt turn 完成 | **不通知**(等价于 idle) |

**作用域**:这些通知**只投 Lead pending**;Teammate 间通信仍只能靠 Teammate 主动 SendMessage。

### 5.8 第一 turn `team_context` attachment(Teammate 发现队友的入口)

对齐 claude-code-best `attachments.ts:3776 getTeamContextAttachment` + `messages.ts:3505-3537`,Teammate 第一 turn(LLM 还没生成过任何 assistant 消息)前注入一次 `<system-reminder>` user message(中文版,**结构 1:1 对齐 claude-code-best**):

```markdown
<system-reminder>
# 团队协作

你是团队 "{team_name}" 的成员。

**你的身份**:
- 名字: {agent_name}

**团队资源**:
- 团队配置: {team.json 完整路径,例 ~/.renlijia/users/{scope}/conversations/{conv_id}/team.json}
- 任务列表: {tasks/ 完整路径}

**团队负责人**:Lead 的名字是 "team-lead"。把进度和完成情况发给 Lead。

读取团队配置文件了解队友名单。定期检查任务列表。需要分工时创建新任务,完成后标记任务为 resolved。

**重要**:始终用名字(如 "team-lead", "researcher", "analyzer")称呼队友,绝不用 UUID。发消息时直接用名字:

\`\`\`json
{
  "to": "team-lead",
  "message": "你的消息内容",
  "summary": "5-10 字预览"
}
\`\`\`
</system-reminder>
```

**规则**:只在第一 turn 注入一次(`hasAssistantMessage === false`);后续新成员加入,已有成员**不通知**——如有需要由 Lead 显式 SendMessage 告知,或成员自主重新 Read team.json。

### 5.9 TEAMMATE_ADDENDUM(中文版,1:1 对齐 claude-code-best `teammatePromptAddendum.ts`)

```markdown
# 团队协作通信

重要:你正在团队中作为成员运行。与团队中任何人通信时:
- 使用 SendMessage 工具,设置 `to: "<队友名字>"` 给指定队友发消息
- 使用 SendMessage 工具,设置 `to: "*"` 广播给团队所有人(请谨慎使用)

仅仅在回复中写文字是不会被团队其他人看到的——你必须使用 SendMessage 工具。

用户主要与团队负责人(team-lead)交互。你的工作通过任务系统和队友消息进行协调。
```

**Lead 默认名字 = `"team-lead"`**(对齐 claude-code-best `TEAM_LEAD_NAME` 常量)。

---

## 6. 生命周期模型

### 6.1 Team 生命周期

```
创建:LLM 调 TeamCreate(name, description?, agent_type?)
     → 检查 session 是否已有 Team(单例;有则抛"Already leading team X")
     → 名字去重(若磁盘已存在同名 team 目录,生成新词槽 word slug)
     → 写 team.json(members=[Lead 一个人]),派活那个 Employee 作为 Lead 加入
     → 建空 tasks/ 目录
     → 返回 { team_name, team_file_path, lead_agent_id }

成员加入:Lead 调 Agent(name=..., team_name=..., employee_id=X, prompt=...)
         → 检查成员数 ≤ 4(硬限);超出抛 MaxTeammateLimitReached
         → 检查 employee_id 是否已在其他 Team 活跃(同一 Employee MVP 限 1 个 Team)
         → 调 EmployeeStore::get_employee(X) 拿 EmployeeRecord
         → 校验 tool_whitelist 含 SendMessage / TaskList / TaskGet 三个强制工具
         → 注册到 AgentNameRegistry(name 在同 SessionId 内唯一)
         → 加入 Team.members,写 team.json
         → 启动 idle loop(tokio task)

关闭:三条路径
  (a) LLM 显式:TeamDelete() → cancel 所有 Teammate + 清 tasks/inboxes/teammates 目录(team.json 留作审计)
  (b) Session 结束:cleanupSessionTeams() 自动(4 个触发点见 §7.3)
  (c) 所有 Teammate 都 shutdown:Team 变空壳,但不自动关(等 session 结束)

入口决策(对齐 claude-code-best):
- 派活的 Employee 跟普通 chat 一样跑,LLM **自主判断**是否调 TeamCreate
- 不想让某员工当 Lead → EmployeeRecord.tool_whitelist 去掉 TeamCreate
```

### 6.2 Teammate 生命周期

```
spawn → 初始 prompt turn → 进 idle loop 前先扫一次 unclaimed task
           ↓
      (空才进 select! 等 wakeup)
           ↓
  wakeup1: pending message → run_turn → LLM 回应 → 回 idle
  wakeup2: task_list_watch 变化 → check_unclaimed → 决策是否 claim → run_turn → 回 idle
  wakeup3: cancellation → cleanup → exit
           ↓
      (一直循环直到被关)
           ↓
  shutdown_request → LLM 决策 → approve → 等 Lead TaskStop → exit
  TaskStop → abort → exit
  session end → abort → exit
  app crash → (不恢复,退出后留磁盘文件但 in-mem 全丢)

收尾约定:
- Teammate 完成任务用 SendMessage(to=lead) 报告 + TaskUpdate(status=completed)
- 结构化数据(findings / artifacts 等)放 TaskUpdate.metadata,Lead 用 TaskGet 读
- Teammate 不主动退出,继续 idle 等下一个任务/消息/shutdown_request
```

### 6.3 何时会有"phase"概念(仅展示层)

数据模型里**没有 phase 字段**,但前端展示需要给用户看一个状态。这个状态**派生计算**,不存:

```
derive_phase(member) = match (member.cancellation_token.is_cancelled(), member.last_active_at, member.has_pending) {
  (true, _, _)                     → Cancelled
  (false, recent(< 1min), _)       → Running
  (false, _, pending > 0)          → Running   // 有消息待处理
  (false, older, 0)                → Idle
}
```

**`last_active_at` 更新时机**(turn 级粒度,不到工具级):
- Member 从 idle select! 醒来开始一个 turn 时:更新一次
- Member turn 跑完进入 idle 之前:再更新一次
- 不在每个工具调用时更新(过细,无价值)

**红线**:后端 runtime 不维护 phase 枚举字段;前端需要就自己派生。避免"状态机同步错"这类 bug。

---

## 7. 取消传播模型

### 7.1 CancellationToken 子树(不变,沿用现有)

```
SessionRoot CancellationToken
  ├─ Lead's run token                    ← 用户 cancel 主会话时连 Lead 一起取消
  │   ├─ Lead's tool_call tokens
  │   │   └─ MCP subprocess              ← P0 修复此链路
  │   └─ Lead's Agent(...) spawn Teammate:
  │       ├─ 同步 Teammate:child token of parent tool(跟随父 cancel)
  │       └─ 异步 Teammate(run_in_background=true):**独立 token**(不挂父)
  │                                       ← 保护 Employee 长任务
```

### 7.2 各层 cancel 时动作

| 层 | cancel 触发 | 动作 |
|---|---|---|
| Session | 用户"中断" | root.cancel(UserCancel),所有同步子树停 |
| Run(Lead) | Session cancel 级联 或 Lead 自 SendMessage(shutdown_response, approve=true)→TaskStop | Lead LLM 调用中断 + 同步 tool token 取消 |
| Tool call | 父 run cancel | 工具 execute 内 select cancellation(已有约定) |
| MCP subprocess | 工具 token cancel(**P0 新补**) | `McpRuntimeTool` 内 select,触发 `connection.disconnect_on_cancel()` |
| 同步 Teammate | 父 run cancel 级联 | idle loop select 命中,cleanup + exit |
| 异步 Teammate | 仅 `TaskStop(teammate_id)` | 独立 token cancel;不被父级联 |
| Pending message | Run cancel 时 | drain 丢弃,不再 inject |

### 7.3 Session 结束 cleanup 触发点

| 触发点 | 是否触发 cleanup |
|---|---|
| 用户在前端点"停止/中断" | **是**(对应用户中断主会话,root.cancel) |
| 用户退出 app(Tauri window close) | **是** |
| 主 conversation 被删除 | **是** |
| App crash | **是**(进程已死,所有 tokio task 自然退出) |
| 用户切到别的对话(原对话仍存在) | **否**(Team 留着,用户回来 Lead 还能继续) |
| 用户长时间不操作(idle timeout) | **否**(MVP 不做 §2.2 第 8 条) |

### 7.4 `is_async` flag 与 permission auto-deny

Teammate 与 async subagent 没有同步 UI 通道,弹 permission ask 没意义。LTR 引入 `is_async: bool` flag 控制此行为(对齐 claude-code-best `runAgent.ts:440-450`):

| Agent 类型 | `is_async` | permission 行为 |
|---|---|---|
| Lead(用户派活) | false | 正常弹 `permission:ask` 事件,前端弹 modal,与现有 chat 一致 |
| Lead(cron 派活) | **false** | 同上,弹 ask 留 pending,用户下次打开 app 处理 |
| Teammate | **true** | avoid prompts |
| async subagent(`run_in_background=true`) | **true** | avoid prompts |
| 同步 subagent | false | 正常弹 |

**avoid prompts 行为**(对齐 claude-code-best `permissions.ts:932-952`):
1. 先跑 permission hook(如果配置),hook 可决策 allow / deny
2. hook 无决策 → auto deny,返回���确错误给 LLM,LLM 自己处理(换工具 / 写 task 留给用户复盘 / 跳过)
3. **不发** RuntimeEvent `permission:ask`(避免前端展示无人能回应的弹窗)

**flag 传递**:`RunCtx` 加 `is_async: bool` 字段;spawn 子 agent 时**从父继承**(Teammate 不能 spawn 子 Teammate,继承链很浅)。

### 7.5 失败恢复(MVP 不做自动恢复)

| 失败类型 | MVP 行为 |
|---|---|
| Teammate LLM 错误 | 自己 SendMessage(to=lead) 报告失败 + TaskUpdate(status=failed),Lead 决定是否重派 |
| Teammate 工具调用失败 | tool error 进 transcript,Teammate 自己决定是否继续 |
| 应用崩溃后重启 | **不恢复**进行中的 Team;磁盘留 team.json + tasks/ 作审计 |
| Lead 自身崩溃 | 整个 session run cancel,同步 Teammate 级联;异步 Teammate 由 `ActiveRunGuard` 善后 |

---

## 8. 与现有 runtime 的映射

### 8.1 新增模块(能力层,不含实施)

| 模块 | 责任 |
|---|---|
| `runtime/agent/team.rs`(替换 stub) | Team / Member / AgentTask / PendingMessage 数据模型 + Team 操作 |
| `runtime/agent/team_store.rs`(新) | `team.json` / `tasks/*.json` / `inboxes/*.json` 持久化 |
| `runtime/agent/name_registry.rs`(新) | AgentNameRegistry,按 SessionId 分区 |
| `runtime/agent/shared_task_list.rs`(新) | 共享 Task list + tokio Mutex claim + blocks DFS 环检测 |
| `runtime/agent/pending_mailbox.rs`(新) | per-Member pending_messages channel + Lead idle 触发 kick |
| 扩展 `runtime/agent/worker_runtime.rs` | **不新建 `teammate_runtime.rs`**;Teammate 是"常驻 idle 的 worker",共享同一份 LLM turn driver 和 tool 执行链路,差异处(idle loop / pending drain / shutdown 协议)通过 trait / 配置开关实现 |

### 8.2 新增工具(LLM 可见,共 9 个 Team 协作工具)

| 工具 | 入参要点 | 强制级别 |
|---|---|---|
| `TeamCreate` | `team_name`,可选 `description` / `agent_type`(对齐 claude-code-best,只建空 Team,不 spawn 任何 Member;`agent_type` LTR 不读,仅记审计) | 默认勾,可去(限制员工当 Lead 用) |
| `TeamDelete` | 空入参(关当前 SessionId 内的 Team) | 默认勾,可去 |
| `Agent`(扩展现有) | 现有:`description` / `prompt` / `model` / `run_in_background`;**新增可选**:`employee_id`、`name`、`team_name`;**判断**:`team_name && name` → 派 Teammate,否则 → 普通 subagent | 默认勾,可去(Teammate 派活时此工具自动屏蔽——禁止递归 spawn) |
| `SendMessage` | `to`(name / `"*"` / agent_id),`message`(union:text / shutdown_request / shutdown_response / plan_approval_request / plan_approval_response),`summary?` | **强制必含** |
| `TaskCreate` | `subject`, `description`, `activeForm?`, `metadata?` | 默认勾,可去 |
| `TaskUpdate` | `task_id`,可选 `owner` / `status`(pending/in_progress/completed/deleted) / `addBlocks` / `addBlockedBy` / `subject` / `description` / `metadata`;blocks 写入时 DFS 环检测 | 默认勾,可去 |
| `TaskList` | 可选过滤(status / owner 等) | **强制必含** |
| `TaskGet` | `task_id` | **强制必含** |
| `TaskStop` | `task_id`(对 Teammate agent_id 的强制关闭,绕过 LLM 决策;也可关 unfinished task) | 默认勾,可去 |

**强制必含校验**(`runtime/employee/store.rs`):EmployeeRecord 的 `tool_whitelist` 缺任一强制工具(SendMessage / TaskList / TaskGet)→ 派 Teammate 时失败,返回明确错误。

**不引入工具**:`WorkerComplete`(删,Teammate 收尾走 SendMessage + TaskUpdate);`ArtifactRegister`(删,文件系统就是 SoT)。

### 8.3 改动模块

| 模块 | 改动概要 |
|---|---|
| `runtime/mcp/*` | P0:迁 ToolDispatcher + cancel 挂钩 |
| `runtime/llm/executor` 相关 | P0:`compact_summary` 迁 trait |
| `runtime/tools/builtin/spawn_subagent.rs` | 入参加 `name` / `team_name` / `employee_id`(全部可选);判断 `team_name && name` 时派 Teammate(走常驻 idle 路径);工具 description 拼接 `EmployeeStore::list_employees()` 返回的可派清单 |
| `runtime/agent/async_task_store.rs` | 暴露 pending_messages 给 SendMessage 工具;接 Lead idle 触发机制(消息入队时 kick Lead) |
| `runtime/tools/catalog.rs` | 注册 9 个新工具到 catalog |
| `runtime/employee/store.rs` | **暴露 `list_employees` / `get_employee` 只读查询接口供 LTR 使用**(加载机制由独立工作流负责,本方案不规定);tool_whitelist 校验必须含 SendMessage / TaskList / TaskGet 三个强制工具 |
| `runtime/employee/dispatch_prompt.rs` | 加 TEAMMATE_ADDENDUM(只在 role=Teammate 时注入);**Lead 不叠 LEAD_ADDENDUM**(对齐 G2 决策) |
| `runtime/events.rs` | 加 3 个 variant:TeamCreated / MemberJoined / MemberPhaseChanged |
| `transport/tauri_event_adapter.rs` | 3 新 variant 暂不映射(Phase 3 前端接入时再开) |
| 主 chat turn driver(`runtime/chat/chat_turn_driver.rs`) | Lead turn 结束时自检 pending_messages 非空 → 自动起下一 turn(Lead idle 触发机制 A 路径,见 §5.6) |

### 8.4 持久化布局

**租户分区**:lotus 数据按 `t_{tenant_id}__u_{user_id}` 分区到 `~/.renlijia/users/{scope_key}/` 下;`conversations/` 等所有用户数据都在这个 scope 内。Team 数据沿用同一布局,**自动按租户隔离**(实现层用现有 `UserScopedPaths::conversations_dir()` 等 API,不需要为 Team 新设计租户路径)。

**核心设计原则**(对齐真实代码事实):

| 设计点 | claude-code-best 实际做法 | lotus 做法 | 理由 |
|---|---|---|---|
| 数据放在哪 | 三个独立顶级目录:`~/.claude/projects/{cwd}/{sessionId}/`、`~/.claude/teams/{name}/`、`~/.claude/tasks/{id}/`(`teamHelpers.ts:112-122` / `teammateMailbox.ts:56-66` / `tasks.ts:221-231`) | **全部收进 `conversations/{conv_id}/` 单一目录** | claude-code-best 多进程必须分散到顶级共享路径;lotus 单 Tauri 进程无此包袱,统一到 conversation 目录更整洁,删 conversation 时一并清理 |
| Team transcript 文件名 | `agent-{agentId}.jsonl` + `agent-{agentId}.meta.json` sidecar(`sessionStorage.ts:247-262`) | **沿用相同格式** | 直接对齐 |
| 普通 subagent 与 Teammate 区分 | 路径**完全相同**(都在 `subagents/` 扁平),靠读 TeamFile.members 列表区分(`transcriptSubdir` 机制预留但未激活) | **路径直接区分**:普通走 `subagents/`,Teammate 走 `teammates/` | claude-code-best 不区分是因为多进程下 subdir 机制不便;lotus 单进程,路径区分更直观,删 Team 时直接 `rm -rf teammates/` 不影响普通 subagent |
| 文件锁 | proper-lockfile retries:30(`tasks.ts:504-506` `.lock` 文件) | **tokio Mutex,不创建 `.lock` 文件** | 同进程纳秒级锁,远快于文件锁 |
| 高水位线 | `.highwatermark` 文件防 ID 复用(`tasks.ts:111`) | **不创建,用 uuid** | 单 Tauri 进程无 ID 复用风险 |
| inbox/mailbox | 磁盘 JSON 文件是 SoT(`teammateMailbox.ts:60`) | **tokio mpsc channel 是 SoT**(零延迟),磁盘 `inboxes/{name}.json` 仅作审计备份 | 单进程不需要磁盘做跨进程通信;但保留磁盘文件方便审计/排错 |

**完整目录树**:

```
~/.renlijia/users/{scope_key}/                  [scope_key = "t_<tenant>__u_<user>"]
└── conversations/{conv_id}/
    ├── conv.json                               [现有,不变]
    ├── messages.N.jsonl                        [现有,不变 — Lead 主 transcript]
    ├── _current                                [现有,不变]
    ├── compact_boundaries.jsonl                [现有,不变]
    ├── file_index.json                         [现有,不变]
    │
    ├── team.json                               [新 — Team + Members 列表,对应 claude-code-best teams/{name}/config.json]
    │
    ├── inboxes/                                [新 — mailbox 磁盘审计备份(主路径是 tokio channel)]
    │   ├── lead.json                           [Lead 收到的消息]
    │   └── {teammate_name}.json                [每个 Teammate 收到的消息]
    │
    ├── tasks/                                  [新 — 共享 Task list,每 task 一文件]
    │   └── {task_id}.json                      [task_id = uuid v4 字符串]
    │   └── (无 .lock,无 .highwatermark)
    │
    ├── subagents/                              [新 — 普通 subagent transcript,扁平,跑完即死那种]
    │   ├── agent-{agent_id}.jsonl              [transcript]
    │   └── agent-{agent_id}.meta.json          [sidecar metadata]
    │
    └── teammates/                              [新 — Team 成员 transcript,扁平,常驻 idle 那种]
        ├── agent-{agent_id}.jsonl              [transcript,每次 LLM turn append]
        └── agent-{agent_id}.meta.json          [sidecar metadata]
```

**关于 Teammate 产出文件**:**不强制目录约束**(对齐 claude-code-best `runAgent.ts` 的 cwd 行为)。Teammate 与 Lead 共享 cwd / workspace,写文件直接写到合理位置(代码改到项目内、报告写到 workspace `reports/`、临时文件 `tmp/` 等),受 permission 系统(§7.4)管控。Lead 想看产物用 `Read` / `Glob` / `Grep` 工具,跟普通文件一致——**不引入 ArtifactRef 元数据登记机制**。

**`.meta.json` sidecar 字段**(普通 subagent 和 Teammate 共用,但 Teammate 的 `team_id` / `agent_name` 字段必填):

```json
{
  "agent_id": "...",
  "agent_name": "researcher",            // AgentNameRegistry 注册名(Teammate 必填,普通 subagent 可省)
  "kind": "teammate" | "subagent",        // 冗余字段,与目录路径一致,方便 grep
  "employee_id": "xiaoyan",               // 从哪个 EmployeeRecord 模板物化(可选,普通 subagent 可省)
  "team_id": "{conv_id}",                 // 所属 Team(Teammate 必填,普通 subagent null)
  "spawned_by": "{lead_agent_id}",        // 谁派的(Lead 的 agent_id)
  "spawned_at": "2026-05-10T10:30:00Z",
  "model": "sonnet",
  "is_async": true,                       // 对齐 §7.4,Teammate 永远 true,async subagent 也是 true
  "tool_whitelist": ["Read", "Edit", "SendMessage", ...]   // 实际授予的工具集快照
}
```

**关键约束**:
- Team 数据**严格不跨租户**——`AgentNameRegistry` 已按 SessionId 分区,SessionId 通过 conversation_id 隐含挂在 scope 下,自然不会跨租户
- 不同租户/用户的 `conversation_id` 即使同名也是隔离的(物理路径不同)
- 跨租户的"Team 协作"在 MVP 不支持(也不规划)
- `teammates/` 子目录在 Team 创建时按需创建,Team 关闭/conversation 删除时一并清理
- 删除 Team 操作:删除 `team.json` + `tasks/` + `inboxes/` + `teammates/`(整个目录),`subagents/` **不动**
- inbox 磁盘备份只在 SendMessage 入队时同步 append(运行时主路径是 tokio channel 内存),崩溃后磁盘文件可读但**不用于状态恢复**(对齐 claude-code-best 不恢复语义);用途仅限开发期审计/排错,LLM / 前端均不直接读

**lotus 实际现状的偏离**:lotus 当前 `UserScopedPaths::subagent_transcripts_dir()` 把 subagent transcript 放在 `~/.renlijia/users/{scope}/subagent_transcripts/`(user 级扁平,跨 conversation 共享)。本方案改为 conversation 级,**产品未上线无兼容负担**,直接迁移即可。需要在实施时清理:
- `UserScopedPaths::subagent_transcripts_dir()` 标记 deprecated 或删除
- `runtime/agent/output_writer.rs:48` 改写到新路径
- 旧路径下的现有数据不迁移(开发期数据,无价值)

### 8.5 Transcript 文件内容规范

`subagents/agent-{id}.jsonl` 和 `teammates/agent-{id}.jsonl` 都使用**同一种格式**:JSONL append-only,每行一个 entry,完整记录该 agent 这次运行的所有 LLM turn 输入输出和工具调用。

#### Entry 类型

| type | subtype | 何时 append | 字段要点 |
|---|---|---|---|
| `system` | `init` | spawn 启动时,写一次 | `agent_id`, `model`, `tool_whitelist`(快照), `system_prompt_hash`, `kind`(subagent/teammate), `timestamp` |
| `user` | — | 每个 LLM turn 起手 drain pending_messages 时,每条消息一行 | `content`, `source`(lead/teammate/user/system), `from_agent_id`, `from_name`, `timestamp` |
| `assistant` | — | LLM 流式输出 done 时 | `content`(完整文本), `model`, `usage`(input/output tokens), `timestamp` |
| `tool_use` | — | LLM 决定调工具时 | `tool_use_id`, `tool_name`, `input`(JSON), `timestamp` |
| `tool_result` | — | 工具执行完返回时 | `tool_use_id`(对齐 tool_use), `output`, `is_error`, `timestamp` |
| `system` | `compact` | 触发 compaction 时 | `reason`(context_overflow / explicit), `summary`(被压缩段的摘要), `boundary_id`, `timestamp` |
| `system` | `abort` | TaskStop / Session cancel / shutdown approve 后被中断时 | `reason`(user_cancel / shutdown_approved / sibling_error / ...), `timestamp` |

#### 示例(一个 Teammate 的简化 transcript)

```jsonl
{"type":"system","subtype":"init","agent_id":"abc","model":"sonnet","kind":"teammate","timestamp":"2026-05-10T10:00:00Z"}
{"type":"user","content":"调研 X 公司的商业模式...","source":"lead","from_name":"小研","timestamp":"2026-05-10T10:00:01Z"}
{"type":"assistant","content":"开始调研。先看官网。","usage":{"input":1200,"output":40},"timestamp":"2026-05-10T10:00:05Z"}
{"type":"tool_use","tool_use_id":"t1","tool_name":"WebFetch","input":{"url":"https://x.com"},"timestamp":"2026-05-10T10:00:05Z"}
{"type":"tool_result","tool_use_id":"t1","output":"...","is_error":false,"timestamp":"2026-05-10T10:00:08Z"}
{"type":"assistant","content":"...找到关键信息","usage":{"input":3500,"output":120},"timestamp":"2026-05-10T10:00:15Z"}
{"type":"tool_use","tool_use_id":"t2","tool_name":"SendMessage","input":{"to":"小研·技术","message":"他们用 Stripe..."},"timestamp":"2026-05-10T10:00:15Z"}
{"type":"tool_result","tool_use_id":"t2","output":"Message queued","is_error":false,"timestamp":"2026-05-10T10:00:15Z"}
(...idle 一段时间,不写任何 entry...)
{"type":"user","content":"@商业研究员 你那边搞定了吗?","source":"lead","from_name":"team-lead","timestamp":"2026-05-10T10:05:00Z"}
{"type":"assistant","content":"...","timestamp":"2026-05-10T10:05:10Z"}
{"type":"tool_use","tool_use_id":"t3","tool_name":"SendMessage","input":{"to":"team-lead","message":{"type":"text","text":"调研完成,关键发现见 task metadata"}},"timestamp":"2026-05-10T10:05:10Z"}
{"type":"tool_result","tool_use_id":"t3","output":"Message queued","is_error":false,"timestamp":"2026-05-10T10:05:10Z"}
{"type":"tool_use","tool_use_id":"t4","tool_name":"TaskUpdate","input":{"task_id":"...","status":"completed","metadata":{"findings":[...]}},"timestamp":"2026-05-10T10:05:11Z"}
{"type":"tool_result","tool_use_id":"t4","output":"updated","is_error":false,"timestamp":"2026-05-10T10:05:11Z"}
(...继续 idle 等 shutdown_request 或新任务...)
{"type":"user","content":"shutdown_request: 任务完成","source":"lead","from_name":"team-lead","timestamp":"2026-05-10T10:10:00Z"}
{"type":"assistant","content":"同意关闭","timestamp":"2026-05-10T10:10:05Z"}
{"type":"tool_use","tool_use_id":"t5","tool_name":"SendMessage","input":{"to":"team-lead","message":{"type":"shutdown_response","approve":true}},"timestamp":"2026-05-10T10:10:05Z"}
{"type":"tool_result","tool_use_id":"t5","output":"Message queued","is_error":false,"timestamp":"2026-05-10T10:10:05Z"}
{"type":"system","subtype":"abort","reason":"shutdown_approved","timestamp":"2026-05-10T10:10:06Z"}
```

#### 关键规则

| 规则 | 说明 |
|---|---|
| **Append-only** | 绝不改写已有 entry。所有变化都通过新 entry 表达 |
| **同 agent 多 turn 同文件** | 常驻 idle Teammate 会随时间不断 append,文件可能变大 |
| **SendMessage 入队也写** | 不仅工具结果写,Teammate 收到的 SendMessage drain 出来作为 user turn 写入(对齐 §5.7 决策) |
| **不存 raw CoT** | LLM 内部 thinking / scratchpad 不写;只写 tool_use / assistant 完整输出 / tool_result 结构化结果 |
| **idle 期间不写** | Teammate 在 `select!` 等 wakeup 时不消耗 LLM,不产生 entry。文件不会"心跳膨胀" |
| **compact 边界双写** | `system/compact` entry 写本文件 + 边界详情写 `conversations/{id}/compact_boundaries.jsonl`(沿用现有机制) |
| **崩溃后文件残留** | 进程崩溃时正在写的最后一行可能不完整。读取方需容错(skip 不可 parse 的最后一行) |
| **大小上限** | 对齐 claude-code-best `MAX_TRANSCRIPT_READ_BYTES = 50MB`(`sessionStorage.ts:229`)。超过时停止 read(避免 OOM)但**继续 append**(不截断历史) |

#### 跟 Lead 主 transcript 的格式区别

| 维度 | Lead 主 transcript(`messages.N.jsonl`) | Subagent/Teammate transcript(`agent-{id}.jsonl`) |
|---|---|---|
| 路径 | `conversations/{conv_id}/messages.N.jsonl`(分片) | `conversations/{conv_id}/subagents-or-teammates/agent-{id}.jsonl`(单文件) |
| 分片机制 | 每 100 条切一片,`_current` 指针记录 | 不分片,单文件 append |
| 格式 | `StoredMessage`(`storage/file_store/types.rs:52`,含 `tool_calls`) | claude-code-best 风格 entry(本节定义) |
| 用途 | 用户主会话 UI 渲染、resume、export | 后台 agent 审计、`TaskOutput` 增量读、调试 |
| 现有/新 | 现有不变 | 新格式,本方案引入 |

**为什么不统一**:Lead 主 transcript 已经稳定且对接前端 UI;Teammate transcript 是新引入的,对齐 claude-code-best 更省事。两种 transcript 各管各的,中间通过 `task-notification`(注入 Lead 主 transcript)和 `TaskOutput`(读 Teammate transcript)桥接。Phase 4+ 可考虑统一格式,MVP 不动。

---

## 9. 风险 / FAQ

### 9.1 已知风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| MCP 迁 ToolDispatcher 引入回归 | 中 | 高 | P0 严格回归;新旧路径并存 1 周观察 |
| Lead LLM 不愿用 Team 模式 | 中 | 中 | TeamCreate 工具 description 引导(对齐 claude-code-best 113 行长 prompt);A/B 测试 |
| Teammate 不回 shutdown_response 或 Lead 不发 shutdown_request | 中 | 中 | Session 结束 cleanup 兜底(§7.3 4 触发点) |
| Teammate 之间消息互推死循环 | 低 | 中 | Teammate budget 硬限(max 20 turns);监控 SendMessage 频率 |
| 共享 Task list claim 竞态 | 低 | 高 | tokio Mutex + tmp+rename 原子写 |
| 异步 Teammate 独立 token 导致僵尸 | 中 | 中 | `ActiveRunGuard` RAII + TaskStop 手动清理 |
| tool_whitelist 校验升级破坏现有测试 | 中 | 中 | 新建员工强制,已有员工配置层直接补齐(产品未上线无兼容负担) |
| Lead idle 触发机制(§5.6)误起 turn 浪费 token | 低 | 中 | A 路径仅在 pending 非空时起;C 路径仅在 Lead idle 时 kick(运行中跳过);两条路径都不会"空转" |
| 广播 `to:"*"` 被 LLM 滥用导致消息风暴 | 低 | 中 | prompt 引导 sparingly 使用(对齐 claude-code-best);MVP 不加频控,有问题再加 |

### 9.2 FAQ

**Q:为什么 Worker = Employee,而不是新实体?**
A:减少产品概念数量。用户只需理解"数字员工"一个概念;被 Lead 派 = 变成 Teammate 角色;被用户派 = 变成 Lead 角色。tool_whitelist / skill 等配置在 EmployeeRecord 已完备,不需要再建一套。

**Q:为什么删 Coordinator Mode?**
A:它在 claude-code-best 里是 session 级开关,因为那里没有 Employee 这层用户配置。lotus 有 EmployeeRecord.tool_whitelist 能精确控权(想让员工只指挥不动手,就不勾 Bash/Edit),不需要再加运行时 mode。

**Q:为什么删 WorkerComplete?**
A:对齐 claude-code-best 的真实做法——它没有这个工具。Teammate 收尾用 `SendMessage(to=lead, message=text)` 报告 + `TaskUpdate(status=completed)` 改状态,结构化数据(findings / artifacts 等)走 `TaskUpdate.metadata`,Lead 用 `TaskGet` 读。少一个工具,概念更少,生命周期与 Task 解耦(一个 Teammate 一辈子可做多个 task)。

**Q:为什么删 ArtifactRef + artifacts/ 目录?**
A:对齐 claude-code-best 的真实做法——它没有"Artifact"独立概念,Teammate 与 Lead 共享 cwd,写文件直接写,Lead 用 Read/Glob 查产物。文件系统就是 SoT,不在文件之上再造一层登记仪式。lotus 用户是办公人员,产物可能要写代码到项目内、写报告到 workspace,锁在 artifacts/ 目录反而废了 Teammate 的实际能力。

**Q:Teammate 之间消息会不会被 Lead 看到?**
A:**不会**。各 Member 私有 transcript,Teammate 间 SendMessage 只进对方 pending_messages。Lead 只通过 task-notification(TaskUpdate 状态变化)+ SendMessage(Teammate 主动报告)看结果。这是 swarm 核心,也是 Lead 上下文不爆炸的关键。

**Q:app 崩溃后能恢复吗?**
A:**不能**(对齐 claude-code-best)。磁盘 team.json / tasks/ / inboxes/ / teammates/ 保留作审计,但 in-memory Teammate runner 全丢。session 重启即新 session;MVP 不做历史 Team 视图。

**Q:共享 Task list 和 claude-code-best 实现差异大吗?**
A:能力一致,实现更快。claude-code-best 用 proper-lockfile 文件锁(为跨进程/跨 tmux pane 设计,重试 30 次 5-100ms),lotus 是单 Tauri 进程用 tokio Mutex 就够(纳秒级)。

**Q:LLM 决策 shutdown 靠谱吗?**
A:两路径并存:**优雅关用 shutdown_request 让 Teammate LLM 自己决定**(灵活,但 LLM 可能拒绝);**强制关用 TaskStop 直接 abort**(兜底,Lead 想关必然能关)。Lead 收到 approve=true 后漏调 TaskStop 不做自动兜底,完全靠 LLM 协议自洽(对齐 claude-code-best),session 结束 cleanup 是最终保险。

**Q:cron 触发的 Lead 遇到 permission ask 怎么办?**
A:**正常弹**,跟用户主动派活的 Lead 行为完全一致。cron Lead 仍是同步 chat session,有 RuntimeEvent 通道,弹的 ask 留 pending,用户下次打开 app 看到这个对话的待处理 ask,点回应即可。`is_async` flag 只对 Teammate / async subagent 为 true,Lead 永远 false(§7.4)。

**Q:Lead 怎么知道当前 Team 里有哪些成员?**
A:**主动 Read `~/.renlijia/users/{scope}/conversations/{conv_id}/team.json`,看 members 数组**——对齐 claude-code-best 的"通讯录文件"哲学。Lead 自己派的成员当然记得本 turn 派了谁;跨 turn 想查最新状态就 Read 文件。Teammate 也是同一机制,通过第一 turn 的 `team_context` attachment 拿到文件路径(§5.8)。

**Q:Lead 怎么知道有哪些 employee_id 可派?**
A:`Agent` 工具的 description 动态拼接 `EmployeeStore::list_employees()` 返回的清单,每行格式 `- {employee_id}: {description} (Tools: ...)`(对齐 claude-code-best `formatAgentLine`)。LLM 在 tools schema 里读到 `Agent` 工具时就看到了清单——不需要 `ListEmployees` 工具。

**Q:为什么 lotus 不学 claude-code-best 用多进程(tmux pane / 独立进程)?会不会有差距?**
A:**功能能力上没有任何差距,性能上 lotus 单进程方案显著更优**。
claude-code-best 是 CLI 工具,运行在终端里,所以它**顺手**用了 tmux 给用户提供"打开几个 pane,每个 pane 是一个 Teammate"的可视化体验——这是它**形态决定的实现选择**,不是 multi-agent 必须多进程。
它的 backend 实际上有三种(`spawnMultiAgent.ts:840 / 305 / 545`):InProcess(同进程)/ SplitPane(tmux 分屏)/ SeparateWindow(独立 tmux 窗口);**InProcess 才是默认**,跨进程是为终端用户的可视化体验加的可选项。
lotus 是 Tauri **GUI 桌面 app**,有 React 窗口而不是终端 pane,Teammate 可视化由前端组件做(Phase 3 Team Room 视图),**根本用不上 tmux**。
具体能力对比:

| 维度 | claude-code-best 多进程 | lotus 单进程 + tokio |
|---|---|---|
| 功能能力 | 见 §3 表格 | **1:1 对齐,无差距** |
| spawn 一个 Teammate | fork/exec 新进程,数十~百毫秒 | tokio::spawn,微秒级 |
| Teammate 间 SendMessage | 文件锁 + 500ms 文件轮询 | tokio mpsc channel,纳秒级零延迟 |
| Task list claim | proper-lockfile 重试 30 次 | tokio Mutex,纳秒级 |
| Teammate idle 资源占用 | 整个 Node.js 进程(几十 MB) | 一个 tokio task(几十 KB) |
| 隔离性(panic/泄漏) | 进程级隔离,略强 | `tokio::spawn` + `panic=unwind` + `ActiveRunGuard` RAII,实际够用 |
| 跨机�� / 远程 Teammate | 支持(`uds:` / `bridge:`) | 不做(MVP 范围外;未来走 A2A 协议层) |
| 用户介入某 Teammate | 切到对应 pane 输入 | 前端 UI 走 SendMessage |
| 崩溃恢复 | 都不恢复(磁盘留审计文件) | 同 |

**结论**:lotus 选单进程**不是妥协,是更好的选择**——同样的 Teammate 能力,实现更简单、性能快几个数量级、GUI 可视化上限更高。唯一不做的是跨机器(用不上)。

### 9.3 实施前已敲定的决策(v3 → v4 收口)

v3 阶段挂着的 4 个待拍板决策,经 G1-G22 讨论已全部落定:

| 决策 | v4 取值 | 落点 |
|---|---|---|
| TEAMMATE_ADDENDUM 文案 | 中文版 1:1 对齐 claude-code-best,定稿见 §5.9 | §5.9 |
| Teammate 个数上限 | **硬限 4**,超出抛 `MaxTeammateLimitReached` | §4.4 |
| 同 Employee 跨 Team | **MVP 限 1 个 Team** 活跃;跨 Session 无限制 | §4.4 / §6.1 |
| Lead 是否叠 LEAD_ADDENDUM | **不叠**,协作引导全在工具 description | §3.B.1 / §8.3 |

---

## 10. 完成标志(客观可验证)

1. ✅ §1.5 三条端到端验收全部通过
2. ✅ §3 能力清单 A / B / C(MVP 7 项)/ D 全部实现;E 按决策删除;F 已有(扩展 spawn_subagent);G 全部 MVP 做(含 plan_approval_*)
3. ✅ `cargo test review_ --tests` 全绿(含新 LTR 架构回归)
4. ✅ `cargo test --tests --no-fail-fast` 整体不退化
5. ✅ 前端关键集成测试不退化
6. ✅ 至少一个真实 Employee(建议小研)在真实 LLM 下端到端跑通 Team 模式(手测 1 次,含广播 / shutdown_request / 共享 Task 一遍)
7. ✅ 风险 §9.1 全部有缓解措施落地到代码/测试
8. ✅ Lead idle 触发机制(§5.6)有专项回归:turn 结束自检 + 消息入队 kick 两条路径均覆盖
9. ✅ `is_async` flag(§7.4)有回归:Teammate 弹 ask 自动 deny;Lead(用户/cron 派活)正常弹

---

## §A 附录:决策依据(吸收自已删除的调研文档)

### A.1 为什么选 claude-code-best 路线

| 选项 | 判断 |
|---|---|
| 全用 claude-code-best | 不建议"全用",但作为 runtime 主基线 |
| 全用 HiClaw | 不建议(K8s/Matrix/MinIO 对桌面太重) |
| 自研混合方案 | **推荐:claude-code-best runtime + MCP/A2A protocol-ready** |
| 直接用 LangGraph/CrewAI/AutoGen | 不建议作为 Rust/Tauri 内核 |

### A.2 为什么 HiClaw 的"Manager 不进 Team Room"不适用 lotus

HiClaw 的 ChannelPolicy 白名单是**企业权限边界**(外部 Manager 不能插手 Team 内),不是禁止 Worker 互相说话。lotus 的等价边界是:**AgentNameRegistry 按 SessionId 分区**——跨 Session 的 agent 天然不能直接对话,必须经派活流程。

### A.3 为什么不存 raw CoT

1. raw CoT 含敏感推断;2) 噪声高 token 贵;3) 多 agent 中污染上下文;4) 用户/审计需要的是"为什么 + 依据 + 验证 + 风险"。
存的是 tool_use 完整入参 + assistant 完整输出 + tool_result + 任务收尾的 TaskUpdate metadata,足够审计。

### A.4 业界对比(关键启示)

| 来源 | 对 LTR 的决策影响 | 反例 |
|---|---|---|
| Anthropic Research | Lead 给 Teammate 明确目标 + 输出格式 + 工具/来源边界 + budget | 不抄"运行时动态扩缩 worker" |
| OpenAI Agents SDK | 印证 Manager vs Handoff 必须分离 | 不采用 Handoff 作为默认模式 |
| LangGraph | State + Node + Edge 视角用于 UI 可视化 | 不引入 LangGraph 库本身 |
| AutoGen | Topic/subscription 启发事件路由 | 不抄 SelectorGroupChat |
| Google ADK | Parallel fan-out/gather 模型 | 不抄确定性 workflow agent |
| MCP | 已用作工具/资源接口,继续用 | 不替代内部 runtime |
| A2A | 内部 Task/Message 形状与之兼容 | 不直接用 A2A 实体作为内部主模型 |

### A.5 lotus 现状关键文件(实施时的锚点)

- `runtime/ids.rs`(ID 模型)
- `runtime/cancellation.rs`(CancellationToken)
- `runtime/event_bus.rs` / `runtime/events.rs`(17 个 RuntimeEventKind)
- `runtime/session_runtime.rs`(run_chat_request / cancel_session)
- `runtime/chat/chat_turn_driver.rs`(turn driver / compaction 触发 / Lead idle 触发机制 A 路径接入点)
- `runtime/tools/dispatcher.rs` / `tools/context.rs` / `tools/capability.rs`(工具系统)
- `runtime/tools/builtin/spawn_subagent.rs`(Agent 工具)
- `runtime/tools/builtin/load_skill.rs`(Skill 工具)
- `runtime/agent/async_task_store.rs`(pending queue;Lead idle 触发机制 C 路径 kick 接入点)
- `runtime/agent/output_writer.rs`(transcript JSONL)
- `runtime/agent/worker_runtime.rs`(子代理执行核心;扩展承载 Teammate idle loop)
- `runtime/agent/team.rs`(占位 stub,本方案替换)
- `runtime/employee/store.rs` / `active_runs.rs` / `dispatch_prompt.rs`(数字员工;LTR 只调只读查询接口,加载机制由独立工作流负责)
- `storage/file_store/types.rs`(StoredMessage)
- `storage/process_ext.rs`(NoWindowExt,Windows 子进程黑窗抑制)
- `runtime::employee::store::write_atomic`(原子写参考)

---

## §B 附录:术语表

| 术语 | 含义 |
|---|---|
| **LTR** | Lotus Team Runtime,本方案的代号 |
| **Team** | SessionId 范围内的一个临时编排(1 Lead + ≤ 4 Teammate) |
| **Member** | Team 的成员(Lead 或 Teammate) |
| **Lead** | 被用户/cron 派活的那个 Employee 的本次运行实例;默认名字 `team-lead`(对齐 claude-code-best `TEAM_LEAD_NAME`) |
| **Teammate** | Lead 通过 `Agent(name=..., team_name=..., employee_id=...)` 派的常驻 idle Member |
| **Employee** | 数字员工(EmployeeRecord),产品层身份,复用现有 |
| **常驻 idle** | Teammate spawn 后进 idle loop 等待消息/任务,不"跑完即死" |
| **swarm 通信** | Teammate 之间可点对点 SendMessage,各自独立 transcript |
| **共享 Task list** | Lead 写 task,Teammate 可自主 claim(对齐 claude-code-best 的任务市场) |
| **StructuredMessage** | SendMessage 的 message 字段是 discriminated union(text / shutdown_\* / plan_approval_\*) |
| **shutdown_request** | Lead 请 Teammate 优雅关闭的结构化消息 |
| **shutdown_response** | Teammate LLM 决定是否批准关闭的结构化回复 |
| **TaskStop** | 强制关闭 Teammate 的工具,绕过 LLM 决策 |
| **AgentNameRegistry** | 按 SessionId 分区的 name → AgentId 映射,SendMessage 按名寻址 |
| **MessageSource** | 注入 LLM 时的来源标记:Lead / Teammate / User / System |
| **TEAMMATE_ADDENDUM** | 叠加在 base system prompt 之上的 Teammate 专用 prompt 片段(中文版定稿见 §5.9) |
| **task-notification** | Teammate 任务状态变化时投递给 Lead 的 XML 包装 user message(§5.7) |
| **is_async flag** | RunCtx 上的标志位,Teammate / async subagent 为 true,Lead 永远 false;控制 permission auto-deny 行为(§7.4) |
| **Lead idle 触发** | turn 结束自检 pending(A) + 消息入队 kick(C) 双保险机制,让 Lead 在 Teammate 消息到达后自动续 turn(§5.6) |
| **强制必含工具** | Employee tool_whitelist 必须含的 3 个工具:SendMessage / TaskList / TaskGet;缺一即派 Teammate 失败 |

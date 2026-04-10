# 第 3 期：Task / Agent Runtime（SubAgent 阶段 A + B）

> 目标：让 task 和 sub-agent 成为一等模型，不再作为聊天主流程中的附属分支
> 关键原则：sub-agent 不一步追平 claude-code-best，而是按 A/B 两段演进；Skill 明确作为 QueryEngine 的策略插件

---

## 一、本期目标

完成以下五件事：

1. 建立一等 `TaskRuntime`
2. 建立一等 `AgentRuntime` / `AgentInvocation`
3. 完成 SubAgent 阶段 A：child run + 受限工具集 + cancel 真正生效
4. 完成 SubAgent 阶段 B：可后台 + 可向主 run 回传结果/消息
5. 明确 Skill / Workflow 归属：Skill 是 QueryEngine 的策略插件

### 本期解决的挑战
- C6：Python session 作用域提前拍板
- C7：SubAgent 分段演进（A + B）
- C8：Skill / Workflow 和 Runtime 的归属先定

---

## 二、核心设计

### 2.1 TaskRuntime

新增一等任务模型：

```rust
pub struct TaskRecord {
    pub task_id: TaskId,
    pub parent_run_id: RunId,
    pub owner_agent_id: Option<AgentId>,
    pub subject: String,
    pub status: TaskStatus,
    pub output_path: Option<String>,
    pub blocked_by: Vec<TaskId>,
}
```

TaskRuntime 职责：
- 创建任务
- 状态流转（pending → in_progress → completed / failed / cancelled）
- 输出与增量事件
- background task 管理
- 子代理与任务绑定

### 2.2 AgentRuntime / AgentInvocation

新增模型：

```rust
pub struct AgentInvocation {
    pub agent_id: AgentId,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub mode: AgentMode,
    pub tool_scope: Vec<String>,
    pub status: AgentStatus,
    pub prompt: String,
    pub background: bool,
    pub summary_or_output_ref: Option<String>,
}
```

原则：
- agent 不是 tool 的特殊分支，而是 Runtime 可调度的一种能力
- 子代理内部也有自己的 `RunId`
- 主 run 与 child run 通过 `AgentInvocation` 关联
- background agent 的状态必须可持久化读取

### 2.3 SubAgent 阶段 A

本期前半只实现：
- child run
- 独立 `AgentId`
- 独立 `RunId`
- 受限工具集
- `CancellationToken` 真正接到 sub-agent loop
- 独立 Python session（按 child run 隔离）

**不做**：
- resume
- worktree
- continue-message
- team 协作

### 2.4 SubAgent 阶段 B

在 A 的基础上补：
- background run
- 异步结果回传
- 将 summary / result / error 回发主 run
- agent 输出进入 TaskRuntime / AgentStore

注意：
- 后台只是“脱离主 turn 阻塞”，不是可恢复
- 仍不实现 worktree / continue-message / team

### 2.5 Skill / Workflow 归属

本期拍板：
- Skill 是 `QueryEngine` 的策略插件
- Skill 提供：prompt augmentation / tool filter / step policy / precompute
- Workflow step 的调度由 Runtime 管
- TaskRuntime 只管 step 的生命周期，不理解 skill 语义细节

这样可以避免：
- skill 自己变成一套平行 runtime
- workflow 与 task runtime 双重编排

### 2.6 Python Session Scope Migration

决策固定：
- Python session 按 `RunId` 作用域隔离
- 子 agent 独立 Python session
- background run 独立 Python session
- 不再按 conversation_id 共享

理由：
- 减少跨 run 污染
- 避免主 run 与 child run 相互影响
- 为后续恢复/审计做准备

#### 跨 Run 恢复策略

从 conversation scope 切换到 `RunId` scope 后，跨 run 不能再依赖常驻 Python 进程本身，而要依赖可恢复产物：

- 文件快照
- analysis snapshot
- precompute cache
- loaded file manifest
- analysis 目录下的 artifact

规则：
- step1 完成后，step2 通过文件快照 / analysis snapshot 重建数据上下文
- 用户确认继续下一步时，不再依赖旧 conversation 级 Python 常驻内存
- `_df` / `_text` / `_dfs` 等变量视为**可恢复缓存**，不是会话真相源
- precompute 结果继续写入 analysis 目录，并在新 run 中按 manifest 读回
- `loaded:{conversation_id}:*` 这类旧 memory key 迁移为 session/run 关联的 loaded manifest

#### Conversation-Scoped Python State -> New Recovery Source

| 旧状态 | 新恢复来源 |
|--------|-----------|
| `_df` / `_dfs` / `_text` | analysis artifact / 文件快照 / precompute cache |
| loaded file marker | loaded file manifest |
| checkpoint / restore | run-scoped checkpoint + analysis snapshot |
| precompute 结果 | analysis 目录产物 + precompute cache |
| `loaded:{conversation_id}:*` | session/run 关联的 loaded manifest |

---

## 三、新增文件（建议）

```text
src-tauri/src/runtime/task/
├── mod.rs
├── task_runtime.rs          # TaskRuntime
├── task_models.rs           # TaskRecord / TaskStatus
└── task_events.rs           # TaskEvent

src-tauri/src/runtime/agent/
├── mod.rs
├── agent_runtime.rs         # AgentRuntime
├── invocation.rs            # AgentInvocation / AgentStatus
├── child_run.rs             # child run orchestration
├── background.rs            # 阶段 B 的后台执行支持
├── agent_store.rs           # AgentInvocationStore 正式真相源实现
└── message_bridge.rs        # child run 结果回传主 run

src-tauri/src/runtime/skill/
├── mod.rs
├── policy.rs                # SkillPolicy
├── precompute.rs            # precompute bridge
└── workflow_bridge.rs       # workflow step -> runtime step 桥接
```

迁移涉及旧文件：

```text
src-tauri/src/llm/sub_agent.rs
src-tauri/src/plugin/skill_trait.rs
src-tauri/src/llm/orchestrator.rs
src-tauri/src/python/session.rs
src-tauri/src/commands/chat.rs   # 仅移除残余子代理编排逻辑
```

---

## 四、迁移方式（文件级）

### 4.1 llm/sub_agent.rs
角色重定义：
- 从“tool 里的 mini loop”迁移为 `AgentRuntime` 的兼容 wrapper
- `_cancel_rx` 必须真正接入 child run cancellation
- 阶段 A 完成后，该文件不再承载主实现

### 4.2 plugin/skill_trait.rs
保留 Skill 定义抽象，但职责收敛：
- 定义 skill 策略接口
- 不直接编排生命周期
- 与 Runtime 的边界通过 `SkillPolicy` 桥接

### 4.3 orchestrator.rs
移除其中残余的 workflow / step / sub-agent 主编排逻辑，进入：
- QueryEngine（如果是 turn loop）
- TaskRuntime（如果是 step/task 生命周期）
- AgentRuntime（如果是 child run）

### 4.4 python/session.rs
作用域调整：
- session key 从 conversation_id 切换为 RunId
- 提供 child run / background run 获取独立 session 的接口
- 保留底层实现，不改 Python protocol

---

## 五、Compatibility Boundary

本期必须保持：
- 前端看见的 sub-agent 兼容事件仍为 `agent:idle` 等 legacy Tauri events
- 现有 workflow/skill 可继续运行
- 主聊天流程对用户仍可发起 sub-agent
- Python tool 对用户的基本表现不变

允许变化：
- 子代理内部实现完全更换
- background task 状态变得更可观测
- task 状态开始可持久化
- agent 状态从 `AgentInvocationStore` 读取

---

## 六、Kill List

本期末必须废掉：

1. `sub_agent.rs` 作为主实现的角色
2. `chat.rs` / `QueryEngine` 直接手写 sub-agent loop 的路径
3. workflow/step 与 task lifecycle 混在同一块逻辑里的实现
4. conversation_id 作为 Python session 复用键的角色

允许保留：
- skill trait 的语义层抽象
- workflow 旧配置格式（通过 bridge 兼容）

---

## 七、Truth Source

第 3 期拍板：

| 状态 | 真相源 |
|------|-------|
| task 当前状态 | `TaskStore` / `TaskRuntime` |
| sub-agent 当前状态 | `AgentInvocationStore` / `AgentRuntime` |
| child run 生命周期 | `RunStore` + `AgentRuntime` |
| background 任务是否运行中 | `TaskRuntime` |
| python session 归属 | `RunId` |
| skill 当前策略配置 | `SkillPolicy` |

---

## 八、Golden Trace 验收

### Trace G：sub-agent 阶段 A（前台 child run）
要求：
- 主 run 创建 child run
- child run 有独立 `RunId`
- child run 工具集受限
- cancel 能真正中断 child run
- `agent:idle` 兼容事件最终可达

### Trace H：sub-agent 阶段 B（后台）
要求：
- 主 run 发起 background sub-agent
- 主 run 不被长期阻塞
- child run 完成后结果能回传主 run
- `TaskStore / AgentInvocationStore` 有完整记录

### Trace I：Python tool in child run
要求：
- child run 使用独立 Python session
- 不污染主 run 环境
- cancel 后 session 正确关闭或标记结束
- 下一步 run 能从 artifact / snapshot 重建上下文

---

## 九、Cutover Strategy

本期采用**直接替换**：
- sub-agent 主路径直接切到 `AgentRuntime`
- background agent 直接走 `TaskRuntime + AgentInvocationStore`
- Python session 作用域直接切换为 `RunId`

切换前提：
- child run / background run / Python 恢复链路 trace 通过
- `AgentInvocationStore` 已能稳定记录状态

## 十、Rollback Strategy

若第 3 期切换失败：
- 回退到旧 `sub_agent.rs` 主路径
- background 模式关闭，只保留前台子代理
- Python session 作用域退回旧 conversation 级逻辑
- `AgentInvocationStore` 保留为旁路记录，不作为读路径

回滚判据：
- child run cancel 失效
- background agent 状态丢失
- Python 上下文恢复失败导致主流程不可用

---

## 十一、Not Doing

本期明确不做：
- 不支持 sub-agent resume
- 不支持 continue-message
- 不支持 worktree 隔离
- 不支持 team/swarm 协作
- 不改 Python 底层协议
- 不重构 browser/connector 底层实现

这些放到第 4 期或更后。

---

## 十二、本期完成定义

第 3 期完成的标志：

1. task 已成为一等模型
2. sub-agent 已成为一等模型（不再只是 tool 分支）
3. child run 有独立 RunId / AgentId
4. sub-agent cancel 真正生效
5. background sub-agent 可运行并回传结果
6. `AgentInvocationStore` 已成为 background agent 状态真相源
7. Python session 已按 RunId 作用域隔离，并具备 artifact/snapshot 恢复策略
8. Skill / Workflow 归属已收敛到 QueryEngine 策略插件模型
9. 3 条 golden trace 回放通过

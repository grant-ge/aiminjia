# Agent Command Queue 对齐记录

> 日期：2026-06-15
> 状态：已沉淀；当前版本暂不重构
> 范围：后台 agent 完成通知、主 agent 唤醒、query loop 消费，以及未来统一 command queue 的改造方案。

## 结论

当前版本先不做统一 command queue 重构。

现在的实现已经具备一条可工作的后台完成通知链路：

1. 后台 agent 或后台 shell task 结束；
2. 生成 `<task-notification>` 并进入通知队列；
3. 父会话空闲时被唤醒；
4. `ChatTurnDriver` 把通知注入模型上下文；
5. 主 agent 可以继续自己的 agentic loop。

这次发现的问题主要是架构一致性问题，不是“完全不能工作”。AIjia 当前有多条专用队列和唤醒路径；Claude Code Best 更偏向一个统一 `commandQueue`，同一份队列既能被空闲时的 queue processor 消费，也能被运行中的 query loop 消费。

## 当前 AIjia 形态

现在队列职责按事件类型拆开。

| 职责 | 当前归属 | 说明 |
|---|---|---|
| 后台 agent / shell 完成通知 | `TaskNotificationQueue` | 专门存 `<task-notification>`。 |
| task notification 后唤醒父会话 | `TauriChatCommandAdapter::schedule_task_notification_resume` | 通过内部 `__resume_from_task_notification__` turn 唤醒。 |
| query loop 内注入 | `ChatTurnDriver` | drain task notification，并作为 user-role 消息注入。 |
| App 输入 / IM pending 消息 | `PendingQueueManager` | per-session 队列，带 debounce、持久化、output binding、UI chip。 |
| Lead/team 空闲唤醒 | `LeadIdleSupervisor` | 独立状态机，处理 lead-agent 唤醒。 |

关键源码位置：

- `src-tauri/src/runtime/agent/task_notification.rs`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- `src-tauri/src/transport/tauri_commands/chat.rs`
- `src-tauri/src/runtime/pending/queue_manager.rs`
- `src-tauri/src/connector/im/shared/pending_adapter.rs`
- `src-tauri/src/runtime/agent/lead_idle.rs`

所以当前不是“没有队列”，而是“存在多个队列孤岛”。

## Claude Code Best 形态

Claude Code Best 的参考形态更统一：

1. task 完成、queued prompt、计划任务等事件进入同一个 `commandQueue`；
2. 空闲时，queue processor 发现没有 active query，就拉起下一轮 query；
3. query 运行中，`query()` loop 也会 drain inline commands，尤其是 `prompt` 和 `task-notification`；
4. queued command 被转成 attachment/message，进入模型上下文；
5. task notification 以 user-role `<task-notification>` 形式喂给模型，但不等同于真人输入。

本次对标参考的本机路径：

- `src/utils/messageQueueManager.ts`
- `src/hooks/useQueueProcessor.ts`
- `src/query.ts`
- `src/utils/attachments.ts`
- `src/tasks/LocalAgentTask/LocalAgentTask.tsx`
- `packages/builtin-tools/src/tools/AgentTool/AgentTool.tsx`

注意：`claude-code-best` 是架构参考，不是 lotus-app 的运行依赖。未来真正施工前需要重新核对参考仓库的最新实现。

## 已确认问题

### 1. 队列职责分散

`TaskNotificationQueue`、`PendingQueueManager`、`LeadIdleSupervisor` 都在处理“某个事件到达时，agent 可能空闲、忙碌、阻塞或处于 turn 间隙”的问题。

这种拆分会让这些语义变难统一：

- 优先级；
- session / agent 作用域；
- 唤醒去重；
- 失败或取消后的 re-enqueue；
- 用户 prompt 和 task notification 之间的顺序；
- 事件到底是被 active loop 消费，还是等下一次 idle resume 消费。

### 2. task notification 唤醒依赖 sentinel turn

当前唤醒路径会发送内部 `__resume_from_task_notification__` turn。`ChatTurnDriver` 识别这个 sentinel，避免持久化假的用户消息，并改为 drain task notification。

这个方案能工作，但它是 task-notification 专用路径。未来统一队列后，queue processor 应该调度的是“消费队列”的通用内部 turn，而不是 task-notification 专属 sentinel。

### 3. active-loop 消费已有雏形，但还不通用

AIjia 已经具备 loop 运行时 drain task notification 的能力。缺口不是“不能在 loop 中消费”，而是当前只 drain task notification，没有抽象成通用 command drain。

未来目标应该是：

```text
drain 当前 session/agent 可见的 inline commands
  -> prompt command
  -> task-notification command
  -> 未来允许注入的 permission/system command
  -> 注入模型上下文
  -> 如果取消、provider 失败或工具失败，未消费 command 重新入队
```

### 4. IM pending 不是散落的，但它是另一套队列

IM pending 现在集中进入 `PendingQueueManager`。它承担的职责比较重：

- 空闲直发，忙时排队；
- turn 结束后 debounce drain；
- `pending.json` 持久化和恢复；
- UI pending chip；
- 附件转换；
- `TurnOrigin` 和 `OutputBinding`，用于回复正确的 IM 会话；
- 人类交互挂起时的特殊处理；
- 多条 pending 合并 drain，同时保留多条 user message 语义。

所以 IM 的问题不是“散落在外面”，而是它和后台任务通知不在同一个队列模型里。

### 5. 内部标识和原始通知不应成为普通助手内容

异步 task id、工具返回 JSON、`<task-notification>` XML 对运行时和调试有价值，但不应该被默认回显成普通 assistant 文本，除非用户明确要求查看。

用户侧应该看到的是合适的任务完成提示、摘要或状态，而不是内部 UUID、内部 JSON、内部 XML。

## 当前版本决策

当前版本不引入统一 command queue。

短期行为保持如下：

1. 后台 agent 完成后继续进入 task notification；
2. 父会话空闲时继续走现有 task-notification wake path；
3. 父会话运行中时，继续由 `ChatTurnDriver` 在模型 loop 边界 drain；
4. `PendingQueueManager` 继续负责 App / IM pending；
5. 当前版本不迁移 IM pending。

本版本只建议做测试、用户展示修正和文档沉淀，不做大范围运行时重构。

## 未来目标

未来目标是后端统一的 `RuntimeCommandQueue`。

草案模型：

```rust
enum RuntimeCommandMode {
    Prompt,
    TaskNotification,
    PeerMessage,
    Permission,
    SystemWake,
}

enum RuntimeCommandPriority {
    Now,
    Next,
    Later,
}

struct RuntimeCommand {
    id: String,
    session_id: SessionId,
    target_agent_id: Option<String>,
    mode: RuntimeCommandMode,
    priority: RuntimeCommandPriority,
    payload: RuntimeCommandPayload,
    created_at: DateTime<Utc>,
}
```

目标链路：

```text
producer
  -> RuntimeCommandQueue.enqueue(...)
  -> 如果 session 空闲：QueueWakeProcessor 调度一次队列消费 turn
  -> 如果 session 忙碌：ChatTurnDriver 在安全 loop 边界 drain inline commands
  -> command payload 转成模型可见的 message / attachment
  -> 如果未消费成功，取消或失败时 re-enqueue
```

## 推荐迁移方案

### Phase 1：只迁 task notification

先引入 `RuntimeCommandQueue`，但只承载 `task-notification`。

`TaskNotificationQueue` 可以先保留为兼容 facade 或薄 adapter，等所有 producer 迁完后再删除。

影响面：

- 新增 runtime queue 模块；
- 改 task notification enqueue path；
- 改 `ChatTurnDriver`，从 drain task notification 改为 drain generic inline command；
- 保留当前取消、provider 失败、工具失败后的 re-enqueue 行为；
- 不改 `PendingQueueManager`。

预计改动：500-800 行。

### Phase 2：抽通用 wake processor

把 task-notification 专用 resume scheduler 收敛成通用 queue wake processor。

影响面：

- 弱化或移除 `schedule_task_notification_resume`；
- 用通用 queue-resume turn 或内部 runtime request 替换 `__resume_from_task_notification__`；
- 按 session 做 wake 去重；
- 保留“不展示 fake user message”的行为。

预计新增改动：400-700 行。

### Phase 3：桥接 PendingQueueManager

不要一上来删除 `PendingQueueManager`。

更稳的做法是先桥接：

- `PendingItem` 可以包装成 `RuntimeCommandPayload::UserMessage`；
- `PendingQueueManager` 暂时继续负责 IM 持久化、output binding 和 UI chip；
- 统一队列负责排序、唤醒和 loop 消费。

这个阶段应在 Phase 1 / Phase 2 稳定后再做。

预计新增改动：300-800 行，取决于是否把持久化也迁入统一队列。

### Phase 4：完全收敛

前面阶段稳定后，再考虑把 App composer pending、IM pending、task notification、peer message、permission/orphan event 全部合成一个 command model。

这属于较大的架构迁移，不应该和当前后台 agent 能力绑定在同一个版本里。

完全收敛预计：2500-4000 行，20+ 个文件。

## 测试要求

未来做任何重构前，至少要有这些回归覆盖：

1. 前台 agent 运行超过阈值后自动转后台；
2. 后台 agent 完成后生成 task notification；
3. 父会话空闲时能被唤醒并消费通知；
4. 父会话运行中时能在 loop 边界消费通知；
5. 取消、provider 失败或工具失败时，未消费通知会 re-enqueue；
6. 不持久化 `__resume_from_task_notification__` 假用户消息；
7. 普通用户输入和 IM pending 仍由 `PendingQueueManager` 正常处理；
8. 内部 task id 和原始 XML 不展示为普通 assistant 文本。

代码测试建议覆盖：

- task notification enqueue / drain / re-enqueue；
- session / agent 作用域；
- wake 去重；
- session 仍 busy 时跳过 drain；
- `PendingQueueManager` 的 App / IM pending 行为保持不变。

意图测试建议覆盖：

- 后台 agent 完成后的用户可见结果；
- task 完成后主 agent 被唤醒；
- happy path 不需要用户手动调用 `TaskOutput`；
- 除非明确要求，否则不在普通对话里暴露原始 task id。

## 风险

- 全量合并队列可能破坏 IM 回复路由，因为 `OutputBinding` 对移动端回复非常关键。
- 如果优先级规则不清晰，统一队列可能错误改变用户 prompt 和 task notification 的顺序。
- 过早移除 sentinel 会让 idle wake 行为变得更难验证。
- 过早迁移 `PendingQueueManager` 的持久化，可能导致重启后 IM pending 丢失。
- 如果 UI 展示层不配套，内部 command 可能泄露成普通聊天内容。

## 当前不做

当前版本明确不做：

- 不实现统一 command queue；
- 不迁移 IM pending；
- 不删除 `PendingQueueManager`；
- 不重写 `LeadIdleSupervisor`；
- 不做大范围 agent runtime 重构。

## 下一步

当前版本保持现有运行时实现。

如果后续重新打开这个方向，建议从 Phase 1 开始：只为 task notification 引入 `RuntimeCommandQueue`，并先用测试证明 idle wake、active-loop drain、取消 re-enqueue、用户展示都保持正确。

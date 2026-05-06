# rules.md — task-notification 路由与注入测试意图

`<task-notification>` 是后台子代理完成后的回传信号。它必须按 SessionId 路由、在正确的位置注入、并且在取消时不丢失。

---

## 意图 1：会话 A 的 turn 只能消费会话 A 的 task-notification

**场景**
两个会话同时存在后台子代理完成通知时，A 会话的下一轮 turn 不能把 B 会话的通知抢走；B 的通知必须继续留在队列里，等待 B 自己的下一轮 turn。

**前提**
- 会话 A 的标识是 `session-a`
- 会话 B 的标识是 `session-b`
- 会话 A 的通知 XML 为：
  ```xml
  <task-notification>
    <task-id>agent-a-1</task-id>
    <status>completed</status>
    <summary>Agent A finished</summary>
  </task-notification>
  ```
- 会话 B 的通知 XML 为：
  ```xml
  <task-notification>
    <task-id>agent-b-1</task-id>
    <status>completed</status>
    <summary>Agent B finished</summary>
  </task-notification>
  ```
- 两个通知都已经入队，且分别标记自己的 session/run 归属

**操作**
1. 让会话 `session-a` 开始下一轮 turn
2. 读取该轮最终传给 LLM 的 `messages`
3. 再检查队列中剩余的通知

**断言**
- `session-a` 这一轮注入的 `<task-notification>` 只有 A 的 XML
- `session-a` 这一轮的 `messages` 中不出现 B 的 XML
- 队列中仍保留 B 的通知
- `session-b` 下一轮 turn 才能消费 B 的 XML

---

## 意图 2：notification 必须出现在当前 user_message 之后，而不是夹在 history 和 user_message 中间

**场景**
父代理开启新 turn 时，如果已有后台完成通知，系统必须把通知附着在当前用户输入之后。它不能插进历史消息和当前用户消息之间，否则模型会把它误当成上一轮上下文。

**前提**
- 会话标识是 `session-continue`
- 当前用户消息内容是 `请继续分析我刚刚说的这份表`
- 历史消息里已经有一条 assistant 内容 `上一轮已完成`
- 队列里已经有一条通知 XML：
  ```xml
  <task-notification>
    <task-id>agent-4a11</task-id>
    <status>completed</status>
    <summary>Agent done</summary>
  </task-notification>
  ```

**操作**
1. 开始这一轮 turn
2. 读取最终传给 LLM 的 `messages` 顺序

**断言**
- `messages` 中当前用户消息 `请继续分析我刚刚说的这份表` 出现在 notification 之前
- `<task-notification>` 不是 `history` 和当前用户消息之间的那条消息
- 如果 notification 作为独立消息注入，它的 `role` 必须是 `"user"`
- 如果 notification 被合并进当前用户消息，它必须出现在当前用户消息内容之后，而不是前面

---

## 意图 3：execute_round 后立刻取消时，已 drain 的 notification 必须重新入队

**场景**
LLM 已经开始处理本轮 tool round，但在 `execute_round` 之后、后续消费之前用户取消了对话。此时已经 drain 出来的 notification 不能被吞掉，下一轮 turn 仍要能看到它。

**前提**
- 会话标识是 `session-cancel-after-round`
- 队列中已有一条通知 XML：
  ```xml
  <task-notification>
    <task-id>agent-cancel-1</task-id>
    <status>completed</status>
    <summary>Round finished</summary>
  </task-notification>
  ```
- 本轮 LLM 会产生一次 tool round
- 取消点发生在 `execute_round` 返回之后、`resolve_permission_asks` 之前

**操作**
1. 运行这一轮 turn
2. 在 `execute_round` 之后触发取消
3. 让这一轮退出
4. 再开启下一轮 turn

**断言**
- 这一轮退出前没有把 notification 丢弃
- 下一轮 turn 还能再次 drain 到同一份 `agent-cancel-1` XML
- 第二轮看到的 XML 内容与第一轮入队的 XML 完全一致
- XML 中的 `task-id`、`status`、`summary` 三个字段都保持不变

---

## 意图 4：逐条 staged tool result 过程中取消时，已 drain 的 notification 也必须重新入队

**场景**
工具结果已经开始逐条写回 history，但在第一条或中途取消时，已注入但尚未确认消费的 notification 不能消失。

**前提**
- 会话标识是 `session-cancel-after-result`
- 队列中已有一条通知 XML：
  ```xml
  <task-notification>
    <task-id>agent-cancel-2</task-id>
    <status>completed</status>
    <summary>Tool result staged</summary>
  </task-notification>
  ```
- 本轮 LLM 产生至少 1 条 tool_result
- 取消点发生在第一条 staged tool result 写入 `history_batch` 之后、整轮完成之前

**操作**
1. 运行这一轮 turn
2. 在第一条 staged tool result 写入后触发取消
3. 让这一轮退出
4. 再开启下一轮 turn

**断言**
- 这一轮退出前没有把 notification 丢弃
- 下一轮 turn 还能再次 drain 到同一份 `agent-cancel-2` XML
- 第二轮看到的 XML 与第一轮一致
- 这一轮不会把 `agent-cancel-2` 变成一次性丢失的通知


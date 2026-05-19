# rules.md — chat-turn-boundary Chat Turn 主循环边界测试意图

来源：[LUT-5](mention://issue/646d0af5-f4ca-4a90-9774-ba041ca55a23)

---

## 意图 1：LLM 持续工具调用达到 max_iterations 时 turn 终止，工具调用记录落盘

**场景**
Agent 在执行过程中持续调用工具不给出最终回复，达到系统设定的最大轮次上限后，turn 主动终止。

**前提**
- 使用会触发持续工具调用的 prompt，如 `"请循环调用搜索工具，搜索'天气'，不要停止"`
- 系统 max_iterations 配置为较小的值（如 2）
- 新建对话，conversation_id 记录备用

**操作**
- 用户发送消息，等待 turn 结束

**验收标准**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值包含 `"MaxIterationsReached"`
- EventBus 中 `ToolCallExecuting` 事件出现次数等于 max_iterations 配置值
- EventBus 中最后一个事件类型为 `AgentIdle`，`scope` 字段值为 `"primary"`
- `messages.jsonl` 存在，文件行数 ≥ 2，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`

---

## 意图 2：用户点击停止后已流出内容落盘，StreamDone 不出现，后续可发新消息

**场景**
用户在 AI 流式输出过程中点击停止，已流出的内容被保存，对话可以继续使用。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"请详细介绍量子计算的原理，至少写500字"`（触发较长回复以便中途停止）
- 确认 AI 已开始流式输出

**操作**
- 用户发送消息
- 在 AI 流式输出过程中点击「停止」按钮

**验收标准**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Cancelled"`
- EventBus 中 `StreamDone` 不出现
- EventBus 中最后一个事件类型为 `AgentIdle`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"请详细介绍量子计算的原理，至少写500字"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 不为空（已流出内容被保存）
- 停止后在同一对话发送新消息 `"你好"`，新 turn 的 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`

---

## 意图 3：正常对话完成后消息落盘，事件序列完整

**场景**
用户正常发消息，AI 回复文本，turn 以 Success 结束，user 和 assistant 消息均写入存储。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"你好"`

**操作**
- 用户发送消息，等待 AI 完整回复

**验收标准**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`
- `StreamDone` 出现在 `TurnCompleted` 之前
- `TurnCompleted` 出现在 `AgentIdle` 之前
- `AgentIdle` 的 `scope` 字段值为 `"primary"`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"你好"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 不为空

---

## 意图 4：对话历史过长触发 compaction 后 turn 正常完成，assistant 消息落盘

**场景**
对话历史积累超出上下文窗口，系统自动触发 compaction 压缩历史后继续执行，用户感受到的是对话正常完成。

**前提**
- 已有包含大量历史消息的对话（历史 token 数接近 provider 上下文窗口的 80%）
- 用户消息为 `"请继续"`

**操作**
- 用户发送消息，等待 AI 完整回复

**验收标准**
- EventBus 中出现 `TurnStageChanged` 事件，`stage.kind` 字段值为 `"compacting"`
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`
- `messages.jsonl` 新增 1 行，`role` 字段值为 `"assistant"`，`content.text` 不为空

---

## 意图 5：前一轮对话报错后，下一轮正常发消息可正常收到回复

**场景**
上一轮对话因错误中止，不应污染下一轮的状态，用户重新发消息能正常收到 AI 回复。

**前提**
- 触发一次对话报错（如使用无效 key 发送消息，使 turn 以错误结束）
- 恢复有效 API key 配置
- 用户消息为 `"你好"`

**操作**
- 在同一对话发送新消息，等待 AI 完整回复

**验收标准**
- 新 turn 的 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`
- 前端正常展示 AI 回复，无错误提示
- `messages.jsonl` 新增 1 行，`role` 字段值为 `"assistant"`，`content.text` 不为空

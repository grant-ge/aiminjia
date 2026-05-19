# rules.md — streaming-e2e 流式输出端到端测试意图

来源：[LUT-7](mention://issue/77843043-e831-4c28-90be-7feced8a9fcc)

---

## 意图 1：正常对话完成后 StreamDelta 拼接内容与存储消息完全一致

**场景**
用户发消息，AI 流式输出回复。前端逐字展示的内容与磁盘存储的消息必须完全一致，任何一处不一致都会导致用户看到的与历史记录不同。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"请用一句话介绍你自己"`

**操作**
- 用户发送消息，等待 AI 完整回复

**验收标准**
- EventBus 中 `StreamStarted` 出现在所有 `StreamDelta` 之前
- EventBus 中所有 `StreamDelta` 事件的 `content` 字段按出现顺序拼接后与前端展示内容一致
- `StreamDone` 出现在最后一个 `StreamDelta` 之后，在 `TurnCompleted` 之前
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`
- EventBus 中最后一个事件类型为 `AgentIdle`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"请用一句话介绍你自己"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 不为空，且与前端展示内容完全一致

---

## 意图 2：流式输出中途停止后已输出内容落盘，StreamDone 不出现

**场景**
用户在 AI 流式输出一半时停止，已到达的内容不能丢失，StreamDone 不应出现（流未正常完成）。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"请详细介绍量子力学，至少写500字"`
- 确认 AI 已开始流式输出

**操作**
- 用户发送消息
- 在 AI 流式输出过程中点击「停止」按钮

**验收标准**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Cancelled"`
- EventBus 中 `StreamDone` 不出现
- EventBus 中最后一个事件类型为 `AgentIdle`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"请详细介绍量子力学，至少写500字"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 不为空（已流出内容被保存，不丢失）

---

## 意图 3：同一 turn 内所有 streaming 事件的 run_id 与发起请求一致

**场景**
前端通过 run_id 将流式 delta 归属到正确的 turn，run_id 不一致会导致内容追加到错误的对话气泡。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"你好"`
- 记录本次请求的 run_id

**操作**
- 用户发送消息，等待 AI 完整回复

**验收标准**
- EventBus 中所有 `StreamStarted`、`StreamDelta`、`StreamDone`、`TurnCompleted` 事件的 `run_id` 字段均相同，且等于本次请求的 run_id
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 不为空

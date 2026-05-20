# rules.md — llm-provider-routing LLM Provider 路由与降级测试意图

来源：[LUT-4](mention://issue/8fb70292-f4aa-4ec9-8c10-2ec6bcb05c76)

---

## 意图 1：API key 无效时用户看到错误提示，assistant 消息不落盘

**场景**
用户配置了一个无效的 API key，发消息后应看到错误提示，对话历史中不产生空的 assistant 消息。

**前提**
- 应用配置了一个已知无效的 API key（如 `sk-invalid-key-for-testing`）
- 新建对话，conversation_id 记录备用
- 用户消息为 `"你好"`

**操作**
- 用户发送消息，等待系统响应

**验收标准**
- 前端出现错误提示，提示内容包含认证失败相关字样（如「认证」「key」「401」之一）
- `messages.jsonl` 存在，文件共 1 行，该行 `role` 字段值为 `"user"`（无 assistant 消息写入）

---

## 意图 2：provider 限流时用户看到错误提示，assistant 消息不落盘

**场景**
provider 返回限流错误（429），用户应看到错误提示而非无限等待。

**前提**
- 使用已达到配额上限的 API key，或通过网络层模拟 429 响应
- 新建对话，用户消息为 `"你好"`

**操作**
- 用户发送消息，等待系统响应

**验收标准**
- 前端出现错误提示，提示内容包含限流相关字样（如「限流」「quota」「429」之一）
- `messages.jsonl` 存在，文件共 1 行，`role` 字段值为 `"user"`

---

## 意图 3：上下文过长时系统自动压缩历史后继续完成对话

**场景**
对话历史积累到超出上下文窗口，系统应自动触发 compaction 压缩历史后继续完成对话，用户感受到的是对话正常完成。

**前提**
- 已有一个包含大量历史消息的对话（历史消息 token 数接近或超过当前 provider 上下文窗口的 80%）
- 用户消息为 `"请继续"`

**操作**
- 用户发送消息，等待系统响应完成

**验收标准**
- 前端正常展示 AI 回复，无错误提示
- EventBus 中出现 `TurnStageChanged` 事件，`stage.kind` 字段值为 `"compacting"`
- `messages.jsonl` 新增 1 行，`role` 字段值为 `"assistant"`，`content.text` 不为空

---

## 意图 4：正常对话中 StreamDelta 拼接内容与存储消息完全一致

**场景**
用户发消息，前端通过流式 delta 逐字展示回复。流式内容与磁盘存储的消息必须完全一致，不能丢字或重复。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"请用一句话介绍你自己"`

**操作**
- 用户发送消息，等待 AI 完整回复

**验收标准**
- 前端逐字展示 AI 回复，无跳变或内容缺失
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 不为空且与前端展示内容一致

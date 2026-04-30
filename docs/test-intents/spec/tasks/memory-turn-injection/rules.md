# rules.md — memory Turn 注入层测试意图

Turn 注入层测试的是：一次用户消息进入 `RuntimeChatTurnDriver` 后，memory 如何被加载、选择、渲染，并传给 LLM。

---

## 意图 1：每个 turn 开始时，用当前用户消息作为 query 加载 project memory

**场景**
用户发送一条新消息，driver 需要用这条消息检索当前 workspace 的项目记忆。

**前提**
- executor 能记录 `load_project_memory(workspace_path, query)` 的调用参数
- ChatTurnRequest.content 为一个明确字符串，如 `"请继续分析薪资分布，优先用箱线图"`
- TurnConfig 中 workspace_path 已解析完成

**操作**
- 执行一次 `RuntimeChatTurnDriver::run_chat_turn()`

**断言**
- `load_project_memory()` 被调用 1 次
- 传入的 workspace_path 等于本 turn 的 workspace_path
- 传入的 query 等于 `ChatTurnRequest.content`
- 不使用历史消息、系统提示词或 dynamic context 作为 query

---

## 意图 2：project memory 命中时，被注入 dynamic_context 的 `[项目记忆]` 区块

**场景**
当前问题命中了项目记忆，LLM 应该在 dynamic_context 中看到这段记忆。

**前提**
- executor 的 `load_project_memory()` 返回非空 ProjectMemoryContext
- 该 context 的 `render_for_prompt()` 包含一条可识别内容，如 `薪资分析偏好箱线图`

**操作**
- 执行一次 turn，并捕获 `run_llm_step()` 收到的 `LlmStepInput.dynamic_context`

**断言**
- dynamic_context 包含 `[项目记忆]`
- dynamic_context 包含 `薪资分析偏好箱线图`
- dynamic_context 保留 `[动态上下文 — 请勿回复此消息]` 开头
- project memory 出现在 workspace/env info 之前（保持 context_builder 的拼接顺序）

---

## 意图 3：project memory 不混入 messages 历史

**场景**
项目记忆是动态上下文，不应该伪装成用户消息或助手消息进入 message history。

**前提**
- project memory context 非空，内容含 `薪资分析偏好箱线图`
- executor 能捕获 `run_llm_step()` 收到的 `LlmStepInput.messages`

**操作**
- 执行一次 turn

**断言**
- `dynamic_context` 包含 `薪资分析偏好箱线图`
- `messages` 中所有 message 的 content 拼接后不包含 `[项目记忆]`
- `messages` 中所有 message 的 content 拼接后不包含 `薪资分析偏好箱线图`

---

## 意图 4：project memory 为空时才回退加载 legacy core memory

**场景**
新版项目记忆为空，为兼容旧版本，driver 才加载 legacy core memory。

**前提**
- executor 的 `load_project_memory()` 返回空 ProjectMemoryContext
- executor 的 `load_core_memory()` 返回 `旧核心记忆内容`

**操作**
- 执行一次 turn，并捕获 dynamic_context

**断言**
- `load_project_memory()` 被调用 1 次
- `load_core_memory(conversation_id)` 被调用 1 次
- dynamic_context 包含 `[核心记忆]`
- dynamic_context 包含 `旧核心记忆内容`
- dynamic_context 不包含 `[项目记忆]`

---

## 意图 5：project memory 非空时不再加载 legacy core memory

**场景**
新版项目记忆已经可用，旧核心记忆不应再进入上下文，避免重复和污染。

**前提**
- executor 的 `load_project_memory()` 返回非空 ProjectMemoryContext
- executor 的 `load_core_memory()` 如果被调用会记录次数

**操作**
- 执行一次 turn

**断言**
- `load_project_memory()` 被调用 1 次
- `load_core_memory()` 被调用 0 次
- dynamic_context 包含 `[项目记忆]`
- dynamic_context 不包含 `[核心记忆]`

---

## 意图 6：多轮工具调用中 project memory 只在 turn 开始加载一次

**场景**
一次 turn 里 LLM 可能经历多轮 tool calls，但 memory 应该是 turn 级快照，不应每轮重新搜索。

**前提**
- executor 预设 LLM 响应：多次 ToolCalls + 最后一次 ContentComplete
- executor 记录 `load_project_memory()` 调用次数
- executor 记录每次 `run_llm_step()` 收到的 dynamic_context

**操作**
- 执行一次多轮 turn

**断言**
- `load_project_memory()` 总调用次数为 1
- 每一轮 `run_llm_step()` 的 dynamic_context 都包含同一份 `[项目记忆]` 内容
- 后续轮次不会因为工具调用结果而重新检索 project memory

---

## 意图 7：load_project_memory 失败时不阻断 turn

**场景**
memory 读取失败不应该导致用户对话失败；driver 应该降级为空 memory。

**前提**
- executor 的 `load_project_memory()` 返回错误
- executor 的 `load_core_memory()` 返回空字符串或成功值
- executor 的 `run_llm_step()` 正常返回 ContentComplete

**操作**
- 执行一次 turn

**断言**
- turn 整体执行成功
- `run_llm_step()` 仍然被调用
- dynamic_context 不包含 project memory 失败错误文本
- 如果 core memory 也为空，dynamic_context 不包含 `[项目记忆]` 和 `[核心记忆]`

---

## 意图 8：project memory 渲染内容为空时视为空上下文

**场景**
ProjectMemoryContext 没有 index_text，也没有 recalled_entries，不能注入空的 `[项目记忆]` 标题。

**前提**
- executor 的 `load_project_memory()` 返回默认空 ProjectMemoryContext
- executor 的 `load_core_memory()` 返回空字符串

**操作**
- 执行一次 turn

**断言**
- dynamic_context 不包含 `[项目记忆]`
- dynamic_context 不包含 `[核心记忆]`
- dynamic_context 仍然包含 `[动态上下文 — 请勿回复此消息]`

---

## 意图 9：project memory 与 RENLIJIA.md / env_info 保持独立区块

**场景**
一次 turn 同时有项目记忆、RENLIJIA.md、环境信息时，它们需要保持可区分的上下文边界。

**前提**
- project memory 内容包含 `薪资分析偏好箱线图`
- RENLIJIA.md 内容包含 `项目指令内容`
- env_info 内容包含 `# env_info`

**操作**
- 执行一次 turn，并捕获 dynamic_context 与 messages

**断言**
- dynamic_context 包含 `[项目记忆]` 与 `薪资分析偏好箱线图`
- dynamic_context 包含 `# env_info`
- messages 中包含 RENLIJIA.md 注入的 `项目指令内容`
- project memory 不出现在 RENLIJIA.md message 中
- RENLIJIA.md 内容不出现在 `[项目记忆]` 区块中

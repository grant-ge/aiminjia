# rules.md — subagent 执行行为测试意图

WorkerRuntime 执行行为：父对话取消级联、文件缓存隔离、权限 Ask 冒泡、迭代上限、结果 envelope 格式。

---

## 意图 1：父对话取消后，子代理必须随之停止

**场景**
用户取消了正在进行的父对话，子代理不能继续独立运行，否则会产生无法回收的资源占用。

**前提**
- 父对话有一个 CancellationToken（父 token）
- 子代理启动时从父 token 派生出独立的子 token

**操作**
1. 确认子 token 初始状态：is_cancelled() == false
2. 触发父 token 取消
3. 再次检查子 token 状态

**断言**
- 父 token 取消前，子 token 的 `is_cancelled()` 返回 `false`
- 父 token 取消后，子 token 的 `is_cancelled()` 返回 `true`

---

## 意图 2：子代理的文件读取记录与父代理完全隔离

**场景**
父代理和子代理都可能读取文件，但它们各自的文件状态缓存应该互不干扰——子代理的行为不能污染父代理的记录。

**前提**
- 父代理的 FileStateCache 已记录文件 `file_a.csv`（模拟父代理已读取该文件）
- 子代理从父 cache 派生出独立 child cache
- 子代理的 child cache 中记录文件 `file_b.csv`

**操作**
- 检查父 cache 中是否包含 `file_b.csv`
- 检查 child cache 中是否包含 `file_a.csv`

**断言**
- 父 cache 不包含 `file_b.csv`（子代理新增的文件不污染父代理）
- child cache 包含 `file_a.csv`（继承父代理已有记录）
- 两个 cache 是不同的对象引用

---

## 意图 3：子代理工具执行时能感知自己处于子代理上下文

**场景**
某些工具行为在子代理中需要有所不同（如日志标记、权限范围）。工具执行时要能知道"我现在是在子代理里运行的"，以及当前子代理的 ID。

**前提**
- 通过完整的 spawn 流程创建子代理（不手动构造 CapabilityContext）
- spawn 时得到 child_agent_id

**操作**
- 从子代理的 ToolExecutionContext 读取 capability 字段
- 读取 `capability.agent_id` 和 `capability.is_subagent`

**断言**
- `capability.is_subagent` 为 `true`
- `capability.agent_id` 等于 spawn 时生成的 child_agent_id

---

## 意图 4：子代理达到最大迭代次数时输出固定提示并正常返回

**场景**
子代理不能无限循环。达到 max_iterations 后应给出说明性输出，而不是 panic 或空结果。

**前提**
- `max_iterations = 2`
- Mock LLM 每次都返回 ToolUse 响应（工具名 `dummy_tool`，无有效内容），迫使循环继续
- Mock 工具执行始终成功返回空内容

**操作**
- 执行子代理 run()

**断言**
- 返回 `Ok(SubAgentResult)`，不返回 Err
- `result.output` 包含字符串 `"Sub-agent reached iteration limit."`
- `result.iterations_used == 2`

---

## 意图 5：子代理被取消时输出取消提示并正常返回

**场景**
用户取消操作后，子代理应安全退出并说明原因，不能挂起或 panic。

**前提**
- 子代理的 CancellationToken 在第一轮 LLM 调用开始前被触发取消

**操作**
- 执行子代理 run()

**断言**
- 返回 `Ok(SubAgentResult)`，不返回 Err
- `result.output` 包含字符串 `"Sub-agent cancelled."`

---

## 意图 6：工具权限 Ask 被冒泡为错误返回给父代理

**场景**
子代理执行某工具时触发了权限确认（Ask），这个确认请求不能在子代理内部消化——子代理没有能力展示 UI 让用户确认，必须向上抛给父代理处理。

**前提**
- 子代理调用的某个工具返回 AskRequired 结果
- 该工具名为 `mcp__demo__action`

**操作**
- 执行子代理 run()

**断言**
- run() 返回 `Err`，错误类型为 `LegacyToolError::AskRequired`
- AskRequired 携带的 decision 信息中可以提取到工具名 `mcp__demo__action`

---

## 意图 7：结果 envelope 包含完整的输出、迭代数、文件列表、转录快照

**场景**
子代理完成后，父代理需要从 envelope 中提取输出内容和执行摘要。

**前提**
- 子代理设置 `max_iterations = 3`
- Mock LLM 第 1 轮返回 ToolUse，第 2 轮返回 ContentComplete，输出 `"分析完成"`
- Mock 工具第 1 轮执行成功，生成文件路径 `reports/result.md`

**操作**
- 执行子代理 run()，读取返回的 SubAgentResultEnvelope

**断言**
- `envelope.schema_version == 1`
- `envelope.output == "分析完成"`
- `envelope.iterations_used == 2`
- `envelope.generated_files` 包含 `"reports/result.md"`
- `envelope.generated_files` 已去重（无重复路径）
- `envelope.transcript_snapshot.len() <= 16`
- `envelope.transcript_ref` 以 `"subagent://"` 开头

---

## 意图 8：envelope 可序列化为 storage_summary 并能反序列化还原

**场景**
envelope 需要持久化到 invocation store 的 summary 字段，格式必须可逆。

**前提**
- 构造一个 SubAgentResultEnvelope：`output = "test output"`，`schema_version = 1`，`iterations_used = 3`

**操作**
- 调用 `envelope.to_storage_summary()`
- 调用 `SubAgentResultEnvelope::from_storage_summary(&summary)`

**断言**
- `to_storage_summary()` 返回值以 `"subagent-envelope:v1:"` 开头
- `from_storage_summary()` 返回 `Some`，不返回 `None`
- 还原后 `envelope.output == "test output"`
- 还原后 `envelope.schema_version == 1`
- 还原后 `envelope.iterations_used == 3`

---

## 意图 9：子代理完整转录条目数与消息轮次严格对应

**场景**
主对话需要能读取完整的子代理对话记录，条目数量必须与实际消息轮次一致，不能有遗漏。

**前提**
- Mock LLM 预设：第 1 轮返回 ToolUse（1 条 assistant 消息 + 1 条 tool_result）、第 2 轮返回 ContentComplete（1 条 assistant 消息）
- 初始 1 条 user 消息
- AgentRuntime 使用 InMemorySubagentTranscriptStore

**操作**
- 执行子代理 run()
- 通过 `AgentRuntime::load_transcript(child_run_id)` 读取转录

**断言**
- 转录条目共 4 条（1 user + 1 assistant tool_use + 1 tool_result + 1 assistant final）
- 第 1 条 `role == "user"`
- 最后 1 条 `role == "assistant"`
- `transcript_ref` 格式为 `"subagent://<child_run_id>"`
- transcript_snapshot 中最后一条 `role == "assistant"`（保留最新的）

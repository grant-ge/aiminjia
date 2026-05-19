# rules.md — async 子代理生命周期顺序测试意图

后台子代理的承诺不只是“能跑起来”，还包括：catalog 文案不能撒谎、终态出现时 transcript 已经可读、写盘失败不能静默吞掉。

---

## 意图 1：spawn_subagent 的 catalog 文案必须如实描述 async 行为

**场景**
父 LLM 通过工具说明决定要不要用 `run_in_background=true`。如果 catalog 还写着“暂未实现”或“not_implemented_yet”，模型会被误导成不用 async 路径。

**前提**
- 工具名是 `spawn_subagent`
- 工具参数包含：
  - `subagent_type = "explore"`
  - `prompt = "找出 Cargo.toml 里的 package 名称"`
  - `description = "explore"`
  - `run_in_background = true`

**操作**
- 读取 `spawn_subagent` 的工具说明文本和参数说明文本

**验收标准**
- 工具说明中包含 `"async"` 或 `"run_in_background"`
- 工具说明中包含 `"agent_id"` 或 `"立即返回"`
- 工具说明中不包含 `"not_implemented_yet"`
- `run_in_background` 的说明中不包含 `"暂未实现"`

---

## 意图 2：当 async 子代理状态变为 Completed 时，它的 transcript 已经可读，notification 也已经入队

**场景**
后台子代理完成后，父代理看到的终态必须和可观察结果一致：state 已经是 Completed，`task_output` 已经能读到 transcript，notification 队列里也已经有那条完成通知。

**前提**
- async 子代理的 `agent_id = "agent-7c1d"`
- async 子代理的 `name = "w1"`
- transcript 文件名固定为 `users/t_test__u_test/subagent_transcripts/agent-7c1d.jsonl`
- transcript 文件里最终写入的一行是：
  `{"role":"assistant","content":"完成内容"}`
- 对应的 `<task-notification>` XML 为：
  ```xml
  <task-notification>
    <task-id>agent-7c1d</task-id>
    <status>completed</status>
    <summary>Agent 'explore' completed</summary>
    <result>完成内容</result>
  </task-notification>
  ```

**操作**
1. 让 async launcher 完成这一条任务
2. 观察 task store 的 state
3. 立即查询 task_id 为 agent-7c1d 的任务输出，起始偏移量为 0
4. 检查 notification queue

**验收标准**
- `AsyncTaskState` 的序列化值是 `"completed"`
- 查询 task_id 为 agent-7c1d、起始偏移量为 0 的任务输出，返回的 `lines` 正好是 1 条
- 这 1 条 `lines[0]` 正好等于 `{"role":"assistant","content":"完成内容"}`
- 任务输出返回的 `new_offset` 字段值为 1
- notification queue 中已经有 1 条与 `agent-7c1d` 对应的通知

---

## 意图 3：transcript 写失败时必须可观察，warning 里要带 agent_id

**场景**
后台子代理写 transcript 失败时，系统不能悄悄吞掉这个错误。即使继续完成流程，也必须留下可观察的 warning，便于定位为什么父代理读不到 task_output。

**前提**
- async 子代理的 `agent_id = "agent-9f9f"`
- async 子代理的 `name = "w2"`
- transcript 写入阶段返回一个明确的 I/O 错误
- 写入失败发生在终态处理路径中

**操作**
1. 让 async launcher 走到 transcript append 失败
2. 捕获日志输出

**验收标准**
- 日志中出现 warning 级别记录
- 该 warning 中包含字符串 `"agent-9f9f"`
- 该 warning 中包含 transcript 文件路径或 `output_file`
- 该 warning 不能只是空白的吞错


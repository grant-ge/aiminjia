# rules.md — subagent 生命周期测试意图

AgentRuntime 管理子代理的状态机：从 spawn 到 complete/cancel/fail，以及转录持久化和恢复。

---

## 意图 1：spawn 后子代理状态为 Running，并持有独立 ID

**场景**
主对话启动一个子代理任务，系统需要为它分配独立的 agent_id 和 child_run_id。

**前提**
- 构造 SpawnChildRunRequest，指定 parent_run_id

**操作**
- 调用 `AgentRuntime::spawn_child_run()`

**断言**
- 返回的 ChildRunHandle 包含非空的 agent_id
- 返回的 ChildRunHandle 包含非空的 child_run_id
- child_run_id 与 parent_run_id 不相同
- 立刻查询 status，返回 `"running"`

---

## 意图 2：complete 后状态变为 Completed

**场景**
子代理完成任务，主对话需要知道它已结束。

**前提**
- 已 spawn 一个子代理，状态为 Running

**操作**
- 调用 `AgentRuntime::complete_run(child_run_id)`
- 查询 status

**断言**
- status 返回 `"completed"`
- 不返回 `"running"` 或其他状态

---

## 意图 3：cancel 后状态变为 Cancelled

**场景**
用户中止操作，父 run 需要取消正在运行的子代理。

**前提**
- 已 spawn 一个子代理，状态为 Running

**操作**
- 调用 `AgentRuntime::cancel_run(child_run_id)`
- 查询 status

**断言**
- status 返回 `"cancelled"`

---

## 意图 4：fail 后状态变为 Failed

**场景**
子代理执行过程中遇到不可恢复的错误。

**前提**
- 已 spawn 一个子代理，状态为 Running

**操作**
- 调用 `AgentRuntime::fail_run(child_run_id)`
- 查询 status

**断言**
- status 返回 `"failed"`

---

## 意图 5：查询不存在的 child_run_id 返回 missing

**场景**
主对话查询一个从未创建过的子代理。

**前提**
- AgentRuntime 为空

**操作**
- 调用 `status(RunId::new("nonexistent"))` 查询任意不存在的 run_id

**断言**
- 返回 `"missing"`，不报错

---

## 意图 6：background 子代理完成后发出 AgentIdle 事件

**场景**
后台子代理完成时，前端需要收到通知，以便显示任务结束。

**前提**
- 已 spawn 一个 background = true 的子代理
- 构造 RuntimeEventBus，收集事件

**操作**
- 调用 `complete_background_run(child_run_id, summary, transcript_ref, session_id, parent_run_id, bus)`

**断言**
- EventBus 中包含至少一个事件
- 该事件类型为 AgentIdle（通过 event_labels() 验证包含 `"AgentIdle"`）
- 子代理状态变为 `"completed"`

---

## 意图 7：子代理转录在完成后可按 transcript_ref 读取

**场景**
主对话需要回放子代理的完整对话记录，用于调试或结果提取。

**前提**
- 构造若干 SubagentTranscriptEntryRecord（至少含 user 和 assistant 各一条）

**操作**
- 调用 `store_transcript(transcript_ref, entries)`
- 调用 `transcript_store_get(transcript_ref)`

**断言**
- 返回值非 None
- 返回的 entries 数量与写入时一致
- 每条 entry 的 role 和 content 与写入时完全一致

---

## 意图 8：通过 child_run_id 关联读取 transcript_ref

**场景**
主对话只知道 child_run_id，需要通过它找到 transcript_ref，再读取完整转录。

**前提**
- 已 spawn 子代理
- 调用 `complete_background_run` 时传入了 transcript_ref

**操作**
- 调用 `get_transcript_ref(child_run_id)` 获取 transcript_ref
- 用 transcript_ref 读取转录内容

**断言**
- `get_transcript_ref()` 返回非 None
- 用返回的 transcript_ref 读取到完整转录条目

---

## 意图 9：resume 后能恢复已有 invocation 的 handle

**场景**
子代理在某些情况下需要恢复（如应用重启后继续追踪已知子代理状态）。

**前提**
- 已 spawn 一个子代理，保存 agent_id
- 构造 ResumeChildRunRequest(agent_id)

**操作**
- 调用 `resume_child_run(request)`

**断言**
- 返回的 ChildRunHandle 的 agent_id 与 spawn 时一致
- 返回的 ChildRunHandle 的 child_run_id 与 spawn 时一致

---

## 意图 10：resume 不存在的 agent_id 时返回错误

**场景**
传入一个从未 spawn 过的 agent_id 尝试 resume。

**操作**
- 调用 `resume_child_run(ResumeChildRunRequest::new("nonexistent-agent"))`

**断言**
- 返回 Err，不 panic

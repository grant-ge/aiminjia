# rules.md — tool round 并发策略测试意图

`ToolRoundDriver` 只能并发真正安全的工具，不能把所有工具一股脑并发。安全工具可以并发，非安全工具必须串行。

---

## 意图 1：两个 concurrency-safe 工具可以同时执行，最大并发数为 2

**场景**
一轮 LLM 返回两个都标记为 concurrency-safe 的工具调用时，系统应该允许它们并发执行，而不是强制串行。否则父代理会把本可并行的工作慢一倍。

**前提**
- 注册两个 RuntimeTool：
  - `safe_tool_a`
  - `safe_tool_b`
- 这两个工具都返回 `is_concurrency_safe = true`
- 它们各自的输入固定为：
  - `safe_tool_a` 的 input 是 `{ "label": "A" }`
  - `safe_tool_b` 的 input 是 `{ "label": "B" }`
- 两个工具都会记录自己的 start / finish 时刻，并共享一个 inflight 计数器
- LLM 在同一轮里返回两个 tool call，顺序为：
  1. `safe_tool_a`
  2. `safe_tool_b`

**操作**
1. 执行这一轮 tool round
2. 读取两个工具的开始时间、结束时间和最大 inflight 计数

**断言**
- `safe_tool_a` 和 `safe_tool_b` 的执行区间有重叠
- 最大 inflight 计数等于 `2`
- 两个工具都返回成功结果
- `ToolCallCompleted` 事件各出现 1 次

---

## 意图 2：一安全一非安全时，非安全工具必须串行执行，最大并发数为 1

**场景**
如果同一轮里同时出现一个 safe 工具和一个 unsafe 工具，系统不能把它们并发起来。非安全工具必须等前一个结束后再开始。

**前提**
- 注册两个 RuntimeTool：
  - `safe_tool_a`
  - `unsafe_tool_c`
- `safe_tool_a` 返回 `is_concurrency_safe = true`
- `unsafe_tool_c` 返回 `is_concurrency_safe = false`
- 两个工具都记录自己的 start / finish 时刻，并共享一个 inflight 计数器
- LLM 在同一轮里返回两个 tool call，顺序为：
  1. `safe_tool_a`
  2. `unsafe_tool_c`

**操作**
1. 执行这一轮 tool round
2. 读取两个工具的开始时间、结束时间和最大 inflight 计数

**断言**
- `unsafe_tool_c` 的开始时间晚于 `safe_tool_a` 的结束时间，或者两者没有重叠
- 最大 inflight 计数等于 `1`
- 两个工具都返回成功结果
- `unsafe_tool_c` 不会在 `safe_tool_a` 还在运行时启动


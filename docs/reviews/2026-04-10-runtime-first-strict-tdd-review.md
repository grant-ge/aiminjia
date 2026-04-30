# 2026-04-10 / 2026-04-11 Runtime-First 严格 TDD Review

## 2026-04-12 关闭结论

- 状态：已关闭
- 说明：2026-04-11 这份 review 中追加的行为级 findings，已在后续修复中关闭

2026-04-12 复验证据：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test --test tauri_event_adapter_test -- --nocapture
cargo test review_ --tests --no-fail-fast
```

结论：

- 这份文档保留为历史 review 记录
- 文档中的 finding 用于说明当时暴露过什么问题
- 但这些 finding 已不再阻塞当前 runtime-first 核心验收

## 背景与范围

这次 review 只针对 `lotus-app` 的 runtime-first 迁移核心，不做全仓泛扫。

覆盖范围：

- 入口与宿主适配：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat.rs`
- 运行时主链路：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/session_runtime.rs`、`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/query_engine.rs`
- 事件兼容层：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs`
- 任务/状态通知：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/task/task_runtime.rs`
- 相关 review/TDD 测试：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_*.rs`

不纳入本轮正式 finding 的范围：

- provider 细节：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/providers/*`
- 前端 React hooks / UI 层
- 与 runtime-first 迁移无直接关系的历史模块

## 2026-04-11 复查结论

这轮是对 Claude 声称“已修复”的代码做二次严格复查，不是静态扫代码。

先复跑了上一轮 4 个新增 review tests，再复跑整套 `review_`：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test review_ --tests --no-fail-fast
```

复查结果分两部分：

- Claude 已经把上一轮锁定的 4 个问题修掉了：
  - `review_transport_runtime_bus_wiring_test`
  - `review_task_terminal_notification_mapping_test`
  - `review_task_runtime_event_emission_test`
  - `review_sub_agent_background_caller_wiring_test`
- 但在补了 3 个更强的行为级 TDD review tests 之后，当前仍有 3 个真实问题未修完：
  - `review_message_updated_payload_compatibility_test`
  - `review_runtime_executor_duplicate_events_test`
  - `review_task_status_event_context_test`

也就是说：

- 第一层“有无接线 / 有无枚举 / 有无字段”的修复已经到位
- 第二层“真实运行语义 / 兼容契约 / 真正归属上下文”的问题还在

## 本轮新增的行为级 review tests

为了避免“字符串包含即算修好”的伪通过，本轮新增并保留了 3 个行为级测试：

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_message_updated_payload_compatibility_test.rs`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_runtime_executor_duplicate_events_test.rs`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_task_status_event_context_test.rs`

这些测试不是临时脚本，会作为正式 review/TDD 证据保留。

## 当前命令与结果

### 单独复跑已转绿的旧问题

```bash
cargo test --test review_transport_runtime_bus_wiring_test -- --nocapture
cargo test --test review_task_terminal_notification_mapping_test -- --nocapture
cargo test --test review_task_runtime_event_emission_test -- --nocapture
cargo test --test review_sub_agent_background_caller_wiring_test -- --nocapture
```

结果：全部通过。

### 当前红灯命令

```bash
cargo test --test review_message_updated_payload_compatibility_test -- --nocapture
cargo test --test review_runtime_executor_duplicate_events_test -- --nocapture
cargo test --test review_task_status_event_context_test -- --nocapture
cargo test review_ --tests --no-fail-fast
```

结果：整套 `review_` 当前只剩 3 个红灯，上面 3 个测试均可稳定复现。

## Review 方法

这次 review 继续按“真实正向使用流转”验证，而不是按文件静态扫：

1. `send_message -> transport adapter -> SessionRuntime -> QueryEngine -> RuntimeEventBus -> TauriEventAdapter`
2. `executor-backed transport -> SessionRuntime -> legacy executor`
3. `TaskRuntime -> RuntimeEventBus -> TauriEventAdapter -> host/UI`

每条链路都按同一套标准检查：

- 真实业务正确性
- TDD 是否真正约束行为，而不是只约束字符串存在
- 兼容事件是否还能满足旧 UI/宿主契约
- 运行时身份与状态归属是否来自真实 run/session，而不是伪值

## Findings

### Finding 1 - Runtime `message:updated` 兼容事件仍不满足旧 UI 契约

- 严重级别：`P1`
- 影响范围：runtime-only transport、消息 upsert、前端兼容层
- 真实使用路径：`send_message -> SessionRuntime -> QueryEngine -> RuntimeEventBus -> TauriEventAdapter -> message:updated`

#### 问题描述

`TauriEventAdapter` 当前把 `RuntimeEventKind::MessagePersisted` 映射成：

- `conversationId`
- `messageId`
- `runId`

但 legacy `message:updated` 事件的最小可用契约至少包含：

- `id`
- `conversationId`
- `role`
- `content`

当前 runtime adapter 虽然会发 `message:updated`，但 payload 不足以让 UI 直接 upsert 一条 assistant 消息。

#### 为什么这是 bug / 设计缺陷

- 这不是“字段少一点也没关系”的问题，而是 runtime-only path 根本不能复刻旧宿主契约
- UI 如果只收到 `messageId`，并不知道消息内容、角色，也无法直接落消息
- 这说明当前 runtime 事件模型还没有真正承接 legacy 消息事件的语义

#### 代码定位

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs:60`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs:66`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/events.rs:28`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/events.rs:30`

#### 现有测试为什么没拦住

已有测试只验证“事件名有无发出”，没有验证 payload 兼容性：

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/send_message_runtime_path_test.rs:4`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_transport_runtime_bus_wiring_test.rs:2`

它们证明了 runtime 会发 `message:updated`，但没有证明这个事件仍然可被旧 UI 使用。

#### 最小复现方式

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test --test review_message_updated_payload_compatibility_test -- --nocapture
```

对应复现测试：

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_message_updated_payload_compatibility_test.rs:4`

当前失败点：

- payload 里没有 legacy 需要的 `id`
- 随后也会继续卡在缺少 `role` / `content`

#### 修复建议

- 不要把 `MessagePersisted` 继续建模成“只有 message_id 的轻事件”
- 要么扩展 runtime event，让它携带 assistant message 的最小渲染载荷
- 要么让 adapter 能从持久化层补全 legacy 事件 payload，但这个补全过程必须有明确真相源

#### 通过条件

- `review_message_updated_payload_compatibility_test` 转绿
- runtime-only transport 下，`message:updated` payload 至少包含 `id`、`conversationId`、`role`、`content`
- 不再依赖 legacy executor 的额外事件去补齐消息内容

---

### Finding 2 - executor-backed 生产链路仍会双发 legacy chat 事件

- 严重级别：`P1`
- 影响范围：生产聊天链路、流式 UI、消息重复、事件审计
- 真实使用路径：`TauriChatCommandAdapter::send_message() -> SessionRuntime::run_chat_request() -> QueryEngine` 先发一轮 legacy 兼容事件，随后 legacy executor 再发一轮自己的 legacy 事件

#### 问题描述

`SessionRuntime::run_chat_request()` 当前顺序是：

1. 先创建 `TurnState`
2. 调 `self.run_turn(&mut turn)`，通过 bus + adapter 发一轮 legacy 兼容事件
3. 如果配置了 `turn_executor`，再调用 `executor.run_chat_turn(...)`

只要 executor 本身也负责真实 streaming / message emit，这条链路就会出现一份 runtime 兼容事件 + 一份 legacy executor 事件，形成重复输出。

#### 为什么这是 bug / 设计缺陷

- 生产 transport 现在不是“runtime 驱动，legacy 只兜底”，而是“两套发射器同时生效”
- UI 会看到重复 `streaming:delta`、重复 `message:updated`、重复 `streaming:done`
- 这会掩盖掉 runtime-first 的真实边界：到底谁才是 chat 事件的唯一来源并不清楚

#### 代码定位

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/session_runtime.rs:79`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/session_runtime.rs:96`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat.rs:150`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2382`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:3459`

#### 现有测试为什么没拦住

现有测试分别只验证了半边：

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/session_runtime_executor_test.rs:22` 只验证 executor 收到请求
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_runtime_executor_bypass_test.rs:20` 只验证 runtime 自己会记录事件

但没有任何测试验证“当两边都能发 legacy 事件时，生产链路是否会重复发”。

#### 最小复现方式

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test --test review_runtime_executor_duplicate_events_test -- --nocapture
```

对应复现测试：

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_runtime_executor_duplicate_events_test.rs:48`

当前失败输出：

- 实际事件：`["streaming:delta", "message:updated", "streaming:done", "streaming:delta", "message:updated", "streaming:done"]`
- 期望事件：`["streaming:delta", "message:updated", "streaming:done"]`

#### 修复建议

- 明确 chat 事件的唯一来源
- 如果 executor 仍是生产真流路径，就不要在 `run_chat_request()` 里先跑一轮 runtime chat happy path
- 如果 runtime 要接管生产聊天，就让 executor 退化成 provider/tool/backing capability，而不是继续自己发 legacy UI 事件

#### 通过条件

- `review_runtime_executor_duplicate_events_test` 转绿
- executor-backed 生产路径只出现一轮 legacy chat 事件
- 能明确回答“谁拥有 streaming/message terminal signal 的唯一发射权”

---

### Finding 3 - Task terminal notification 仍然使用伪造的 run 上下文

- 严重级别：`P1`
- 影响范围：task terminal notification、宿主归因、后台任务/UI 关联
- 真实使用路径：`TaskRuntime::set_status() -> RuntimeEventBus -> TauriEventAdapter -> task:status-changed`

#### 问题描述

`TaskRuntime::set_status()` 虽然已经开始发 `TaskStatusChanged`，但它构造 `RuntimeEvent` 时仍硬编码：

- `SessionId::new("task-runtime")`
- `RunId::new("task-runtime")`

这会让真正的 `task:status-changed` payload 带着假的 `runId` / `conversationId` 出去，而不是任务所属的 parent run。

#### 为什么这是 bug / 设计缺陷

- 这不是“小细节”问题，而是 task terminal signal 没有真实归属上下文
- host/UI 无法把 task 完成或失败关联回真正的 run
- 这等于 task 状态事件虽然“存在”，但仍然不是可用的生产语义

#### 代码定位

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/task/task_runtime.rs:50`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/task/task_runtime.rs:67`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs:76`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/task/task_models.rs:13`

#### 现有测试为什么没拦住

原有 task tests 只证明“有事件”“有映射”，没证明事件上下文正确：

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_task_runtime_event_emission_test.rs:2`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_task_terminal_notification_mapping_test.rs:2`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_task_terminal_state_test.rs:11`

它们没有验证 `runId` 是否来自真实 `parent_run_id`。

#### 最小复现方式

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test --test review_task_status_event_context_test -- --nocapture
```

对应复现测试：

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_task_status_event_context_test.rs:10`

当前失败断言：

- 实际 `runId`：`task-runtime`
- 期望 `runId`：`run-parent-ctx`

#### 修复建议

- `TaskRuntime` 发事件时必须拿到真实任务上下文，而不是硬编码伪值
- 最低限度要能从 `TaskRecord.parent_run_id` 还原真实 `runId`
- 如果还要兼容 `conversationId`，则需要能通过 `RunStore` 找到 `RunRecord.session_id`
- 这意味着 task runtime 不能只依赖 `TaskStore`，还需要一个明确的 run/session 真相源

#### 通过条件

- `review_task_status_event_context_test` 转绿
- `task:status-changed` payload 使用真实 `parent_run_id`
- 事件 payload 不再出现 `task-runtime` 这类伪造上下文

## TDD 覆盖评估

当前这轮复查可以明确得出一个结论：

- 第一层 review tests 主要覆盖“接线有没有做、枚举有没有加、字段名有没有出现”
- 第二层更强的行为级 tests 才覆盖“真实链路是不是可用、兼容契约是不是成立、归属上下文是不是真实”

因此现在不能说“因为前一轮 tests 都绿了，所以迁移核心就没问题”。

更准确的结论是：

- Claude 已修掉一批表层 wiring 问题
- 但还没修完 runtime-first 迁移里最关键的语义问题

## 总体结论

截至 2026-04-11，这轮复查的权威结论是：

- 上一轮锁定的 4 个问题已经修复
- 当前补出的 3 个更强 TDD review tests 仍然失败
- 所以现在还不能判定 runtime-first 迁移核心“已经没有问题”

当前最值得继续让 Claude 修的，不是再补字符串级测试，而是先把下面 3 件事修到行为正确：

1. 让 runtime `message:updated` 真正满足 legacy UI 契约
2. 让 executor-backed 生产聊天路径只保留一份事件发射源
3. 让 task terminal notification 带真实 run/session 上下文

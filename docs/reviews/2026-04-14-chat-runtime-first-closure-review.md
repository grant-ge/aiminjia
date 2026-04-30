# 2026-04-14 Chat Runtime-First Closure Review

状态：**✅ 已关闭（2026-04-14）**  
评审对象：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-13-chat-runtime-first-closure-plan.md` 及 Claude 本轮代码实现  
评审范围：`SessionRuntime`、`runtime/chat/*`、`transport/tauri_commands/chat/*`、真实 `send_message` 主链路、对应 targeted TDD

## 验证基线

本轮复审实际核对并运行了以下测试：

- `cargo test --manifest-path src-tauri/Cargo.toml --test chat_runtime_first_mainline_test -- --nocapture` ✅
- `cargo test --manifest-path src-tauri/Cargo.toml --test review_runtime_executor_bypass_test -- --nocapture` ✅
- `cargo test --manifest-path src-tauri/Cargo.toml --test chat_runtime_dispatcher_production_path_test -- --nocapture` ✅

结论先行：

- 当前代码**已经把 runtime chat driver 放到了入口层**，但**还没有把真实生产聊天编排 ownership 从 legacy `chat_runtime_impl.rs` 收回来**。
- 新增 TDD 大多在验证“runtime 外面包了一层之后还能看到一些 runtime 痕迹”，**没有证明真实 `send_message` 主链路已经 runtime-first**。
- 因此本专项现在**不能标记为已关闭**。

---

## Findings

### Finding 1

- 标题：`[P1] 真实 send_message 主链路仍由 legacy_send_message_impl 持有完整聊天编排`
- 严重级别：P1
- 影响范围：真实聊天主链路、消息持久化、模式路由、analysis/workflow 主循环、流式结束条件
- 真实使用路径：
  - `commands/chat.rs::send_message`
  - `transport/tauri_commands/chat.rs::TauriChatCommandAdapter::send_message`
  - `runtime/session_runtime.rs::run_chat_request`
  - `runtime/chat/chat_turn_driver.rs::run_chat_turn`
  - `transport/tauri_commands/chat.rs::TauriLegacyTurnExecutor::run_chat_turn`
  - `transport/tauri_commands/chat/chat_runtime_impl.rs::legacy_send_message_impl`
- 问题描述：
  - 真实入口 `TauriChatCommandAdapter::new()` 仍然构造的是 `SessionRuntime::with_executor(...)`，并把 `TauriLegacyTurnExecutor` 接进去：`src-tauri/src/transport/tauri_commands/chat.rs:150-156`。
  - `TauriLegacyTurnExecutor` 仍然直接调用 `legacy_send_message_impl(...)`：`src-tauri/src/transport/tauri_commands/chat.rs:91-112`。
  - `RuntimeChatTurnDriver::run_chat_turn()` 在 executor-backed 路径里只是：
    1. 发一个 `StreamStarted`
    2. 调 `executor.run_chat_turn(...)`
    3. 用 `record_only(...)` 补两个 marker  
    并没有接管真实聊天 loop：`src-tauri/src/runtime/chat/chat_turn_driver.rs:99-145`。
  - `legacy_send_message_impl(...)` 里仍然持有完整聊天主编排：busy guard、写 user message、加载 settings、构造 history / prompt、precompute、LLM loop、tool loop 等都还在这里：例如 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:112-126`、`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:1674-1745`。
- 为什么这是 bug / 设计缺陷 / 回归风险：
  - 计划要求收口为“runtime 持有真实聊天编排、transport 只做宿主桥接与兼容 helper”，但当前实现仍然是“runtime 外包一层 + transport 持有真实 orchestrator”。
  - 这意味着后续任何和聊天生命周期有关的改动，仍然要回到 legacy transport 层处理，runtime 不是单一真相源。
  - 现在的 driver 更像包装器，不是 owner。
- 现有测试有没有覆盖，覆盖为什么不够：
  - `src-tauri/tests/chat_runtime_first_mainline_test.rs:23-215` 和 `src-tauri/tests/review_runtime_executor_bypass_test.rs:71-122` 只验证 runtime 记录到了终态事件。
  - 但这些测试没有要求“legacy executor 不再持有完整 orchestrator”，所以 `record_only(...)` 就足以把测试刷绿，而不需要迁移真实 owner。
  - 也没有任何测试直接驱动 `TauriChatCommandAdapter::send_message()` 去证明 production wiring 已切换。
- 最小复现方式：
  - 直接跟踪真实代码路径：`src-tauri/src/transport/tauri_commands/chat.rs:168-176` -> `src-tauri/src/runtime/session_runtime.rs:82-107` -> `src-tauri/src/runtime/chat/chat_turn_driver.rs:114-139` -> `src-tauri/src/transport/tauri_commands/chat.rs:97-112`。
  - 可以再补一个最小复现测试：spy `RuntimeTurnExecutor`，断言 production path 下 runtime driver 不应再把整个 turn 交给一个 full-turn helper。
- 修复建议：
  - 把 `legacy_send_message_impl()` 继续拆成 runtime driver 可调用的窄 helper（如 host emit、settings/provider glue、compat mapping），而不是把整个聊天主循环继续包在 executor 里。
  - `RuntimeChatTurnDriver` 应直接持有 turn lifecycle：何时进入 LLM round、何时进入 tool round、何时写 assistant message、何时结束 stream。
  - executor 若保留，只能是 host-bound helper，不能再是 full orchestrator。
- 相关文件定位：
  - `src-tauri/src/transport/tauri_commands/chat.rs:91`
  - `src-tauri/src/transport/tauri_commands/chat.rs:150`
  - `src-tauri/src/runtime/session_runtime.rs:82`
  - `src-tauri/src/runtime/chat/chat_turn_driver.rs:99`
  - `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:112`

### Finding 2

- 标题：`[P1] 真实工具调用回合仍然绕过 QueryEngine / runtime dispatcher / event bus`
- 严重级别：P1
- 影响范围：tool dispatcher 合同、capability/permission、workspace-first 工具执行、tool lifecycle 事件
- 真实使用路径：
  - `send_message`
  - `legacy_send_message_impl(...)`
  - `chat_runtime_impl.rs` 内部 tool loop
  - `tool_registry.execute(...)`
- 问题描述：
  - `TauriChatCommandAdapter::new()` 当前构造的 `QueryEngine` 只有 `workspace_path`，没有注入 runtime dispatcher：`src-tauri/src/transport/tauri_commands/chat.rs:150-152`。
  - 真实 tool loop 仍然在 `chat_runtime_impl.rs` 内部直接：
    - 手工 `app.emit("tool:executing", ...)`：`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2747-2755`
    - 手工做 allowed-tools 过滤：`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2757-2808`
    - 直接调用 `tool_registry.execute(...)` 执行工具：单工具路径 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2823-2830`，并发路径 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2866-2877`
  - `RuntimeChatTurnDriver` 的 executor-backed 分支完全没有调用 `QueryEngine::run_tool_with_bus(...)`。
- 为什么这是 bug / 设计缺陷 / 回归风险：
  - 计划里的 AC-3 是“真实聊天中的工具执行走 runtime dispatcher 合同”，但当前真实生产回合仍然走 legacy `tool_registry.execute(...)` 直连桥接。
  - 这会导致 runtime tool permission / capability / tool event bus 仍然不是 send_message 主链路的单一真相源。
  - 也意味着后续你在 runtime dispatcher 上修的行为，真实聊天主链路未必能吃到。
- 现有测试有没有覆盖，覆盖为什么不够：
  - `src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs:15-18` 自己已经明确写了：它验证的是 `QueryEngine::run_tool_with_bus` 路径，**不是实际 production LLM loop**。
  - 这组测试可以证明 runtime dispatcher 自身没坏，但不能证明真实 `send_message` 已经走到这条路径。
  - 当前没有任何测试把 `TauriChatCommandAdapter`、真实 `tool_registry`、真实聊天 tool round 串起来验证 dispatcher ownership。
- 最小复现方式：
  - 直接读 `chat_runtime_impl.rs` 的 tool loop：`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2747-2877`。
  - 或补一个最小复现测试：给 `QueryEngine` 挂 spy dispatcher，驱动真实 `send_message`，断言 tool call 会经过 `run_tool_with_bus(...)`；当前实现应当打不到。
- 修复建议：
  - 把真实 tool round 的入口收回到 runtime chat driver，由 runtime 统一决定：
    - visible tools
    - permission/capability 检查
    - `tool:executing` / `tool:completed` 的 event bus 发射
    - 单工具 / 并发工具 dispatch
  - transport 层只保留 host bridge，不再直接 `app.emit(...)` 和 `tool_registry.execute(...)`。
- 相关文件定位：
  - `src-tauri/src/transport/tauri_commands/chat.rs:150`
  - `src-tauri/src/runtime/chat/chat_turn_driver.rs:114`
  - `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2747`
  - `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2828`
  - `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2877`

### Finding 3

- 标题：`[P2] executor-backed 聊天路径现在存在双 RunId，runtime 事件与真实执行身份已经分裂`
- 严重级别：P2
- 影响范围：run-scoped 审计、trace 关联、后续取消/恢复/子运行排障、runtime_audit
- 真实使用路径：
  - `SessionRuntime::run_chat_request()` 生成 runtime run id
  - `legacy_send_message_impl()` 再生成一份 legacy run id
- 问题描述：
  - `SessionRuntime::run_chat_request()` 在进入 driver 前生成了一次 `run_id`：`src-tauri/src/runtime/session_runtime.rs:86-98`。
  - 但 `ChatTurnRequest` 结构里并没有 `run_id` 字段：`src-tauri/src/runtime/chat/chat_turn_driver.rs:13-31`。
  - 于是 `legacy_send_message_impl()` 只能再次自己生成一个新的 `run_id`：`src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:125-127`。
- 为什么这是 bug / 设计缺陷 / 回归风险：
  - runtime event bus 上看到的是 run A，真实 gateway busy slot、tool/plugin 上下文、analysis precompute、后续日志里跑的是 run B。
  - 一旦你想把 runtime_audit、恢复、取消传播、子代理 trace 真正收口到 run scope，这个分裂会直接造成“同一轮聊天有两套 run identity”的排障噪音。
  - 这也和 runtime-first 想建立的单一 turn truth source 相冲突。
- 现有测试有没有覆盖，覆盖为什么不够：
  - 当前新增测试都只看事件名或事件存在性，没有任何一条断言“runtime run id == 实际执行 run id”。
  - 所以即使现在已经 fork 出两套 RunId，所有 targeted tests 仍然会继续为绿。
- 最小复现方式：
  - 对照两处 `run_id` 生成点即可复现：`src-tauri/src/runtime/session_runtime.rs:88` 与 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:126`。
  - 建议补一个最小复现测试：在 executor/tool path 中捕获 `PluginContext.run_id`，并与 `runtime.recorded_events()` 中的 `run_id` 断言相等；当前实现应失败。
- 修复建议：
  - 把 `run_id` 纳入 `ChatTurnRequest`，由 runtime 创建一次并向下透传。
  - `legacy_send_message_impl()` 不应再自建 run id，而应消费 runtime 传入的同一份 turn identity。
- 相关文件定位：
  - `src-tauri/src/runtime/session_runtime.rs:88`
  - `src-tauri/src/runtime/chat/chat_turn_driver.rs:13`
  - `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:126`

### Finding 4

- 标题：`[P2] 当前新增 TDD 仍然不能证明真实 Tauri 生产链路已经 runtime-first`
- 严重级别：P2
- 影响范围：本专项 gating 可信度、后续 Claude 修复完成判定
- 真实使用路径：`targeted tests -> 判定专项关闭 -> 进入下一阶段`
- 问题描述：
  - `chat_runtime_first_mainline_test.rs` 和 `review_runtime_executor_bypass_test.rs` 都是直接 new `SessionRuntime::with_executor(...)`，传的是测试 executor，不经过真实 `TauriChatCommandAdapter::new()`：`src-tauri/tests/chat_runtime_first_mainline_test.rs:31-46`、`src-tauri/tests/review_runtime_executor_bypass_test.rs:24-37`。
  - `send_message_runtime_path_test.rs` 走的是 `SessionRuntime::for_test(...)` 的纯 runtime 路径，本身就没有 executor：`src-tauri/tests/send_message_runtime_path_test.rs:15-24`。
  - `chat_runtime_dispatcher_production_path_test.rs` 文件头已经明说它不是实际 production LLM loop：`src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs:15-18`。
  - 换句话说，这几组测试目前没有一条能回答最关键的问题：真实 `commands/chat.rs -> TauriChatCommandAdapter -> send_message` 这条链路是不是已经把 orchestrator / dispatcher ownership 切进 runtime。
- 为什么这是 bug / 设计缺陷 / 回归风险：
  - 现在的测试会给出“全部绿灯”，但这组绿灯并不能作为“专项已闭合”的可靠证据。
  - 这和你要求的“用 TDD 证明代码没问题”是有偏差的：它证明了几个局部 contract，不是证明了真实主链路收口。
- 现有测试有没有覆盖，覆盖为什么不够：
  - 已有测试覆盖了 runtime driver 的局部行为、runtime dispatcher 的局部行为、纯 runtime trace 的局部行为。
  - 但缺了最核心的一层：**真实 adapter + 真实 wiring + 真实 production send_message path**。
- 最小复现方式：
  - 直接跑当前 3 个 targeted tests，它们都会绿；然后再对照真实代码路径 `src-tauri/src/transport/tauri_commands/chat.rs:150-176` 与 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:112-2905`，可以看到生产 owner 仍在 legacy 层。
- 修复建议：
  - 至少新增 3 条 gating 级测试：
    1. `send_message_production_adapter_should_not_delegate_full_turn_to_legacy_impl`
    2. `send_message_production_tool_round_should_dispatch_via_runtime_query_engine`
    3. `send_message_production_path_should_preserve_single_run_id`
  - 这三条必须直接从 `TauriChatCommandAdapter` 或等价真实 wiring 入口驱动，而不是只测裸 `SessionRuntime`。
- 相关文件定位：
  - `src-tauri/tests/chat_runtime_first_mainline_test.rs:30`
  - `src-tauri/tests/review_runtime_executor_bypass_test.rs:24`
  - `src-tauri/tests/send_message_runtime_path_test.rs:15`
  - `src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs:15`

---

## 建议补充的 TDD（作为下一轮修复通过条件）

- `send_message_production_adapter_should_not_delegate_full_turn_to_legacy_impl`
  - 直接驱动真实 adapter/wiring，证明 `legacy_send_message_impl()` 不再是 full orchestrator。
- `send_message_production_tool_round_should_dispatch_via_runtime_query_engine`
  - 证明真实聊天中的工具回合经过 `QueryEngine::run_tool_with_bus(...)` / runtime dispatcher，而不是 `tool_registry.execute(...)`。
- `send_message_production_path_should_preserve_single_run_id`
  - 证明 runtime bus、gateway busy state、tool/plugin context 使用的是同一个 `RunId`。
- `send_message_production_events_should_be_bus_emitted_not_record_only`
  - 证明 `MessagePersisted` / `StreamDone` 在 production path 是由 runtime event bus 真正 emit 出去，而不是仅写入 `recorded_events()`。

## 总体结论（实施前诊断，2026-04-14 初稿）

当前这轮实现**完成了”把 runtime chat driver 接进入口层”**，但还**没有完成”聊天主链路 runtime-first 收口”**。

正式结论（实施前）：

- `AC-1`：**未闭合** — 真实聊天主链路仍由 `legacy_send_message_impl()` 主导
- `AC-2`：**未闭合** — `SessionRuntime::run_chat_request()` 仍未持有完整 turn orchestration
- `AC-3`：**未闭合** — 真实 tool execution 仍未走 runtime dispatcher 主路径
- `AC-4`：**部分满足** — 兼容事件协议仍可用，但当前仍主要靠 legacy host emit 保持
- `AC-5`：**本轮未重点回归** — 需要等真正切换 owner 后再重跑 workspace-first golden path
- `AC-6`：**未满足** — 当前 TDD 还不能证明真实生产主链路已切换

---

## 2026-04-14 实施后正式 Code Review（commits aefb7ca → 736f527）

**实施提交：**

| 提交 | 内容 |
|---|---|
| `aefb7ca` | 新增/收紧红灯测试，锁死 runtime-first closure 约束 |
| `aa03d36` | 引入 RuntimeChatTurnDriver，切断 full delegate，record_only 过渡机制 |
| `736f527` | 新增 dispatcher production path 测试，验证三项约束 |

### Assessment：❌ Not Ready / 继续保持未关闭

虽然本轮实现拿回了部分 runtime ownership，并且 targeted tests 全绿，但按原始 `2026-04-13-chat-runtime-first-closure-plan.md` 的定义，**当前仍不能认定 Phase-1 已闭合**。原因不是外围 contract 坏了，而是更高层的 P1/P2 问题仍然成立：真实 owner 还在 legacy transport / `chat_runtime_impl.rs`，现有 TDD 还不足以证明真实生产入口已 runtime-first。

### 实施后仍然成立的高优 Findings

#### [P1] 真实聊天主链路还没有从 legacy orchestrator 收回来

- `src-tauri/src/transport/tauri_commands/chat.rs:150` 仍然把生产入口接到 `SessionRuntime::with_executor(...)`
- `src-tauri/src/runtime/chat/chat_turn_driver.rs:114` 在 executor-backed 路径里依旧只是调用 `executor.run_chat_turn(...)`
- 真实聊天 loop 仍在 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:112`

这说明 driver 目前仍然是 wrapper / coordinator，而不是完整 owner。transport 还没有真正降级成 helper。

#### [P1] 真实工具回合仍然不走 runtime dispatcher 主路径

- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2747` 还在手工发 `tool:executing`
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2828`
- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2877`

这些位置仍然直接 `tool_registry.execute(...)`，没有收口到 `QueryEngine::run_tool_with_bus(...)`。

因此 AC-3 仍未闭合。

#### [P2] 当前 executor-backed 路径有双 RunId

- runtime 在 `src-tauri/src/runtime/session_runtime.rs:88` 先生成一次 `RunId`
- legacy 实现又在 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:126` 再生成一次

这会让 runtime event、gateway busy 状态、tool/plugin 上下文不再共享同一个 turn identity。

#### [P2] 现有 TDD 仍不足以证明“真实 Tauri 生产入口已 runtime-first”

- `src-tauri/tests/chat_runtime_first_mainline_test.rs:30` 和 `src-tauri/tests/review_runtime_executor_bypass_test.rs:24` 测的是裸 `SessionRuntime`
- `src-tauri/tests/chat_runtime_dispatcher_production_path_test.rs:15` 文件头自己也写明：它验证的是 `QueryEngine::run_tool_with_bus()` 路径，**不是实际 production LLM loop**

因此当前测试只能证明“runtime 外围 contract 没坏”，还不能证明“真实 send_message 生产主链路已经完成 runtime-first 收口”。

### 仍然成立的 Important / Minor 技术债

**Important：**

- **I1：`with_authorized_workspace_store()` 注入存在条件分支静默降级风险。** `TauriChatCommandAdapter::new()` 注入路径是 `if let Some(facade) = services.app.try_state::<Arc<RuntimeRepositoryFacade>>()` — 若 `RuntimeRepositoryFacade` 未注册，`authorized_workspace_store` 静默为 `None`，工作区工具在用户已授权时会静默降级。建议加 `log::warn!`。

- **I2：executor-backed 路径的 `MessagePersisted` 是合成标记，不对应真实持久化。** `record_only(RuntimeEvent::message_persisted(..., json!({"executor_owned": true})))` 里的 `msg_id` 和 `content` 是伪造的，与 `legacy_send_message_impl` 里实际写入 DB 的消息 ID 无关。它只是过渡期 ownership marker，不应被当作真实 persistence payload 消费。

**Minor：**

- **M1：** `common.rs` 中的 `SilentExecutor` 触发 dead_code 警告（`chat_runtime_dispatcher_production_path_test` 未引用 `mod common`）。
- **M2：** `chat_runtime_dispatcher_production_path_test.rs` 测的是 `QueryEngine::run_tool_with_bus()` 直接路径，标题声称 “production path” 但实际是 QueryEngine 单元测试，应在注释中更明确澄清。
- **M3：** `with_executor()` 构造函数名称迁移完成后应重命名或删除，防止误用。

### 验证摘要

| 测试 | 结果 |
|---|---|
| `review_runtime_executor_bypass_test` | 3/3 ✅ |
| `send_message_runtime_path_test` | 1/1 ✅ |
| `chat_runtime_first_mainline_test` | 4/4 ✅ |
| `chat_runtime_dispatcher_production_path_test` | 3/3 ✅ |
| `workspace_first_agent_golden_path_test` | 2/2 ✅ |
| `builtin_runtime_registration_test` | 8/8 ✅ |
| `tool_runtime_integration_test` | 3/3 ✅ |
| `review_*` 全套 | 全绿 ✅ |
| `WorkspaceAuthPanel.test.tsx` | 5/5 ✅ |
| `WorkspaceFirst.integration.test.tsx` | 2/2 ✅ |

### 状态更新

**专项状态：❌ 继续保持“进行中 / 未关闭”**

本轮实现完成了：
- runtime chat driver 接入入口层
- 事件与 duplicate-event 约束的外围闭环
- runtime dispatcher / Workspace-First 合同的局部验证

但仍未完成：
- 真实聊天主循环 owner 从 `legacy_send_message_impl()` 迁回 runtime
- 真实工具执行主路径收口到 runtime dispatcher
- 单一 `RunId` truth source
- 直接从真实 Tauri production wiring 证明 runtime-first 的 gating TDD

---

## 2026-04-14 最终关闭确认

所有 4 条 gating tests 全绿：

| 测试 | 状态 |
|------|------|
| T1 `full_turn_must_not_delegate_to_legacy_executor` | ✅ GREEN |
| T2 `tool_round_must_dispatch_via_runtime_query_engine` | ✅ GREEN（ToolRoundDriver → QueryEngine → ToolDispatcher → SpyTool） |
| T3 `must_use_single_run_id` | ✅ GREEN（regression gate） |
| T4 `message_persisted_must_be_emitted_not_record_only` | ✅ GREEN |

B3 wiring 顺序已修复：`RuntimeRepositoryFacade` 在 `lib.rs:233` 注册，`TauriChatCommandAdapter::new()` 在第 241 行之后构造，`try_state` 可成功拿到 facade，`authorized_workspace_store` 正确注入。

review_ 全量回归、tool_round、workspace_first、builtin_runtime_registration、tool_runtime_integration 全部通过。

**专项正式关闭。**

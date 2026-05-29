# 2026-04-14 P1-A Chat Tool Dispatch Runtime Review

状态：**P1 已关闭 / P2 仍有 1 条未闭合（2026-04-14）**  
评审对象：`/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-14-p1-a-chat-tool-dispatch-runtime-plan.md` 及本轮代码实现  
评审范围：`runtime/query_engine.rs`、`runtime/chat/tool_round_*`、`transport/tauri_commands/chat/chat_runtime_impl.rs`、runtime bus → Tauri host 事件映射、对应 TDD

## 验证基线

本轮实际核对并运行了：

- `cargo test --manifest-path src-tauri/Cargo.toml --test builtin_runtime_registration_test browser_tool_without_connector_engine_is_permission_denied -- --nocapture` ✅
- `cargo test --manifest-path src-tauri/Cargo.toml --test chat_runtime_dispatcher_production_path_test -- --nocapture` ✅
- `cargo test --manifest-path src-tauri/Cargo.toml --test review_tool_error_terminal_event_test -- --nocapture` ✅

本轮另外补了两条回归测试，锁死刚修掉的两个 P1：

- `runtime_chat_mainline_passes_browser_capability_to_runtime_tool_round`
- `review_runtime_tool_failure_maps_tool_completed_success_false`

---

## Findings

### Finding 1

- 标题：`[P1][已关闭] 新的 production runtime tool round 永远不给浏览器工具注入 browser capability`
- 关闭结论：
  - 这条 P1 现在可以关闭。
- 关闭证据：
  - `QueryEngine` 已新增 `browser_available` 字段与 `with_browser_available(bool)` builder：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/query_engine.rs:23`、`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/query_engine.rs:60`
  - 真实聊天 tool round 现在会从 host 是否存在 `ConnectorEngine` 推导并下传该能力：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2733`
  - `run_tool_call_with_bus(...)` 已把 `browser_available` 注入 `CapabilityContext`：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/query_engine.rs:188`
- TDD 证据：
  - 负向保护仍在：`browser_tool_without_connector_engine_is_permission_denied` 证明无 connector 时会被 capability 层拒绝。
  - 新增正向保护：`runtime_chat_mainline_passes_browser_capability_to_runtime_tool_round` 证明 `with_browser_available(true)` 后，browser-scoped runtime tool 能穿过 `CapabilityPermissionPipeline` 并真正到达 `execute()`。
- 结论说明：
  - 之前的“永远 false”回归已经不成立；这条问题已闭合。

### Finding 2

- 标题：`[P1][已关闭] runtime tool round 把所有 tool:completed 都映射成 success=true，前端会把失败工具显示成成功`
- 关闭结论：
  - 这条 P1 现在可以关闭。
- 关闭证据：
  - `RuntimeEventKind::ToolCallCompleted` 已新增 `is_error: bool`：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/events.rs:23`
  - `QueryEngine::run_tool_call_with_bus(...)` 在成功/失败两条分支都会显式写入 `is_error`：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/query_engine.rs:221`、`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/query_engine.rs:243`
  - `TauriEventAdapter` 已把前端收到的 `tool:completed.success` 改为 `!is_error`：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs:47`
- TDD 证据：
  - 原有 `review_failing_tool_should_still_emit_terminal_completed_event` 保留了“失败也必须发 terminal completed event”的合同。
  - 新增 `review_runtime_tool_failure_maps_tool_completed_success_false`，直接锁死 runtime bus → adapter → host 的 payload 语义：失败工具必须映射成 `success=false`。
- 结论说明：
  - 之前的“前端把失败显示成成功”回归已经不成立；这条问题已闭合。

### Finding 3

- 标题：`[P2][未关闭] P1-A 新路径丢掉了 file_meta / degradation metadata，导出结果校验与降级提示被静默禁用`
- 严重级别：P2
- 真实使用路径：
  - runtime tool round
  - tool result 汇总
  - `finish_agent(...)`
  - `verify_file_claims(...)`
- 问题描述：
  - `RuntimeToolCallOutcome` 现在仍只保留 `tool_call_id`、`tool_name`、`content`、`is_error`，没有 `file_meta`、`generated_files`、`is_degraded`、`degradation_notice`：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/chat/tool_round_types.rs:25`
  - `chat_runtime_impl.rs` 仍然会创建 `all_file_metas`，并在收尾阶段继续依赖它做格式纠错和文件元数据处理：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:2080`、`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:3195`
  - 同文件注释也明确承认 runtime dispatcher migration 之后这条 `file_meta` 路径还没恢复：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:3701`
- 为什么这条仍然成立：
  - 目前导出类工具虽然能执行，但 runtime tool round 还没有把“实际生成了什么文件、是否 degraded、要不要给用户降级提示”这类 metadata 带回上层。
  - 这会让 `verify_file_claims(...)`、降级提示、文件卡片语义继续停留在静默退化状态。
- 当前 TDD 为什么还不够：
  - 本轮新增/现有测试都集中在 dispatcher ownership、browser capability、tool terminal event success 语义。
  - 还没有任何一条测试驱动“文件生成工具经过 runtime tool round 后，`finish_agent(...)` 仍拿得到 file metadata 并正确保留 degraded/export 语义”。
- 最小修复建议：
  - 扩展 `RuntimeToolCallOutcome`，透传 `file_meta`、`generated_files`、`is_degraded`、`degradation_notice`。
  - 增加最小回归测试，至少覆盖：
    - 文件生成工具通过 runtime tool round 后，上层能拿到 `FileMeta`
    - degraded export 情况下，收尾路径仍会写出纠正/提示信息

---

## 额外说明

- `review_chat_tool_dispatch_runtime_test.rs` 里保留的 future-scope 红灯（如 T1/T2/T3）仍然代表更后续的 runtime-first 收口目标；我本轮没有把它们重新升级成 P1-A finding。
- 但这不影响本轮结论：**P1-A 的两个新增生产级回归已经修掉，并且现在有对应自动化测试锁住；当前剩余唯一正式 open finding 是 file_meta / degradation metadata 丢失。**

## 当前结论

我现在的复审结论是：

- P1 已闭合，可以确认：
  - browser capability 注入回归已修复
  - `tool:completed.success` 语义回归已修复
- P2 仍有 1 条未闭合：
  - runtime tool round 尚未恢复 file metadata / degradation metadata 透传

所以这份 review 文档的准确状态应是：**P1 关闭，但专项整体还不能写成 fully closed；还需要补完 file_meta 这条 P2。**

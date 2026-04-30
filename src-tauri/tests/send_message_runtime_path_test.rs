// 此文件专注于 send_message 主链路的 legacy event 序列验证。
// 与 chat_runtime_first_mainline_test.rs 的分工：
//   - 此文件：legacy event 名称（streaming:delta 等）的观测面（绿灯：纯 runtime 路径）
//   - chat_runtime_first_mainline_test.rs：RuntimeEventKind 级别的 ownership 约束（红灯：executor-backed 路径）
//
// 注：executor-backed path 的 ownership RED-LIGHT 由 chat_runtime_first_mainline_test.rs 的
// `runtime_chat_driver_should_own_turn_orchestration` 负责（检查 MessagePersisted/StreamDone 是否由 runtime 发出）。
// legacy event 层的映射已由 tauri_event_adapter_test.rs 验证（StreamDone → "streaming:done" 的映射）。
// 因此本文件不再测试 executor-backed path 的 ownership。

mod common;

use app_lib::commands::chat::testsupport::run_send_message_through_runtime;

#[tokio::test]
async fn send_message_emits_legacy_events_via_runtime_adapter() {
    let trace = run_send_message_through_runtime("conv-1", "hello")
        .await
        .unwrap();
    assert_eq!(
        trace.event_names(),
        vec!["streaming:delta", "message:updated", "streaming:done"]
    );
}

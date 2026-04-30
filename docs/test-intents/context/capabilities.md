# capabilities.md — 测试工具箱

AI 执行测试意图时可用的工具和基础设施。

## 运行测试

```bash
# 运行全部 Rust 测试
cd src-tauri && cargo test

# 运行单个测试文件
cd src-tauri && cargo test --test <test_file_name> -- --nocapture

# 运行 review_ 系列架构回归测试
cd src-tauri && cargo test review_ --tests --no-fail-fast

# 修改 settings 分层后，补跑配置层回归测试
cd src-tauri && cargo test --test plan_ae_config_layers_test -- --nocapture
```

## 构造隔离测试环境

```rust
use tempfile::TempDir;
use app_lib::storage::file_store::AppStorage;

let dir = TempDir::new().unwrap(); // 测试结束自动清理
let storage = AppStorage::new(dir.path()).unwrap();
// 注意：dir 必须在测试结束前保持存活，否则目录被提前删除
```

## 构造 workspace-level settings

```rust
std::fs::create_dir_all(workspace.join(".aijia")).unwrap();
std::fs::write(
    workspace.join(".aijia").join("settings.json"),
    r#"{ "someKey": "someValue" }"#,
).unwrap();
```

- 用 TempDir 就能模拟 workspace 覆盖，不需要真实项目目录

## 读取 settings 分层后的实际生效值

```rust
use app_lib::models::settings::AppSettings;

let settings_map = storage.get_effective_settings(Some(&workspace)).unwrap();
let settings = AppSettings::from_string_map(&settings_map);
```

- 适合验证 global/workspace 合并后的实际生效值

## 模拟 LLM（MockLlmExecutor）

```rust
use app_lib::runtime::chat::{LlmStepResult, RuntimeLlmExecutor};

let executor = Arc::new(MockLlmExecutor::new(vec![
    LlmStepResult::ToolCalls {
        assistant_content: "thinking".to_string(),
        tool_calls: vec![],
        tokens_in: 10,
        tokens_out: 5,
    },
    LlmStepResult::ContentComplete {
        content: "done".to_string(),
        tokens_in: 4,
        tokens_out: 2,
        stop_reason: Some("end_turn".to_string()),
    },
]));
```

## 验证 turn 内 settings 只读取一次

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

struct ProbeExecutor {
    load_calls: AtomicUsize,
    seen_values: Mutex<Vec<String>>,
}

async fn load_llm_settings_for_turn(&self, _request: &ChatTurnRequest) -> Result<ResolvedLlmSettings, TurnError> {
    self.load_calls.fetch_add(1, Ordering::SeqCst);
    Ok(ResolvedLlmSettings::default())
}

async fn run_llm_step(&self, input: &LlmStepInput<'_>, ...) -> Result<LlmStepResult, TurnError> {
    self.seen_values.lock().unwrap().push(input.masking_level.to_string());
    Ok(LlmStepResult::ContentComplete { ... })
}
```

- `load_calls == 1` 验证 turn 级快照
- `seen_values` 验证多轮是否复用同一份设置

## 驱动完整 Turn

```rust
use app_lib::runtime::chat::RuntimeChatTurnDriver;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::query_engine::QueryEngine;

let driver = RuntimeChatTurnDriver::with_llm_executor(
    QueryEngine::default(),
    RuntimeEventBus::new(),
    executor.clone(),
);
let mut turn = TurnState::new(
    IdentityMapping::from_legacy_conversation_id("conv-id"),
    RunId::new("run-id"),
    "用户消息".to_string(),
);
let request = ChatTurnRequest::new("conv-id", "用户消息", vec![]);
driver.run_chat_turn(&mut turn, &request).await.unwrap();
```

## 收集事件序列

```rust
// 参考 src-tauri/tests/common.rs 的 event_labels() 工具函数
use tests::common::event_labels;
let labels = event_labels(&bus.collected_events());
assert!(labels.contains(&"StreamDone"));
```

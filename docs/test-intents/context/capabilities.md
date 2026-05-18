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

---

## Agent 跑：tauri-pilot `aijia` CLI（端到端真实环境）

> 仅用于 README.md §4 "方式二：agent 跑（产品验收）" —— **不进 cargo test，不进 CI**。
> 前置：`pnpm tauri:dev` 起在 5173，dev build 自动注册 `tauri-plugin-pilot`（仅 `cfg(debug_assertions)`）。
> 通信：CLI 通过 Unix Domain Socket `/tmp/tauri-pilot-com.aijia.app.sock` 调 app；socket 残留时先 `rm -f` 再起 server。

### 启动与健康检查

```bash
# 启动 dev server（90s 内 ready）
cd ~/IdeaProjects/lotus-app
rm -f /tmp/tauri-pilot-com.aijia.app.sock
pnpm tauri:dev &

# 等 vite 起来
until lsof -ti tcp:5173 >/dev/null; do sleep 2; done

# 连通性
tauri-pilot ping                                  # → ✓ ok
tauri-pilot aijia health-check --json             # → {"ok":true,"readyState":"complete",...}

# 验前端 hook 已 expose
tauri-pilot eval "typeof window.__aijia"          # → "object"
```

### 16 个 `aijia` 子命令分组

按职责分四类，**所有命令默认 stdout 一行 JSON**（加 `--json`）。

#### 组 1：会话流（chat mainline，6 个 P0 命令）

| 命令 | 作用 | 关键返回字段 |
|---|---|---|
| `aijia new-task` | 点侧栏"新任务" → 路由到 `/`，新建空对话 | `{ok, currentRoute}` |
| `aijia type-message <text>` | Tiptap `execCommand('insertText', false, text)` | `{ok, filledText}` |
| `aijia send` | 点发送按钮 | `{ok}` |
| `aijia wait-reply [--timeout 30]` | 阻塞等流式回复结束（**stability window: 3 连续 ready tick**） | `{ok, elapsedMs, strategy}` |
| `aijia ui-message [--last N] [--role user\|assistant\|tool_call] [--since 2m] [--include-tools]` | dump 当前对话 DOM 渲染的消息 | `{messages:[{role,content,...}]}` |
| `aijia last-reply` | `ui-message --last 1 --role assistant` 的快捷别名 | `{role,content}` |

**最小回合**：`new-task` → `type-message "..."` → `send` → `wait-reply` → `last-reply`。

#### 组 2：会话管理（4 个 P1 命令）

| 命令 | 作用 |
|---|---|
| `aijia list-sessions` | 列出侧栏所有会话 `[{id,name,index,isActive,isArchived}]` |
| `aijia switch-session <id\|index>` | 切到指定会话（id 优先，数字按 0=最新） |
| `aijia archive-session <id\|index>` | 走 Tauri IPC `archive_conversation`，**不走 UI hover**（pointer model 不稳） |
| `aijia cleanup-test-sessions [--prefix e2e-test-]` | 批量归档 title 前缀匹配的会话；**只匹配 title，发消息不更新 title** —— 想匹配必须先 rename |

#### 组 3：流式与取消（1 个 P2 命令）

| 命令 | 作用 |
|---|---|
| `aijia cancel` | 流式中点停止按钮；流式未开始时报错 |

#### 组 4：诊断与现场（3 个命令）

| 命令 | 作用 |
|---|---|
| `aijia where` | dump 现场：`{url,title,route,activeConversationId,messageCount,isStreaming,hasEditor}` —— **失败时第一步跑这个** |
| `aijia screenshot [--name <label>]` | 截图到 `/tmp/aijia-e2e-{label}-{ts}.png`（默认 `skipFonts:true`，否则 30s+ 超时） |
| `aijia health-check` | app ready 探测（启动后第一个跑） |

#### 组 5：明确未实现（占位，2 个命令）

`aijia select-workspace` / `aijia restart-app` — 当前返回 `not implemented`，不要在 rules.md 里依赖。

### Agent 跑回归断言

CLI 跑出的是 DOM 视图；持久化断言走文件系统（仍是 `~/.renlijia/` 真实路径）：

```bash
# 跑完一回合后取 conv_id
conv_id=$(tauri-pilot aijia where --json | jq -r .activeConversationId)

# 断言 1：会话目录已落盘
test -d ~/.renlijia/users/t_28__u_54/conversations/$conv_id/ || { echo "FAIL: no conv dir"; exit 1; }

# 断言 2：messages.jsonl 含 user + assistant 各 1 条
#   注意：每行末尾有 `\t✓` 校验位，解析时要先 split('\t')[0]
python3 -c "
import json
with open('$HOME/.renlijia/users/t_28__u_54/conversations/$conv_id/messages.jsonl') as f:
    roles = [json.loads(l.split('\t', 1)[0]).get('role') for l in f if l.strip()]
print('user count:', roles.count('user'))
print('assistant count:', roles.count('assistant'))
"

# 断言 3：UI 看到的回复 == JSONL 里的 content.text（content 是嵌套对象）
ui_reply=$(tauri-pilot aijia last-reply --json | python3 -c "import json,sys; print(json.load(sys.stdin)['text'])")
disk_reply=$(python3 -c "
import json
last = ''
with open('$HOME/.renlijia/users/t_28__u_54/conversations/$conv_id/messages.jsonl') as f:
    for line in f:
        m = json.loads(line.split('\t', 1)[0])
        if m.get('role') == 'assistant':
            last = (m.get('content') or {}).get('text', '')
print(last)
")
[ \"\$ui_reply\" = \"\$disk_reply\" ] || { echo \"FAIL: UI != disk\"; exit 1; }
```

### 已知边界

1. **Tiptap 输入只能 `execCommand('insertText', ...)`** —— 合成 `input` 事件 / `dispatchEvent` 都无效。
2. **wait-reply 必须 stability window**（3 连续 `isStreaming===false`）—— 单点采样会在 tool_calls 之间误判完成。
3. **archive-session 走 IPC** —— UI hover 在 pointer model 下不稳。Tauri IPC arg 是 **camelCase**：`conversationId` 不是 `conversation_id`；conversation 字段是 `isArchived` 不是 `archived` / `lifecycle`。
4. **archive 后侧栏 list-sessions 不会自动刷新** —— 磁盘 `conv.json.isArchived=true` 已更新，但 DOM 还显示老状态。要么 sleep + 触发别处刷新，要么直接读 `~/.renlijia/.../conv.json` 验证。
5. **cleanup-test-sessions 只看 title** —— rename 后才能匹配；首条用户消息不会更新 title。
6. **`aijia screenshot` 当前可能 30s 超时** —— wrapper 走 html-to-image，需要 plugin 重新编译注入新版 `bridge.js`（`skipFonts:true`）。用 raw `tauri-pilot screenshot <path>` 兜底（直接走 webview screenshot，~100ms 完成）。
7. **改 `bridge.js` 后 cargo 不会自动重编** —— `touch crates/tauri-plugin-pilot/src/lib.rs` 强制重新 `include_str!`。
8. **跨工作目录测试不支持** —— `select-workspace` 未实现，第一版默认沿用当前 workspace。
9. **socket 残留** —— app crash 后 `/tmp/tauri-pilot-com.aijia.app.sock` 不会被清理，下次启动前 `rm -f` 一下。
10. **list-sessions 字段命名** —— 实际返回字段是 `active` / `archived` / `title`（不是 `isActive` / `isArchived` / `name`，与设计稿 `e2e-aijia-subcommands.md` 不一致，以实际为准）。
11. **last-reply 返回字段是 `text` 不是 `content`** —— 设计稿写的 `content` 字段名错了，实际是 `{id, index, role, text, tool_calls}`。
12. **messages.jsonl 格式** —— 单文件 ndjson（不是分片），每行末尾 `\t✓` 校验位，content 是 `{text: "..."}` 嵌套对象。

### 与 rules.md 对接示例

一条 rule（产品视角）：
> 在新对话发"你好" → AI 回复非空 + 会话被持久化。

翻译成 agent 跑：
```bash
tauri-pilot aijia new-task
tauri-pilot aijia type-message "你好"
tauri-pilot aijia send
tauri-pilot aijia wait-reply --timeout 60
reply=$(tauri-pilot aijia last-reply --json | jq -r .content)
conv=$(tauri-pilot aijia where --json | jq -r .activeConversationId)

[ -n "$reply" ]
test -d ~/.renlijia/users/t_28__u_54/conversations/$conv/
```

3 行 shell 跑完一条 rule —— 这是 "方式二：agent 跑" 的标准形态。

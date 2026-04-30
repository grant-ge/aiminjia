# System Prompt Section Refactor — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `prompts.rs` 改造为 section 化结构（static/dynamic 分层），把当前日期移出 system prompt 改为首条 user message `<system-reminder>` 注入，修复 `build_system_prompt` 中被忽略的 `is_analysis` 参数，实现 tool_defs 按模式精确过滤，并从 DB 加载多轮历史对话。

**Architecture:** `prompts.rs` 新增 `PromptMode` enum 和 `SystemPromptParts` struct，提供 `build_system_prompt_parts()` 函数，旧 `get_system_prompt()` 降级为 shim。`RuntimeLlmExecutor` trait 新增 `get_tool_defs(is_analysis)` 和 `load_history(conversation_id)` 两个方法（带默认实现）。`run_chat_turn_s4` 在初始化 `TurnIterationState` 时注入 `<system-reminder>` user message 并加载历史，`TurnConfig.tool_defs` 从 registry 按模式过滤填充。

**Tech Stack:** Rust, Tauri v2, async_trait, serde_json, chrono

---

## File Structure

### 修改文件

| 文件 | 改什么 |
|------|--------|
| `src-tauri/src/llm/prompts.rs` | 新增 `PromptMode`, `SystemPromptParts`, `build_system_prompt_parts()`；`get_system_prompt()` 降级为 shim；移除日期行生成 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | `RuntimeLlmExecutor` trait 新增 `get_tool_defs()` 和 `load_history()`；`run_chat_turn_s4` 注入 system-reminder user message、加载历史、填充 `tool_defs` |
| `src-tauri/src/transport/tauri_commands/chat.rs` | `TauriLegacyTurnExecutor::build_system_prompt` 修复 `is_analysis` 分支；实现 `get_tool_defs()` 和 `load_history()` |

### 无新建文件

工具选择偏好章节作为 Rust const 写入 `prompts.rs`，不新增 `.md` 文件（避免 3 级加载链变复杂）。

---

## Task 1: prompts.rs — section 化重构 + 工具选择偏好章节

**Files:**
- Modify: `src-tauri/src/llm/prompts.rs`
- Test: `src-tauri/src/llm/prompts.rs` (mod tests 内)

- [ ] **Step 1: 写测试（先红）**

在 `prompts.rs` 的 `mod tests` 末尾追加以下测试，确保在实现前**全部红**：

```rust
#[test]
fn test_build_system_prompt_parts_daily_has_static_and_dynamic() {
    let _guard = PROMPT_TEST_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    setup_prompts(
        &bundled,
        &[("base", "AI小家 base"), ("daily", "日常工作助手")],
    );
    fs::create_dir_all(&user).unwrap();
    init_prompts(&bundled, &user);

    let parts = build_system_prompt_parts(PromptMode::Daily, None, None);
    assert!(parts.static_section.contains("AI小家 base"),
        "static_section must contain base prompt");
    assert!(parts.static_section.contains("工具选择偏好"),
        "static_section must contain tool preference section");
    assert!(!parts.static_section.contains("今天是"),
        "static_section must NOT contain date");
    assert!(parts.dynamic_section.contains("日常工作助手"),
        "dynamic_section must contain daily prompt");
    assert!(!parts.dynamic_section.contains("AI小家 base"),
        "dynamic_section must NOT repeat base prompt");
}

#[test]
fn test_build_system_prompt_parts_analysis_no_daily() {
    let _guard = PROMPT_TEST_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    setup_prompts(
        &bundled,
        &[("base", "AI小家 base"), ("daily", "日常工作助手")],
    );
    fs::create_dir_all(&user).unwrap();
    init_prompts(&bundled, &user);

    let parts = build_system_prompt_parts(PromptMode::Analysis, None, None);
    assert!(!parts.dynamic_section.contains("日常工作助手"),
        "Analysis dynamic_section must NOT contain daily prompt");
}

#[test]
fn test_build_system_prompt_parts_product_name_replacement() {
    let _guard = PROMPT_TEST_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    setup_prompts(&bundled, &[("base", "你是 AI小家"), ("daily", "")]);
    fs::create_dir_all(&user).unwrap();
    init_prompts(&bundled, &user);

    let parts = build_system_prompt_parts(PromptMode::Daily, None, Some("智能办公"));
    assert!(parts.static_section.contains("智能办公"),
        "product_name replacement must work in static_section");
    assert!(!parts.static_section.contains("AI小家"),
        "original brand name must be replaced");
}

#[test]
fn test_build_system_prompt_parts_persona_in_dynamic() {
    use crate::storage::file_store::persona::Persona;
    let _guard = PROMPT_TEST_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    setup_prompts(&bundled, &[("base", "AI小家"), ("daily", "日常")]);
    fs::create_dir_all(&user).unwrap();
    init_prompts(&bundled, &user);

    let persona = Persona {
        id: "test".to_string(),
        version: 1,
        builtin: false,
        name: "Test".to_string(),
        icon: "🧪".to_string(),
        description: "test".to_string(),
        name_en: "".to_string(),
        description_en: "".to_string(),
        identity: "你是专业 HR 顾问".to_string(),
        expertise: vec!["薪酬分析".to_string()],
        memory_hints: vec![],
        linked_categories: vec![],
        created_at: "2026-01-01".to_string(),
        updated_at: "2026-01-01".to_string(),
    };

    let parts = build_system_prompt_parts(PromptMode::Daily, Some(&persona), None);
    assert!(parts.dynamic_section.contains("你是专业 HR 顾问"),
        "persona identity must appear in dynamic_section");
    assert!(parts.dynamic_section.contains("薪酬分析"),
        "persona expertise must appear in dynamic_section");
    assert!(!parts.static_section.contains("你是专业 HR 顾问"),
        "persona must NOT be in static_section");
}

#[test]
fn test_get_system_prompt_shim_backward_compatible() {
    let _guard = PROMPT_TEST_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    setup_prompts(&bundled, &[("base", "AI小家 base"), ("daily", "日常工作助手")]);
    fs::create_dir_all(&user).unwrap();
    init_prompts(&bundled, &user);

    let prompt = get_system_prompt(None, None, None);
    assert!(prompt.contains("AI小家 base"), "shim: base must be present");
    assert!(prompt.contains("日常工作助手"), "shim: daily must be present for step=None");
    let prompt_step = get_system_prompt(Some(0), None, None);
    assert!(prompt_step.contains("AI小家 base"), "shim: base must be present for step");
    assert!(!prompt_step.contains("日常工作助手"), "shim: daily must be absent for step=Some");
    assert!(!prompt.contains("今天是"), "shim: date must NOT be in system prompt");
}

#[test]
fn test_tool_preference_section_content() {
    let _guard = PROMPT_TEST_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let bundled = tmp.path().join("bundled");
    let user = tmp.path().join("user");
    setup_prompts(&bundled, &[("base", "AI小家"), ("daily", "")]);
    fs::create_dir_all(&user).unwrap();
    init_prompts(&bundled, &user);

    let parts = build_system_prompt_parts(PromptMode::Daily, None, None);
    assert!(parts.static_section.contains("优先使用专用工具"),
        "must mention prefer dedicated tools");
    assert!(parts.static_section.contains("execute_python"),
        "must mention execute_python in context");
    assert!(parts.static_section.contains("web_search"),
        "must mention web_search in context");
    assert!(parts.static_section.contains("save_memory"),
        "must mention save_memory in context");
}
```

- [ ] **Step 2: 验证测试失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test prompts -- --nocapture 2>&1 | grep -E "FAILED|error\[E"
```

预期：`build_system_prompt_parts`、`PromptMode`、`SystemPromptParts` 编译错误。

- [ ] **Step 3a: 新增类型和工具偏好常量**

在 `prompts.rs` 中，插入到现有 `use` 语句之后、`PromptStore` 之前：

```rust
// 工具选择偏好章节——静态内容，写入 static_section
const TOOL_PREFERENCE_SECTION: &str = r#"

【工具选择偏好】
- 优先使用专用工具：有专用工具时，不要改用 execute_python 模拟
- 文件操作：读文件用 load_file/read_workspace_file，不要用 execute_python 读文件
- 搜索：信息查询用 web_search，不要伪造搜索结果
- 内存：需要记忆时用 save_memory/search_memory，不要依赖对话历史
- 分析：数据分析用 execute_python，结果必须来自实际执行"#;

/// System prompt 构建模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    /// 日常助手模式（base + 工具偏好 + daily.md + persona）
    Daily,
    /// 分析步骤模式（base + 工具偏好，无 daily.md，无 persona）
    Analysis,
    /// 浏览器子代理模式（base + 工具偏好 + browser_agent.md）
    BrowserAgent,
}

/// System prompt 的分层结构。
///
/// `static_section`：可跨会话复用的稳定内容（base + 工具偏好）。
/// `dynamic_section`：会话级动态内容（persona + mode-specific prompt）。
///
/// 调用者通过 `static_section + "\n\n" + dynamic_section` 拼接成完整 system prompt。
#[derive(Debug, Clone)]
pub struct SystemPromptParts {
    /// 稳定前缀（base.md 品牌替换后 + 工具选择偏好章节）
    pub static_section: String,
    /// 动态后缀（persona 段 + daily.md / browser_agent.md，Analysis 时为空）
    pub dynamic_section: String,
}
```

- [ ] **Step 3b: 新增 `build_system_prompt_parts()` 函数**

插入到现有 `get_system_prompt()` 之前：

```rust
/// 构建分层 system prompt（section 化版本）。
///
/// - `static_section` = base.md（品牌替换后）+ TOOL_PREFERENCE_SECTION
/// - `dynamic_section` = persona 段 + mode-specific prompt（Analysis 时无 daily.md）
///
/// **注意：** 不再注入当前日期——日期改为首条 user message `<system-reminder>` 注入。
pub fn build_system_prompt_parts(
    mode: PromptMode,
    persona: Option<&crate::storage::file_store::persona::Persona>,
    product_name: Option<&str>,
) -> SystemPromptParts {
    let guard = PROMPT_STORE.read().expect("PromptStore read lock poisoned");

    // ── static_section: base.md + 品牌替换 + 工具偏好 ───────────────
    let base_raw = guard.get("base");
    let base = match product_name {
        Some(name) if !name.is_empty() && name != "AI小家" => base_raw.replace("AI小家", name),
        _ => base_raw.to_string(),
    };
    let static_section = format!("{}{}", base, TOOL_PREFERENCE_SECTION);

    // ── dynamic_section: persona + mode prompt ───────────────────────
    let mut dynamic_parts: Vec<String> = Vec::new();

    // Daily 和 BrowserAgent 模式注入 persona
    if matches!(mode, PromptMode::Daily | PromptMode::BrowserAgent) {
        if let Some(p) = persona {
            if !p.identity.is_empty() {
                dynamic_parts.push(format!("【角色设定】{}", p.identity));
            }
            if !p.expertise.is_empty() {
                dynamic_parts.push(format!("【专业领域】{}", p.expertise.join("、")));
            }
            if !p.memory_hints.is_empty() {
                let hints = p.memory_hints.iter()
                    .map(|h| format!("- {}", h))
                    .collect::<Vec<_>>()
                    .join("\n");
                dynamic_parts.push(format!("【记忆管理（白名单制）】\n{}", hints));
            }
        }
    }

    // Mode-specific prompt
    match mode {
        PromptMode::Daily => {
            let daily = guard.get("daily");
            if !daily.is_empty() {
                let has_persona_memory = persona.map_or(false, |p| !p.memory_hints.is_empty());
                let final_daily = if has_persona_memory {
                    strip_memory_section(daily)
                } else {
                    daily.to_string()
                };
                if !final_daily.trim().is_empty() {
                    dynamic_parts.push(final_daily);
                }
            }
        }
        PromptMode::Analysis => {
            // analysis 模式不加 daily.md
        }
        PromptMode::BrowserAgent => {
            let browser = guard.get("browser_agent");
            if !browser.is_empty() {
                dynamic_parts.push(browser.to_string());
            }
        }
    }

    let dynamic_section = dynamic_parts.join("\n\n");

    SystemPromptParts { static_section, dynamic_section }
}
```

- [ ] **Step 3c: 改造 `get_system_prompt()` 为 shim**

将现有 `get_system_prompt()` 函数体替换为：

```rust
/// Compose the full system prompt (backward-compatible shim).
///
/// 调用 `build_system_prompt_parts` 后拼接 static + dynamic。
/// **不再注入当前日期**——日期改为 `run_chat_turn_s4` 的首条 user message 注入。
///
/// - `step = None` → PromptMode::Daily
/// - `step = Some(_)` → PromptMode::Analysis
pub fn get_system_prompt(
    step: Option<u32>,
    persona: Option<&crate::storage::file_store::persona::Persona>,
    product_name: Option<&str>,
) -> String {
    let mode = match step {
        None => PromptMode::Daily,
        Some(_) => PromptMode::Analysis,
    };
    let parts = build_system_prompt_parts(mode, persona, product_name);
    if parts.dynamic_section.is_empty() {
        parts.static_section
    } else {
        format!("{}\n\n{}", parts.static_section, parts.dynamic_section)
    }
}
```

- [ ] **Step 4: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test prompts -- --nocapture
```

所有 prompts 测试全绿。若 `test_api_unchanged` 有断言检查"今天是"，删除该行断言（日期已移出 system prompt 是期望的行为变更）。

- [ ] **Step 5: commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
git add src/llm/prompts.rs
git commit -m "feat(prompts): section化重构 — SystemPromptParts + PromptMode + 工具选择偏好章节

- 新增 PromptMode enum (Daily/Analysis/BrowserAgent)
- 新增 SystemPromptParts struct (static_section + dynamic_section)
- 新增 build_system_prompt_parts() 主函数
- static_section = base.md + TOOL_PREFERENCE_SECTION（工具选择偏好）
- dynamic_section = persona + mode-specific prompt
- 移除 system prompt 中的日期注入（改由 run_chat_turn_s4 注入首条 user message）
- get_system_prompt() 降级为向后兼容 shim"
```

---

## Task 2: run_chat_turn_s4 — 当前日期外移为 `<system-reminder>` user message

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 写测试（先红）**

在 `s4_driver_loop_test.rs` 末尾追加：

```rust
struct RecordingMockExecutor {
    responses: std::sync::Mutex<Vec<LlmStepResult>>,
    received_messages: std::sync::Mutex<Vec<Vec<serde_json::Value>>>,
}

impl RecordingMockExecutor {
    fn new(responses: Vec<LlmStepResult>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
            received_messages: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn all_messages(&self) -> Vec<Vec<serde_json::Value>> {
        self.received_messages.lock().unwrap().clone()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for RecordingMockExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.received_messages.lock().unwrap().push(input.messages.clone());
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }
}

#[tokio::test]
async fn driver_s4_injects_system_reminder_as_first_user_message() {
    let executor = Arc::new(RecordingMockExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-reminder");
    let request = ChatTurnRequest::new("conv-reminder", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let all_messages = executor.all_messages();
    assert!(!all_messages.is_empty(), "executor must have received messages");
    let first_call_messages = &all_messages[0];

    let first_msg = &first_call_messages[0];
    assert_eq!(first_msg["role"], "user", "first message must be user role");
    let content = first_msg["content"].as_str().unwrap_or("");
    assert!(content.contains("<system-reminder>"),
        "first user message must contain <system-reminder> tag, got: {}", content);
    assert!(content.contains("今天是"),
        "system-reminder must contain date info, got: {}", content);
    assert!(content.contains("</system-reminder>"),
        "system-reminder must have closing tag, got: {}", content);
}

#[tokio::test]
async fn driver_s4_system_reminder_precedes_user_content_message() {
    let executor = Arc::new(RecordingMockExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-reminder-order");
    let request = ChatTurnRequest::new("conv-reminder-order", "what is today?", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let first_call_messages = &executor.all_messages()[0];
    assert!(first_call_messages.len() >= 2,
        "must have at least system-reminder + user content");

    let first = &first_call_messages[0];
    let second = &first_call_messages[1];

    assert!(first["content"].as_str().unwrap_or("").contains("<system-reminder>"),
        "index 0 must be system-reminder");
    assert_eq!(second["content"], "what is today?",
        "index 1 must be the actual user content");
}
```

- [ ] **Step 2: 验证测试失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test driver_s4_injects_system_reminder -- --nocapture
```

预期：断言失败（当前代码不注入 system-reminder）。

- [ ] **Step 3: 实现 run_chat_turn_s4 中的日期注入**

在 `chat_turn_driver.rs` 的 `run_chat_turn_s4` 的 Step 2 初始化处，将现有 user message 初始化替换为：

```rust
// ── Step 2: Initialize iteration state ───────────────────────────────
// messages[0] = <system-reminder>（日期注入，对齐 claude-code-best getUserContext()）
// messages[1] = 用户实际 content
let now = chrono::Local::now();
let today = now.format("%Y年%m月%d日");
let today_iso = now.format("%Y-%m-%d");
let system_reminder_message = serde_json::json!({
    "role": "user",
    "content": format!(
        "<system-reminder>\n今天是 {}（{}）。\n</system-reminder>",
        today, today_iso
    ),
});
let user_message = serde_json::json!({
    "role": "user",
    "content": request.content,
});
let mut state = TurnIterationState::new(vec![system_reminder_message, user_message]);
```

- [ ] **Step 4: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test driver_s4 -- --nocapture
```

- [ ] **Step 5: commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
git add src/runtime/chat/chat_turn_driver.rs tests/s4_driver_loop_test.rs
git commit -m "feat(chat-driver): 日期从 system prompt 外移为首条 <system-reminder> user message

将 chrono::Local::now() 格式化为中文 + ISO 日期注入到 TurnIterationState
的 messages[0]（<system-reminder> user message），messages[1] 为实际用户内容。
对齐 claude-code-best getUserContext() 模式。"
```

---

## Task 3: build_system_prompt — 修复 is_analysis 分支

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 写测试（先红）**

在 `s4_driver_loop_test.rs` 末尾追加：

```rust
struct CapturingMockExecutor {
    is_analysis: bool,
    responses: std::sync::Mutex<Vec<LlmStepResult>>,
    received_system_prompts: std::sync::Mutex<Vec<String>>,
}

impl CapturingMockExecutor {
    fn new_daily() -> Self {
        Self {
            is_analysis: false,
            responses: std::sync::Mutex::new(vec![
                LlmStepResult::ContentComplete {
                    content: "ok".to_string(),
                    tokens_in: 0,
                    tokens_out: 0,
                },
            ]),
            received_system_prompts: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn new_analysis() -> Self {
        Self {
            is_analysis: true,
            responses: std::sync::Mutex::new(vec![
                LlmStepResult::ContentComplete {
                    content: "ok".to_string(),
                    tokens_in: 0,
                    tokens_out: 0,
                },
            ]),
            received_system_prompts: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CapturingMockExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.received_system_prompts.lock().unwrap()
            .push(input.system_prompt.to_string());
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn build_system_prompt(
        &self,
        _conversation_id: &str,
        is_analysis: bool,
    ) -> Result<String, TurnError> {
        if is_analysis {
            Ok("[ANALYSIS-SYSTEM-PROMPT]".to_string())
        } else {
            Ok("[DAILY-SYSTEM-PROMPT]".to_string())
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-id".to_string())
    }
}

#[tokio::test]
async fn driver_s4_passes_is_analysis_true_to_build_system_prompt() {
    let executor = Arc::new(CapturingMockExecutor::new_analysis());
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-analysis");
    let request = ChatTurnRequest::new_analysis("conv-analysis", "analyze data", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let prompts = executor.received_system_prompts.lock().unwrap();
    assert!(!prompts.is_empty());
    assert_eq!(prompts[0], "[ANALYSIS-SYSTEM-PROMPT]",
        "analysis mode must use analysis system prompt, got: {}", prompts[0]);
}
```

- [ ] **Step 2: ChatTurnRequest 新增 is_analysis 字段**

在 `chat_turn_driver.rs` 中更新 `ChatTurnRequest`：

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurnRequest {
    pub conversation_id: String,
    pub content: String,
    pub file_ids: Vec<String>,
    pub run_id: RunId,
    /// true = analysis 步骤模式；false = daily 日常模式（默认）
    pub is_analysis: bool,
}

impl ChatTurnRequest {
    pub fn new(
        conversation_id: impl Into<String>,
        content: impl Into<String>,
        file_ids: Vec<String>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            content: content.into(),
            file_ids,
            run_id: RunId::new(uuid::Uuid::new_v4().to_string()),
            is_analysis: false,
        }
    }

    /// Convenience constructor for analysis mode turns.
    pub fn new_analysis(
        conversation_id: impl Into<String>,
        content: impl Into<String>,
        file_ids: Vec<String>,
    ) -> Self {
        let mut r = Self::new(conversation_id, content, file_ids);
        r.is_analysis = true;
        r
    }
}
```

- [ ] **Step 3: run_chat_turn_s4 传递 request.is_analysis**

在 `run_chat_turn_s4` Step 1 中，将 `build_system_prompt` 和 `TurnConfig` 更新为：

```rust
let system_prompt = executor
    .build_system_prompt(&request.conversation_id, request.is_analysis)
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;

let config = TurnConfig {
    system_prompt,
    tool_defs: vec![],  // Task 4 填充
    allowed_tools: None,
    max_iterations: 30,
    token_budget: 4096,
    chunk_timeout_secs: 90,
    is_analysis: request.is_analysis,  // 不再硬编码
    masking_level: "strict".to_string(),
    workspace_path: std::path::PathBuf::new(),
    conversation_id: request.conversation_id.clone(),
    run_id: request.run_id.as_str().to_string(),
};
```

- [ ] **Step 4: 修复 TauriLegacyTurnExecutor::build_system_prompt**

在 `chat.rs` 的 `build_system_prompt` 中替换为：

```rust
async fn build_system_prompt(
    &self,
    _conversation_id: &str,
    is_analysis: bool,
) -> Result<String, TurnError> {
    let persona = self.services.db.get_active_persona().ok();

    let product_name: Option<String> = self
        .services
        .auth_manager
        .get_auth_info()
        .await
        .tenant
        .and_then(|t| t.product_name.filter(|n| !n.is_empty()));

    let mode = if is_analysis {
        prompts::PromptMode::Analysis
    } else {
        prompts::PromptMode::Daily
    };

    let parts = prompts::build_system_prompt_parts(
        mode,
        persona.as_ref(),
        product_name.as_deref(),
    );
    let prompt = if parts.dynamic_section.is_empty() {
        parts.static_section
    } else {
        format!("{}\n\n{}", parts.static_section, parts.dynamic_section)
    };

    log::info!(
        "[build_system_prompt] mode={:?} len={} persona={} product_name={}",
        mode,
        prompt.len(),
        persona.as_ref().map(|p| p.identity.as_str()).unwrap_or("(none)"),
        product_name.as_deref().unwrap_or("(none)"),
    );

    Ok(prompt)
}
```

- [ ] **Step 5: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test s4_driver -- --nocapture
```

- [ ] **Step 6: commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
git add src/runtime/chat/chat_turn_driver.rs src/transport/tauri_commands/chat.rs tests/s4_driver_loop_test.rs
git commit -m "fix(chat-driver): is_analysis 不再被忽略，analysis 模式走 PromptMode::Analysis

ChatTurnRequest 新增 is_analysis 字段（默认 false）
run_chat_turn_s4 将 request.is_analysis 传入 build_system_prompt 和 TurnConfig
TauriLegacyTurnExecutor::build_system_prompt 按 mode 路由 PromptMode::Daily/Analysis
移除 let _ = is_analysis 忽略逻辑"
```

---

## Task 4: tool_defs 精确传递

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 写测试（先红）**

在 `s4_driver_loop_test.rs` 末尾追加：

```rust
struct ToolDefsCapturingExecutor {
    is_analysis: bool,
    captured_tool_defs: std::sync::Mutex<Vec<Vec<serde_json::Value>>>,
}

impl ToolDefsCapturingExecutor {
    fn new(is_analysis: bool) -> Self {
        Self {
            is_analysis,
            captured_tool_defs: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for ToolDefsCapturingExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.captured_tool_defs.lock().unwrap()
            .push(input.tool_defs.to_vec());
        Ok(LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
        })
    }

    async fn get_tool_defs(
        &self,
        is_analysis: bool,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        use app_lib::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;
        let names: Vec<String> = if is_analysis {
            vec!["all_tool_a".to_string(), "all_tool_b".to_string()]
        } else {
            DAILY_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect()
        };
        Ok(names.iter().map(|n| serde_json::json!({"name": n, "description": ""})).collect())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-id".to_string())
    }
}

#[tokio::test]
async fn driver_s4_tool_defs_non_empty_in_daily_mode() {
    let executor = Arc::new(ToolDefsCapturingExecutor::new(false));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-tool-defs-daily");
    let request = ChatTurnRequest::new("conv-tool-defs-daily", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_tool_defs.lock().unwrap();
    assert!(!captured.is_empty(), "must have captured tool defs");
    assert!(!captured[0].is_empty(),
        "tool_defs must be non-empty for daily mode (was vec![] before fix)");
}

#[tokio::test]
async fn driver_s4_daily_tool_defs_match_whitelist() {
    use app_lib::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;
    let executor = Arc::new(ToolDefsCapturingExecutor::new(false));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-tool-defs-whitelist");
    let request = ChatTurnRequest::new("conv-tool-defs-whitelist", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_tool_defs.lock().unwrap();
    let received_names: std::collections::HashSet<String> = captured[0]
        .iter()
        .filter_map(|v| v["name"].as_str())
        .map(|s| s.to_string())
        .collect();

    for allowed in DAILY_ALLOWED_TOOLS {
        assert!(received_names.contains(*allowed),
            "daily whitelist tool '{}' must be in tool_defs", allowed);
    }
}
```

- [ ] **Step 2: RuntimeLlmExecutor trait 新增 `get_tool_defs` 方法**

在 `chat_turn_driver.rs` 的 `RuntimeLlmExecutor` trait 中添加：

```rust
/// 返回本次 Turn 使用的 tool definitions（JSON schema）。
///
/// - `is_analysis=false`（daily）：从 registry 按 DAILY_ALLOWED_TOOLS 白名单过滤
/// - `is_analysis=true`（analysis）：全量工具
///
/// 默认实现返回空 vec（向后兼容旧 mock executor）。
/// 生产 executor（TauriLegacyTurnExecutor）必须 override。
async fn get_tool_defs(
    &self,
    _is_analysis: bool,
) -> Result<Vec<serde_json::Value>, TurnError> {
    Ok(vec![])
}
```

- [ ] **Step 3: run_chat_turn_s4 调用 get_tool_defs 填充 TurnConfig**

将 Step 1 的 `tool_defs: vec![]` 替换为：

```rust
let tool_defs = executor
    .get_tool_defs(request.is_analysis)
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;

let config = TurnConfig {
    system_prompt,
    tool_defs,   // 不再 vec![]
    // ... 其余字段不变
};
```

- [ ] **Step 4: TauriLegacyTurnExecutor 实现 get_tool_defs**

在 `chat.rs` 中添加：

```rust
async fn get_tool_defs(
    &self,
    is_analysis: bool,
) -> Result<Vec<serde_json::Value>, TurnError> {
    use crate::plugin::skill_trait::ToolFilter;
    use crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;

    let filter = if is_analysis {
        ToolFilter::All
    } else {
        ToolFilter::Only(
            DAILY_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect()
        )
    };

    let tool_definitions = self.services.tool_registry
        .get_schemas_filtered(&filter)
        .await;

    // ToolDefinition 实现了 Serialize（参考 run_llm_step 中的 from_value 调用）
    let json_defs: Vec<serde_json::Value> = tool_definitions
        .into_iter()
        .filter_map(|td| serde_json::to_value(&td).ok())
        .collect();

    log::info!(
        "[get_tool_defs] is_analysis={} returned {} tool definitions",
        is_analysis,
        json_defs.len(),
    );

    Ok(json_defs)
}
```

**注意：** 若 `ToolDefinition` 未实现 `Serialize`，改用手动构建（字段名用 `"parameters"`，与 `ToolDefinition` 的 serde 字段名一致，确保 `run_llm_step` 中 `from_value::<ToolDefinition>` 可以反序列化）：
```rust
.map(|td| serde_json::json!({
    "name": td.name,
    "description": td.description,
    "parameters": td.parameters,
}))
```

- [ ] **Step 5: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test tool_defs -- --nocapture
```

- [ ] **Step 6: commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
git add src/runtime/chat/chat_turn_driver.rs src/transport/tauri_commands/chat.rs tests/s4_driver_loop_test.rs
git commit -m "feat(chat-driver): tool_defs 精确传递——daily 用白名单，analysis 用全量

RuntimeLlmExecutor trait 新增 get_tool_defs(is_analysis) 方法
run_chat_turn_s4 调用 get_tool_defs 填充 TurnConfig.tool_defs，不再 vec![]
TauriLegacyTurnExecutor 实现：daily 模式用 DAILY_ALLOWED_TOOLS 过滤，analysis 全量
修复 gateway fallback 问题：tool_defs 非空时 run_llm_step 使用精确列表"
```

---

## Task 5: 多轮历史加载

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: `src-tauri/tests/s4_driver_loop_test.rs`

- [ ] **Step 1: 写测试（先红）**

在 `s4_driver_loop_test.rs` 末尾追加：

```rust
struct HistoryAwareMockExecutor {
    history: Vec<serde_json::Value>,
    captured_initial_messages: std::sync::Mutex<Vec<serde_json::Value>>,
}

impl HistoryAwareMockExecutor {
    fn new(history: Vec<serde_json::Value>) -> Self {
        Self {
            history,
            captured_initial_messages: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for HistoryAwareMockExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let mut captured = self.captured_initial_messages.lock().unwrap();
        if captured.is_empty() {
            *captured = input.messages.clone();
        }
        Ok(LlmStepResult::ContentComplete {
            content: "response".to_string(),
            tokens_in: 0,
            tokens_out: 0,
        })
    }

    async fn load_history(
        &self,
        _conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(self.history.clone())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-id".to_string())
    }
}

#[tokio::test]
async fn driver_s4_loads_history_into_messages() {
    let history = vec![
        serde_json::json!({"role": "user", "content": "previous question"}),
        serde_json::json!({"role": "assistant", "content": "previous answer"}),
    ];
    let executor = Arc::new(HistoryAwareMockExecutor::new(history.clone()));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-history");
    let request = ChatTurnRequest::new("conv-history", "current question", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_initial_messages.lock().unwrap();
    assert!(!captured.is_empty(), "must have captured messages");

    let has_prev_question = captured.iter().any(|m| m["content"].as_str() == Some("previous question"));
    let has_prev_answer = captured.iter().any(|m| m["content"].as_str() == Some("previous answer"));
    assert!(has_prev_question, "history: 'previous question' must be in messages");
    assert!(has_prev_answer, "history: 'previous answer' must be in messages");

    let has_current = captured.iter().any(|m| m["content"].as_str() == Some("current question"));
    assert!(has_current, "current user content must be in messages");
}

#[tokio::test]
async fn driver_s4_message_order_is_reminder_history_current() {
    let history = vec![
        serde_json::json!({"role": "user", "content": "past user msg"}),
        serde_json::json!({"role": "assistant", "content": "past assistant msg"}),
    ];
    let executor = Arc::new(HistoryAwareMockExecutor::new(history));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-order");
    let request = ChatTurnRequest::new("conv-order", "new msg", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_initial_messages.lock().unwrap();
    assert!(
        captured[0]["content"].as_str().unwrap_or("").contains("<system-reminder>"),
        "messages[0] must be system-reminder, got: {:?}", captured[0]
    );
    let last = captured.last().unwrap();
    assert_eq!(last["content"], "new msg",
        "last message must be current user content");

    let middle_contents: Vec<&str> = captured[1..captured.len()-1]
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert!(middle_contents.contains(&"past user msg"), "history user msg must be in middle");
    assert!(middle_contents.contains(&"past assistant msg"), "history assistant msg must be in middle");
}

#[tokio::test]
async fn driver_s4_empty_history_works_normally() {
    let executor = Arc::new(HistoryAwareMockExecutor::new(vec![]));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-no-history");
    let request = ChatTurnRequest::new("conv-no-history", "first message", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "must work without history");

    let captured = executor.captured_initial_messages.lock().unwrap();
    assert_eq!(captured.len(), 2,
        "without history: messages must be [system-reminder, user-content]");
}
```

- [ ] **Step 2: RuntimeLlmExecutor trait 新增 `load_history` 方法**

```rust
/// 加载 conversation 的历史对话消息（格式：[{role, content}, ...]）。
///
/// 返回的消息将被插入到 messages 中（在 system-reminder 之后、当前 user message 之前）。
/// 默认实现返回空 vec（无历史）。生产 executor 必须 override。
async fn load_history(
    &self,
    _conversation_id: &str,
) -> Result<Vec<serde_json::Value>, TurnError> {
    Ok(vec![])
}
```

- [ ] **Step 3: run_chat_turn_s4 加载历史并拼装 messages**

> **⚠️ 注意依赖：** 此 Step 会**完整替换** Task 2 Step 3 写入的代码块。定位时需找到以下 anchor（Task 2 Step 3 留下的代码）：
> ```rust
> // ── Step 2: Initialize iteration state ───────────────────────────────
> // messages[0] = <system-reminder>（日期注入，对齐 claude-code-best getUserContext()）
> // messages[1] = 用户实际 content
> let now = chrono::Local::now();
> ...
> let mut state = TurnIterationState::new(vec![system_reminder_message, user_message]);
> ```
> 用下方完整代码整体替换该段。

替换为（消息顺序：[system-reminder, ...history, user]）：

```rust
// ── Step 2: Initialize iteration state ───────────────────────────────
// messages 顺序：[system-reminder, ...history, current-user-content]

let now = chrono::Local::now();
let today = now.format("%Y年%m月%d日");
let today_iso = now.format("%Y-%m-%d");
let system_reminder_message = serde_json::json!({
    "role": "user",
    "content": format!(
        "<system-reminder>\n今天是 {}（{}）。\n</system-reminder>",
        today, today_iso
    ),
});

// 加载历史对话
let history = executor
    .load_history(&request.conversation_id)
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;

let user_message = serde_json::json!({
    "role": "user",
    "content": request.content,
});

let mut initial_messages = Vec::with_capacity(1 + history.len() + 1);
initial_messages.push(system_reminder_message);
initial_messages.extend(history);
initial_messages.push(user_message);

let mut state = TurnIterationState::new(initial_messages);
```

- [ ] **Step 4: TauriLegacyTurnExecutor 实现 load_history**

```rust
async fn load_history(
    &self,
    conversation_id: &str,
) -> Result<Vec<serde_json::Value>, TurnError> {
    const HISTORY_LIMIT: u32 = 50;

    let raw_messages = self.services.db
        .get_recent_messages(conversation_id, HISTORY_LIMIT)
        .map_err(|e| TurnError::PersistenceError(format!(
            "Failed to load conversation history: {}", e
        )))?;

    // 转换格式：content 可能是 {"text": "..."} 或直接字符串
    let chat_messages: Vec<serde_json::Value> = raw_messages
        .into_iter()
        .filter_map(|msg| {
            let role = msg["role"].as_str()?.to_string();
            let content = if let Some(text) = msg["content"]["text"].as_str() {
                text.to_string()
            } else if let Some(text) = msg["content"].as_str() {
                text.to_string()
            } else {
                return None;
            };
            if content.trim().is_empty() {
                return None;
            }
            Some(serde_json::json!({
                "role": role,
                "content": content,
            }))
        })
        .collect();

    log::info!(
        "[load_history] conv={} loaded {} messages (limit={})",
        conversation_id,
        chat_messages.len(),
        HISTORY_LIMIT,
    );

    Ok(chat_messages)
}
```

- [ ] **Step 5: 验证测试通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test history -- --nocapture
```

- [ ] **Step 6: 完整回归测试**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test -- --nocapture 2>&1 | tail -20
```

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast
```

- [ ] **Step 7: commit**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
git add src/runtime/chat/chat_turn_driver.rs src/transport/tauri_commands/chat.rs tests/s4_driver_loop_test.rs
git commit -m "feat(chat-driver): 多轮历史加载——TurnIterationState.messages 从 DB 填充

RuntimeLlmExecutor 新增 load_history() 方法（默认空）
run_chat_turn_s4 消息顺序：[system-reminder, ...history, current-user]
TauriLegacyTurnExecutor 实现：调 get_recent_messages(50) 并转换为 {role, content} 格式
修复多轮对话跨 session 失忆问题"
```

---

## Self-Review

### Spec 覆盖度检查

| 目标 | 覆盖 Task |
|------|-----------|
| section 化结构（static + dynamic 分层） | Task 1：`SystemPromptParts`, `build_system_prompt_parts()` |
| 工具选择偏好章节 | Task 1：`TOOL_PREFERENCE_SECTION` const，写入 `static_section` |
| 当前日期外移为首条 `<system-reminder>` user message | Task 2 + Task 5 Step 3 |
| `is_analysis` 不再被忽略 | Task 3 |
| `tool_defs` 精确传递（daily 白名单，analysis 全量） | Task 4 |
| 多轮历史加载 | Task 5 |

### 类型一致性检查

- `PromptMode` 在 `prompts.rs` 中定义为 `pub enum`，所有调用处均使用 `prompts::PromptMode::*`
- `SystemPromptParts.static_section` 和 `dynamic_section` 均为 `String`
- `get_tool_defs()` 返回 `Vec<serde_json::Value>`，与 `TurnConfig.tool_defs: Vec<JsonValue>` 一致
- `load_history()` 返回 `Vec<serde_json::Value>`，与 `TurnIterationState.messages` 一致
- `ChatTurnRequest::new()` 签名不变（`is_analysis` 新字段默认 `false`）
- `RuntimeLlmExecutor` 新增方法均有默认实现，不破坏现有 mock

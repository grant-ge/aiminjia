# LLM Skill Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 skill 初始路由从关键词匹配改成 LLM 语义判断，并让 `switch_skill` 工具的描述中注入所有合法 skill ID，防止 LLM 幻觉。

**Architecture:** 在 `resolve_turn_context`（`skill_session.rs`）里，当对话处于 default skill 时，不再调用 `detect_activation`（关键词匹配），而是构建一个包含所有可用 skill id + description 的 system prompt 片段注入到当前 turn，让主 LLM 在第一次回复时自行判断是否调用 `switch_skill`。同时，`switch_skill` 的 catalog 定义改为动态注入合法 skill ID 列表（通过 `SwitchSkillRuntimeTool::definition()` 返回动态 schema），防止 LLM 填写不存在的 ID。

**Tech Stack:** Rust（`plugin/registry.rs`, `runtime/chat/skill_session.rs`, `runtime/tools/builtin/switch_skill.rs`, `runtime/tools/catalog.rs`）

---

## 文件改动地图

| 文件 | 动作 | 职责 |
|------|------|------|
| `src-tauri/src/runtime/chat/skill_session.rs` | 修改 | 移除 `detect_activation` 调用；在 default skill 下，将 skill 目录注入 system prompt |
| `src-tauri/src/runtime/tools/builtin/switch_skill.rs` | 修改 | `definition()` 动态读取 skill_registry 构建含合法 skill ID enum 的 schema |
| `src-tauri/src/runtime/tools/catalog.rs` | 修改（可选） | switch_skill 的静态 fallback description 里去掉举例的具体 skill ID |
| `src-tauri/tests/skill_routing_llm_test.rs` | 新增 | 验证 default skill 下 system_prompt 包含 skill 目录；验证 switch_skill definition 含正确 enum |

---

## Task 1：给 `switch_skill` 注入合法 skill ID enum（防幻觉）

这是优先级最高的修复，独立可部署，不影响现有路由逻辑。

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/switch_skill.rs`
- Test: `src-tauri/tests/skill_routing_llm_test.rs`

- [ ] **Step 1: 写失败测试**

新建 `src-tauri/tests/skill_routing_llm_test.rs`，内容：

```rust
mod common;

#[tokio::test]
async fn switch_skill_definition_contains_registered_skill_ids() {
    use std::sync::Arc;
    use aijia::plugin::{SkillRegistry, ToolRegistry};
    use aijia::runtime::chat::SkillSessionStore;
    use aijia::runtime::tools::builtin::switch_skill::SwitchSkillRuntimeTool;
    use aijia::runtime::tools::RuntimeTool;

    let skill_registry = Arc::new(SkillRegistry::new("daily-assistant"));
    // 注册两个假 skill
    common::register_mock_skill(&skill_registry, "comp-analysis-v2", "薪酬分析").await;
    common::register_mock_skill(&skill_registry, "sales-analysis", "销售分析").await;

    let tool_registry = Arc::new(ToolRegistry::new());
    let skill_sessions = Arc::new(SkillSessionStore::new());
    let tool = SwitchSkillRuntimeTool::new(
        skill_registry,
        skill_sessions,
        tool_registry,
    );

    let def = tool.definition();
    let schema = def.input_schema();
    let skill_id_enum = schema
        .get("properties")
        .and_then(|p| p.get("skill_id"))
        .and_then(|p| p.get("enum"))
        .and_then(|e| e.as_array())
        .expect("switch_skill definition must have skill_id enum");

    let ids: Vec<&str> = skill_id_enum
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    assert!(ids.contains(&"comp-analysis-v2"), "must list comp-analysis-v2");
    assert!(ids.contains(&"sales-analysis"), "must list sales-analysis");
    assert!(!ids.contains(&"data-analysis-v2"), "must not list non-existent skill");
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cd src-tauri && cargo test --test skill_routing_llm_test switch_skill_definition -- --nocapture 2>&1 | tail -20
```

期望：编译失败（`register_mock_skill` 不存在）或测试失败。

- [ ] **Step 3: 在 common.rs 里添加 `register_mock_skill` helper**

打开 `src-tauri/tests/common.rs`，在文件末尾添加：

```rust
use aijia::plugin::SkillRegistry;
use std::sync::Arc;

pub async fn register_mock_skill(registry: &Arc<SkillRegistry>, id: &str, description: &str) {
    // DeclarativeSkill 需要一个 PluginManifest，最小构造如下
    use aijia::plugin::manifest::PluginManifest;
    use aijia::plugin::declarative_skill::DeclarativeSkill;
    let manifest = PluginManifest {
        id: id.to_string(),
        name: id.to_string(),
        description: description.to_string(),
        ..Default::default()
    };
    let skill = Arc::new(DeclarativeSkill::from_manifest(manifest, None, None));
    registry.register_skill(skill, "test").await;
}
```

> 注：如果 `PluginManifest::default()` 不存在，需要手动填充必要字段。先编译看报错再补。

- [ ] **Step 4: 修改 `SwitchSkillRuntimeTool::definition()` 为异步动态生成**

`definition()` 在 `RuntimeTool` trait 中是同步的（`fn definition(&self) -> ToolDefinition`），无法直接 await。改法：在 `SwitchSkillRuntimeTool` 上缓存一份 skill ID 列表，在 `execute` 时更新，`definition()` 读缓存。

更简单的方式：新增一个 `async fn build_definition` 方法，`definition()` 返回静态 fallback，在 `execute` 时校验 skill_id 是否合法（已有此逻辑），同时在工具 **description 字符串**里动态列出所有合法 ID。

打开 `src-tauri/src/runtime/tools/builtin/switch_skill.rs`，修改 `definition()` 方法：

```rust
fn definition(&self) -> ToolDefinition {
    // 从 catalog 取基础定义，然后把 skill_id 的 description 替换为动态列表
    // 注意：definition() 是同步的，用 futures::executor::block_on 读 registry
    let skill_ids: Vec<String> = futures::executor::block_on(async {
        self.skill_registry
            .list()
            .await
            .into_iter()
            .map(|s| s.id)
            .collect()
    });

    let ids_str = skill_ids.join(", ");
    let description = format!(
        "切换当前会话 skill。可用的 skill_id 列表：{}。只���填写列表中的 ID，不能填写其他值。",
        ids_str
    );

    let schema = serde_json::json!({
        "type": "object",
        "required": ["skill_id"],
        "properties": {
            "skill_id": {
                "type": "string",
                "description": format!("目标 skill id，必须是以下之一：{}", ids_str),
                "enum": skill_ids,
            }
        }
    });

    ToolDefinition::new("switch_skill", &description).with_input_schema(schema)
}
```

> `ToolDefinition::with_input_schema` 如果不存在，查看 `ToolDefinition` 的构造方法，按实际 API 调整。

- [ ] **Step 5: 确认 `ToolDefinition` 的 API**

```bash
cd src-tauri && grep -n "pub fn\|pub struct\|impl ToolDefinition" src/runtime/tools/definition.rs | head -20
```

按输出调整 Step 4 中的代码。

- [ ] **Step 6: 编译**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -20
```

修复所有编译错误后继续。

- [ ] **Step 7: 运行测试确认通过**

```bash
cd src-tauri && cargo test --test skill_routing_llm_test switch_skill_definition -- --nocapture 2>&1 | tail -20
```

期望：`test switch_skill_definition_contains_registered_skill_ids ... ok`

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/runtime/tools/builtin/switch_skill.rs \
        src-tauri/tests/skill_routing_llm_test.rs \
        src-tauri/tests/common.rs
git commit -m "fix(switch_skill): inject valid skill ID list into tool definition to prevent LLM hallucination"
```

---

## Task 2：在 default skill 的 system prompt 里注入 skill 目录

让 LLM 在第一次回复时就能看到所有可用 skill 的描述，从而自行决定是否调用 `switch_skill`，替代关键词匹配。

**Files:**
- Modify: `src-tauri/src/runtime/chat/skill_session.rs`
- Test: `src-tauri/tests/skill_routing_llm_test.rs`

- [ ] **Step 1: 写失败测试**

在 `src-tauri/tests/skill_routing_llm_test.rs` 追加：

```rust
#[tokio::test]
async fn default_skill_system_prompt_contains_skill_directory() {
    use std::sync::Arc;
    use aijia::plugin::{SkillRegistry, ToolRegistry};
    use aijia::runtime::chat::SkillSessionStore;

    let skill_registry = Arc::new(SkillRegistry::new("daily-assistant"));
    common::register_mock_skill(&skill_registry, "comp-analysis-v2", "专门用于薪酬数据对比分析").await;
    common::register_mock_skill(&skill_registry, "sales-analysis", "销售漏斗和业绩分析").await;
    // 注册 default skill
    common::register_mock_skill(&skill_registry, "daily-assistant", "通用日常助手").await;

    let all_tools: Vec<String> = vec!["switch_skill".to_string()];
    let skill_sessions = SkillSessionStore::new();

    let ctx = skill_sessions
        .resolve_turn_context(
            &skill_registry,
            &all_tools,
            "conv-test-001",
            "帮我生成一个 Excel 示例",  // 不包含任何关键词
            false,
        )
        .await
        .expect("resolve_turn_context should succeed");

    // 当前 skill 应该是 default（关键词不匹配任何 skill）
    assert_eq!(ctx.skill_id, "daily-assistant");

    // system_prompt 里应该包含 skill 目录
    assert!(
        ctx.system_prompt.contains("comp-analysis-v2"),
        "system_prompt must list available skill IDs"
    );
    assert!(
        ctx.system_prompt.contains("销售漏斗和业绩分析"),
        "system_prompt must include skill descriptions"
    );
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cd src-tauri && cargo test --test skill_routing_llm_test default_skill_system_prompt -- --nocapture 2>&1 | tail -20
```

期望：测试失败（system_prompt 不包含 skill 目录）。

- [ ] **Step 3: 修改 `resolve_turn_context`，移除关键词匹配，改为注入 skill 目录**

打开 `src-tauri/src/runtime/chat/skill_session.rs`，找到这段逻辑（约第 110 行）：

```rust
} else if let Some(next_skill_id) = registry
    .detect_activation(user_message, has_files, default_skill.id())
    .await
{
    if let Some(next_skill) = registry.get(next_skill_id.as_str()).await {
        state = initial_state_for_skill(next_skill.as_ref(), has_files);
        skill = next_skill;
    }
}
```

替换为：

```rust
// LLM-based routing: 不做关键词匹配，而是把 skill 目录注入 system prompt，
// 让 LLM 通过 switch_skill 工��自行决定是否切换。
// skill_directory 会在 build_skill_directory_prompt 里构建。
let skill_directory = build_skill_directory_prompt(registry, default_skill.id()).await;
state.pending_skill_directory = Some(skill_directory);
```

然后在 `initialize_state_for_turn` 之后，修改 system_prompt 构建逻辑——在 `SkillTurnContext` 里把 `pending_skill_directory` 追加到 `system_prompt`：

在 `Ok(SkillTurnContext { ... })` 返回之前加：

```rust
let system_prompt = if let Some(directory) = state.pending_skill_directory.take() {
    format!("{}\n\n{}", skill.system_prompt(&state), directory)
} else {
    skill.system_prompt(&state)
};
```

并把 `SkillTurnContext` 的 `system_prompt` 改为用这个变量。

- [ ] **Step 4: 实现 `build_skill_directory_prompt`**

在 `skill_session.rs` 底部（测试区块之前）添加：

```rust
async fn build_skill_directory_prompt(registry: &SkillRegistry, default_skill_id: &str) -> String {
    let skills = registry.list().await;
    let non_default: Vec<_> = skills
        .iter()
        .filter(|s| s.id != default_skill_id)
        .collect();

    if non_default.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "## 可用专项技能（如用户需求匹配，请调用 switch_skill 切换）".to_string(),
        String::new(),
    ];
    for skill in &non_default {
        lines.push(format!("- `{}`: {}", skill.id, skill.short_description));
    }
    lines.push(String::new());
    lines.push("如果当前对话更适合某个专项技能，请立即调用 switch_skill 并告知用户已切换。否则直接用通用能力回答。".to_string());

    lines.join("\n")
}
```

- [ ] **Step 5: 处理 `SkillState` 的 `pending_skill_directory` 字段**

`SkillState` 需要加这个临时字段。打开 `skill_session.rs` 找 `SkillState` struct，追加：

```rust
#[serde(skip)]  // 不持久化，每次 turn 重新生成
pub pending_skill_directory: Option<String>,
```

- [ ] **Step 6: 编译**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -20
```

修复所有编译错误。

- [ ] **Step 7: 运行测试**

```bash
cd src-tauri && cargo test --test skill_routing_llm_test default_skill_system_prompt -- --nocapture 2>&1 | tail -20
```

期望：`test default_skill_system_prompt_contains_skill_directory ... ok`

- [ ] **Step 8: 确认 Task 1 的测试也仍然通过**

```bash
cd src-tauri && cargo test --test skill_routing_llm_test -- --nocapture 2>&1 | tail -10
```

期望：2 个测试全部 ok。

- [ ] **Step 9: 提交**

```bash
git add src-tauri/src/runtime/chat/skill_session.rs \
        src-tauri/tests/skill_routing_llm_test.rs
git commit -m "feat(routing): replace keyword-based skill activation with LLM-driven routing via skill directory in system prompt"
```

---

## Task 3：清理 `detect_activation` 关键词路由（可选，保持代码整洁）

Task 2 完成后 `detect_activation` 已经不再被 `resolve_turn_context` 调用，可以删除或标记 deprecated。

**Files:**
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/plugin/declarative_skill.rs`

- [ ] **Step 1: 确认 `detect_activation` 和 `should_activate` 没有其他调用方**

```bash
cd src-tauri && grep -rn "detect_activation\|should_activate" src/ tests/ | grep -v "declarative_skill.rs\|registry.rs"
```

如果输出为空，继续。如果有其他调用方，先不删，跳过本 Task。

- [ ] **Step 2: 删除 `detect_activation` 和 `should_activate`**

从 `src/plugin/registry.rs` 删除 `detect_activation` 方法（约第 978-1000 行）。

从 `src/plugin/declarative_skill.rs` 删除 `should_activate` 方法（约第 329-357 行）和 `keywords` 字段。

- [ ] **Step 3: 编译确认无残留引用**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -20
```

- [ ] **Step 4: 运行所有测试**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -20
```

期望：所有测试通过。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/plugin/registry.rs \
        src-tauri/src/plugin/declarative_skill.rs
git commit -m "chore(routing): remove unused keyword-based detect_activation and should_activate after LLM routing migration"
```

---

## Self-Review

**Spec coverage:**
- ✅ 初始路由改为 LLM 语义 → Task 2 实现
- ✅ switch_skill 注入合法 ID → Task 1 实现
- ✅ 关键词路由清理 → Task 3 实现

**Placeholder scan:**
- Task 4 Step 3 中提到「按实际 API 调整」→ Step 5 要求先确认 API，已覆盖
- `PluginManifest::default()` 可能不存在 → 已在注释中提示先编译看报错

**Type consistency:**
- `build_skill_directory_prompt` 接收 `&SkillRegistry` 和 `&str`，与调用处一致
- `pending_skill_directory: Option<String>` 在 SkillState 和 resolve_turn_context 中用法一致
- `SkillInfo.short_description` 字段名来自 registry.rs 第 133 行，已确认

**风险提示:**
- `definition()` 是同步方法，用 `futures::executor::block_on` 读 registry 会有微小阻塞，如果 skill 数量很多（>50）可能有延迟。目前 skill 数量约 20 个，可接受。后续可考虑在 `SwitchSkillRuntimeTool` 上缓存 skill 列表。
- Task 3 是可选的，如果其他地方还在用 `should_activate` 则跳过，不影响主要功能。

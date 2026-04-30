# Skill System Stateless Migration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 AIjia 的 skill 加载从有状态的 `switch_skill`（切 prompt + 限工具 + 持久化 + SkillRuntimePatch 热更新）切换到 Claude Code 风格的 `load_skill` 无状态指令注入，消除 skill 切换锁死会话的 bug 类。

**Architecture:** 新增 `load_skill` RuntimeTool（无状态，返回 skill 的 base.md 作为 tool result），在 daily 模式的 dynamic context 中注入 skill 目录表（name + description），让 LLM 自主决定何时加载 skill。这是 skill-first 架构改造的第一步——后续 switch_skill + SkillRuntimePatch + SkillSessionStore 整条有状态链路将被完全废弃。

**Base Branch:** `pzc`���runtime-first 架构已就位）

**Tech Stack:** Rust（Skill trait / SkillRegistry / RuntimeTool / context_builder / chat_turn_driver）

---

## 当前架构快照（pzc 分支）

### 消息主链路

```
Frontend → send_message (Tauri command)
  → TauriChatCommandAdapter::send_message()
      → RequestScopedRuntimeDeps（注入 skill_registry, skill_sessions）
      → ToolRegistry::to_runtime_dispatcher(deps)  // 构造 switch_skill RuntimeTool
      → SessionRuntime::run_chat_request(ChatTurnRequest)
          → RuntimeChatTurnDriver::run_chat_turn_s4()
              1. executor.load_turn_config_overrides()   // ← 当前 skill routing 入口
                   → SkillSessionStore::resolve_turn_context()
                   → 返回 TurnConfigOverrides{system_prompt, tool_defs, allowed_tools, ...}
              2. TurnConfig 快照冻结
              3. 'turn 循环 (≤30 iterations):
                   a. build_iteration_context()  // ← dynamic context 注入点
                   b. executor.run_llm_step()
                   c. ToolRoundDriver::execute_round()
                   d. tool_result_collector::collect_results()
                   e. apply_skill_runtime_patch()  // ← switch_skill 热更新 TurnConfig
```

### 当前 skill routing 的问题

`switch_skill` 是"重操作"——通过 `SkillRuntimePatch` 在迭代间热更新 `TurnConfig`（system_prompt + tool_defs + allowed_tools + max_iterations + token_budget）。一旦切换：
- system prompt 被替换为目标 skill 的 prompt
- allowed_tools 被限制为目标 skill 声明的工具集
- 状态持久化到 `SkillSessionStore`（下一轮 `resolve_turn_context` 恢复）
- LLM 可以在受限工具集中再次调用 `switch_skill` 跳到其他 skill → 恶性循环

### 关键文件位置

| 组件 | 文件 | 备注 |
|------|------|------|
| SwitchSkillRuntimeTool | `runtime/tools/builtin/switch_skill.rs` | 要被 load_skill 替代 |
| SkillSessionStore | `runtime/chat/skill_session.rs` | resolve_turn_context + switch_skill + build_skill_directory_prompt |
| SkillRuntimePatch | `runtime/chat/tool_round_types.rs:3-11` | switch_skill 热更新载体 |
| apply_skill_runtime_patch | `runtime/chat/chat_turn_driver.rs:1724` + `runtime/agent/worker_runtime.rs:743` | 消费 patch |
| skill_runtime_patch 字段 | `runtime/chat/tool_result_collector.rs:50` | 收集 patch |
| extract_skill_runtime_patch | `runtime/query_engine.rs:57-92` | 从 tool result 解析 patch |
| build_skill_directory_prompt | `runtime/chat/skill_session.rs:197` | 现有技能目录（注入 system prompt） |
| build_iteration_context | `runtime/chat/context_builder.rs:9` | dynamic context 纯函数 |
| TurnConfig 冻结 | `runtime/chat/chat_turn_driver.rs:1002-1016` | run_chat_turn_s4 |
| Skill trait | `plugin/skill_trait.rs:153` | 核心 trait |
| DeclarativeSkill | `plugin/declarative_skill.rs:17` | base_prompt 字段 |
| SkillRegistry | `plugin/registry.rs:994` | skills HashMap + get/list |
| PluginContext | `plugin/context.rs:78` | 已有 skill_registry 字段 |
| RuntimeTool 注册 | `plugin/builtin/tools/mod.rs:68` | register_builtin_tools() |
| request-scoped 工具构造 | `plugin/registry.rs:778` (try_build_request_scoped_tool) | switch_skill 在此构造 |
| TauriChatServices | `transport/tauri_commands/chat.rs:365` | skill_registry + skill_sessions 字段 |
| load_turn_config_overrides | `transport/tauri_commands/chat.rs:1341` | skill routing 入口 |

---

## 核心决策

### 为什么做

当前 skill routing 有两个问题：
1. **switch_skill 是重操作**：切 prompt + 限工具 + 持久化 + 热更新 TurnConfig，一旦出错会话被锁死
2. **skill 目录注入在 system prompt**（`build_skill_directory_prompt`），引导 LLM 调用 `switch_skill` 而非无状态的 `load_skill`

对齐 Claude Code 的三级渐进式加载：
- **L1 Metadata**：name + description 始终在 context（~100 tokens/skill）
- **L2 Body**：按需加载 base.md 全文（<5000 tokens）
- **L3 Resources**：scripts/ references/ 按需执行或读取

### 方案偏向 Claude Code 的原因

1. **验证过的设计** — Claude Code 的 Skill tool 在大规模使用中验证了"无状态指令注入"模型
2. **天然防呆** — 无状态意味着 LLM 调错了 skill 也没有副作用
3. **Token 效率** — L1 只注入 name+description，L2 按需加载 body

### 架构方向

**Skill-First**：所有能力最终都通过 skill 模式提供。switch_skill + SkillRuntimePatch + SkillSessionStore 整条有状态链路后续完全废弃。本次改造是第一步——打通无状态的 skill 发现和加载链路。

### Claude Code (claude-code-best) 精确对照

基于对 claude-code-best 源码的完整探索，其 Skill 系统链路如下：

```
SKILL.md on disk
  → parseFrontmatter() 剥离 YAML frontmatter，body 缓存在闭包中
  → createSkillCommand() 构建 Command 对象（含 getPromptForCommand 闭包）
  → getSkillDirCommands() 聚合所有来源（.claude/skills/ + plugins），memoized
  → getSkillToolCommands() 过滤出 model-visible 子集
  → getSkillListingAttachments() 格式化 skill catalog（context window 的 1% 预算）
  → wrapMessagesInSystemReminder() → <system-reminder> user message，每轮注入

Model 看到 skill listing → 决定调用 Skill tool({ skill, args })

  → SkillTool.call():
      inline: processPromptSlashCommand → getPromptForCommand() → 展开 SKILL.md body
                → addInvokedSkill() 存储内容（用于 compact 后重注入）
                → 返回 newMessages（isMeta user message 包含完整 SKILL.md body）
      fork:   prepareForkedCommandContext → runAgent(promptMessages)
```

**claude-code-best 的关键设计选择：**

| 维度 | Claude Code | AIjia 对齐方案 | 差异说明 |
|------|------------|---------------|---------|
| **Catalog 注入方式** | `<system-reminder>` user message（独立消息） | dynamic context 的一个 section | AIjia 的 dynamic context 等价于 claude-code-best 的 `prependUserContext`，都是 user message。section vs 独立 message 效果相同 |
| **Catalog 增量发送** | 只发新发现的 skill（tracked by `sentSkillNames`） | 每轮全量发送 | Phase 1 简化。后续可加增量追踪 |
| **Catalog token 预算** | context window 的 1%（~8000 chars） | 无预算控制 | 当前 ~23 个 skill，catalog 约 2000 chars，不需要截断。skill 数超过 50 时需加 |
| **Skill 执行返回** | 注入 isMeta user message（非 tool result） | 返回 ToolResult（tool result） | RuntimeTool 只能返回 ToolResult。LLM 同样能在消息历史中看到 body 内容，效果等价 |
| **Skill 内容持久化** | `STATE.invokedSkills` + compact 后重注入 | 无。body 在 tool result 中自然保留 | tool result 会被消息历史保留，不需要额外机制 |
| **allowed-tools** | pre-approval（自动批准，不限制） | 不实现（load_skill 不限制工具） | 对齐：两者都不限制工具 |
| **Fork 模式** | `context: 'fork'` → 独立 sub-agent 执行 | 不实现 | 后续独立实现 |
| **变量替换** | `$ARGUMENTS`、`${CLAUDE_SKILL_DIR}`、`${CLAUDE_SESSION_ID}` | 不实现 | 后续随 SKILL.md 格式迁移一起做 |
| **Shell 命令** | `` !`cmd` `` 在 prompt 中执行 | 不实现 | 后续随 SKILL.md 格式迁移一起做 |

**核心对齐点**：无状态指令注入 + LLM 自主发现 + 三级加载 + 无工具限制——这四点完全对齐。差异都在实现细节层面，是 lotus-app RuntimeTool 架构下的合理简化。

### 不做的事

- **不动 `switch_skill` 工具实现 / `SkillRuntimePatch` / `SkillSessionStore` 主体** — 这些代码本期保留作为 UI 入口的过渡兼容（`WelcomeScreen` 卡片 / slash 命令 / SkillPopover 仍可触发 stateful workflow）。**例外**：本期允许在 `SkillSessionStore::resolve_turn_context()` 中收窄一处 `ensure_switch_skill_tool` 的注入条件（仅 default skill 路径下不再注入到 LLM 的 `allowed_tools`），见 Task 5。
- **不改前端 UI** — WelcomeScreen 卡片、Slash command、SkillPopover 保持原样
- **不改 plugin.toml 格式** — 现有 skill 的配置文件不需要迁移
- **不实现 fork 模式** — skill 的 sub-agent 执行后续独立实现
- **不实现变量替换和 shell 命令** — 随 SKILL.md 格式迁移一起做
- **不实现 catalog 增量发送和 token 预算** — 当前 skill 数量不需要，后续按需加

### Phase 2（不在本期，由独立 plan 承载）

以下方向已经达成共识，但全部不在本计划范围内，等本期 review 修复（Task 7、Task 8）合入后再单独开 plan：

1. **Skill 规范统一**：废弃 AIjia 自创的 `plugin.toml` + `workflow.toml` + `prompts/step*.md` + precompute 这一整套格式，改为完全对齐 Claude Code 的 `SKILL.md`：单目录 + `SKILL.md`（YAML frontmatter `name` / `description` + Markdown body）+ 可选 `scripts/` `references/` `assets/`，变量名本地化为 `${AIJIA_SKILL_DIR}` / `${AIJIA_SESSION_ID}` / `$ARGUMENTS`。
2. **Skill 来源唯一**：runtime 只扫描 `~/.renlijia/skills/`，不再扫 `src-tauri/plugins/`，也不扫任何应用预置目录。
3. **现有 `src-tauri/plugins/*`（10 个 skill）按新规范全部重写**：重写后通过初始化机制安装/复制到 `~/.renlijia/skills/` 才会被 runtime 识别。该重写工作由用户后续独立拉新需求处理，不属于本次 PR。
4. **`daily-assistant` builtin skill 废弃**：会话起手用最小内置 prompt（写在代码里，但不再以 Skill 形式注册），不存在"default skill"概念。
5. **UI 入口（卡片 / slash / popover）触发的 stateful workflow 路径整体废弃**：`SkillSessionStore::switch_skill()`、`SkillRuntimePatch`、`apply_skill_runtime_patch`、`build_skill_directory_prompt`、`ensure_switch_skill_tool`、`extract_skill_runtime_patch` 等 stateful 链路完全移除，前端三个入口（WelcomeScreen 卡片 / Slash command / SkillPopover）改为只 prepend 提示文本或彻底删除。
6. **不需要兼容**：以上重构不保留任何旧格式过渡兼容，旧 stateful 链路一次性删除。

---

## 改造后的 Skill 全链路（对齐 Claude Code）

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Claude Code 链路                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ① 安装时                                                                   │
│     扫描 skills/ 目录，读 SKILL.md frontmatter（name + description）        │
│                                                                             │
│  ② 会话启动时                                                                │
│     所有 skill 的 name + description 注入 system prompt                     │
│     LLM 获得"技能目录"，知道有哪些 skill 可用                               │
│                                                                             │
│  ③ 用户发消息时                                                              │
│     LLM 读 description 判断需要哪个 skill → 调用 Skill tool                 │
│     不需要 → 直接回答                                                        │
│                                                                             │
│  ④ Skill tool 执行                                                          │
│     将 SKILL.md body 全文作为 tool result 返回                              │
│     无副作用：不改 system prompt、不限制工具、不持久化                        │
│                                                                             │
│  ⑤ 下一轮                                                                   │
│     skill body 在消息历史中，LLM 可继续参照或忽略                            │
│     新任务不相关 → 自然不再参照 → 天然回退                                   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                  AIjia 改造后（对齐 Claude Code）                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ① 启动扫描（不变）                                                         │
│     src-tauri/plugins/ + ~/.renlijia/skills/                                │
│     → scan_external_plugins() 读 plugin.toml/SKILL.md                       │
│     → DeclarativeSkill 注册到 SkillRegistry                                 │
│     → 新增：body_prompt() 方法暴露 base.md 内容                             │
│                                                                             │
│  ② 每轮迭代时（daily 模式，default skill）                                   │
│     → build_iteration_context() 构建 dynamic context                        │
│     → SkillRegistry.build_catalog_markdown() 生成技能目录                    │
│     → 注入 dynamic context（非 system prompt，不影响 KV cache）              │
│     → 目录格式：                                                             │
│       ## 可用专项技能                                                        │
│       当用户需求匹配时，请调用 load_skill 加载详细指令。                      │
│       - `biz-writing` — 📝 商务写作: 邮件/报告/备忘录                       │
│       - `comp-analysis-v2` — 📊 薪酬公平性分析: 数据驱动诊断                │
│       - ...                                                                 │
│                                                                             │
│  ③ 用户发消息时（LLM 决策）                                                  │
│     → LLM 读目录中的 description 判断需要哪个 skill                         │
│     → 匹配：调用 load_skill(skill_id="biz-writing")                         │
│     → 不匹配：直接用 daily-assistant 通用能力回答                            │
│                                                                             │
│  ④ load_skill 执行（轻操作）                                                │
│     → SkillRegistry.get(skill_id) 查找 skill                                │
│     → 调用 skill.body_prompt() 获取 prompts/base.md 全文                    │
│     → 作为 tool result 返回给 LLM                                           │
│     → 无副作用：                                                             │
│       ✗ 不改 system prompt                                                  │
│       ✗ 不限制 allowed_tools                                                │
│       ✗ 不持久化任何状态                                                     │
│       ✗ 不产生 SkillRuntimePatch                                            │
│     → LLM 阅读返回的指令并遵照执行当前任务                                  │
│                                                                             │
│  ⑤ 下一轮                                                                   │
│     → base.md 内容在消息历史中（作为 load_skill 的 tool result）            │
│     → LLM 可继续参照（用户追问同领域话题）                                  │
│     → 新话题不相关 → LLM 自然不再参照 → 天然回退，无锁定                    │
│     → 需要时可再次调用 load_skill 加载其他 skill                             │
│                                                                             │
│  ⑥ Workflow 触发（过渡兼容，后续废弃）                                       │
│     → 用户点击 WelcomeScreen 卡片 / Slash command / CMD+K                   │
│     → selected_skill_id 触发 → SkillSessionStore.switch_skill()             │
│     → 进入现有有状态 workflow 流程                                           │
│     → 此路径后续将被完全移除                                                 │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 关键对齐点（对照 claude-code-best 源码）

| 维度 | Claude Code 实现 | AIjia 改造后 | 对齐程度 |
|------|-----------------|-------------|---------|
| **L1 Metadata** | `getSkillListingAttachments()` → `<system-reminder>` user message，含 name + description + when_to_use | `build_catalog_markdown()` → dynamic context section | ✅ 对齐（注入方式不同但效果等价） |
| **L2 Body** | `SkillTool.call()` → `getPromptForCommand()` → isMeta user message | `load_skill.execute()` → tool result | ✅ 对齐（载体不同但 LLM 都能读到完整 body） |
| **L3 Resources** | `${CLAUDE_SKILL_DIR}` 变量 + `Read`/`Bash` 按需访问 | `plugin_dir` 路径 + scripts/ 按需执行 | ✅ 对齐 |
| **无状态** | 无副作用，无持久化，天然可回退 | 无副作用，无 SkillRuntimePatch，天然可回退 | ✅ 对齐 |
| **工具限制** | `allowed-tools` 是 pre-approval（自动批准），不限制 | load_skill 不限制工具 | ✅ 对齐 |
| **触发方式** | LLM 读 skill listing 自主决定 + / 命令 | LLM 读 catalog 自主决定 + / 命令 + 卡片 | ✅ 对齐 |
| **Compact 后** | `createSkillAttachmentIfNeeded()` 重注入 invoked skill body | tool result 在消息历史中自然保留 | ⏳ 后续对齐（需要 compact 机制成熟后） |

---

## 文件改动地图

### 新建（1 个）

| 文件 | 职责 |
|------|------|
| `src-tauri/src/runtime/tools/builtin/load_skill.rs` | `LoadSkillRuntimeTool`：RuntimeTool 实现，读 skill 的 base.md 返回为 tool result |

### 修改（5 个）

| 文件 | 变更 |
|------|------|
| `src-tauri/src/plugin/skill_trait.rs` | Skill trait 增加 `fn body_prompt(&self) -> String` 默认方法 |
| `src-tauri/src/plugin/declarative_skill.rs` | 实现 `body_prompt()` 返回 `self.base_prompt` |
| `src-tauri/src/plugin/registry.rs` | SkillRegistry 增加 `build_catalog_markdown()` + `try_build_request_scoped_tool` 增加 `load_skill` 分支 |
| `src-tauri/src/runtime/chat/context_builder.rs` | `build_iteration_context()` 增加 `skill_catalog: &str` 参数 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 调用 `build_iteration_context` 时传入 skill catalog |

### 可能需要微调（1 个）

| 文件 | 变更 |
|------|------|
| `src-tauri/src/runtime/chat/skill_session.rs` | `resolve_turn_context()` 中 default skill 路径：禁用 `build_skill_directory_prompt()` 注入 system prompt（避免与 dynamic context 的 catalog 双重注入） |

### 不动的文件

- `src-tauri/src/plugin/context.rs` — PluginContext 已有 `skill_registry` 字段
- `src-tauri/src/plugin/manifest.rs` — plugin.toml 解析不变
- `src-tauri/src/lib.rs` — scan_external_plugins() 不变
- `src-tauri/plugins/*/` — 所有 plugin 定义不变
- `src-tauri/src/runtime/tools/builtin/switch_skill.rs` — 过渡保留
- `src-tauri/src/runtime/chat/skill_session.rs` — 整体过渡保留（仅禁用 directory prompt）
- 前端组件（WelcomeScreen, SlashCommandPopover, useStreaming.ts）不变

---

## Task 1: Skill trait 增加 `body_prompt()` 方法

**Files:**
- Modify: `src-tauri/src/plugin/skill_trait.rs`
- Modify: `src-tauri/src/plugin/declarative_skill.rs`

- [ ] **Step 1: 在 Skill trait 增加默认方法**

在 `skill_trait.rs` 的 `pub trait Skill`（line ~153）中增加：

```rust
/// Full prompt body for stateless injection via load_skill tool.
/// Returns the skill's base.md content (Level 2 in three-level loading).
fn body_prompt(&self) -> String { String::new() }
```

- [ ] **Step 2: DeclarativeSkill 实现 body_prompt**

在 `declarative_skill.rs` 的 `impl Skill for DeclarativeSkill` 中增加：

```rust
fn body_prompt(&self) -> String {
    self.base_prompt.clone()
}
```

- [ ] **Step 3: 编译验证**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

- [ ] **Step 4: 写单元测试**

在 `declarative_skill.rs` 的 `#[cfg(test)] mod tests`（line ~692）中追加：

```rust
#[test]
fn body_prompt_returns_base_prompt_content() {
    let plugin_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/comp-analysis-v2");
    if !plugin_dir.exists() { return; }
    let content = std::fs::read_to_string(plugin_dir.join("plugin.toml")).unwrap();
    let manifest = crate::plugin::manifest::parse_plugin_manifest(&content).unwrap();
    let skill = DeclarativeSkill::load(&manifest, &plugin_dir).unwrap();
    let body = skill.body_prompt();
    assert!(!body.is_empty(), "comp-analysis-v2 should have a base.md");
}
```

- [ ] **Step 5: 运行测试**

```bash
cd src-tauri && cargo test --lib body_prompt -- --nocapture 2>&1 | tail -10
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/plugin/skill_trait.rs src-tauri/src/plugin/declarative_skill.rs
git commit -m "feat(skill): add body_prompt() to Skill trait for stateless load_skill"
```

---

## Task 2: SkillRegistry 增加 `build_catalog_markdown()`

**Files:**
- Modify: `src-tauri/src/plugin/registry.rs`

**Context:** SkillRegistry（line ~994）的 `skills` 字段是 `RwLock<HashMap<String, RegisteredSkill>>`，其中 `RegisteredSkill` 是私有 struct 包含 `skill: Arc<dyn Skill>` 和 `source: String`。`default_skill_id` 在 `lib.rs` 硬编码为 `"daily-assistant"`。目前没有 tests 模块。

- [ ] **Step 1: 写失败测试**

在 `registry.rs` 底部新增 `#[cfg(test)] mod skill_registry_tests`（注意不要与已有 `ToolRegistry` 的测试冲突）：

```rust
#[cfg(test)]
mod skill_registry_tests {
    use super::*;
    use crate::plugin::skill_trait::{Skill, SkillState, ToolFilter};
    use std::sync::Arc;

    struct MockSkill {
        id: String,
        name: String,
        desc: String,
        short_desc: String,
        icon_str: String,
    }

    impl MockSkill {
        fn new(id: &str, name: &str, desc: &str) -> Self {
            Self {
                id: id.to_string(), name: name.to_string(),
                desc: desc.to_string(), short_desc: desc.to_string(),
                icon_str: "📋".to_string(),
            }
        }
    }

    impl Skill for MockSkill {
        fn id(&self) -> &str { &self.id }
        fn display_name(&self) -> &str { &self.name }
        fn description(&self) -> &str { &self.desc }
        fn short_description(&self) -> &str { &self.short_desc }
        fn icon(&self) -> &str { &self.icon_str }
        fn should_activate(&self, _: &str, _: bool, _: &str) -> bool { false }
        fn system_prompt(&self, _: &SkillState) -> String { String::new() }
        fn tool_filter(&self, _: &SkillState) -> ToolFilter { ToolFilter::UseAll }
    }

    #[tokio::test]
    async fn build_catalog_empty_when_no_non_default_skills() {
        let registry = SkillRegistry::new("daily-assistant");
        registry.register(Arc::new(MockSkill::new("daily-assistant", "Daily", "default")), "builtin").await;
        let catalog = registry.build_catalog_markdown().await;
        assert!(catalog.is_empty());
    }

    #[tokio::test]
    async fn build_catalog_excludes_default_includes_others() {
        let registry = SkillRegistry::new("daily-assistant");
        registry.register(Arc::new(MockSkill::new("daily-assistant", "Daily", "default")), "builtin").await;
        registry.register(Arc::new(MockSkill::new("biz-writing", "商务写作", "邮件/报告")), "plugin").await;
        let catalog = registry.build_catalog_markdown().await;
        assert!(catalog.contains("biz-writing"));
        assert!(catalog.contains("商务写作"));
        assert!(!catalog.contains("daily-assistant"));
    }

    #[tokio::test]
    async fn build_catalog_sorted_by_id() {
        let registry = SkillRegistry::new("daily-assistant");
        registry.register(Arc::new(MockSkill::new("daily-assistant", "Daily", "default")), "builtin").await;
        registry.register(Arc::new(MockSkill::new("zzz-skill", "ZZZ", "last")), "plugin").await;
        registry.register(Arc::new(MockSkill::new("aaa-skill", "AAA", "first")), "plugin").await;
        let catalog = registry.build_catalog_markdown().await;
        let pos_aaa = catalog.find("aaa-skill").unwrap();
        let pos_zzz = catalog.find("zzz-skill").unwrap();
        assert!(pos_aaa < pos_zzz);
    }
}
```

- [ ] **Step 2: 运行确认编译失败**（方法不存在）

- [ ] **Step 3: 实现 build_catalog_markdown**

在 `impl SkillRegistry`（line ~1003）中添加：

```rust
/// Build a markdown skill catalog for injection into dynamic context.
/// Excludes the default skill. Returns empty string if no non-default skills.
pub async fn build_catalog_markdown(&self) -> String {
    let skills = self.skills.read().await;
    let mut entries: Vec<_> = skills.values()
        .filter(|rs| rs.skill.id() != self.default_skill_id)
        .collect();

    if entries.is_empty() { return String::new(); }

    entries.sort_by(|a, b| a.skill.id().cmp(b.skill.id()));

    let mut md = String::from("## 可用专项技能\n\n");
    md.push_str("当用户的需求与以下某个技能的领域匹配时，请调用 `load_skill` 工具加载详细指令。\n\n");
    for rs in &entries {
        let s = &rs.skill;
        let desc = if !s.short_description().is_empty() { s.short_description() } else { s.description() };
        md.push_str(&format!("- `{}` — {} {}: {}\n", s.id(), s.icon(), s.display_name(), desc));
    }
    md.push_str("\n如果没有匹配的技能，直接用通用能力回答。\n");
    md
}
```

- [ ] **Step 4: 运行测试确认通过**

- [ ] **Step 5: Commit**

---

## Task 3: 创建 LoadSkillRuntimeTool

**Files:**
- Create: `src-tauri/src/runtime/tools/builtin/load_skill.rs`
- Modify: `src-tauri/src/plugin/registry.rs`（`try_build_request_scoped_tool` 增加 `load_skill` 分支）

**Context:** pzc 分支上所有新工具都是 `RuntimeTool`。`load_skill` 需要 `SkillRegistry` 才能工作，而 `SkillRegistry` 是 request-scoped 依赖（在 `RequestScopedRuntimeDeps` 中传入）。因此 `load_skill` 应该和 `switch_skill` 一样，作为 request-scoped tool 在 `try_build_request_scoped_tool()` 中构造。

`REQUEST_SCOPED_RUNTIME_TOOL_NAMES`（`registry.rs:129`）是一个常量列表，声明了所有 request-scoped 工具名。`try_build_request_scoped_tool`（`registry.rs:778`）是工厂函数，根据 tool name 构造对应的 RuntimeTool。

- [ ] **Step 1: 确认 RuntimeTool trait 接口**

```bash
cd src-tauri && grep -n "fn definition\|fn execute\|trait RuntimeTool" src/runtime/tools/mod.rs | head -20
```

同时检查 `switch_skill.rs` 作为 request-scoped RuntimeTool 的参考实现。

- [ ] **Step 2: 创建 load_skill.rs**

在 `src-tauri/src/runtime/tools/builtin/load_skill.rs`：

```rust
//! load_skill — stateless skill instruction injection.
//!
//! Reads a skill's full prompt body (prompts/base.md) and returns it as
//! tool result. No state mutation, no system prompt change, no SkillRuntimePatch.

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use crate::plugin::SkillRegistry;
use crate::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolExecutionContext, ToolKind, ToolResult,
};

pub struct LoadSkillRuntimeTool {
    skill_registry: Arc<SkillRegistry>,
    skill_ids: Vec<String>,  // snapshot at construction for schema description
}

impl LoadSkillRuntimeTool {
    pub async fn new(skill_registry: Arc<SkillRegistry>) -> Self {
        let skill_list = skill_registry.list().await;
        let default_id = skill_registry.default_skill_id().to_string();
        let skill_ids: Vec<String> = skill_list.into_iter()
            .filter(|s| s.id != default_id)
            .map(|s| s.id)
            .collect();
        Self { skill_registry, skill_ids }
    }
}

#[async_trait]
impl RuntimeTool for LoadSkillRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        let ids_desc = if self.skill_ids.is_empty() {
            "（当前无可用技能）".to_string()
        } else {
            format!("可用 skill_id: {}", self.skill_ids.join(", "))
        };

        ToolDefinition {
            name: "load_skill".to_string(),
            description: format!(
                "加载一个专项技能的详细指令到当前对话。当用户的需求与可用技能目录中的\
                 某个技能匹配时，调用此工具传入 skill_id，获取该技能的完整操作指南。\
                 无副作用：不改变系统提示、不限制工具、不持久化。{}",
                ids_desc
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_id": {
                        "type": "string",
                        "description": "技能 ID，必须来自系统提示中的可用技能目录"
                    }
                },
                "required": ["skill_id"]
            }),
            kind: ToolKind::Support,
            ..Default::default()
        }
    }

    async fn execute(
        &self,
        _ctx: &ToolExecutionContext,
        input: serde_json::Value,
    ) -> ToolResult {
        let skill_id = input.get("skill_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if skill_id.is_empty() {
            return ToolResult::error("缺少 skill_id 参数".to_string());
        }

        let skill = match self.skill_registry.get(skill_id).await {
            Some(s) => s,
            None => return ToolResult::error(
                format!("技能 '{}' 不存在。可用技能: {}", skill_id, self.skill_ids.join(", "))
            ),
        };

        let body = skill.body_prompt();
        if body.is_empty() {
            return ToolResult::success(format!(
                "技能 '{}' ({}) 已加载，但没有详细指令。请根据描述操作：{}",
                skill_id, skill.display_name(), skill.description()
            ));
        }

        let mut result = format!("## {} ({})\n\n", skill.display_name(), skill_id);
        result.push_str(&body);
        ToolResult::success(result)
    }
}
```

- [ ] **Step 3: 在 mod.rs 中声明模块**

在 `src-tauri/src/runtime/tools/builtin/mod.rs` 中添加 `pub mod load_skill;`

- [ ] **Step 4: 在 try_build_request_scoped_tool 中注册**

在 `plugin/registry.rs` 的 `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 常量中添加 `"load_skill"`。

在 `try_build_request_scoped_tool()` 函数（line ~778）中，在 `"switch_skill"` 分支之后添加：

```rust
"load_skill" => {
    if let Some(ref skill_registry) = ctx.skill_registry {
        let tool = crate::runtime::tools::builtin::load_skill::LoadSkillRuntimeTool::new(
            Arc::clone(skill_registry),
        ).await;
        Some(Arc::new(tool))
    } else {
        None
    }
}
```

- [ ] **Step 5: 编译验证**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

注意：需要确认 `ToolDefinition`、`ToolResult`、`ToolKind`、`ToolExecutionContext` 的确切导入路径和 API。参考 `switch_skill.rs` 的实现适配。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/load_skill.rs src-tauri/src/runtime/tools/builtin/mod.rs src-tauri/src/plugin/registry.rs
git commit -m "feat(skill): add LoadSkillRuntimeTool for stateless skill instruction injection"
```

---

## Task 4: Dynamic context 注入 Skill Catalog

**Files:**
- Modify: `src-tauri/src/runtime/chat/context_builder.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`

**Context:** `build_iteration_context()`（`context_builder.rs:9`）是一个纯函数，接收 8 个 `&str`/`Option<&str>` 参数，返回构建好的 dynamic context 字符串。它在 `chat_turn_driver.rs:1234` 的 `'turn` 循环内每轮调用。

Skill catalog 应作为新参数注入，而非在函数内部调用 `SkillRegistry`——保持纯函数设计。

- [ ] **Step 1: 给 build_iteration_context 增加 skill_catalog 参数**

在 `context_builder.rs` 的 `build_iteration_context()` 签名中增加参数：

```rust
pub fn build_iteration_context(
    core_memory: &str,
    project_memory: &str,
    workspace_context: &str,
    file_context: &str,
    analysis_notes: &str,
    precompute_result: Option<&str>,
    connector_context: Option<&str>,
    analysis_ctx_prompt: Option<&str>,
    skill_catalog: &str,   // NEW: skill catalog markdown for LLM discovery
) -> String {
```

在函数体末尾（`ctx` return 之前），追加：

```rust
// 9. Skill catalog — injected for LLM-driven skill discovery
if !skill_catalog.is_empty() {
    ctx.push_str("\n\n");
    ctx.push_str(skill_catalog);
}
```

- [ ] **Step 2: 更新所有调用点**

`build_iteration_context` 的调用点：

1. `chat_turn_driver.rs:1234` — 主生产路径。在调用前通过 executor 获取 skill catalog：

```rust
// 在 'turn 循环外（一次性准备，因为 skill 列表不变）
let skill_catalog = /* 见 Step 3 */;

// 调用时
let iteration_delta_context = build_iteration_context(
    &core_memory_str,
    &project_memory_prompt,
    &env_info,
    "",
    "",
    precompute_result.as_deref(),
    None,
    None,
    &skill_catalog,  // NEW
);
```

2. `context_builder.rs` 的测试（`mod tests`）——所有现有测试调用增加 `""` 作为最后一个参数。

- [ ] **Step 3: 在 chat_turn_driver 中准备 skill catalog**

skill catalog 需要通过 `RuntimeLlmExecutor` 获取（因为 `SkillRegistry` 在 transport 层持有，不在 runtime 层）。两种选择：

**选项 A（推荐）：给 RuntimeLlmExecutor trait 加方法**

在 `RuntimeLlmExecutor` trait 中增加：

```rust
async fn get_skill_catalog(&self) -> String { String::new() }
```

在 `TauriLegacyTurnExecutor` 中实现：

```rust
async fn get_skill_catalog(&self) -> String {
    self.services.skill_registry.build_catalog_markdown().await
}
```

在 `run_chat_turn_s4` 中，`'turn` 循环前调用：

```rust
let skill_catalog = executor.get_skill_catalog().await;
```

**选项 B：通过 ChatTurnRequest 传入**

在 `ChatTurnRequest` 中增加 `skill_catalog: String` 字段，由 `TauriChatCommandAdapter::send_message()` 构建时填充。

选项 A 更干净——不改 request 契约，且与 `get_env_info()` 模式一致。

- [ ] **Step 4: 编译验证 + 修复所有调用点**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -20
```

- [ ] **Step 5: 更新 context_builder 测试**

```rust
#[test]
fn test_skill_catalog_block() {
    let catalog = "## 可用专项技能\n- `biz-writing` — 商务写作";
    let result = build_iteration_context("", "", "", "", "", None, None, None, catalog);
    assert!(result.contains("可用专项技能"));
    assert!(result.contains("biz-writing"));
}

#[test]
fn test_empty_skill_catalog_not_injected() {
    let result = build_iteration_context("", "", "", "", "", None, None, None, "");
    assert!(!result.contains("可用专项技能"));
}
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/context_builder.rs src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "feat(skill): inject skill catalog into dynamic context for LLM-driven discovery"
```

---

## Task 5: 禁用 build_skill_directory_prompt 的 system prompt 注入

**Files:**
- Modify: `src-tauri/src/runtime/chat/skill_session.rs`

**Context:** `resolve_turn_context()`（line ~81）在 default skill 路径会调用 `build_skill_directory_prompt()`（line ~116）将技能目录注入到 **system prompt** 中，引导 LLM 调用 `switch_skill`。

改造后，技能目录通过 dynamic context 注入，引导 LLM 调用 `load_skill`。如果两处同时注入，LLM 会看到两份技能目录——一份说"调用 switch_skill"，一份说"调用 load_skill"，产生矛盾。

**方案**：在 `resolve_turn_context()` 中，当处于 default skill 时：
1. 跳过 `build_skill_directory_prompt()` 的注入（解决双重注入）
2. **同时**把 `ensure_switch_skill_tool(allowed_tools)` 的调用收窄到 `skill.id() != default_skill.id()` 分支——默认会话不再向 LLM 暴露 `switch_skill` 工具，所有专项加载只能走无状态 `load_skill`。这是本期"无状态加载"目标的硬边界。

> ⚠️ 越界说明：此处对 `SkillSessionStore::resolve_turn_context()` 的修改超出了"不改 SkillSessionStore"的字面承诺。本期已将其作为唯一允许的例外列入"不做的事"。`SkillSessionStore::switch_skill()`、`SkillRuntimePatch`、`apply_skill_runtime_patch` 等其余 stateful 链路保持原样，由 Phase 2 独立 plan 整体废弃。

- [ ] **Step 1: 定位注入点**

```bash
cd src-tauri && grep -n "build_skill_directory_prompt" src/runtime/chat/skill_session.rs
```

- [ ] **Step 2: 注释掉或条件禁用 directory prompt，并移除 default 路径的 switch_skill 注入**

在 `resolve_turn_context()` 中，将 `build_skill_directory_prompt` 调用删除/注释，并确保 `ensure_switch_skill_tool` 只在非 default skill 状态下执行：

```rust
// LLM-based routing now uses dynamic context + stateless load_skill.
// Keep switch_skill workflow compatibility for explicit stateful workflow turns,
// but do not expose switch_skill to the default/daily skill.
state = initialize_state_for_turn(skill.as_ref(), state, has_files);
let mut allowed_tools = resolve_allowed_tools(
    all_tool_names,
    skill.tool_filter(&state),
    skill.allowed_tool_names(&state),
);
if skill.id() != default_skill.id() {
    allowed_tools = ensure_switch_skill_tool(allowed_tools);
}

let system_prompt = skill.system_prompt(&state);
```

验收标准：
- default skill 的 `allowed_tools` 不包含 `switch_skill`
- 非 default skill 的 stateful workflow 仍可保留 `switch_skill` 出口
- system prompt 不再包含 legacy skill directory prompt

- [ ] **Step 3: 编译验证**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

- [ ] **Step 4: 更新 skill_session 测试**

`skill_session.rs` 底部有测试模块（line ~400+）。检查是否有测试依赖 `build_skill_directory_prompt` 的输出，如果有则更新断言。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/chat/skill_session.rs
git commit -m "feat(skill): disable switch_skill directory prompt in favor of load_skill catalog"
```

---

## Task 6: 验证 load_skill 可被 LLM 调用

**Files:**
- Read: `src-tauri/src/plugin/registry.rs`（DAILY_ALLOWED_TOOLS / tool filtering 逻辑）

**Context:** pzc 分支没有 `daily_blocked` 黑名单。工具可见性由 `TurnConfig.allowed_tools`（来自 `load_turn_config_overrides`）和 `ToolRoundDriver.allowed_tools` 控制。

`load_skill` 是 request-scoped RuntimeTool，在 `to_runtime_dispatcher()` 时构造。它会出现在 `get_all_schemas()` 返回的工具列表中。

需要验证：
1. `load_skill` 的 schema 出现在 LLM 收到的 tool_defs 中
2. default skill 的 `tool_filter` 不排除 `load_skill`
3. 如果 `allowed_tools` 是 `Some(set)`，`load_skill` 在 set 中

- [ ] **Step 1: 确认 default skill 的 tool_filter**

```bash
cd src-tauri && grep -n "tool_filter\|UseAll\|ToolFilter" src/plugin/builtin/skills/daily_assistant.rs | head -10
```

如果 default skill 返回 `ToolFilter::UseAll`，则 `load_skill` 天然可用。

- [ ] **Step 2: 确认 request-scoped 工具注入时机**

`load_skill` 在 `to_runtime_dispatcher()` 时通过 `try_build_request_scoped_tool` 注入。确认这发生在 `get_tool_defs()` 之前（即 `load_skill` 会出现在 TurnConfig.tool_defs 中）。

- [ ] **Step 3: 手动测试**

1. 启动 `pnpm tauri:dev`
2. 新建对话，输入 "帮我写一封商务邮件"
3. 验证 LLM 是否调用 `load_skill("biz-writing")` 并返回技能指南
4. 下一轮对话输入无关内容，验证 LLM 不再应用 biz-writing 指令

- [ ] **Step 4: Commit（如有变更）**

---

## Task 7: 子代理路径透传 SkillRegistry，支持无状态 load_skill

**Files:**
- Modify: `src-tauri/src/llm/sub_agent.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: `src-tauri/tests/builtin_runtime_registration_test.rs`

**Context:** `load_skill` 是 request-scoped RuntimeTool，构造时必须拿到 `RequestScopedRuntimeDeps.skill_registry`。主会话路径已经由 `TauriLegacyTurnExecutor` 注入 `skill_registry`；但 `SubAgentRuntimeDeps::request_scoped_tool_deps()` 当前硬编码 `skill_registry: None`，会导致子代理 dispatcher 静默丢掉 `load_skill` schema。Phase 1 的边界是：所有运行态只要显式允许 `load_skill`，都必须能走无状态 skill 加载；不能 silently drop。

- [ ] **Step 1: 写失败测试：子代理 deps 透传 skill_registry**

在 `src-tauri/tests/builtin_runtime_registration_test.rs` 中增加测试（复用现有 mock skill / deps 构造方式；如果文件内已有 `BodySkill`，直接复用）：

```rust
#[tokio::test]
async fn load_skill_available_when_subagent_deps_include_skill_registry() {
    let registry = Arc::new(SkillRegistry::new("daily-assistant"));
    registry
        .register(Arc::new(BodySkill::new("biz-writing", "body")), "test")
        .await;

    let deps = RequestScopedRuntimeDeps {
        skill_registry: Some(registry),
        ..test_request_scoped_deps()
    };
    let tool_registry = ToolRegistry::new();
    let dispatcher = tool_registry.to_runtime_dispatcher(deps).await;
    let schemas = dispatcher.get_all_schemas();

    assert!(
        schemas.iter().any(|schema| schema.name == "load_skill"),
        "subagent request-scoped deps with skill_registry must expose load_skill"
    );
}
```

- [ ] **Step 2: 运行测试确认当前失败**

```bash
cd src-tauri && cargo test --test builtin_runtime_registration_test load_skill_available_when_subagent_deps_include_skill_registry -- --nocapture
```

Expected before implementation: FAIL（如果测试直接构造 `RequestScopedRuntimeDeps` 可能已通过；此时需要改为通过 `SubAgentRuntimeDeps::request_scoped_tool_deps()` 构造，验证真实子代理链路）。

- [ ] **Step 3: 给 SubAgentRuntimeDeps 增加 skill_registry 字段**

在 `src-tauri/src/llm/sub_agent.rs` 的 `SubAgentRuntimeDeps` 中增加：

```rust
pub skill_registry: Option<Arc<crate::plugin::SkillRegistry>>,
```

在 `request_scoped_tool_deps()` 返回值中改为：

```rust
skill_registry: self.skill_registry.clone(),
skill_sessions: None,
```

注意：`skill_sessions` 仍保持 `None`。子代理只支持无状态 `load_skill`，不参与 stateful `switch_skill`。

- [ ] **Step 4: 在生产构造点传入 skill_registry**

在 `src-tauri/src/transport/tauri_commands/chat.rs` 构造 `SubAgentRuntimeDeps` 的位置，增加：

```rust
skill_registry: Some(self.services.skill_registry.clone()),
```

如果测试 helper 构造 `SubAgentRuntimeDeps` 报缺字段，按测试语义填 `None` 或 `Some(registry)`。

- [ ] **Step 5: 运行目标测试**

```bash
cd src-tauri && cargo test --test builtin_runtime_registration_test load_skill_available_when_subagent_deps_include_skill_registry -- --nocapture
```

Expected: PASS。

- [ ] **Step 6: 运行相关注册测试**

```bash
cd src-tauri && cargo test --test builtin_runtime_registration_test -- --nocapture
```

Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/llm/sub_agent.rs src-tauri/src/transport/tauri_commands/chat.rs src-tauri/tests/builtin_runtime_registration_test.rs
git commit -m "fix(skill): expose load_skill to subagent request-scoped tools"
```

---

## Task 8: 加固无状态 skill 迁移测试

**Files:**
- Modify: `src-tauri/tests/skill_tool_contract_test.rs`
- Modify: `src-tauri/src/plugin/declarative_skill.rs`
- Modify: `src-tauri/tests/skill_routing_llm_test.rs`
- Modify: `src-tauri/tests/load_skill_runtime_tool_test.rs`

**Context:** 本次迁移的核心风险不是编译失败，而是行为回退：daily 允许工具列表被悄悄扩大、`body_prompt()` 不再返回真实 `base.md`、system prompt 被清空但测试仍通过、LLM 畸形调用 `load_skill` 未覆盖。以下测试必须作为本 PR 的强制验收。

- [ ] **Step 1: 恢复 DAILY_ALLOWED_TOOLS 漂移检测**

在 `src-tauri/tests/skill_tool_contract_test.rs` 中保留 catalog 存在性测试，并把 `daily_skill_allowed_tools_match_runtime_constant` 改成独立期望列表：

```rust
#[test]
fn daily_skill_allowed_tools_match_expected_stateless_set() {
    const EXPECTED_DAILY_ALLOWED_TOOLS: &[&str] = &[
        "bash",
        "read_file",
        "write_file",
        "edit_file",
        "list_files",
        "glob",
        "grep",
        "web_search",
        "web_fetch",
        "ask_user",
        "agent",
        "task_list",
        "task_create",
        "task_update",
        "task_get",
        "todo_list",
        "todo_create",
        "todo_update",
        "todo_get",
        "permission_ask",
        "load_skill",
    ];

    assert_eq!(
        DAILY_ALLOWED_TOOLS,
        EXPECTED_DAILY_ALLOWED_TOOLS,
        "daily allowed tools changed; update this test only after reviewing stateless-skill safety"
    );
    assert!(
        !DAILY_ALLOWED_TOOLS.contains(&"switch_skill"),
        "daily/default skill must not expose stateful switch_skill"
    );
}
```

> 运行前先打开 `src-tauri/src/runtime/tools/catalog.rs` 的 `DAILY_ALLOWED_TOOLS`，把上面的列表调整为当前真实顺序。不要从生产常量生成期望值。

- [ ] **Step 2: 加固 body_prompt 测试**

在 `src-tauri/src/plugin/declarative_skill.rs` 的 `body_prompt_returns_base_prompt_content` 中，把静默 skip 改成显式失败，并与真实 `prompts/base.md` 比对：

```rust
#[test]
fn body_prompt_returns_base_prompt_content() {
    let plugin_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("plugins/comp-analysis-v2");
    assert!(
        plugin_dir.exists(),
        "test fixture missing: {}",
        plugin_dir.display()
    );

    let content = std::fs::read_to_string(plugin_dir.join("plugin.toml")).unwrap();
    let manifest = crate::plugin::manifest::parse_plugin_manifest(&content).unwrap();
    let skill = DeclarativeSkill::load(&manifest, &plugin_dir).unwrap();
    let expected = std::fs::read_to_string(plugin_dir.join("prompts/base.md")).unwrap();

    assert_eq!(skill.body_prompt(), expected);
}
```

- [ ] **Step 3: 加固 default system prompt 测试**

在 `src-tauri/tests/skill_routing_llm_test.rs` 的 `default_skill_system_prompt_omits_switch_skill_directory` 中保留否定断言，并增加正向断言：

```rust
assert!(
    !ctx.system_prompt.contains("comp-analysis-v2"),
    "default system prompt must not contain legacy skill directory"
);
assert!(
    ctx.system_prompt.contains("日常") || ctx.system_prompt.contains("AI") || !ctx.system_prompt.trim().is_empty(),
    "default system prompt must still contain the base daily-assistant prompt"
);
```

优先使用 daily-assistant base prompt 中稳定且具体的短语，不要只依赖 `!is_empty()`；如果该 prompt 没有稳定中文短语，先读 `src-tauri/src/plugin/builtin/skills/daily_assistant.rs` 后替换为具体断言。

- [ ] **Step 4: 覆盖 load_skill 畸形输入**

在 `src-tauri/tests/load_skill_runtime_tool_test.rs` 中增加：

```rust
#[tokio::test]
async fn load_skill_execute_rejects_missing_or_empty_skill_id() {
    let registry = Arc::new(SkillRegistry::new("daily-assistant"));
    registry
        .register(Arc::new(BodySkill::new("biz-writing", "body")), "test")
        .await;
    let tool = LoadSkillRuntimeTool::new(registry).await;
    let ctx = test_tool_context();

    for input in [json!({}), json!({ "skill_id": "" }), json!({ "skill_id": "   " })] {
        let err = tool.execute(input, ctx.clone()).await.unwrap_err();
        assert!(
            err.to_string().contains("Missing required field: skill_id"),
            "unexpected error: {err}"
        );
    }
}
```

如果 `ToolExecutionContext` 不能 clone，则在循环内重新调用 `test_tool_context()`。

- [ ] **Step 5: 运行目标测试（串行）**

```bash
cd src-tauri && cargo test body_prompt_returns_base_prompt_content --lib -- --nocapture
cd src-tauri && cargo test --test skill_tool_contract_test -- --nocapture
cd src-tauri && cargo test --test skill_routing_llm_test default_skill_system_prompt_omits_switch_skill_directory -- --nocapture
cd src-tauri && cargo test --test load_skill_runtime_tool_test load_skill_execute_rejects_missing_or_empty_skill_id -- --nocapture
```

Expected: all PASS。注意不要并行跑多个 `cargo test`。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/tests/skill_tool_contract_test.rs src-tauri/src/plugin/declarative_skill.rs src-tauri/tests/skill_routing_llm_test.rs src-tauri/tests/load_skill_runtime_tool_test.rs
git commit -m "test(skill): strengthen stateless load_skill regressions"
```

---

## Phase 2（后续独立 PR，废弃 switch_skill 链路）

> 以下是后续方向，不在本次范围。记录在此作为路线图。

### Task 9: 移除 switch_skill 及 SkillRuntimePatch 整条链路

当 load_skill 稳定运行后，按以下顺序清理：

| 文件 | 移除内容 |
|------|---------|
| `runtime/tools/builtin/switch_skill.rs` | 删除整个文件 |
| `plugin/registry.rs:129` | `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 中移除 `"switch_skill"` |
| `plugin/registry.rs:966-979` | `try_build_request_scoped_tool` 中移除 `"switch_skill"` 分支 |
| `runtime/chat/tool_round_types.rs:3-11` | 删除 `SkillRuntimePatch` struct |
| `runtime/chat/tool_result_collector.rs:50` | 移除 `skill_runtime_patch` 字段 |
| `runtime/chat/chat_turn_driver.rs:1536-1537` | 移除 `apply_skill_runtime_patch` 调用 |
| `runtime/chat/chat_turn_driver.rs:1724-1741` | 删除 `apply_skill_runtime_patch` 函数 |
| `runtime/agent/worker_runtime.rs:743-868` | 移除 worker 路径的 `apply_skill_runtime_patch` |
| `runtime/query_engine.rs:57-92` | 移除 `extract_skill_runtime_patch` |
| `runtime/chat/skill_session.rs:44-79` | 移除 `switch_skill()` 方法 |
| `runtime/chat/skill_session.rs:197-225` | 移除 `build_skill_directory_prompt()` |
| `runtime/chat/skill_session.rs:226-240` | 移除 `ensure_switch_skill_tool()` |
| `transport/tauri_commands/chat.rs:1341-1421` | 简化 `load_turn_config_overrides`（不再需要 skill routing） |

### Task 10: 简化 SkillSessionStore

当 switch_skill 链路移除后：
1. `resolve_turn_context()` 简化为：始终返回 default skill 的 prompt（不再检查持久化的 active skill state）
2. 移除 `SkillState` 的会话级持久化（`MemoryStore` 依赖）
3. 最终 `SkillSessionStore` 本身可以删除——skill routing 完全由 load_skill 处理

---

## 验证清单

| 场景 | 预期结果 |
|------|---------|
| Daily 对话，用户问匹配 skill 的问题 | LLM 调用 load_skill，返回技能指南，按指南回答 |
| Daily 对话，用户问通用问题 | LLM 不调用 load_skill，正常回答 |
| 连续两轮，第一轮触发 skill，第二轮不相关 | 第二轮不应用上一轮 skill 的指令 |
| LLM 对未知 skill_id 调用 load_skill | 返回错误提示，列出可用技能 |
| LLM 调用 load_skill 但 skill_id 缺失/空串 | 返回 `Missing required field: skill_id` 错误（Task 8 测试覆盖） |
| WelcomeScreen 卡片点击 | 仍进入有状态 workflow（过渡兼容；Phase 2 废弃） |
| Slash command 触发 | 仍进入有状态 workflow（过渡兼容；Phase 2 废弃） |
| Daily 默认会话 LLM 工具列表 | 包含 `load_skill`，**不**包含 `switch_skill`（Task 5 + Task 8 测试覆盖） |
| 显式 selected_skill_id 切到非 default skill | `allowed_tools` 仍含 `switch_skill`（向后兼容 Phase 2 之前） |
| Skill catalog 在 dynamic context 中 | `[动态上下文]` 末尾包含 `## 可用专项技能` |
| Skill catalog 不在 system prompt 中 | `build_skill_directory_prompt` 已禁用，default system prompt 仍包含 daily-assistant base prompt |
| 子代理 dispatcher 暴露的 schema | 当 `SubAgentRuntimeDeps.skill_registry = Some(...)` 时含 `load_skill`（Task 7 测试覆盖） |
| `DAILY_ALLOWED_TOOLS` 漂移 | `tests/skill_tool_contract_test.rs` 中显式期望列表与生产常量一致；新增/删除工具必须同步更新（Task 8） |
| `DeclarativeSkill::body_prompt()` | 与 `prompts/base.md` 文件内容逐字节相等（Task 8） |

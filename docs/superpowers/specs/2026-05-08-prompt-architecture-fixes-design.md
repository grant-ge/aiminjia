# Prompt 架构一次性修复 — 设计方案

> 日期：2026-05-08
> 关联调研：`docs/2026-05-08-claude-code-system-prompt-comparison.md`
> 范围：把当前 prompt 架构所有已知的"已实现但被废掉 / 已设计但没接通 / 已声称但没做对"的问题一次性修完，未实现的功能写好 TODO 留口子。

## 0. 决策汇总（已与用户确认）

1. **P0**：取消 `chat.rs:1394` 对 `DAILY_BASE_PROMPT` 的强制覆盖，让 PromptAssembler 的产物真正进入 LLM 请求。
2. **DAILY 白名单**：保留 17 个工具的白名单，让它在主对话路径上**真正生效**（接入 `build_visible_tool_defs` / 等价入口），不删。
3. **AGENT.md**：维持现状不动，**不**引入 CLAUDE.md / AGENTS.md / `.claude/` 兼容。
4. **PromptCachePolicy**：接入 wire format（Claude provider 多块化 + OpenAI 兼容渲染器输出 content 数组）。
5. **Subagent**：为 4 个内置 subagent 写独立人格（草稿放在本方案 §6，由用户审）。
6. **Coordinator**：保持 `team.rs` 5 行 stub，加 TODO 注释明确"未实现"。
7. **systemPromptExtra 重命名**：本次不改字段名（影响前端 + serde），仅在 `dispatch_prompt.rs` / `EmployeeRecord` 注释说明"实际进入用户消息，安全层级 = 用户输入"。
8. **token 估算**：把 system prompt / dynamic context / tools schema 一并计入 `chat_turn_driver.rs` 的估算；同时修复 `worker_runtime.rs:310` 子代理硬编码 `max_tokens=4096` 的问题。
9. **未实现功能口子**：`runtime/agent/team.rs`、`systemPromptExtra` 命名、AGENTS.md 兼容三处明确加 TODO 注释，写清楚"将来如何继续做 + 为什么现在不做"。

> **修订记录（2026-05-08 全链路 CoT 自审）**：本方案经审查发现 3 处硬错误（详见 §9），已就地修订 §2.1 / §2.2 / §2.6 / §3 / §5 相关章节。

---

## 1. 全局架构目标

修完之后，prompt 装配链路���该是：

```
1. PromptAssembler.build_system_prompt()
   → 输出 PromptAssembly{ blocks: [base, tool_preference, memory_mechanics, persona, daily, ...] }
   → 每个 block 带 PromptCachePolicy（StaticPrefix / SessionDynamic / Volatile）

2. driver 直接使用这个 PromptAssembly，不再被 DAILY_BASE_PROMPT 覆盖
   - load_turn_config_overrides() 默认返回 system_prompt: None
   - 仅在测试 / 显式 custom prompt 场景才返回 Some

3. provider 层按 cache_policy 渲染 wire format
   - Claude provider: 多个 Anthropic text block，static 段加 cache_control
   - OpenAI 兼容渲染器: 输出 content 数组，static 段加 cache_control

4. 工具暴露走单一入口 build_visible_tool_defs
   - 接受 daily_whitelist (Option<&[&str]>) 参数（DAILY_ALLOWED_TOOLS 通过这里生效）
   - 接受 employee_whitelist 参数
   - 接受 has_workspace 参数
   - get_tool_defs() 删除（死代码）

5. 子代理使用独立人格 prompt
   - 4 个内置 agent 各自有几百字的 system_prompt
   - spawn_subagent 仍叠加 build_env_info（保留现有对齐）
   - 不继承主对话 base.md（避免身份混淆）

6. token 估算覆盖完整输入
   - estimated_input_tokens = chars(system + dynamic + messages + tools_schema) / 4
   - 诊断里分项记录
```

---

## 2. 修改清单（按文件分组）

### 2.1 `src-tauri/src/transport/tauri_commands/chat.rs`

| 行号 | 改动 | 类型 |
|---|---|---|
| `chat.rs:1308` 区域 | 删除 `get_tool_defs()` impl（死代码） | 删除 |
| `chat_turn_driver.rs:255` 区域 | **同时删除 `RuntimeLlmExecutor` trait 的 `get_tool_defs` 默认实现**，否则 trait 默认 impl 返回 `vec![]` 会让 mock 测试静默通过 | 删除 |
| `chat.rs:1394` | `system_prompt: Some(DAILY_BASE_PROMPT.into())` → `system_prompt: None` | 修复 |
| `chat.rs:1365-1397` | 重构白名单逻辑，**拆分双重职责**（详见 §2.2 修订） | 重构 |

### 2.2 `chat_runtime_impl.rs::build_visible_tool_defs` + `chat.rs::load_turn_config_overrides`（重大修订）

**问题（审查发现）**：当前 `chat.rs:1365-1372` 的 `allowed_tools` 变量同时承担两个职责：

1. **schema 过滤**：作为 `build_visible_tool_defs(allowed_tools=Some(...))` 参数，决定模型看见哪些 tools schema
2. **运行时权限**：作为 `TurnConfigOverrides.allowed_tools` 进入 `tool_round_driver.rs:265`，决定模型实际调用工具时是否被拦截

如果按"原方案"那样把 employee 派活的 `daily_whitelist=None`、`allowed_tools=Some(employee_whitelist)`，代码可写；但改造前必须**先把这两个职责拆开**，否则容易混淆。

**修订后的设计**：

```rust
// chat_runtime_impl.rs
pub fn build_visible_tool_defs(
    registry: &dyn ToolRegistry,
    has_workspace: bool,
    schema_filter: ToolSchemaFilter,  // 新增枚举，明确语义
) -> Vec<ToolDef> { ... }

pub enum ToolSchemaFilter {
    /// 普通对话：用 DAILY_ALLOWED_TOOLS 白名单过滤 schema
    DailyWhitelist,
    /// Employee 派活：用员工自定义白名单过滤 schema
    EmployeeWhitelist(HashSet<String>),
    /// 无过滤（subagent 路径或显式全量）
    None,
}
```

`load_turn_config_overrides` 重构后：

```rust
// 第一步：决定 schema 过滤策略
let schema_filter = match &employee_overrides {
    Some(ov) if !ov.tool_whitelist.is_empty() =>
        ToolSchemaFilter::EmployeeWhitelist(ov.tool_whitelist.clone()),
    _ => ToolSchemaFilter::DailyWhitelist,
};

// 第二步：决定运行时权限白名单（独立计算）
let runtime_allowed_tools: HashSet<String> = match &employee_overrides {
    Some(ov) if !ov.tool_whitelist.is_empty() =>
        ov.tool_whitelist.iter().cloned().collect(),
    _ => DAILY_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect(),
};

// 第三步：分别使用
let visible_tool_defs = build_visible_tool_defs(
    self.services.tool_registry.as_ref(),
    authorized_workspace.is_some(),
    schema_filter,
).await;

Ok(TurnConfigOverrides {
    system_prompt: None,
    tool_defs: Some(visible_tool_defs.into_iter().filter_map(...).collect()),
    allowed_tools: Some(runtime_allowed_tools),
    max_iterations: Some(max_iterations),
    token_budget: None,
})
```

**关键不变量**：
- 普通对话：schema 看到 17 个白名单 + workspace 工具（如有），运行时也只能调白名单内
- Employee 派活：schema 与运行时双方都用 employee whitelist
- 两条路径各自独立，不互相干扰

**测试覆盖**：在 §2.12 `tool_visibility_unified_test.rs` 中新增：
- `daily_path_schema_and_runtime_match_whitelist`
- `employee_path_schema_and_runtime_match_employee_whitelist`
- `daily_path_excludes_workspace_tools_when_no_workspace`

### 2.3 `src-tauri/src/runtime/chat/prompt/types.rs`

不动结构定义，但补一条文档注释：明确 `PromptCachePolicy` 现在**真的会驱动 wire format**，不再是仅诊断。

### 2.4 `src-tauri/src/runtime/chat/prompt/renderer_openai.rs`

`OpenAiChatPromptRenderer` 改动：
- `render_system_message` 不再 flatten，改为输出 OpenAI content 数组形式：

```rust
{
  "role": "system",
  "content": [
    { "type": "text", "text": "<base + tool_preference + memory_mechanics 拼接>",
      "cache_control": { "type": "ephemeral" } },
    { "type": "text", "text": "<persona + daily 拼接>",
      "cache_control": { "type": "ephemeral" } },
    { "type": "text", "text": "<volatile 部分>" }
  ]
}
```

约束：
- 如果 provider 不支持 content 数组（grep 各 provider 的 system message 处理），降级回 flatten。
- 通过 `provider_capability` 判断（新增字段或读现有 capability）。

### 2.5 `src-tauri/src/llm/providers/claude.rs`

- 删除"整个 system 字符串当一个整体加 cache_control"的逻辑（210-214 区域）。
- 改为接收 `PromptSystemView`，按 `cache_policy` 输出多个 Anthropic text block。
- 保留 tools 的 cache_control（240-245 区域不动）。

### 2.6 `src-tauri/src/llm/gateway.rs`（修订）

**事实更正**（审查发现）：
- `PromptSystemView` 结构体**已经存在**于 `runtime/chat/prompt/types.rs:67`，本次不新建
- gateway 当前签名是 `system_prompt: Option<&str>`（不是原方案说的 `&str`）
- 这是 `pub async fn stream_message / send_message` 公开 API，**有 3 个调用点**

`build_request` / `stream_message` / `send_message` 接口扩展：

旧：
```rust
pub async fn stream_message(
    &self,
    system_prompt: Option<&str>,
    ...
) -> Result<...>
```

新（增加重载，保持向后兼容）：
```rust
pub async fn stream_message_with_view(
    &self,
    system_view: &PromptSystemView,
    ...
) -> Result<...>

// 旧接口保留，内部转换
pub async fn stream_message(
    &self,
    system_prompt: Option<&str>,
    ...
) -> Result<...> {
    let view = PromptSystemView::from_flat_string(system_prompt);
    self.stream_message_with_view(&view, ...).await
}
```

**3 个调用点全部需要决策是否升级到新接口**：

| 调用点 | 路径 | 本次是否升级 | 理由 |
|---|---|---|---|
| `chat.rs:538` | 主对话 | **是** | 主对话是 P0 修复的主战场，必须用 view |
| `worker_runtime.rs:303` | Subagent | **是** | 子代理人格升级后 prompt 变长，cache 收益最大；不升级会让"subagent 系统提示词没生效"再次发生（subagent 第一波改完不接 view 反而更糟） |
| `conversation_service.rs:398` | 标题生成 | **否** | 标题生成是一次性极简调用，无 cache ���值；保留 flat string 接口即可 |

### 2.6.1 关联：`worker_runtime.rs:310` 子代理 max_tokens 硬编码（新增）

审查发现 `worker_runtime.rs:310` 子代理调用硬编码 `max_tokens=4096`，与 §2.7 主对话 token 估算修复独立。本次一并修：

```rust
// 旧：max_tokens: Some(4096)
// 新：使用 default_max_tokens_for_model(model_name)
let max_tokens = crate::llm::max_tokens::default_max_tokens_for_model(&model_name);
```

理由：与 §0.8 "token 估算"决策一致——所有 max_tokens 走 per-model 启发式查表。

### 2.7 `src-tauri/src/runtime/chat/chat_turn_driver.rs`

token 估算修复（1394 区域）：

```rust
let system_chars = effective_system_prompt.len();
let dynamic_chars = state.dynamic_context.as_deref().map(|s| s.len()).unwrap_or(0);
let messages_chars = serde_json::to_string(&state.messages).map(|s| s.len()).unwrap_or(0);
let tools_chars = serde_json::to_string(&tool_defs).map(|s| s.len()).unwrap_or(0);
let estimated_input_tokens = (system_chars + dynamic_chars + messages_chars + tools_chars) / 4;
```

诊断 emit 也分项记录。

### 2.8 4 个内置 subagent — 写独立人格

文件：`src-tauri/src/runtime/agent/builtin/{general_purpose, explore, browse_data_agent, daily_assistant_agent}.rs`

把 `system_prompt: AgentPrompt::Inline("...")` 中的内容替换成 §6 的草稿（用户审过之后）。

### 2.9 `src-tauri/src/runtime/agent/team.rs`

文件头部加 TODO 块注释：

```rust
// TODO(coordinator-not-implemented):
// 这是一个占位 stub。Claude Code 的 coordinatorMode 需要：
// 1. 多 worker 并发调度入口（dispatch_workers）
// 2. 综合层 prompt（汇总 worker 输出 → 给主对话一个最终答案）
// 3. worker prompt 自包含上下文（worker 不能依赖 coordinator 的对话历史）
// 落地路径见 docs/superpowers/specs/2026-04-20-subagent-alignment-design.md
// 暂不实施原因：当前没有真实业务在等多 worker 调度。
pub struct TeamContext { ... }
```

### 2.10 `src-tauri/src/runtime/employee/dispatch_prompt.rs`

在 `system_prompt_extra` 拼装行（35 / 71 区域）加注释：

```rust
// NOTE: 字段名叫 system_prompt_extra 是历史遗留，实际拼入用户派活消息（user_message
// 参数），安全层级 = 用户输入，而不是 system prompt。模型可以选择性遵守。
// TODO(rename): 将来重命名为 dispatch_prompt_extra / identity_extra。
// 重命名时需要：EmployeeRecord 字段名 + 加 #[serde(alias = "systemPromptExtra")] +
// 前端 src/features/employees 同步。
```

### 2.11 `src-tauri/src/runtime/renlijia_md.rs`

文件头注释明确："本 loader 只支持 AGENT.md / AGENT.local.md / .aijia/AGENT.md，不兼容 CLAUDE.md / AGENTS.md。"——避免后续读者误判。

### 2.12 测试

- **新增**：`tests/prompt_architecture_test.rs` 加 4 个测试
  - `effective_system_prompt_includes_base_md`: PromptAssembler 产物真正进入 LLM
  - `effective_system_prompt_includes_tool_preference`
  - `effective_system_prompt_includes_memory_mechanics`
  - `effective_system_prompt_includes_persona_when_set`

- **新增**：`tests/cache_wire_format_test.rs`
  - `claude_provider_emits_multiple_cache_blocks`: 断言 Claude wire body 里 system content 有多个 block
  - `claude_provider_static_block_has_cache_control`
  - `openai_renderer_emits_content_array_when_supported`

- **新增**：`tests/tool_visibility_unified_test.rs`
  - `daily_whitelist_takes_effect_on_main_chat`: mock 主对话路径，断言模型可见工具 = DAILY_ALLOWED_TOOLS
  - `employee_whitelist_overrides_daily`
  - `has_workspace_false_excludes_workspace_tools`

- **修订**：`s4_driver_loop_test.rs:927` 的 mock 改 mock 真实路径（`build_visible_tool_defs`）

- **新增**：每个 subagent 的 `*_persona_includes_safety_clause` 测试，断言 prompt 包含核心安全片段

---

## 3. 实施分波次（修订：从 3 波重排为 4 波）

### 第一波：P0 + 工具可见性双职责拆分（1 天）

理由：相互依赖最强，必须一起改一起验证。

- §2.1 chat.rs 删 `get_tool_defs` impl + `chat_turn_driver.rs:255` 删 trait 默认 impl
- §2.1 改 `system_prompt: None`
- §2.2 重构白名单逻辑（拆 `ToolSchemaFilter` 枚举 + `runtime_allowed_tools` 独立计算）
- §2.7 主对话 token 估算
- 测试：effective_system_prompt 系列 + tool_visibility_unified（含 employee 路径）+ s4_driver_loop_test 迁移到真实路径

**风险拦截**：第一波结束后必须手动验证一次 employee 派活，确认工具仍受白名单限制（用日志 / wire body 抓包）。

### 第二波：Provider wire format 多块化（1.5 天）

理由：Claude provider 接收 PromptSystemView 是相对独立的改动，不动 gateway 接口。

- §2.3 types.rs 注释更新
- §2.4 renderer_openai.rs 输出 content 数组 + provider capability 判断
- §2.5 claude.rs 接收 PromptSystemView，按 cache_policy 多块输出
- 测试：cache_wire_format_test（用 mock provider 捕获 wire body）

**降级策略**：渲染器读 provider capability，不支持 content 数组就降级 flatten。

### 第三波：Gateway 接口升级（1.5 天）—— 单独成波

理由：审查发现接口改造波及 3 个调用点，工作量被低估，单独成波降低与第二波的耦合风险。

- §2.6 gateway 新增 `stream_message_with_view` 重载，保留旧接口
- 升级 `chat.rs:538` 主对话调用点
- 升级 `worker_runtime.rs:303` 子代理调用点
- `conversation_service.rs:398` 标题生成保留旧接口
- §2.6.1 修复 `worker_runtime.rs:310` max_tokens 硬编码
- 测试：3 个调用点的 wire body snapshot 测试

**风险拦截**：第三波结束后必须手动验证子代理调用 Claude 不再 400。

### 第四波：subagent 独立人格 + TODO 留口子（2 天）

- §2.8 替换 4 个 subagent prompt 内容（见 §6）
- §2.9 team.rs TODO 注释
- §2.10 dispatch_prompt.rs 注释
- §2.11 renlijia_md.rs 注释
- 测试：每个 subagent 的 persona 包含安全片段；spawn_subagent 上下文注入仍正常

总工时：6 天（原方案估 5 天，第三波单独成波后 +1 天）。

---

## 4. 风险与缓解（修订：补 3 项审查发现的真实风险）

| 风险 | 严重度 | 缓解 |
|---|---|---|
| **白名单双重职责拆分错误**：employee 派活的 schema/runtime 不一致，导致员工要么看到不该看的工具，要么调用被拦截 | **高** | §2.2 显式拆 `ToolSchemaFilter` 枚举 + `runtime_allowed_tools` 独立计算；测试覆盖 employee 路径 |
| **gateway 接口升级遗漏调用点**：worker_runtime / conversation_service 仍传 flat string，Claude 多块路径与之冲突导致 400 | **高** | §3 第三波单独成波，3 个调用点显式逐个审视；wire body snapshot 测试每个调用点 |
| **删 `get_tool_defs` trait 默认 impl 后 mock 测试静默通过**：测试不报错但其实没断言任何内容 | 中 | §2.1 同时删 trait 默认 impl 强制 mock 必须显式 override；迁移 `s4_driver_loop_test.rs:885` mock 到 `build_visible_tool_defs` 真实路径 |
| P0 修复后某些场景模型行为剧烈变化（base.md 之前没生效，现在生效了） | 中 | 实施时先在内部账号 / staging 跑一天，对比关键场景输出 |
| Claude 多 block cache_control 改错导致整个会话报 400 | 高 | 用 mock provider + wire body snapshot 测试覆盖；先内部灰度 |
| 普通对话工具数量从 30+ 降到 17（DAILY 白名单生效） | 中 | 检查白名单是否包含必需工具（read/write/grep/bash/web_search 等都在）；不在则补到白名单 |
| 4 个 subagent 改成中文长 prompt 后 token 上涨 | 低 | env_info 部分字符数已知，加 prompt 主体后总字符 ~2000，对子代理上下文窗口压力可忽略 |
| OpenAI 兼容端点不支持 cache_control | 低 | 渲染器读 capability，不支持降��� flatten；不影响功能 |

---

## 5. 验收标准（修订：可机器化测试优先）

修完之后必须满足：

1. **诊断**：`turn.prompt.loaded` 事件里 `sections` 数组列出 `base / tool_preference / memory_mechanics / daily / persona`，且 `system_prompt_chars` ≥ 1500。**测试方式**：mock executor + 断言 LlmStepInput.system_prompt 字符数 / 关键串。
2. **wire body 多块**（Claude）：mock Claude provider 捕获请求 JSON，断言 `system` 字段是数组、长度 ≥ 2、第一个 element 有 `cache_control: { type: "ephemeral" }`。**测试方式**：cache_wire_format_test snapshot。
3. **工具可见性**（普通对话）：mock 一次普通 turn，断言 schema 数量 = `len(DAILY_ALLOWED_TOOLS) - 排除项`，runtime_allowed_tools = `DAILY_ALLOWED_TOOLS`。
4. **工具可见性**（employee 派活）：mock 一次 employee dispatch turn，断言 schema 与 runtime_allowed_tools 都 = employee whitelist，**不包含** `DAILY_ALLOWED_TOOLS` 之外的额外工具。
5. **subagent**：spawn 一个 explore agent，捕获其 system_prompt，断言包含"只读"、"不修改"。**注意**：不应包含"AI小家"——子代理不继承主对话身份，否则人格混淆。
6. **token 估算**：诊断字段 `estimated_input_tokens` 与 `system_chars / dynamic_chars / messages_chars / tools_chars` 同时存在；`worker_runtime.rs` 调用 LLM 时 `max_tokens` 来自 `default_max_tokens_for_model` 而不是硬编码 4096。
7. **TODO 注释**：grep `TODO(coordinator-not-implemented)` / `TODO(rename)` 各能命中至少一处。
8. **回归**：所有 `review_*` 系列测试通过；前端事件联调测试通过；**关键手动验证**：
   - chat 普通对话：模型行为如预期（更尊重身份/记忆机制）
   - employee dispatch：工具白名单仍生效（用日志验证 schema 与 runtime allowed_tools 都受限）
   - 子代理：调用 Claude 不再 400（第三波后必查）
   - resume from task notification：流程不被破坏

---

## 6. 4 个 Subagent 独立人格草稿（请用户审）

> 设计原则：
> - 中文为主，对齐主产品语言
> - 每个 prompt 包含：身份 / 任务边界 / 工具使用偏好 / 数据真实性约束 / 输出格式
> - **不**继承主对话 "你是 AI小家" 这一身份（避免子代理跟主对话身份混淆）
> - 长度控制在 200-400 字，不写废话
> - 每个 prompt 末尾留一行 `## 当前任务` 占位，由 dispatch 时填充

### 6.1 general-purpose（草稿）

```
你是一个通用子代理，由主对话派出来完成一项独立任务。

## 你的工作方式

1. 收到任务后先确认理解，必要时拆解为小步骤
2. 优先用专用工具（read/write/grep/list）而非 bash 实现文件操作
3. 不假设你之前的对话记忆——只看本次任务描述里给出的上下文
4. 数据/文件/搜索结果必须如实汇报，不能为了让任务"看起来完成"而编造

## 边界

- 不要做任务范围之外的事（比如顺手"清理代码"、"优化结构"）
- 涉及破坏性操作（删除、重置、强推）必须在最终答复中明确说明
- 任务完成后给出简洁结论，不要长篇大论

## 输出

- 用纯 Markdown
- 引用具体文件用 `path:line` 格式
- 最后用一段不超过 5 行的话总结结果

## 当前任务
```

### 6.2 explore（草稿）

```
你是只读探索代理，被派来回答关于代码库 / 文档 / 数据的查询问题。

## 严格约束

- **只读**。你的工具集不包含任何写入工具。即使任务描述要求你修改，也只能在最终答复中说明"应该如何修改"，不能实际操作。
- **不要捏造**。如果搜索没结果，如实说"未找到"，不要根据猜测编造文件路径或代码片段。
- **不要预设结论**。先搜索，再下判断。

## 工作方式

1. 拿到问题先想：要找的是什么形态——函数定义？配置项？历史记录？
2. 选合适工具：grep 找字符串、search_files 找文件名、list_directory 看结构、read_workspace_file 读细节
3. 必要时用 web_search 补充背景
4. 多次小搜索 > 一次大搜索

## 输出

- 用 Markdown 列出发现的事实
- 引用代码必须 `path:line` 标注
- 末尾给一段 ≤ 5 行的"结论"总结
- 如果信息不足以回答问题，明确说"信息不足"，列出还需要查的方向

## 当前任务
```

### 6.3 browse_data_agent（草稿）

```
你是浏览器数据提取专家，从企业内部业务系统的网页中抽取结构化数据。

## 你的能力

- browse_navigate / read_page_content：浏览页面
- extract_table_data：从 HTML 表格里抽数据
- extract_with_pagination：跨分页抽取
- page_execute_js：必要时跑 JS 拿数据
- browse_and_extract：综合操作

## 工作方式

1. 先用 read_page_content 看一下页面结构，判断数据放在哪
2. 优先用 extract_table_data / extract_with_pagination 这种结构化工具
3. 只在结构化工具不够用时才退到 page_execute_js
4. 抽完数据立刻返回结构化 JSON 结果，不要做业务解读（那是主对话的事）

## 数据真实性

- 抽到什么写什么，不要补全字段
- 字段缺失时用 null 标识，不用空字符串
- 注明每条数据的来源 URL 与抽取时间
- 翻页失败 / 网页报错时如实说，不要假装抽到了

## 输出

- 顶层用 Markdown 简短描述抽取概况
- 主体用代码块包 JSON 数据
- 末尾标注"已抽取 N 条 / 失败 M 条"

## 当前任务
```

### 6.4 daily_assistant_agent（草稿）

```
你是日常工作助手代理，处理办公场景里的常规任务（写、查、整理、初步分析）。

## 服务范围

- 写：起草邮件 / 周报 / 通知 / 简单文档
- 查：在用户连接的资源里搜索特定信息
- 整理：把零散信息归类成结构化清单
- 初步分析：从数据里看出明显趋势

## 边界（重要）

- 不做需要专业资质的判断：医疗诊断、法律意见、金融投资建议、税务规划
- 不下"应该 / 必须 / 一定"的强建议；用"建议 / 可以考虑 / 通常做法是"
- 不替用户做决定，提供选项让用户选

## 工作方式

1. 任务模糊时先回复一两个澄清问题，不要瞎写一通
2. 写文档时先列大纲，确认后再展开
3. 找信息时优先用 search_memory / read_workspace_file，不要凭空生成

## 输出

- 用 Markdown
- 写作类任务直接给成品，不要"以下是初稿"这种废话
- 整理类任务用清单或表格

## 当前任务
```

---

## 7. 不在本方案范围

显式说明，避免范围蔓延：

- **Coordinator 多 worker 调度** — 仅留 TODO，不实施
- **systemPromptExtra 字段重命名** — 仅留 TODO，不实施（涉及前端）
- **CLAUDE.md / AGENTS.md 兼容** — 已搁置（用户决策）
- **MCP instructions delta** — Claude Code 有，本仓库 runtime/mcp 已有 server 管理，但 instructions 注入逻辑暂不在此次范围
- **环境信息（git status、worktree、shell）作为独立 dynamic section** — 当前混在 dynamic_context 里，已能工作，本次不重构

---

## 8. 后续 plan 化

本方案审过之后，下一步是用 `superpowers:writing-plans` 把它转成可执行的 step-by-step 实施计划，包含每一步的：
- 改动文件具体行号
- 改完跑哪条测试命令
- 验收点
- 回滚方式

writing-plan 文件命名：`docs/superpowers/plans/2026-05-08-prompt-architecture-fixes-plan.md`。

---

## 9. 修订记录（全链路 CoT 自审 — 2026-05-08）

派出独立 reviewer 子代理对方案做全链路代码交叉验证，发现 3 处硬错误 + 3 处建议补充项，全部已就地修订到本文档。

### 硬错误已修订

| # | 位置 | 错误描述 | 修订动作 |
|---|---|---|---|
| A | §2.2 | `allowed_tools` 双重职责未拆开（schema 过滤 + 运行时权限）→ 按原方案改完后 employee 工具白名单会静默失效 | §2.2 重写：引入 `ToolSchemaFilter` 枚举 + `runtime_allowed_tools` 独立计算 |
| B | §2.6 | gateway 接口改造漏掉 `worker_runtime.rs:303` 与 `conversation_service.rs:398` 两个调用点；`PromptSystemView` 已存在不需新建 | §2.6 重写：列出 3 个调用点 + 升级决策；§3 把 gateway 改造单独提成第三波 |
| C | §2.1 | 删 `get_tool_defs()` impl 但 trait 默认 impl 仍返回 `vec![]`，mock 测试会静默通过 | §2.1 增加：同时删除 `chat_turn_driver.rs:255` 的 trait 默认 impl |

### 建议补充已采纳

- §0.8 / §2.6.1：`worker_runtime.rs:310` 子代理 max_tokens 硬编码 4096，纳入本次 token 估算修复
- §5：第 2 条验收从"抓真实请求"改为"mock provider wire body snapshot 测试"
- §2.12：新增端到端测试断言 `LlmStepInput.system_prompt` 真正包含 base.md 关键串（避免 `review_prompt_single_assembly_point_test.rs` 的盲区）

### 范围调整

- 第二波（PromptCachePolicy → wire format）拆成两波：
  - 新第二波：仅 provider 多块化（claude.rs / renderer_openai.rs）
  - 新第三波：gateway 接口升级（3 个调用点逐个）
- 总工时从 5 天调整为 6 天

# Claude Code 系统提示词架构对比与 lotus-app 真实状态

> 日期：2026-05-08（2026-05-08 经 5 个并行调研子代理交叉核对后重写）
> 对照仓库：`/Users/a20250311/github/claude-code-best`
> 当前仓库：`/Users/a20250311/.codex/worktrees/5c88/lotus-app`
> 范围：只读源码调研，重点是「文档/设计声称的样子」与「代码实际跑的样子」的差。

## 0. 一句话总结

我们的 prompt 架构**设计**已经接近 Claude Code，但**实际运行链路里有一行代码把它废掉了**：每一轮对话最终发给 LLM 的 system prompt 都是一段只有 100 字左右的极简版本，base.md / 工具偏好 / 记忆机制 / persona / daily 全部不生效。

这一行就是元凶：

```rust
// src-tauri/src/transport/tauri_commands/chat.rs:1394
system_prompt: Some(DAILY_BASE_PROMPT.into()),
```

它写在 `load_turn_config_overrides()` 里，**没有任何 if 分支**——员工对话、普通对话、恢复对话，统统返回同一个极简 prompt。下游 `chat_turn_driver.rs:1082-1093` 用 `unwrap_or_else` 接住它，于是上一步辛辛苦苦组装好的完整 prompt 直接被扔进垃圾桶。

把这一行改成 `None` 就能恢复全部分层，是个零风险一行修复。

---

## 1. 先建立一点基础概念

如果你只看过我们自己的代码，可能会以为「system prompt 就是一串字符串」。Claude Code 不是这样做的，它把 system prompt 切成了几层不同的"东西"，每一层有自己的更新节奏和缓存策略：

| 层 | 是什么 | 多久变一次 |
|---|---|---|
| **静态前缀** | 身份、工程规范、风险动作、工具策略、语气规则 | 几乎不变 |
| **动态段** | 当前会话工具、环境信息、语言偏好、MCP 配置、输出风格 | 每次会话开始时定 |
| **用户上下文** | CLAUDE.md、当前日期 | 每次请求 |
| **系统上下文** | git status、cache breaker | 每次请求 |
| **工具 schema** | 工具列表与参数定义 | 不在 prompt 里，单独传 |
| **技能/子代理** | 技能正文、子代理人格 | 按需加载 |

为什么要切层？因为 LLM 的 prompt cache 命中规则是「前缀完全一样才命中」。如果你把环境信息塞进静态段，光是切个目录就会让缓存全部失效，多花钱又变慢。

我们目前**有这套设计的雏形**（叫 `PromptAssembler` + `PromptBlock` + `PromptCachePolicy`），但下面会看到这套设计在主链路里没真正用上。

---

## 2. lotus 真实的样子：5 个被验证的差距

### 2.1 P0：主对话的 system prompt 被一行代码废掉了 🔥

**这是最严重的一个，也是最容易修的一个。**

完整链路是这样的：

1. `run_chat_turn_s4` 开头，调 `executor.build_prompt_snapshot()`，**完整跑一遍 PromptAssembler**，组出包含 `base / tool_preference / memory_mechanics / persona / daily` 的快照。
   - `src-tauri/src/runtime/chat/chat_turn_driver.rs:1067`
   - `src-tauri/src/transport/tauri_commands/chat.rs:1275`（生产 executor 的实现）

2. 紧接着，又调 `executor.load_turn_config_overrides()` 拿"覆盖项"。生产实现长这样：
   ```rust
   // chat.rs:1394
   Ok(TurnConfigOverrides {
       system_prompt: Some(DAILY_BASE_PROMPT.into()),
       tool_defs: Some(build_visible_tool_defs(...)),
       ...
   })
   ```
   **零条件分支**——无论什么场景都返回 `Some(DAILY_BASE_PROMPT)`。

3. driver 在 `chat_turn_driver.rs:1082-1093` 这样合并：
   ```rust
   let effective_system_prompt = overrides.system_prompt
       .clone()
       .unwrap_or_else(|| prompt_snapshot.assembled_system_prompt.clone());
   ```
   因为 `overrides.system_prompt` 永远是 `Some`，`unwrap_or_else` 那个 fallback 分支**永远不会执行**，第 1 步 PromptAssembler 的产物**永远不会发给 LLM**。

4. `DAILY_BASE_PROMPT` 实际内容只有 7 行、约 100 字符（`src-tauri/src/runtime/chat/base_prompt.rs:1-7`），不包含 base.md、工具偏好、记忆机制、persona、product_name 任何一项。

**这意味着**：
- `src-tauri/prompts/base.md` 里写的"AI小家身份、数据真实性、纯 Markdown 输出、保密、能力边界"——**模型根本看不到**
- 工具选择偏好（`TOOL_PREFERENCE_SECTION`）——**模型看不到**
- 记忆机制说明（`MEMORY_MECHANICS_SECTION`，告诉模型什么时候调 `write_memory` / `search_memory`）——**模型看不到**
- 当前 persona 的角色和领域——**模型看不到**
- 数字员工的身份元数据——**模型看不到**（员工身份是通过派活时的"用户消息"传进去的，不是 system prompt）

**附带损害**：第 1 步的 `build_prompt_snapshot` 还跑了一次完整的 DB 查询和 auth 查询，纯属浪费。

**已有测试为什么没发现？** `tests/prompt_architecture_test.rs:274` 的 `default_system_prompt_uses_base_prompt` 测试**只断言 `DAILY_BASE_PROMPT` 自身的内容**，没有断言"主对话最终发出去的 system prompt 包含 base.md 关键内容"。

**修复方案**：

```rust
// src-tauri/src/transport/tauri_commands/chat.rs:1394
- system_prompt: Some(DAILY_BASE_PROMPT.into()),
+ system_prompt: None,
```

加一个新测试：mock executor 的 overrides 返回 `system_prompt: None`，断言 `effective_system_prompt` 包含 `"AI小家"` 或 persona identity 字符串。

---

### 2.2 P1：工具白名单是死代码

文档原本说"两套来源不清楚谁说了算"——经核实，是更尴尬的情况：**有一套是死代码**。

- `get_tool_defs()` 定义在 `chat.rs:1308`，按 `DAILY_ALLOWED_TOOLS`（17 个工具，定义于 `catalog.rs:900`）过滤生成 schema。
- `build_visible_tool_defs()` 定义在 `chat/chat_runtime_impl.rs:30`，按"有没有 workspace + employee 白名单"过滤。
- driver 合并：`overrides.tool_defs.unwrap_or(tool_defs)` (`chat_turn_driver.rs:1098`)
- 因为 `overrides.tool_defs` 永远是 `Some(...)`，**`get_tool_defs()` 在生产路径里永远不会被使用**。

**还有一个更根本的分歧文档原本没看出来**：

- 普通对话（非员工）走 `build_visible_tool_defs(has_workspace, allowed_tools=None)`
- `allowed_tools=None` 在那个函数里的语义是「不做白名单过滤」——**暴露的是「全量注册工具」减去 8 个 workspace 类工具**
- **这跟 `DAILY_ALLOWED_TOOLS` 那 17 个工具是两个完全不同的集合**

也就是说：你以为日常对话只能用 17 个工具（白名单），实际上模型能看到的是「全部已注册工具 - 8 个 workspace 工具」，可能是 30+ 个。

测试盲区：`s4_driver_loop_test.rs:927` 的 `driver_s4_daily_tool_defs_match_whitelist` mock 的是 `get_tool_defs`（死代码路径），断言通过但**没有覆盖真实的 prod 路径**。

**修复方向**：删除 `get_tool_defs()`（或把 daily 白名单语义合并进 `build_visible_tool_defs`），把测试改成 mock 真实路径。

---

### 2.3 P1：`PromptCachePolicy` 定义了，但没接到 wire format

我们已经有这个枚举：

```rust
// src-tauri/src/runtime/chat/prompt/types.rs:19
pub enum PromptCachePolicy {
    StaticPrefix,    // 永久前缀，应该被 cache
    SessionDynamic,  // 会话内稳定
    Volatile,        // 每轮都可能变
}
```

每个 `PromptBlock` 都带这个标签。问题是**写它的地方有，读它去做事的地方几乎没有**：

- OpenAI 兼容渲染器 (`renderer_openai.rs:7`) 直接 `assembly.flatten()`，把所有 block 拼成一个大字符串扔进 system message，**完全忽略 cache_policy**。
- Claude provider (`claude.rs:210-214`) 把整个 system 字符串当成一个整体加 `cache_control: ephemeral`，再对最后一个 tool 加一个 breakpoint。**没有按 StaticPrefix / SessionDynamic 切多块**。
- `llm/prompts.rs:325-330` 有一处真的按 policy 分了 `static_parts` / `dynamic_parts` 两个 `Vec`，但分完之后又各自 `join` 拼字符串，最终在 `prompts.rs:354-357` 仍然合并成一整条字符串往 gateway 送。**这个分支只影响拼接顺序，不影响 wire format。**

对照 Claude Code：

- `claude-code-best/src/services/api/claude.ts:3255` 的 `buildSystemPromptBlocks()` 把 system prompt 数组转成多个 Anthropic text block
- `claude-code-best/src/utils/api.ts:321` 的 `splitSysPromptPrefix()` 按 boundary 分别加 `cache_control`

**结果**：我们花了功夫在 PromptBlock 上标注 cache 语义，但**这个语义到 LLM 那一端就丢了**。换 persona、换 skill、换环境时整个 system 缓存都会失效。

**修复方向**：让 Claude provider 按 `PromptSystemView.blocks` 的 `cache_policy` 输出多个 content block，static 段带 `cache_control`、volatile 段不带。OpenAI 兼容端点也支持 content 数组形式，可以同样处理。

---

### 2.4 P3：项目指令文件命名维持 AGENT.md（不兼容 CLAUDE.md，是有意决定）

**加载清单经核实是这样的**（`runtime/renlijia_md.rs:22-53`）：

1. `~/.renlijia/AGENT.md`（用户全局）
2. 从当前目录逐级向上，每一级查 `AGENT.md`、`.aijia/AGENT.md`、`AGENT.local.md`
3. **多个文件按追加方式拼接**（不是覆盖），但当前消费侧没有标注每段来源

**不兼容也不打算兼容**：`CLAUDE.md`、`.claude/CLAUDE.md`、`.claude/rules/*.md`、`AGENTS.md`（复数）。

> **决策（2026-05-08）**：保持 AGENT.md 单一命名，不引入 CLAUDE.md / AGENTS.md / `.claude/` 兼容层。理由是减少多源指令的冲突面与维护成本；用户从 Claude Code 迁过来时由用户自己重命名一次即可。本节列出现状供阅读，**不再作为差距项**。

**主对话 messages 拼装的真实顺序**（`chat_turn_driver.rs:1205-1222`，仍是值得修订到文档的事实）：

1. 日期 `<system-reminder>` user 消息（`reminders.rs:5`，格式为 `<system-reminder>\n今天是 {today_cn}（{today_iso}）。\n</system-reminder>`）
2. AGENT.md context 消息（可选）
3. 历史消息
4. 当前用户消息
5. **task notifications 注入到第 4 步的用户消息之后**（`drain_and_inject_task_notifications`）；并且如果是 `is_resume_for_task_notification` 模式，第 4 步的用户消息会被跳过

⚠️ 这个顺序跟旧版文档说的不一样——task notifications 不是独立第 5 项，而是粘在用户消息后面。

**唯一仍建议的小优化**（不阻塞）：多文件拼接时给每段加 `<from path="...">` 来源标记，方便模型���分指令优先级。可选，没数据说明现在不加会出问题。

---

### 2.5 P2：subagent / employee prompt 跟主对话规则脱节

**4 个内置 subagent**（不是文档原本说的 3 个）：

- `general_purpose.rs:14` system_prompt：`"You are a general-purpose sub-agent. Complete the assigned task and return a concise final answer."`
- `explore.rs:19` system_prompt：`"You are a read-only explorer. Search and read files to answer questions. Never modify anything."`
- `browse_data_agent.rs:18` system_prompt：`AgentPrompt::Inline(String::new())` ← 空字符串
- `daily_assistant_agent.rs` system_prompt：`AgentPrompt::Inline(String::new())` ← 空字符串

**继承情况**（`spawn_subagent.rs:101-143`）：

- ✅ 已经注入 `build_env_info`（workspace 上下文），等价于 Claude Code 的 `enhanceSystemPromptWithEnvDetails`——这是少数对齐的一点
- ❌ 没有注入 base.md / 工具偏好 / 记忆机制 / AGENT.md / 安全约束

对照 Claude Code 的内置 agent prompt（`claude-code-best/src/tools/AgentTool/built-in/*Agent.ts`）每个都是几百行的人格定义 + 任务边界。

**Coordinator 角色完全没落地**：`runtime/agent/team.rs` 只有 5 行：

```rust
pub struct TeamContext {
    pub team_id: String,
    pub agent_ids: Vec<AgentId>,
}
```

无调度逻辑、无综合层、无 coordinator prompt。

**Employee `systemPromptExtra` 命名误导**（确认成立）：

- 字段叫 `systemPromptExtra`，听起来像 system prompt 扩展
- 但 `build_dispatch_prompt()` (`runtime/employee/dispatch_prompt.rs:35,71`) 把它通过 `.as_deref().unwrap_or("")` 拼进派活字符串
- 派活字符串在 `chat.rs:2835` 作为 `send_message()` 的 `user_message` 参数传进去
- 也就是说**它是用户消息的一部分，安全层级跟普通用户输入一样，模型可以选择无视**

**修复方向**：
1. 子代理 prompt 至少叠加产品级安全/工具/记忆规则（可以共用 `PromptAssembler` 的 base 部分）
2. 字段重命名为 `dispatch_prompt_extra` 或 `identity_extra`，加 `#[serde(alias = "system_prompt_extra")]` 向前兼容

---

## 3. Claude Code 是怎么做的（速查）

只列骨架，需要细节查对照索引那一节。

### 主 system prompt 三件套
- `getSystemPrompt()` (`prompts.ts:445`)：身份 + 工程规范 + 工具策略 + 风险动作 + 语气规则
- `getUserContext()` (`context.ts:155`)：CLAUDE.md + 日期，包成 `<system-reminder>` 的 meta user message
- `getSystemContext()` (`context.ts:116`)：git status + cache breaker

三者由 `fetchSystemPromptParts` (`utils/queryContext.ts:44`) 并行取出。

### 静态 vs 动态分界
`SYSTEM_PROMPT_DYNAMIC_BOUNDARY` (`prompts.ts:106`) 把静态前缀和动态后缀切开。boundary 之前是 cache 友好的稳定部分，之后是会话级 sections（语言、工具引导、env、MCP、技能引导等）。

### 动态 sections 注册机制
`systemPromptSection()` 注册一个 section（id + 计算函数），`resolveSystemPromptSections()` 缓存到 `/clear` / `/compact` 为止。少数危险 section（比如最新 git status）每次重算。

### 缓存切块
`buildSystemPromptBlocks()` (`claude.ts:3255`) 把数组转成多个 Anthropic text block，`splitSysPromptPrefix()` (`utils/api.ts:321`) 决定哪些段加 `cache_control` 以及用 global cache 还是 org cache。

### 工具说明分两层
- system prompt 讲「**应该如何使用**工具」（工具策略章节）
- tools schema 由 `toolToAPISchema()` (`utils/api.ts:119`) 单独生成，进 API 请求的 tools 字段
- 这两层物理隔离，模型既知道哲学也知道接口

### 子代理是独立人格
- `generalPurposeAgent.ts` / `exploreAgent.ts` / `verificationAgent.ts` 各有完整 prompt
- 调用时 `enhanceSystemPromptWithEnvDetails` (`AgentTool.tsx:762`) 追加环境
- coordinator 模式 (`coordinatorMode.ts:111`) 是更高一层的"调度多 worker"角色

---

## 4. 改造路线（按优先级）

### 第一波（P0，1-2 小时工作量）

**改一行 + 加一个测试**：

```rust
// chat.rs:1394
- system_prompt: Some(DAILY_BASE_PROMPT.into()),
+ system_prompt: None,
```

新增集成测试：mock 一个 turn，断言 `effective_system_prompt` 包含：
- "AI小家"（来自 base.md）
- "工具选择偏好"（来自 TOOL_PREFERENCE_SECTION）
- "记忆"（来自 MEMORY_MECHANICS_SECTION）
- 当前 persona 的 identity 字符串

跑一次回归确认 employee 派活、普通对话、resume 对话三种场景都正常。

**预期效果**：
- LLM 实际 system_prompt 长度从 ~100 字符变成 ~2000+ 字符
- `turn.prompt.loaded` 诊断里 sections 列出 `base / tool_preference / memory_mechanics / daily / persona`
- 模型行为应该会有可感知的变化（更尊重身份边界、更主动写记忆、更稳定输出 Markdown）

### 第二波（P1，1-2 天工作量）

1. **统一工具暴露入口**：删除 `get_tool_defs()` 死代码，把 `DAILY_ALLOWED_TOOLS` 白名单语义合并进 `build_visible_tool_defs`（如果还需要的话——也可能 `DAILY_ALLOWED_TOOLS` 本身就该删）。一个函数 = 一个事实源。

2. **让 PromptCachePolicy 真的影响 wire format**：
   - Claude provider 接收 `PromptSystemView.blocks`（不是 flat string），按 cache_policy 输出多个 Anthropic text block
   - StaticPrefix block 加 `cache_control: ephemeral`，Volatile 不加
   - OpenAI 兼容渲染器同样改成数组形式

3. ~~**AGENT/CLAUDE 兼容加载**~~：**已搁置（2026-05-08 决策）**。维持 AGENT.md 单一命名，不引入 CLAUDE.md / AGENTS.md / `.claude/` 兼容层。详见 §2.4。

4. **token 估算计入完整输入**：
   ```
   estimated_input_tokens = chars(system_prompt + dynamic_context + messages + tools_schema) / 4
   ```
   分别在 diagnostics 里记录每一项的字符数与估算 tokens。

### 第三波（P2，可以慢慢来）

5. **subagent prompt 叠加产品级规则**：让 4 个内置 agent 的 system_prompt 自动追加 base.md 的核心安全/工具/记忆规则，不要让子代理是"裸奔"状态。

6. **Coordinator 落地**：要么真的实现一个调度多 worker 的角色（参考 `coordinatorMode.ts`），要么把 `runtime/agent/team.rs` 那 5 行明确注释为"未实现，仅占位"。

7. **`systemPromptExtra` 重命名**：改成 `dispatchPromptExtra` 或 `identityExtra`，并在文档/UI 里说清楚它的安全层级 = 用户消息。

---

## 5. 横向对比表（已根据子代理校正修订）

| 维度 | Claude Code | lotus 设计 | lotus 真实链路 | 差距 |
|---|---|---|---|---|
| 主 system prompt 形态 | 数组 sections + 静态/动态 boundary | `PromptAssembly.blocks` + `PromptCachePolicy` | **被 `DAILY_BASE_PROMPT` 单赢覆盖** | P0 |
| 静态内容 | 身份/规范/风险/工具/语气 | base/tool_preference/memory_mechanics | 仅 4 条极简规则 | P0 |
| 工具暴露口径 | `toolToAPISchema` 单一来源 | ToolCatalog/ToolRegistry | `get_tool_defs` 是死代码；prod 路径 ≠ DAILY 白名单 | P1 |
| Cache 策略 | boundary + section cache + 多种 cache scope | `PromptCachePolicy` 三档 | provider 整体一个 breakpoint，policy 不影响 wire format | P1 |
| 项目指令文件 | CLAUDE.md / .claude/ / .claude/rules / .local | 文档说 AGENT/AGENTS | 只加载 AGENT.md 系列（**有意决定**，不兼容 CLAUDE.md） | — |
| 日期 reminder | user `<system-reminder>` | user `<system-reminder>` | ✅ 已对齐 | — |
| Messages 顺序 | system + meta-user(claudeMd+date) + history + user | system + reminders + AGENT + history + user + tasks | task notifications **粘在 user 之后**，不是独立项 | P2 |
| 子代理 prompt | 每类 agent 独立长 prompt + env enhance | 4 个内置 agent | 极简英文 + 空字符串；已注入 `build_env_info` | P2 |
| Coordinator | 独立 coordinator 模式 | `team.rs` 5 行 stub | 未实现 | P2 |
| Employee 身份 | 无对应物 | `systemPromptExtra` 字段 | 实际拼进**用户消息**，命名误导 | P2 |
| Token 估算 | 多处计入 | 仅近似估算 messages | 不计 system/dynamic/tools | P1 |

---

## 6. 对照索引

### Claude Code 关键文件

- `claude-code-best/src/QueryEngine.ts:287` — turn 入口取三件套
- `claude-code-best/src/utils/queryContext.ts:44` — `fetchSystemPromptParts`
- `claude-code-best/src/constants/prompts.ts:445` — `getSystemPrompt`
- `claude-code-best/src/constants/prompts.ts:106` — `SYSTEM_PROMPT_DYNAMIC_BOUNDARY`
- `claude-code-best/src/constants/systemPromptSections.ts:20` — `systemPromptSection` 注册
- `claude-code-best/src/context.ts:155` — `getUserContext`（CLAUDE.md + 日期）
- `claude-code-best/src/context.ts:116` — `getSystemContext`（git status）
- `claude-code-best/src/utils/claudemd.ts` — CLAUDE.md 加载规则
- `claude-code-best/src/utils/api.ts:119` — `toolToAPISchema`
- `claude-code-best/src/utils/api.ts:321` — `splitSysPromptPrefix`
- `claude-code-best/src/services/api/claude.ts:3255` — `buildSystemPromptBlocks`
- `claude-code-best/src/tools/AgentTool/built-in/generalPurposeAgent.ts`
- `claude-code-best/src/tools/AgentTool/built-in/exploreAgent.ts`
- `claude-code-best/src/tools/AgentTool/built-in/verificationAgent.ts`
- `claude-code-best/src/tools/AgentTool/AgentTool.tsx:762` — `enhanceSystemPromptWithEnvDetails`
- `claude-code-best/src/coordinator/coordinatorMode.ts:111`

### lotus 关键文件

**P0 修复点**
- `src-tauri/src/transport/tauri_commands/chat.rs:1394` ← 改这一行
- `src-tauri/src/runtime/chat/chat_turn_driver.rs:1082-1093` ← 合并逻辑
- `src-tauri/src/runtime/chat/base_prompt.rs:1-7` ← 极简 prompt 内容
- `src-tauri/tests/prompt_architecture_test.rs:274` ← 现有测试盲区

**完整 prompt 装配**
- `src-tauri/src/runtime/chat/prompt/sections.rs:49` — `PromptAssembler::build_system_prompt`
- `src-tauri/src/runtime/chat/prompt/types.rs:19` — `PromptCachePolicy`
- `src-tauri/src/llm/prompts.rs:31` — TOOL_PREFERENCE_SECTION
- `src-tauri/src/llm/prompts.rs:41` — MEMORY_MECHANICS_SECTION
- `src-tauri/prompts/base.md` — 身份与行为规范
- `src-tauri/prompts/daily.md` — 日常助手能力

**工具白名单**
- `src-tauri/src/transport/tauri_commands/chat.rs:1308` — `get_tool_defs`（死代码）
- `src-tauri/src/runtime/chat/chat_runtime_impl.rs:30` — `build_visible_tool_defs`（真实路径）
- `src-tauri/src/runtime/tools/catalog.rs:900` — `DAILY_ALLOWED_TOOLS`

**Provider 与缓存**
- `src-tauri/src/runtime/chat/prompt/renderer_openai.rs:7` — flatten
- `src-tauri/src/llm/providers/claude.rs:210-214` — Claude system cache_control
- `src-tauri/src/llm/providers/claude.rs:240-245` — tools cache_control
- `src-tauri/src/llm/gateway.rs:180-219` — `build_request`

**项目指令文件 & messages**
- `src-tauri/src/runtime/renlijia_md.rs:22-53` — AGENT.md 加载
- `src-tauri/src/runtime/chat/prompt/reminders.rs:5` — 日期 reminder 格式
- `src-tauri/src/runtime/chat/chat_turn_driver.rs:1205-1222` — messages 拼装顺序

**子代理 / 员工**
- `src-tauri/src/runtime/agent/built_in/general_purpose.rs:14`
- `src-tauri/src/runtime/agent/built_in/explore.rs:19`
- `src-tauri/src/runtime/agent/built_in/browse_data_agent.rs:18`
- `src-tauri/src/runtime/agent/built_in/daily_assistant_agent.rs`
- `src-tauri/src/runtime/agent/spawn_subagent.rs:101-143` — env 注入
- `src-tauri/src/runtime/agent/team.rs` — coordinator stub
- `src-tauri/src/runtime/employee/dispatch_prompt.rs:35,71` — `system_prompt_extra` 拼装
- `src-tauri/src/transport/tauri_commands/chat.rs:2835` — 派活进入 user_message

---

## 7. 致读者

如果你是来 review 的，请重点看 §2.1 那一行修复，那是 80% 的价值。
如果你是来执行的，按 §4 三波节奏走就好，第一波就能看到行为变化。
如果你是来认知对齐的，§1 给你心智模型，§5 给你 cheat sheet。

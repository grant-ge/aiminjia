# context.md — 业务知识

从代码看不出来的通用业务规则和架构知识。

## AppSettings 通用机制

### settings 读取优先级
- workspace-level settings 覆盖 global-level settings
- workspace settings 读取失败时静默回退到 global settings
- workspace 配置文件路径：`.aijia/settings.json`

### turn 内快照语义
- `RuntimeChatTurnDriver` 在 turn 开始时从 executor 读取一次 `ResolvedLlmSettings`
- 这份设置被写入 `TurnConfig`，后续每轮 `run_llm_step` 复用同一份快照
- turn 内不会重新读取 settings，保证一次对话的设置稳定

### max_agent_turns
- 主对话 loop 的最大迭代次数，默认值：1000
- 子代理有独立上限：browse 子代理 200 次，daily 子代理 50 次

## Masking 系统

### masking_level 三个值的含义

- `strict`：脱敏所有 PII（身份证号、银行卡号、手机号、邮箱、人名、公司名）
- `standard`：只脱敏人名和公司名
- `relaxed`：完全不脱敏（用户明确关闭隐私保护）
- 空字符串、未知值、任何格式错误 → 回退 `strict`

### 传递链路

```
DB data_masking_level (字符串)
  → MaskingLevel::from_str_or_strict()  规范化
    → ResolvedLlmSettings.masking_level  canonical 小写字符串
      → TurnConfig.masking_level         turn 开始时快照
        → LlmStepInput.masking_level     每轮 run_llm_step 收到
          → MaskingContext 脱敏          gateway 内部执行
```

- 传进 `ResolvedLlmSettings.masking_level` 的值始终是规范化后的 canonical 小写字符串
- `input.messages` 本身还是未脱敏内容，真正脱敏发生在 gateway 内部

### workspace 覆盖 masking 的字段名

- workspace 文件 `.aijia/settings.json` 中的字段名是 `dataMaskingLevel`（camelCase）
- DB 字段名是 `data_masking_level`（snake_case）

### 验证"LLM 最终收到的脱敏内容"

```rust
use app_lib::llm::masking::{MaskingContext, MaskingLevel};
use app_lib::llm::streaming::ChatMessage;

let chat_messages: Vec<ChatMessage> = input
    .messages
    .iter()
    .filter_map(|v| serde_json::from_value(v.clone()).ok())
    .collect();
let mut mask_ctx = MaskingContext::new(MaskingLevel::from_str_or_strict(input.masking_level));
let masked = mask_ctx.mask_messages(&chat_messages);
// 断言 masked 里是否包含/不包含 PII
```

- 不要直接断言 `input.messages`，要经过 MaskingContext 重放才是 LLM 真正收到的内容

## Memory 系统

### project memory 存储模型

- project memory 按 workspace 隔离，根目录在 app data 下的 `project_memories/<workspace-bucket>/`
- 每条记忆是 `entries/*.md` 独立文件，包含 frontmatter：`type`、`name`、`description`，可选 `source`
- `MEMORY.md` 是 index，不直接承载完整记忆正文；它由 entries 重建
- 保存同一条记忆时，文件名由 `name + description` 稳定生成；相同 `name + description` 再次保存会覆盖旧文件，不产生重复 entry

### memory_type 四类

- `user_preference`：用户偏好、工作习惯、表达偏好
- `project_constraint`：项目约束、业务规则、阶段性决策
- `reference_info`：外部资源、资料入口、参考位置
- `feedback`：用户对 AI 工作方式的反馈，包含以后应如何协作

### recall 规则

- `load_context(query)` 会读取当前 workspace bucket 下的 entries
- recall 匹配范围包括 entry 的 `name`、`description`、`content`
- query 会被拆成 token；长度小于 2 的 token 会被忽略
- 没有有效 token 或没有命中时，`recalled_entries` 为空
- `render_for_prompt()` 在有 recalled entries 时输出 `[相关记忆]` 块
- `render_for_prompt()` 在没有 recalled entries 时回退输出 `MEMORY.md` index 文本
- 单次 recall 最多返回 5 条，优先返回命中分更高的 entry
- 损坏 entry（无 frontmatter、缺 type/name/description、type 非法）会被跳过，不应污染 index 或 prompt

### legacy 迁移

- 旧版核心记忆路径是 `shared/cognitive/mem.md`
- 第一次 `load_context()` 时，如果旧文件存在且非空，会懒迁移成 `entries/legacy-core-memory.md`
- legacy 迁移 entry 的 type 是 `project_constraint`，source 是 `legacy-core-memory`
- 迁移是幂等的，重复 load 不应生成重复 legacy entry

### Turn 注入语义

- `RuntimeChatTurnDriver` 在 turn 开始时调用一次 `load_project_memory(workspace_path, request.content)`
- project memory 是 turn 级快照，多轮 tool calls 中不重新读取
- project memory 渲染后注入 `LlmStepInput.dynamic_context` 的 `[项目记忆]` 区块
- project memory 不进入 `LlmStepInput.messages`，不能伪装成 user/assistant 历史消息
- 只有当 project memory 为空时，才回退加载 legacy core memory 并注入 `[核心记忆]`
- 如果 project memory 非空，不再加载 legacy core memory，避免重复和污染
- memory 加载失败不应阻断 turn，应降级为空 memory 并继续执行

## Subagent 系统

### 核心概念与 ID 模型

- 每个子代理拥有独立的 `AgentId`（全局唯一）和 `child_run_id`（RunId）
- `child_run_id` 不等于 `parent_run_id`，两者同属一个 Session
- 子代理的 `CapabilityContext` 中 `is_subagent = true`，`agent_id` 等于 spawn 时生成的 child agent_id
- 子代理通过 `IdentityMapping::from_legacy_conversation_id(child_run_id)` 建立自己的消息身份

### AgentRuntime 状态机

- `spawn_child_run()` → 状态为 `Running`，创建 invocation 记录
- `complete_run()` → 状态变为 `Completed`
- `cancel_run()` → 状态变为 `Cancelled`
- `fail_run()` → 状态变为 `Failed`
- 查询不存在的 child_run_id → 返回字符串 `"missing"`，不报错
- background 子代理完成时调用 `complete_background_run()`，发出 `AgentIdle` 事件给前端
- `resume_child_run(agent_id)` 恢复已有 invocation；agent_id 不存在时返回 Err

### 取消级联

- 子代理的 `CancellationToken` 必须通过 `parent.child_token()` 创建
- 父 token 取消后，子 token 的 `is_cancelled()` 立刻返回 `true`
- 子代理检测到 `child_cancel.is_cancelled()` 时退出循环，output 设为 `"Sub-agent cancelled."`

### FileStateCache 隔离

- 子代理通过 `parent_cache.clone_for_child()` 创建独立的文件读取缓存
- 子代理对 cache 的任何修改不会影响父代理的 cache
- 父代理在子代理完成后的 cache 状态与子代理启动前一致

### 执行行为

- `max_iterations` 限制子代理的 LLM 调用轮次
- 达到上限且没有输出时，output 设为 `"Sub-agent reached iteration limit."`
- 工具返回 `AskRequired` 时，子代理停止循环，向上抛出 `LegacyToolError::AskRequired(decision)`，不在子代理内部消化
- 工具权限 `Ask` 中包含原始工具名，供父代理展示确认对话

### 结果 Envelope

- 子代理完成后产出 `SubAgentResultEnvelope`，包含：
  - `schema_version = 1`
  - `output`：最终文本输出
  - `iterations_used`：实际迭代次数
  - `generated_files`：生成文件路径列表（已去重排序）
  - `terminal_tool_results`：每次工具调用的摘要（成功/失败/summary）
  - `transcript_snapshot`：最近 16 条消息的截断快照
  - `transcript_ref`：格式为 `"subagent://<child_run_id>"`
- envelope 序列化为 storage_summary 时前缀为 `"subagent-envelope:v1:"`，可通过 `from_storage_summary()` 反序列化还原

### 转录持久化

- 子代理完整转录写入 `SubagentTranscriptStore`，key 为 `transcript_ref`
- 转录条目包含 `role`、`content`、可选的 `tool_call_id` 和 `tool_name`
- 父代理可通过 `AgentRuntime::load_transcript(child_run_id)` 按 child_run_id 读取完整转录
- `FileSubagentTranscriptStore` 将转录写入 `<transcript_root>/<sanitized_ref>.json`，transcript_ref 中的特殊字符被替换为 `_`

## Skill 系统

### skill 的唯一标识
- skill 的 id 来自 `plugin.toml` 或 `SKILL.md` 的 `plugin.id` 字段
- 安装后存放路径：`.renlijia/skills/<plugin_id>/`
- 同一个 plugin_id 安装两次会覆盖，不会产生重复目录

### skill 加载是渐进式的，分两阶段
1. **摘要阶段**：对话开始时，把所有已安装 skill 的 name + description 注入到 LLM 上下文
2. **完整内容阶段**：LLM 调用 skill 工具后，完整 SKILL.md 内容才被注入

### skill 不重复加载
- 同一次对话里，同一个 skill 的摘要只注入一次
- 完整内容也只加载一次

## Permission 系统

### 三态决策模型

权限管线对每次工具调用返回三态之一：
- `Allow`：直接执行，可携带修改后的 input
- `Deny`：拒绝执行，返回说明消息给 LLM
- `Ask`：需要用户确认，包含 message、suggestions、remember_options、default_destination

### CapabilityPermissionPipeline 规则（fail-closed）

- `capability_scope` 为空 → 始终 Allow
- `workspace:read` / `workspace:write` / `python:exec` → 需要 capability.storage 存在，否则 Deny
- `browser` → 需要 capability.has_browser_capability() = true，否则 Deny
- `network` → 始终 Allow（网络不在本地校验）
- 未知 scope → Deny（fail-closed，保守拒绝）

### StorePolicyPipeline 规则（区别在于 unknown scope → Ask）

- 已持久化 AlwaysAllow → 直接 Allow，**绕过 capability 检查**（设计意图：持久化授权不应因 capability 缺失失效）
- 已持久化 AlwaysDeny → 直接 Deny
- 未持久化 + 已知 scope 满足 capability → Allow
- 未持久化 + 已知 scope 缺少 capability → Deny
- 未持久化 + 未知 scope（含 `mcp`）→ **Ask**（与 CapabilityPipeline 的 Deny 不同）

### PermissionMode 对 Ask 的影响

- `Default`：Ask 正常发出，等待用户确认
- `DontAsk`：Ask 被转为 Deny，消息包含工具名和"dontAsk"说明
- `Plan`：Ask 被转为 Deny，消息包含"plan"模式说明
- PermissionMode 存在 TurnState 中，Ask 事件中携带当前 mode

### Ask 交互链路

1. 工具返回 AskRequired → driver 向 control plane 注册 PendingPermissionRequest
2. driver 发出 `PermissionAskRequired` RuntimeEvent → 经 TauriEventAdapter 映射为前端 `permission:ask` 事件
3. 前端展示确认对话框，用户选择后回写 control plane
4. driver 等待 control plane 响应：
   - `Allow` → 工具被重新执行，返回正常 tool_result
   - `Deny` / `Cancel` → 返回错误 tool_result 给 LLM，turn 继续执行
5. 等待期间 CancellationToken 触发 → driver 退出等待，不挂起

### 权限持久化（PermissionStore）

- 三级记住目标：`Session`（当前会话）、`Workspace`（当前项目）、`User`（全局）
- Ask 默认记住目标为 `Session`
- 记住规则按 `tool_name + scope` 精确匹配，不会跨工具或跨 scope 生效
- 一个工具有多个 scopes 时，`persist_permission_decision()` 对每个 scope 分别写入规则
- Session 级规则只在内存中，Workspace/User 级规则持久化到磁盘，重启后仍有效
- 持久化 Allow 规则的 source 对应 Workspace 或 User，可通过 source 字段区分来源

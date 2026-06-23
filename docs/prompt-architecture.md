# Lotus Prompt Architecture

lotus-app 的 prompt 主链路由 `runtime/chat/prompt/` 负责组装。

## 分层

1. Static prefix：品牌、核心行为、少量稳定工具偏好、memory mechanics。
2. Session dynamic：persona、skill/session guidance、语言偏好、output style。
3. Volatile：MCP delta、runtime env delta、precompute result、当前 iteration 的临时上下文；当前主要是类型和诊断边界，不在 static system prompt assembly 里承载。
4. User reminders：日期、AGENTS/renlijia 文件、附件提示、runtime notices。
5. Tool schema：来自 `TOOL_CATALOG` / `ToolRegistry`，不写进 system prompt。
6. Provider output：本轮 prompt architecture / Lotus 当前目标主链路是 OpenAI-compatible Chat；provider-specific wire format 由 adapter 层处理。

## 对标 claude-code-best

- `PromptAssembly.blocks` 对应 `SystemPrompt string[]`。
- `PromptCachePolicy::StaticPrefix` 对应稳定前缀，当前用于诊断和后续缓存分析。
- `PromptCachePolicy::Volatile` 对应 cache-breaking section，必须写明 reason；当前是类型和诊断边界/后续扩展边界。
- `<system-reminder>` 只用于系统自动注入给模型的 user-context，不用于替代权限或安全控制。

## OpenAI-first 边界

本轮 prompt architecture / Lotus 当前目标主链路是 OpenAI-compatible Chat。内部可以保留结构化 `PromptAssembly` 方便诊断、测试和后续缓存分析；OpenAI-compatible 渲染时产出一个 OpenAI system message。

`PromptAssembler` / `PromptAssembly` 不产生 provider-specific Anthropic `cache_control`。如果仓库保留 Claude provider，它属于 provider adapter 的 wire-format 兼容层，可能做 provider-specific adaptation，但不是 prompt assembly 的主架构边界。

OpenAI renderer 产出 OpenAI system message；gateway 在 regular messages masking 后 prepend system，`dynamic_context` / reminders 跟在 system 后面，然后才是 regular messages。system prompt 不进入 regular messages masking 路径；普通对话消息的脱敏、裁剪或 masking 不能误处理 system prompt。

当前 `PromptAssembler` 已落地 static/session dynamic。iteration delta / runtime dynamic context 通过 `LlmStepInput.dynamic_context` / gateway 注入为 system 后、regular messages 前的 user context，不在 static system prompt assembly 里。

## 代码边界

- 生产组装源是 `runtime/chat/prompt/sections.rs::PromptAssembler`。
- `src-tauri/src/llm/prompts.rs` 只是 raw prompt store + compatibility shim，负责加载 base、daily 原始 prompt 片段，并保留旧调用点可用的 `get_system_prompt()` / `build_system_prompt_parts()`。
- 新代码如需 system prompt assembly，应直接使用 `PromptAssembler`，而不是自行拼接 raw prompt fragments。
- 旧 shim 可以继续服务 tests、legacy fallback 或尚未迁移的插件接口，但不应扩展为新的 provider 行为入口。

## Runtime enforcement

工具权限、文件沙箱、MCP 可见性、取消和超时都由 runtime 执行。Prompt 只解释可用能力，不作为安全边界。

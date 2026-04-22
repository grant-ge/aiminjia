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

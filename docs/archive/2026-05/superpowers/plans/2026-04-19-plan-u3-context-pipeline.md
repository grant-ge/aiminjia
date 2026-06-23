# 长会话上下文预处理管道补齐（Plan-U3）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:test-driven-development` — 先锁住主循环顺序和恢复行为，再改实现。 REQUIRED SUB-SKILL: `superpowers:verification-before-completion` — 必须验证正常路径与 `prompt_too_long` 恢复路径都跑同一条管道。

**Goal:** 把 lotus 主循环从当前零散的 `microcompact + auto_compact + chars/4` 观测，收口为可预测、可回归测试的上下文预处理管道，提升长会话稳定性。

**Architecture:** 对标 `claude-code-best/docs/conversation/the-loop.mdx` 的预处理顺序：先做 tool-result budget，再做微压缩与 collapse，最后才是 auto-compact / reactive compact。lotus 继续复用现有 `compaction.rs` 与 compact boundary 存储，但把 scattered 逻辑抽成单入口 pipeline。

**Tech Stack:** Rust, serde_json

**Worktree branch:** pzc

---

## 背景与现状

| 文件 | 现状 |
|---|---|
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 每轮只显式做 `microcompact()` 与 `should_auto_compact()`；`prompt_too_long` 恢复路径又复制了一套 compact 分支 |
| `src-tauri/src/runtime/chat/compaction.rs` | 已有 `microcompact`、`compact_messages_via_llm`、compact boundary，但没有统一的 pre-processing orchestration |
| `src-tauri/src/llm/context_decay.rs` | 仍保留旧 chars-based decay 思路，但没有和主循环形成稳定顺序 |
| `src-tauri/src/runtime/chat/tool_result_collector.rs` | 有 tool result 结果结构，但没有“预算优先级”这一层治理 |

### 当前问题

- 正常路径与恢复路径各自 compact，逻辑复制，未来很容易漂移。
- 大 tool result 现在更多靠“碰到阈值再压”，缺少 budget-first 的主动治理。
- 没有 collapse 阶段，重复或超长结果只能靠更激进的 compact。

## 范围

- 纳入：
  - 主循环统一 `prepare_messages_for_llm()` 入口
  - tool result budget、microcompact、collapse、auto-compact 顺序固化
  - 正常回合与 `prompt_too_long` 恢复复用同一预处理链
- 不纳入：
  - 更精确的 provider token tokenizer
  - 远程上下文、团队记忆、云端摘要服务
  - 继续维持 daily / analysis 双轨预处理

## 任务拆分

### U3-1：抽出单入口 pre-processing pipeline

- [ ] 新建 `runtime/chat/preprocess.rs` 或同级模块，收口 `prepare_messages_for_llm()`。
- [ ] `chat_turn_driver.rs` 正常回合与 `prompt_too_long` 恢复都通过该入口预处理消息。
- [ ] 把当前 scattered `microcompact / auto_compact` 逻辑迁入统一阶段函数，避免 driver 内重复拼装。

### U3-2：补上 tool result budget 阶段

- [ ] 在 microcompact 之前先做 tool result budget trimming，优先裁剪过大、过旧、低价值的结果。
- [ ] 明确保留规则：最近结果、错误结果、生成文件索引、仍可能被下一步引用的结果不能优先裁掉。
- [ ] 把裁剪决策写成可测试的纯函数，而不是散落在 collector 或 provider 里。

### U3-3：补上 collapse 阶段

- [ ] 为大体积或重复结果增加 `collapse` 表示层，而不是直接进入 LLM summary compact。
- [ ] `collapse` 要和 compact boundary 共存，不能破坏现有历史恢复语义。
- [ ] 如果 `Plan-AI` 仍未执行，先冻结旧分叉；U3 不再接受新双轨分支进入 pipeline。

### U3-4：统一恢复路径

- [ ] `prompt_too_long`、正常 auto-compact、reactive compact 共享同一套 transition 记录，防止重复 compact 或无限重试。
- [ ] 长会话恢复不再在 driver 中散落多个 `continue 'turn` 分支，而是通过明确的 pipeline result 表达。
- [ ] 保留现有 compact boundary 持久化语义，不退回旧的时间戳过滤方案。

### U3-5：回归测试

- [ ] 增加 pipeline 顺序测试：budget -> microcompact -> collapse -> auto-compact。
- [ ] 增加恢复路径测试：`prompt_too_long` 与正常路径输出同一预处理结果结构。
- [ ] 增加幂等性测试：同一轮重复执行 pre-processing 不应不断重写摘要或 boundary。

## 验收标准

- 预处理顺序成为一个单独、可测试、可复用的模块，而不是 driver 里的散点逻辑。
- 大 tool result 优先走 budget / collapse，不再一上来就依赖重型 compact。
- 正常路径与恢复路径的上下文治理顺序一致。
- 整条链路只处理本地上下文治理，不引入远程摘要前提。

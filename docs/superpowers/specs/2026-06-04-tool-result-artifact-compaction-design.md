# Tool Result Artifact Compaction Design

> Date: 2026-06-04
> Scope: AIjia desktop Rust runtime context compaction
> Status: R1 implemented; digest and compact-boundary evidence audit remain follow-up work

## Implementation Status

R1 已落地：
- 长工具结果在进入 `tool_result_collector` 截断投影后，会先保存到 `<conversation>/tool-results/`，并用 `<persisted-tool-result>` 引用替换模型/落盘消息。
- `ToolResultBudget`、`collapse`、`microcompact` 不再把 `<persisted-tool-result>` 引用改写成不可恢复占位符。
- auto/manual compact 的 summary 调用使用预算阶段之前的 evidence snapshot；其中 persisted tool result 会临时展开 artifact 内容，避免 summary 只看到 preview。
- 历史回放已去掉 `char_budget=120_000` 和 `max_rounds=30` 的预裁剪语义，自动压缩前不再先删早期事实。
- AEIT 已增加 `意图-上下文压缩-013` 覆盖长工具输出 artifact 化后的 summary/follow-up 质量。

后续增强：
- tool result digest 生成与 manifest 回写。
- compact boundary metadata 中记录 `evidenceArtifacts` / digest 数量 / lossy projection audit。
- 针对大于 evidence budget 的 artifact 做更细粒度 chunk/digest 策略。

## 背景

当前上下文压缩链路里，工具结果会在多个阶段发生有损改写：

- 工具刚返回时，`tool_result_collector` 会按 `max_tool_result_chars` 直接截断。
- LLM 请求前，`apply_tool_result_budget` 会在全局工具结果字符数超过预算时，把旧工具内容替换成 `[budget-trimmed]`。
- `microcompact` 会在总上下文超过阈值时，只保留最近少量工具结果，把更早工具内容替换成 `[microcompacted]`。

这会破坏自动上下文压缩质量：summary LLM 看到的是已经被裁剪后的投影，而不是完整事实源。如果关键事实只存在于旧工具输出中，summary 根本没有机会保留它。

Claude Code Best 的当前实现采用不同语义：大工具结果先落盘，模型消息只携带 preview 和路径；预算和 microcompact 主要操作请求投影，而不是销毁事实源。本设计将 AIjia 的语义改为同一方向。

## 目标

1. 完整 tool result 必须可恢复，预算策略不能销毁原始证据。
2. 普通 LLM turn 可以使用压缩投影，但投影必须包含可恢复引用。
3. 自动 compact summary 必须基于 evidence packet，而不是基于已经有损压缩的 chat projection。
4. 模型可见消息中不再出现 `[budget-trimmed]` / `[microcompacted]` 这类误导性占位文本。
5. compact boundary 必须记录 summary 使用过的证据来源，便于审计和复现。

## 非目标

- 不在本次重做整个 message storage v2 格式。
- 不要求旧历史中已经被截断的内容神奇恢复。
- 不把所有工具结果永久内联进每次请求。
- 不把 artifact 目录暴露为用户可见文件管理功能。

## 核心语义

系统内区分两类数据：

- `事实源`：用户消息、assistant 关键输出、完整 tool result、文件生成元数据。事实源必须可恢复。
- `请求投影`：为了控制 token，发给 LLM 的压缩视图。投影可以短，但必须带 artifact ref、digest 或完整上下文线索。

预算和 microcompact 只能改变请求投影，不能改变事实源。

## Artifact 存储

每个会话目录新增：

```text
<conv_dir>/
  tool-results/
    manifest.jsonl
    <tool_call_id>.txt
    <tool_call_id>.json
```

`manifest.jsonl` 每行一个记录：

```json
{
  "schemaVersion": 1,
  "toolCallId": "tc_xxx",
  "toolName": "search",
  "path": "C:\\...\\tool-results\\tc_xxx.txt",
  "contentType": "text/plain",
  "originalChars": 123456,
  "previewChars": 2000,
  "sha256": "...",
  "createdAtMs": 1780560000000,
  "digest": null,
  "legacyState": null
}
```

路径规则：

- 文件名只允许由 `tool_call_id` 派生，必须做安全化处理。
- manifest 内保存绝对路径，便于 LLM 和日志定位。
- 写入使用 create-new 语义；同一 `tool_call_id` 重复写入时复用已有记录。
- `tool-results/` 随 conversation 生命周期一起保留。

模型可见引用格式：

```text
<persisted-tool-result tool_call_id="tc_xxx" tool_name="search">
Full output saved to: C:\...\tool-results\tc_xxx.txt
Original chars: 123456
Sha256: ...
Preview:
...
</persisted-tool-result>
```

## 工具结果产生链路

当前 `tool_result_collector` 是纯数据转换模块，原则上不应直接做 I/O。完整改造采用两段式：

1. `tool_result_collector` 继续收集原始 tool result，但不再负责永久截断。
2. `chat_turn_driver` 在拿到 `tool_result_messages` 后、持久化前，调用 artifact projection 服务：
   - 若内容超过工具阈值，完整内容落盘。
   - message content 替换成 persisted ref + preview。
   - manifest 记录 artifact 元数据。

无 `conv_dir` 或落盘失败时：

- 保留原文或现有截断降级，但必须加 telemetry。
- compact evidence projection 将此类结果标记为 `artifact_missing`。
- 后续 budget/microcompact 不允许继续 lossy 清理此类 tool result。

## Budget Projection

`ToolResultBudgetConfig` 的语义从“删旧工具内容”改为“限制本次请求内联工具内容”：

- `aggregate_char_budget` 后续重命名为 `inline_tool_result_char_budget`。
- `keep_recent_tool_results` 后续重命名为 `keep_recent_inline_tool_results`。
- 超预算的旧 tool result 如果有 artifact，投影为 persisted ref。
- 无 artifact 的 tool result 不允许被替换成空 marker。
- error、generated file、preserved tools、recent tool results 默认保留 inline。

禁止模型可见输出：

```text
[budget-trimmed]
```

## Microcompact Projection

`microcompact` 不再输出：

```text
[microcompacted]
```

新行为：

- 当上下文超过触发阈值，只把旧 tool result 从 inline 降级为 persisted ref。
- 最近 N 个 tool result 保留 inline。
- error、generated file、preserved tools、无 artifact 的 tool result 不压缩。
- 与 budget 共用同一个 `can_project_tool_result` 判定。

## Compact Evidence Projection

自动压缩前构造独立 evidence packet：

```rust
prepare_compact_evidence_projection(...)
```

它与普通 chat projection 分离：

- `prepare_chat_projection(...)`：普通 LLM turn，可使用预算投影。
- `prepare_compact_evidence_projection(...)`：summary turn，禁止有损 marker。

Evidence packet 组成：

- 所有非工具 user message。
- assistant 关键文本、工具调用信息、错误信息。
- 小 tool result 全文。
- 大 tool result 的 digest、artifact path、sha256、preview。
- generated file / error tool result 的全文或强摘要。

如果 evidence packet 中出现 `[budget-trimmed]` 或 `[microcompacted]`，视为 bug。

## Tool Result Digest

大工具结果不能只给 summary 一个路径，还需要 digest：

- 小于阈值：直接进入 evidence packet。
- 大于阈值：先生成 digest，写回 manifest。
- digest 内容必须保留：
  - 用户目标相关事实
  - 命令输出结论
  - 错误与失败原因
  - 文件路径和生成物
  - 后续任务依赖的证据

Digest 可以复用现有非流式 summary client，不引入新 provider。

Digest 失败时：

- Evidence packet 使用 preview + artifact ref。
- compact boundary 记录 `digest_missing`。
- 不得伪装成完整 summary。

## Compact Boundary 审计

`CompactBoundaryRecord` 后续新增字段：

```rust
pub evidence_artifacts: Vec<ToolResultArtifactRef>,
pub evidence_digest_count: usize,
pub lossy_projection_used: bool,
```

写入 `compact_boundaries.jsonl` 后，应能追溯：

- summary 使用了哪些 tool result artifact。
- 哪些 artifact 使用 digest。
- 是否存在降级或缺失。

`lossy_projection_used = true` 不应出现在正常自动压缩路径中；测试环境可以用它断言失败。

## 旧会话兼容

旧消息分三类：

1. 完整 tool result 还在消息里：首次需要投影时懒写 artifact。
2. 已经是 `[budget-trimmed]` / `[microcompacted]`：无法恢复，标记 `legacy_lossy_marker`。
3. 旧截断文本含 `[Output truncated: ...]`：标记 `legacy_truncated`。

自动 compact 遇到第 2/3 类时，summary 必须明确知道这是降级证据，不能当作完整事实。

## 测试计划

Rust 单测：

- 大 tool result 能完整落盘，projection 里包含 path、sha256、preview。
- budget 超限后不出现 `[budget-trimmed]`。
- microcompact 后不出现 `[microcompacted]`。
- 无 artifact 的 tool result 不被 lossy 清理。
- compact evidence projection 不接收 lossy marker。

集成测试：

- 第一条旧 tool result 包含唯一事实，后续制造超过预算的工具输出，自动 compact summary 仍保留事实或 digest。
- 连续超过最近 N 个工具结果后，早期工具证据仍可从 boundary 审计恢复。
- generated file / error tool result 不被 artifact projection 误删。

意图测试：

- 长工具型任务压缩后追问早期工具输出里的唯一事实。
- 日志分析任务压缩后追问早期错误码和路径。
- 文件生成任务压缩后追问 fileId / 文件路径。

Benchmark：

- 代码搜索长任务
- shell 日志分析
- MCP 大结果
- 文件生成
- 多轮工具链路

指标：

- fact recall 命中率
- summary 意图保留率
- artifact/digest 引用完整率
- compact 后继续任务成功率
- token 节省比例

## 实施顺序

1. 新增 artifact 存储模块和单测。
2. 在 tool result 持久化前接入 artifact projection。
3. 改 budget 为 artifact ref projection。
4. 改 microcompact 为 artifact ref projection。
5. 拆分 chat projection 和 compact evidence projection。
6. 新增 digest 生成和 manifest 回写。
7. 增强 compact boundary 审计字段。
8. 补齐 Rust 测试、意图测试、benchmark。

## 完成标准

- 模型可见上下文里不再出现 `[budget-trimmed]` / `[microcompacted]`。
- 自动 compact summary 的输入能追溯到完整工具证据或 digest。
- 大工具结果不会因为预算或 microcompact 永久丢失。
- 长工具任务压缩后仍能回答早期工具输出里的关键事实。

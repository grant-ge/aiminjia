# lotus-app vs claude-code-best 全局差距评估

**日期**：2026-04-17  
**调查方式**：三路并行 agent 深度调研（Runtime层、工具/安全层、前端/基础设施层）  
**对标基准**：`/Users/a20250311/github/claude-code-best` 源码  
**关联文档**：
- `docs/2026-04-17-atomic-tool-vs-claude-code-best-gap.md` — 工具层专项差距
- `docs/superpowers/plans/2026-04-17-atomic-tool-capability-upgrade-plan.md` — 当前实施计划

---

## 一、差距总览

| # | 差距 | 层次 | 优先级 | 当前计划覆盖 |
|---|------|------|--------|------------|
| ① | Cancel 后无 synthetic tool_result 注入 | Runtime | **P0** | ❌ |
| ② | 权限 Ask 路径未接通（前端无确认对话框） | Runtime + 前端 | **P0** | ❌ |
| ③ | CapabilityContext 贫瘠（缺 FileStateCache/FileReadingLimits/NotificationSink） | 工具 | **P0** | ✅ Phase 1 |
| ④ | Session state 无 owner（readFileState/usage 每 turn 重建） | Runtime | P1 | ❌ |
| ⑤ | Turn 内部无 cancel checkpoint（只在 iteration 结束检查一次） | Runtime | P1 | ❌ |
| ⑥ | Turn state 可变原地修改（无不可变更新保护） | Runtime | P1 | ❌ |
| ⑦ | 工具执行进度无流式细节（只有 executing/completed 两态） | 前端 | P1 | ❌ |
| ⑧ | 工具结果无大小限制（无 maxResultSizeChars + contentReplacementState） | 工具 | P1 | ❌ |
| ⑨ | bash/file edit/write/grep 基础工具缺失 | 工具 | P1 | ❌ |
| ⑩ | MCP 支持完全缺失 | 基础设施 | P1 | ❌ |
| ⑪ | 工具不能参与权限决策（check_permissions 静态化） | 工具 | P1 | ✅ Phase 3 |
| ⑫ | 工具能力声明静态化（无 isConcurrencySafe/isReadOnly/isDestructive 谓词） | 工具 | P1 | ✅ Phase 2 |
| ⑬ | 并发工具编排缺失（全串行） | 工具 | P1 | ✅ Phase 2 |
| ⑭ | 70% 核心工具仍在 LegacyToolAdapter | 工具 | P1 | ✅ Phase 3（部分） |
| ⑮ | 错误分类体系粗糙（无权限/timeout/capability 分类） | 前端 | P1 | ❌ |
| ⑯ | Subagent state 未隔离（子代理可污染父代理文件缓存） | Runtime | P2 | ❌ |
| ⑰ | 旧工具取消绑定未验证（LegacyToolAdapter fire-and-forget 风险） | 工具 | P2 | ❌ |
| ⑱ | PermissionHook 无 abort 能力 | 权限 | P2 | ❌ |
| ⑲ | Ask suggestions 字段缺失（用户无法一键接受建议规则） | 权限 | P2 | ❌ |
| ⑳ | Tool Pool 无序（prompt cache 不稳定） | 工具 | P2 | ✅ Phase 1 |

---

## 二、P0 — 必须修，会导致功能损坏或 crash

### ① Cancel 后无 synthetic tool_result 注入

**问题**：ESC 发生在工具执行中途时，下一次 LLM 调用的消息历史里出现 `tool_use` 但没有对应 `tool_result`，违反 Anthropic API 格式约束 → API 400 错误 → 对话崩溃无法恢复。

**当前状态**：`CancellationToken` 级联传播机制已有，但 cancel 后没有 synthetic tool_result 注入逻辑。`run_chat_turn_s4` 只在 iteration 结束检查一次 cancel，不在工具执行中途注入。

**对标**：claude-code-best `query.ts` abort 后自动生成 `createToolResultStopMessage(toolUse.id)` 填补每个未完成的 tool_use。

**影响**：高严重度——触发 cancel 后继续对话必然崩溃。

---

### ② 权限 Ask 路径未接通

**问题**：后端 `query_engine.rs` 已能检测到 `ToolDispatchOutcome::AskRequired`，但被注释为 `FIXME(S6)` 直接转成错误返回。前端没有接收 Ask 事件的机制，也没有权限确认对话框。

**当前状态**：权限系统三态（Allow/Deny/Ask）在 `permission.rs` 里定义完整，但 Ask 决策从未真正传到用户——要么被转成 Deny，要么转成错误。

**对标**：claude-code-best 有完整的 `PermissionAskDecision` → UI 对话框 → 用户决策 → `"don't ask again"` 持久化链路。

**影响**：权限系统形同虚设——用户永远无法主动授权，工具只能靠 AlwaysAllow 或被拒绝。

---

## 三、P1 — 架构缺陷，影响核心能力

### ④ Session state 无 owner

**问题**：`readFileState`（文件读取缓存）、`totalUsage`（token 用量）、`permissionDenials` 没有 session 级 owner，每次 turn 从 DB 重新加载历史重建，多轮对话中文件缓存完全失效。

**对标**：claude-code-best `QueryEngine` 持有这些字段，跨 turn 复用。

**影响**：同一文件在多轮对话中被重复读取，token 消耗和延迟更高；token 用量统计不准确。

---

### ⑤ Turn 内部无 cancel checkpoint

**问题**：`run_chat_turn_s4` 只在每次 iteration 结束检查一次 `cancel.is_cancelled()`，工具执行期间（可能数十秒）无法响应 ESC。

**对标**：claude-code-best `query.ts` 在 6 处检查 `abortController.signal.aborted`，覆盖「streaming 进行中」「工具执行前」「工具执行后」。

**影响**：取消响应延迟可达整个工具执行周期。

---

### ⑥ Turn state 可变原地修改

**问题**：`TurnIterationState` 通过 `state.messages.push()`、`state.full_content.push_str()` 直接原地修改，cancel 后 state 可能处于半更新状态。

**对标**：claude-code-best 每次 continue 创建新 `State` object（不可变更新），保证每次迭代状态纯化。

**影响**：调试回溯困难；cancel 后 state 不确定，可能引发后续 turn 异常。

---

### ⑦ 工具执行进度无流式细节

**问题**：工具执行只有 `tool:executing` / `tool:completed` 两个事件，无法展示工具内部进度（bash 命令逐行输出、Python 执行步骤等）。

**对标**：claude-code-best 有分层 `ToolProgress`（`BashProgress`、`MCPProgress`）和 `onProgress` 回调，支持中间状态流式推送。

---

### ⑧ 工具结果无大小限制

**问题**：`ToolResult` 无大小约束，`execute_python` 输出 10MB 数据会直接撑爆 LLM 上下文窗口，无截断机制。

**对标**：claude-code-best 每工具声明 `maxResultSizeChars`，全局 `contentReplacementState` 追踪预算，超限自动截断并通知模型。

---

### ⑨ bash/file edit/write 基础工具缺失

**问题**：没有 `BashTool`、`FileEditTool`、`FileWriteTool`（完整版）、`NotebookEditTool`。这套工具是 claude-code-best agent 能力的基础骨架。

**注意**：`read_workspace_file` 已有，但缺写能力（write/edit）和命令执行能力（bash）。

---

### ⑩ MCP 支持完全缺失

**问题**：代码库中无任何 MCP 相关实现，无法接入 MCP 工具生态。

**对标**：claude-code-best 有完整的 MCP server 生命周期管理（client.ts、config.ts、officialRegistry.ts）、权限模型集成、Progress 流式、动态工具发现。

---

## 四、P2 — 优化项

### ⑯ Subagent state 未隔离

子代理和父代理共享同一 workspace/capability context，并发子代理可互相污染文件读缓存。
**对标**：claude-code-best subagent 的 `setAppState` 是 no-op，`cloneFileStateCache` 共享读/隔离写。

### ⑰ 旧工具取消绑定未验证

~25 个 LegacyToolAdapter 工具是否真正绑定了 CancellationToken 尚未验证，可能存在 fire-and-forget 后台任务泄漏。

### ⑱ PermissionHook 无 abort 能力

Hook 只能观测，无法在工具执行前中断（abort）。

### ⑲ Ask suggestions 字段缺失

Ask 决策中无建议规则字段，用户无法一键接受 "always allow" 等建议。

---

## 五、当前计划覆盖情况

`docs/superpowers/plans/2026-04-17-atomic-tool-capability-upgrade-plan.md` 覆盖以下差距：

| 差距 | 计划 Task |
|------|---------|
| ③ CapabilityContext 扩展（FileStateCache/FileReadingLimits/NotificationSink） | Phase 1 Task 1.2–1.3 |
| ⑳ Tool Pool 排序 | Phase 1 Task 1.1 |
| ⑫ 工具能力谓词（isConcurrencySafe/isReadOnly/isDestructive） | Phase 2 Task 2.1 |
| ⑬ 并发工具编排（dispatch_batch） | Phase 2 Task 2.2–2.3 |
| ⑪ check_permissions 动态权限 | Phase 3 Task 3.1 |
| ⑭ execute_python/generate_report/generate_chart 迁移骨架 | Phase 3 Task 3.2–3.3 |

**计划外的关键差距（按优先级）**：
1. **P0**：① cancel synthetic tool_result 注入、② Ask 路径接通
2. **P1**：④ Session state owner、⑤⑥ Turn cancel 多 checkpoint + 不可变、⑦ 工具进度流式、⑧ 工具结果预算、⑨ bash/file 工具、⑩ MCP

---

## 六、建议后续路线

```
当前计划（Phase 1-3，工具系统内部补齐）
  ↓ 完成后立刻
专项 A：Cancel 修复（synthetic tool_result 注入 + Turn 内多 checkpoint）— P0，单一 sprint
专项 B：Ask 路径接通（后端 AskRequired 事件 + 前端确认对话框）— P0，单一 sprint
  ↓
专项 C：Session state owner（QueryEngine 持有 readFileState/usage 跨 turn）— P1
专项 D：bash/file edit/write 工具 — P1，用户已明确需要
专项 E：工具结果预算（maxResultSizeChars + contentReplacementState）— P1
  ↓
专项 F：MCP 支持 — P1，扩展性
专项 G：Subagent state 隔离 — P2
```

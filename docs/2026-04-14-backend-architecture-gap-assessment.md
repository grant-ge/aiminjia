# lotus-app 后端架构差距全景报告

**日期**：2026-04-14（最后更新：2026-04-14）
**调查方式**：三轮并行 agent 深度调研 + claude-code-best 源码对标
**对标基准**：`/Users/a20250311/github/claude-code-best` 源码

> **阅读说明**
> - 标注 `⚠️ 待复核` 的结论依赖具体行号或当时快照，代码仍在演进，施工前需重新核对
> - 四大架构专项（WF/AT/PS/SK）优先级高于下方所有单点问题
> - 当前唯一必须先关的主 blocker 是 **P1：chat runtime-first 收口**，不与其他专项混做

---

## 一、总体判断

### 当前状态

lotus-app 已具备明确的 runtime-first 结构雏形：`SessionRuntime` / `QueryEngine` / `ToolDispatcher` / `RuntimeEventBus` / `RuntimeRepositoryFacade`。

但对标 claude-code-best，当前更准确的描述是：

> **runtime 外壳成型，真实生产主链路仍由 legacy transport 主导；权限、取消、工具 context 三大核心机制与 claude-code-best 存在设计哲学级差距，而不仅仅是实现进度差距。**

### 三个设计哲学级差距

| 维度 | claude-code-best 的方式 | lotus-app 当前 | 差距性质 |
|------|----------------------|---------------|---------|
| **安全模型** | 权限系统（用户决策 allow/deny/ask）是唯一安全边界 | 静态代码检查 + 路径白名单（可被绕过） | 哲学差距 |
| **取消机制** | AbortController 级联广播，同步传播到 LLM stream + 工具 + 子进程 | `tokio::spawn` fire-and-forget，无真实取消 | 哲学差距 |
| **工具 Context** | per-call `ToolUseContext` 注入，子 agent `setAppState` 为 no-op | `PluginContext` 全局 service locator，206 处引用 | 哲学差距 |

---

## 二、优先级路线图

> 这是执行顺序的顶层定义。各阶段不混做。

| 阶段 | 目标 | 验收标准 |
|------|------|---------|
| **P0** | 修正文档，冻结可信 backlog | 本文档完成更新，待复核项已标注 |
| **P1** | 关闭 chat runtime-first 主链路专项 | 真实 adapter path gating tests 全绿；closure review 可以关闭；文档不再写"未闭合" |
| **P2** | WF + AT 两个专项各自成计划并完成 | 各自验收标准（见下方专项详述） |
| **P3** | PS + SK 两个专项各自成计划并完成 | 各自验收标准（见下方专项详述） |
| **P4** | 基础设施收尾：PluginContext 退出 / AppStorage 迁移 / policy engine / 取消传播 / AgentRuntime 持久化 / Python 安全模型 | 各专项 gating test 通过 |

---

## 三、P1 主 Blocker：chat runtime-first 收口

> 来源：`docs/reviews/2026-04-14-chat-runtime-first-closure-review.md`
> **这一阶段只盯 4 件事，不引入其他改动。**

### 4 件必须关闭的事

| 编号 | 问题 | 证据位置 | 状态 |
|------|------|---------|------|
| **B1** | legacy executor 仍是聊天主循环 owner | `chat.rs:150`、`chat_turn_driver.rs:114`、`chat_runtime_impl.rs:112` | ✅ T1/T2 GREEN：ToolRoundDriver 已接入，executor-backed 路径通过 QueryEngine 分发工具调用 |
| **B2** | production-path gating 不能证明真实收口（测的是裸 SessionRuntime，不经过真实 adapter） | `chat_runtime_first_mainline_test.rs:30`、`review_runtime_executor_bypass_test.rs:24` | ✅ T2 GREEN：`run_chat_turn_with_calls` → `ToolRoundDriver::execute_round` → `QueryEngine` → `SpyTool` |
| **B3** | `authorized_workspace_store` wiring 顺序错误，`try_state` 静默返回 None | `lib.rs:222`（chat_adapter 创建）vs `lib.rs:256`（facade 注册）| ✅ 已修复：facade 现在在第 233 行注册，chat_adapter 在第 241 行创建 |
| **B4** | tool round 的最终 truth source 未统一：transport 层 new 孤立 QueryEngine + 孤立 RunId | `chat_runtime_impl.rs:2734`（孤立 QueryEngine）、`session_runtime.rs:88` vs `chat_runtime_impl.rs:126`（双 RunId）| ✅ T3 regression gate 绿，RunId 已统一 |

### 缺失的 gating tests（已全部补齐并转绿）

| 测试 | 状态 |
|------|------|
| T1 `send_message_production_path_full_turn_must_not_delegate_to_legacy_executor` | ✅ GREEN |
| T2 `send_message_production_tool_round_must_dispatch_via_runtime_query_engine` | ✅ GREEN |
| T3 `send_message_production_path_must_use_single_run_id` | ✅ GREEN |
| T4 `send_message_production_path_message_persisted_must_be_emitted_not_record_only` | ✅ GREEN |

### P1 关闭条件

- [x] B1-B4 全部修复
- [x] 上述 4 条 gating tests 全绿（4/4 通过）
- [x] `2026-04-14-chat-runtime-first-closure-review.md` 状态更新为"已关闭"
- [x] 本文档 B1-B4 行去掉"待复核"标注

**P1 已关闭（2026-04-14）。**

---

## 四、四大架构专项

> 来源：`docs/2026-04-12-runtime-gap-problem-statement.md`
> 这 4 个专项在 P1 收口之后**各自独立成计划**，各自有目标、验收标准和测试路径，不揉成一轮"顺手优化"。

### 专项 1（WF）：Workspace-First 文件能力模型

**优先级**：P2

#### 问题本质

当前系统的文件模型是 `upload-first`，不是 `workspace-first`。

- 用户文件先被复制到 `workspace/uploads/` 后，后续工具才围绕这些副本工作
- "选择本地目录"没有被建模成一等工作对象
- agent 更像"导入文件后再分析的助手"，而不是"可对本地目录进行连续工作的代理"

#### 代码证据

| 文件 | 行 | 说明 |
|------|---|------|
| `storage/file_manager.rs` | 66, 92 | 强制 copy 到 uploads/ |
| `python/sandbox.rs` | 68 | sandbox 路径围绕 workspace 子目录 |
| `llm/tool_executor/report.rs` | 31 | report 工具假设文件在 uploads/ |
| `prompts/base.md` | 13, 37 | prompt 硬写了目录结构约定 |

#### 验收标准（来自原专项定义）

- 用户可选择任意本地目录作为工作目录，不需要先 copy 进 `uploads/`
- agent 至少支持对授权目录：列出内容、读取文件、搜索文件、直接分析并输出
- Python / 文件工具不再要求源文件来自 `file_id + uploads/`
- 安全边界成立：只能访问用户明确授权的目录范围
- 现有上传流程不回归：`upload_file → load_file → execute_python` 仍可用

---

### 专项 2（AT）：Atomic Tool 工具体系

**优先级**：P2

#### 问题本质（两个独立半边）

**半边 A：默认工具面过宽 + 复合工具语义过重**

- daily 模式默认暴露 23 个大工具，LLM 要猜"哪个大工具最接近需求"
- 大量工具带有明显业务假设或多段动作语义（一个工具隐含 "读取 + 分析 + 生成 + 导出"）
- 工具失败时无局部恢复能力（一个复合工具挂掉 → 整条链报废）

**半边 B：工具迁移率不足**

- 16/27 工具仍走 `LegacyToolAdapter`，双轨并存

#### 代码证据

| 文件 | 行 | 说明 |
|------|---|------|
| `plugin/builtin/tools/mod.rs` | 1, 37 | 默认工具注册全量列表 |
| `plugin/context.rs` | 57, 74 | service locator 承载所有工具依赖 |
| `llm/tools.rs` | 29 | 工具 schema 全量暴露给 LLM |
| `chat_runtime_impl.rs` | 1595 | ⚠️ 待复核：daily 模式工具过滤逻辑 |

#### 验收标准（来自原专项定义）

- daily 模式默认工具集显著收敛，不再是"默认 23 个大工具全开"
- 每个基础工具不允许同时隐含多段语义
- 复杂工作流能力转移到 skill / workflow / orchestration 层
- 工具调用链路唯一：lookup → permission → execution → audit/event
- 至少通过 3 类组合场景验证工具原子性（文件读取 / 本地目录 / 联网+本地混合）
- 一个工具失败时，agent 可以局部恢复

---

### 专项 3（PS）：Prompt Slimming 提示词职责回收

**优先级**：P3

#### 问题本质

`base.md + daily.md` 是操作手册，不是轻量 prompt。当前 prompt 同时承载：身份/语气、工具协议、文件处理规则、目录结构说明、数据传递规则、记忆策略、输出规范。

**后果**：prompt 变成隐藏编排层。改运行时能力时必须先改 prompt；简单对话也被厚 prompt 拉成模板化行为；工具和文件语义与 prompt 强耦合。

#### 代码证据

| 文件 | 行 | 说明 |
|------|---|------|
| `prompts/base.md` | 3, 13, 25, 44 | 工具协议/目录结构/规则硬编码 ⚠️ 待复核（prompts.rs 已修改） |
| `prompts/daily.md` | 9, 17 | 工具决策优先级/轮次限制 ⚠️ 待复核 |
| `llm/prompts.rs` | 174 | prompt 加载与构建逻辑 |

#### 验收标准（来自原专项定义）

- daily 默认 system prompt 明显瘦身
- prompt 中不再出现：必须先 `load_file` 再 `execute_python`、报告数据先写 JSON、一轮最多几次工具调用、工作目录子目录列表
- 这些规则改由 runtime / tool schema / permission / workflow 配置保证
- 简单对话场景改善（打一声招呼不再触发长篇模板化介绍）
- prompt 变更后工具调用正确率不能明显下降

---

### 专项 4（SK）：Skill 本地导入 / 打包导入模型统一

**优先级**：P3

#### 问题本质

当前本地 skill 安装只支持"源码目录"，不支持直接导入本地 `.skill` / `.aijia-skill` 文件。

- 用户看到选择器时不清楚应该选"目录"还是"文件"
- 本地开发型导入与本地分发型导入不是统一模型
- marketplace 安装、本地源码安装、本地打包导入之间语义不一致

#### 代码证据

| 文件 | 行 | 说明 |
|------|---|------|
| `src/components/settings/SkillsTab.tsx` | 88, 120 | 只接受目录选择 |
| `src/lib/tauri.ts` | 930 | IPC 声明 |
| `commands/skill_management.rs` | 75, 322 | 后端只处理目录路径 |

#### 验收标准（来自原专项定义）

- Skills 页面明确支持并区分：从目录安装 / 从打包文件安装
- `.skill` / `.aijia-skill` 类型的本地包可以被直接选择并导入
- 目录型 skill 源码安装继续保留
- UI 文案与系统行为一致，不再出现"看起来像能选文件，实际只认目录"的歧义

---

## 五、问题清单（完整版）

> 以下问题按类别排列，P0-P4 优先级参见第二节路线图。

### 分类总览

| 类别 | 编号 | 问题 | 严重程度 | 复核状态 |
|------|------|------|---------|---------|
| **P1 主 Blocker** | B1 | legacy executor 仍是聊天主循环 owner | 🔴 | ⚠️ 待复核行号 |
| | B2 | production gating 不能证明真实收口 | 🔴 | ⚠️ 待复核 |
| | B3 | `authorized_workspace_store` wiring 静默降级 | 🔴 | ⚠️ 待复核行号 |
| | B4 | 双 RunId + 孤立 QueryEngine，tool round truth source 未统一 | 🔴 | ⚠️ 待复核行号 |
| **安全（P0）** | M1 | Claude provider 多轮 tool calling 损坏（`tool_calls` 字段丢失）| 🔴 严重 Bug | 可信 |
| | PY2 | Python 子进程继承父进程全部 env var（含 API key） | 🔴 高危 | 可信 |
| | PY3 | `_restore.py` pickle 反序列化（上传文件触发 RCE） | 🔴 高危 | 可信 |
| | S1 | `AgentRuntime::for_test()` 用于生产（InMemory，重启丢失） | 🔴 严重 | 可信 |
| **聊天主链路** | L1 | `streaming:delta`/`streaming:error` 绕过 EventBus | 🔴 | ⚠️ 待复核（chat_runtime_impl.rs 已修改） |
| | L2 | 工具轮 EventBus 孤立（transport 层 new 新实例） | 🔴 | ⚠️ 待复核 |
| | L3 | `tokio::spawn` fire-and-forget，无真实取消 | 🔴 | ⚠️ 待复核 |
| | L4 | Failing gating test（SpyTool 注册��错误 QueryEngine 实例上）| 🔴 | ⚠️ 待复核是否仍失败 |
| **权限系统** | P1 | 无 ask 流程，无用户确认 UI | 🟠 | 可信（P4） |
| | P2 | 无规则持久化，每次重新 presence check | 🟠 | 可信（P4） |
| | P3 | fail-open 默认（未知 scope 直接放行） | 🟠 | 可信（P4） |
| | P4 | `network` scope 无任何拦截 | 🟠 | 可信（P4） |
| **工具系统** | T1 | `PluginContext` 体系级 service locator（206 处） | 🟠 | ⚠️ 待复核引用数（P4） |
| | T2 | 16/27 工具未迁移到 RuntimeTool | 🟠 | ⚠️ 待复核迁移进度 |
| | T3 | TRANSITIONAL bridge 形成嵌套反模式 | 🟡 | ⚠️ 待复核 |
| | T4 | `REQUEST_SCOPED_RUNTIME_TOOL_NAMES` 白名单无编译期保护 | 🟡 | 可信 |
| | T5 | `plan_update`/`update_plan` 名称不一致，schema 来源分裂 | 🟡 | 可信 |
| **存储/启动** | S2 | `conversation_service.rs` 直接依赖 AppStorage（trait 已定义未迁移）| 🟠 | 可信（P4） |
| | S3 | AppStorage 全局写锁，多会话并发锁竞争 | 🟠 | 可信（P4） |
| **LLM 集成** | M2 | LlmGateway 无 trait，无法 mock | 🟠 | 可信（P4） |
| | M3 | Token 计数用字符数，中文误差 3-4 倍 | 🟠 | 可信 |
| | M4 | 关键词路由过度匹配（"薪酬"触发分析任务路由） | 🟡 | 可信 |
| | M5 | temperature 硬编码 0.7 | 🟡 | 可信 |
| | M6 | 仅输出端防护，无输入端 prompt injection 检测 | 🟡 | 可信 |
| **Python 沙箱** | PY1 | 静态检查可被绕过（3 行代码绕过） | 🔴 | 可信（P4：废弃沙箱） |
| | PY4 | 读操作完全不受限 | 🔴 | 可信（P4） |
| | PY5 | precompute 脚本跳过 `validate_code` | 🟠 | 可信 |
| | PY6 | kill 后不 wait，产生僵尸进程 | 🟡 | 可信 |
| | PY7 | `_restore.py` crash 恢复可能加载错误对话 checkpoint | 🟡 | 可信 |
| **前端 IPC** | F1 | 双路 `streaming:delta` 无去重，内容翻倍 | 🟠 | ⚠️ 待复核（取决于 L1 修复状态） |
| | F2 | `message:updated` content 类型错误（裸字符串 vs 结构体） | 🟠 | ⚠️ 待复核 |
| | F3 | `renameConversation` 无错误处理，乐观更新后失败不回滚 | 🟡 | 可信 |
| | F4 | `taskStates` 从不删除，长期运行内存增长 | 🟡 | 可信 |
| | F5 | `streaming:done` payload 缺 `messageId` | 🟡 | 可信 |
| | F6 | `file:parsed` 事件死代码 | 🟡 | 可信 |
| **Agent/Skill** | A1 | `cancel_run()` 纯 store 操作，不发取消信号 | 🔴 | 可信（P4） |
| | A2 | sub-agent `for_test()` fallback 导致状态孤立 | 🟠 | 可信 |
| | A3 | sub-agent 工具事件无 scope 标记 | 🟠 | 可信 |
| | A4 | Task `set_status()` 每次 `std::thread::spawn` | 🟠 | 可信（P4） |
| | A5 | `tools_only` 白名单是软约束 | 🟡 | 可信 |
| | A6 | 外部 skill 安装无路径穿越防护 | 🟠（安全） | 可信 |
| | A7 | `reload_skill` 接受任意路径，无鉴权 | 🟡（安全） | 可信 |
| **文档/测试漂移** | D1 | T1/T4 测试注释标 RED-LIGHT 但 `record_only` 已修复，应为 GREEN | 🟡 | ⚠️ 需要核实后更新注释 |
| | D2 | `mark_executor_backed()` 死代码（全局无调用） | 🟡 | 可信 |
| | D3 | `tag_file_tool_result` dead code（file metadata 无法透传） | 🟡 | 可信 |
| | D4 | 错误类型碎片化（三套并行）| 🟡 | 可信（P4） |
| | D5 | `RuntimeRunRegistry` 用 `std::sync::Mutex`（非 async-aware） | 🟡 | 可信 |

---

## 六、核心问题详述

### B 系列：P1 主 Blocker 详述

#### B1 + B4：legacy executor 仍是主循环 owner + tool round 未统一

⚠️ **以下调用链和行号基于 2026-04-14 调查快照，代码仍在演进，施工前须重新核对。**

问题定性仍成立：
- `TauriLegacyTurnExecutor` 仍直接调用 `legacy_send_message_impl()`
- `RuntimeChatTurnDriver` 在 executor-backed 路径里是 wrapper，不是 owner
- transport 层自建 `QueryEngine` 实例，不是 `SessionRuntime` 注入的那个
- `SessionRuntime` 生成 run_id A，`legacy_send_message_impl` 再自生成 run_id B

修复方向：
- `ChatTurnRequest` 加 `run_id` 字段，legacy executor 消费 runtime 传入的同一份
- transport 层不再 new 本地 QueryEngine，使用 SessionRuntime 注入的实例
- `legacy_send_message_impl()` 拆成窄 helper（host emit/settings glue/compat mapping），不再持有完整聊天主循环

#### B2：gating 证明力不足

现有测试的证明边界（核实后若已新增真实 adapter 测试则可删除此项）：

| 测试文件 | 证明了什么 | 不能证明什么 |
|---------|-----------|-------------|
| `chat_runtime_first_mainline_test.rs` | 裸 SessionRuntime 的局部 contract | 真实 adapter wiring 已 runtime-first |
| `review_runtime_executor_bypass_test.rs` | executor bypass 约束成立 | 同上 |
| `chat_runtime_dispatcher_production_path_test.rs` | QueryEngine::run_tool_with_bus 路径正确 | 真实 send_message 已走此路径 |

缺失的四条 gating test（必须从 `TauriChatCommandAdapter` 真实入口驱动）：
1. `send_message_production_adapter_should_not_delegate_full_turn_to_legacy_impl`
2. `send_message_production_tool_round_should_dispatch_via_runtime_query_engine`
3. `send_message_production_path_should_preserve_single_run_id`
4. `send_message_production_events_should_be_bus_emitted_not_record_only`

---

### M1：Claude provider 多轮 tool calling 损坏（P0，可立即修复）

`claude.rs:73-82` 消息序列化只发 `role` 和 `content`，完全丢弃 `tool_calls` 和 `tool_call_id`：

```rust
json!({ "role": msg.role, "content": msg.content })
// tool_calls / tool_call_id 字段缺失
```

Anthropic API 要求历史消息中 assistant turn 必须含 `tool_use` content block，user turn 必须含 `tool_result` block。多轮 tool calling 时 API 返回 400。**Claude provider 目前只能安全用于无工具的纯对话场景。** 对比 `openai.rs:111-134` 已正确处理。

---

### PY 系列：Python 沙箱根本性问题（P0 修安全漏洞，P4 废弃沙箱）

**根本性结论：静态检查安全假设错误，沙箱方案应在 P4 阶段废弃，改用权限系统管控。**

claude-code-best 的 BashTool 没有任何沙箱，直接在用户 shell 中执行，安全边界由权限系统（用户决策 allow/deny/ask）保证。

两个 P0 级系统漏洞（无法在 Python 层修补，必须立即修复）：
- **PY2**：子进程完整继承父进程 env var，`import os; print(os.environ)` 一行读取所有 API key
- **PY3**：`pickle.load` 无条件加载 checkpoint 文件，上传构造的 .pkl 文件触发 RCE

静态检查绕过示例（说明 validate_code 无效）：
```python
__import__('sub' + 'process')          # 字符串拼接
getattr(os, 'sys' + 'tem')('id')       # getattr 间接调用
```

---

### 其他重要问题（摘要）

**T2：工具迁移阻碍分级**（⚠️ 具体迁移进度待复核）

| 难度 | 工具 | 核心阻碍 |
|------|------|---------|
| **轻** | `save_analysis_note`, `update_plan`, 4 个 memory 工具 | 只需 AppStorage + conversation_id |
| **中** | `execute_python`, `hypothesis_test`, `detect_anomalies` | 依赖 `app_handle`，需 RuntimeHost trait |
| **重** | `generate_report/chart/slides`, `export_data`, `update_progress` | `auth_manager` 渗透工具层 |
| **结构性** | `browse_data`, `browse_and_extract` | 工具层内启动 sub-agent LLM 循环，需 `AgentDelegate` trait |

**M3：Token 计数中文误差**

中文约 1 char ≈ 1 token，英文约 4 chars ≈ 1 token。`COMPRESS_THRESHOLD_CHARS=24000` 对中文对话实际误差 3-4 倍，导致压缩时机不准确。

---

## 七、claude-code-best 对标：核心设计参考

> 这些设计是 P4 阶段的方向，P1 阶段不触碰。

### 设计一：Per-call ToolUseContext（替代 PluginContext）

```typescript
tool.call(args, context: ToolUseContext, ...)
// 子 agent 的 setAppState 是 () => {}（no-op），天然隔离状态写入
// 子 agent 拿 cloneFileStateCache(parent)，共享读，隔离写
```

### 设计二：责任链权限系统（替代 CapabilityPermissionPipeline）

```
工具调用 → 责任链（deny > ask > allow > passthrough）
        → ask 时弹 UI + 并行 AI 分类器
        → "don't ask again" → 写入持久化规则文件
```

### 设计三：AbortController 级联（替代 fire-and-forget）

```
用户 ESC → abort() → LLM stream 中止 → 工具进程 SIGKILL → synthetic "Interrupted" tool_result
父子 AbortController WeakRef 级联，父 abort 自动传播到子
```

### 设计四：Command Queue（后台任务完成通知 LLM）

```
任务完成 → enqueuePendingNotification(priority, xml)
         → query loop 下次迭代取出 → 作为 user message 送给 LLM
```

---

## 八、已完成的部分

- runtime 入口层已建立，聊天请求进入 `SessionRuntime`
- `RuntimeEventBus` 已建立，`TauriEventAdapter` 事件映射已定义
- `ToolDispatcher` / `PermissionPipeline` / `CapabilityContext` 已有真实落点
- `RuntimeRepositoryFacade` 已开始替代部分直接 file-store 调用
- 11/27 工具已迁移到 RuntimeTool contract（⚠️ 待复核当前迁移数）
- `ToolRoundDriver` 已建立并接入 `QueryEngine`
- Phase 0-4 + 专项联调计划已关闭，大部分 gating tests 通过

---

*本文档基于三轮并行 agent 调查（2026-04-14）及 claude-code-best 源码对标。代码仍在演进，带 ⚠️ 待复核标注的结论施工前需重新核对实际代码状态。*

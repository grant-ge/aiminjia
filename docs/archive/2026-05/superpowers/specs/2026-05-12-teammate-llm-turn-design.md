# Teammate LLM Turn 设计

**Date**: 2026-05-12
**Status**: Draft（待 review）
**Author**: 与 Claude 协作生成
**Scope**: LTR MVP 收口 —— 让 Teammate 真接 LLM 跑起来

---

## 1. 背景

### 1.1 现状

LTR（Lotus Team Runtime）`ltr-mvp` 分支已经实现了 P0/P1 全部 + P2 通信原语（SendMessage / shutdown handshake / Lead idle 触发 / task-notification / is_async / plan_approval / broadcast）。**唯独 Teammate 真接 LLM 这一步没做。**

具体表现：今天 `conversation 72740bae` 的会话，Lead 成功 spawn 了小研 + 小算两个 teammate，但两个 teammate 的 transcript JSONL 里只看到：

```
{"role":"user","content":"<system-reminder>..."}
{"role":"assistant","content":"[P1 stub] 小研 received: <system-reminder>..."}
{"role":"user","content":"你是小研，负责..."}
{"role":"assistant","content":"[P1 stub] 小研 received: 你是小研..."}
```

不是 bug，是占位实现没替换。

### 1.2 根因

`src-tauri/src/runtime/agent/worker_runtime.rs:1175` 收到 inbox 消息后直接调 `teammate_stub_turn`（同文件 1242-1292 行），这个函数只往 transcript append 两行假数据就返回。注释明确写"Full LLM integration is deferred to P2"。

**但 P2 的 11 个 task 全去做通信原语了，没有任何一步是"把 stub 替换成真 LLM call"**。最接近的 P2.11（端到端冒烟）只写了测试 spec 占位（`docs/test-intents/spec/tasks/ltr-e2e/rules.md`），没写实施步骤。

这是 LTR plan 的盲区 —— 设计 plan 时把"Teammate"想当然等同于"长期版的 SubagentWorkerRuntime"，没把"inbox-driven + history 跨 turn"作为独立设计问题。

### 1.3 影响

LTR MVP 在最关键的"teammate 真的会干活"那一步停在了 stub 上：
- Teammate 永远不会主动用 SendMessage 给 Lead 回报
- Lead 永远收不到 teammate 的产出
- P2.11 三场景端到端冒烟无法跑（依赖 teammate 真接 LLM）
- `ltr-mvp` 分支没法合 main —— 合了用户看不到任何价值

---

## 2. 设计目标

### 2.1 In Scope（MVP）

- Teammate 收到 inbox 消息后真正调 LLM
- LLM 能正常使用其工具白名单（SendMessage / TaskList / WebSearch / Read / Write / Bash 等）
- 对话 history 跨 inbox 消息保留 —— Lead 追问时 teammate 记得上一次干了什么
- 工具调用 + 结果完整写入 transcript JSONL（含 `tool_call_id` 字段，符合 Anthropic 协议）
- Cancel / shutdown handshake 行为不变 —— 复用现有 select! loop

### 2.2 Out of Scope（留给后续，但不能堵死路径）

- **重启续命**：teammate 进程死了 / session 结束 → 内存 messages 丢失。未来从 transcript JSONL 还原 messages 再继续。MVP 不实现，但 transcript schema 必须正确。
- **中途打断**：inbox 来新消息时，正在跑的 LLM iteration 不被打断 —— 新消息排队，等当前 turn 跑完。未来可加 "iteration 边界 try_recv inbox"。
- **History compact / 截断**：靠 max_iterations 上限兜底；未来加 token 计数 + 自动 compact。

### 2.3 不在范围（永不做）

- 复用 `RuntimeChatTurnDriver` —— 那是为前端 IPC 准备的（load history from SQLite / persist message to SQLite / emit StreamDelta 到 webview），Teammate 用不上。

---

## 3. 总体方案

### 3.1 一句话

**Teammate idle loop 持有 `Vec<ChatMessage>` 跨 turn 累积，inbox 收消息时直接调 `LlmGateway` + `ToolRoundDriver` 跑一段精简的 LLM iteration 循环，跑完不退出回 select! 等下一条。**

### 3.1.1 设计偏差说明（vs 初稿）

初稿（§3.2 老版）打算抽一个 `run_llm_iterations` 共享函数让 subagent / teammate 共用。**实施时退回**：

- SubagentWorkerRuntime 的循环里掺杂了大量 subagent 特有副作用（前端 emit tool_executing/completed、generated_files 收集、terminal_tool_results 收集、pending_ask 上浮给父级、SubAgentResult envelope 打包）
- 强行抽离会让共享函数签名肿胀成 5+ 个可选回调
- Teammate 完全不需要这些 —— 它通过 SendMessage 给 Lead 回报，没"父级在等返回值"语义

**最终决定**：共享层下沉到 `LlmGateway` + `ToolRoundDriver` 这两个底层引擎（本来就独立可复用）。Teammate 自己写一段精简循环（~120 行），不复用 SubagentWorkerRuntime 的循环骨架。两边代码主结构相似但各自管副作用，避免"伪复用"。

升级路径不受影响 —— teammate 的 messages 仍归调用方持有，cancel 走 CancellationToken，未来重启续命 / 中途打断的扩展点完全一样。

### 3.2 层级图

```
┌─────────────────────────────────────────────────────────────┐
│ LlmGateway（Anthropic /v1/messages 客户端）                  │  现有
└─────────────────────┬───────────────────────────────────────┘
                      │
┌─────────────────────┴───────────────────────────────────────┐
│ ToolRoundDriver（工具执行 + permission + cancel 支持）        │  现有
└─────────────────────┬───────────────────────────────────────┘
                      │
┌─────────────────────┴───────────────────────────────────────┐
│ run_llm_iterations（共享 LLM 循环）                          │  ★ 新建
│  - LLM stream 一次                                            │
│  - 解析响应 → push messages                                  │
│  - 调 tool round → push messages                             │
│  - 重复到 EndTurn 或 max_iterations                          │
│  - 借走 &mut messages，跑完留住，调用方决定 messages 生命周期 │
└─────────┬──────────────────────────────┬────────────────────┘
          │                              │
   SubagentWorkerRuntime           run_teammate_idle
   （subagent::run_worker_turn）   （worker_runtime.rs）
          │                              │
   跑完 return SubAgentResult       跑完不 return，回 select!
                                    等下条 inbox 消息再调
```

### 3.3 关键设计决策

| 项 | 决定 | 理由 |
|---|---|---|
| messages 归属 | idle loop 的局部 `Vec<ChatMessage>` | 跟 session 共生死，符合 LTR cleanup 语义；状态归调用方，共享函数无状态 |
| 持久化 | transcript JSONL 实时 append（已有路径，扩 schema） | 跨进程崩溃不丢；调试可见；为未来 transcript replay 续命留口子 |
| schema 扩展 | `TranscriptLine` 加 `tool_calls / tool_call_id / tool_name` | **铁线**：不带 tool id 则未来 replay 无法满足 Anthropic 协议的 tool_use/tool_result 配对要求 |
| 共享函数 vs 复制 | 抽共享函数 | 防止 subagent/teammate 行为飘移；复用代码 |
| 不走 driver | ✅ | RuntimeChatTurnDriver 是前端 IPC 适配层，会读写存储 / 推 StreamDelta，teammate 用不上 |
| 不打断当前 turn | ✅ | mpsc 天然排队语义；MVP 简单；未来加 iteration 边界 try_recv 可扩展 |
| 不重启续命 | ✅（MVP） | 但 schema 必须支持，否则未来扩展堵死 |

---

## 4. 模块设计

### 4.1 Teammate LLM 循环（直接在 `worker_runtime.rs::run_teammate_idle` 内）

**职责**：跑一段完整的 agentic turn —— LLM stream + tool round + messages 累积，直到 EndTurn 或 max_iterations 或 cancel。

**核心算法**（精简版，对照 SubagentWorkerRuntime:287-617 但去掉 subagent 专属副作用）：

```
loop {
    if cancelled → break;
    if iter ≥ max_iterations → break;
    
    stream = gateway.stream_message(messages.clone(), tool_defs.clone(), ...).await;
    (text, tool_calls, stop_reason) = drain_stream(stream).await;
    
    if stop_reason != ToolUse || tool_calls.is_empty() {
        if !text.empty() {
            messages.push(assistant(text));
            transcript.append(...);
        }
        break;
    }
    
    messages.push(assistant_with_tool_calls(text, tool_calls));
    transcript.append(...);
    
    round_results = tool_round.execute_round(turn, bus, tool_calls).await;
    for r in round_results {
        match r {
            Completed { tool_call_id, tool_name, content, .. } => {
                messages.push(tool_result(tool_call_id, tool_name, content));
                transcript.append(...);
            }
            Blocked { tool_call_id, tool_name, reason } => {
                messages.push(tool_result(tool_call_id, tool_name, reason));
                transcript.append(...);
            }
            AskRequired { tool_call_id, tool_name, .. } => {
                // P2.8: is_async = true, permission Ask 已 auto-deny
                // 走到这里说明 deny 反馈以 tool_result 形式回到 LLM
                messages.push(tool_result(tool_call_id, tool_name, "User interaction required..."));
                transcript.append(...);
            }
            InteractionRequired { ... } => 同 AskRequired
        }
    }
}
```

**关键性质**：
- messages 跨 turn 留在 idle loop 局部变量
- 实时 append transcript JSONL（每条 message 落一行）
- cancel 在 stream drain + tool round 内自动响应
- 不 emit tool_executing/completed 到前端（teammate 不出现在前端 UI）
- 不收集 generated_files / terminal_tool_results（teammate 通过 SendMessage 给 Lead 汇报）

### 4.2 SubagentWorkerRuntime 不动

行为完全保持现状 —— 仍跑自己的 `'agent_loop`，仍一次性 store_transcript，仍打包 SubAgentResultEnvelope。

未来若要进一步合并代码，可单独立项；本期不动以降低风险。

### 4.3 Teammate idle loop 接 LLM

**架构修正（实施时发现的设计层错位）**：

P1.6 把 teammate 的 idle loop 启动放在了 `runtime/tools/builtin/spawn_subagent.rs`。这是 **runtime 层**，跟 `CapabilityContext` 一样故意不持有 `LlmGateway`（runtime 层的纯度约束）。因此 idle loop 拿不到 gateway，只能写 stub。

对照 subagent 路径：subagent 的真正执行**走 launcher trait**（`SpawnSubagentLauncher::launch_sync` / `launch_async`），实现在 `llm/tool_executor/spawn_subagent.rs`（**infra 层**，持有 gateway）。runtime 层只调 launcher trait，不知道 gateway 存在。

→ 修正：**teammate 启动也必须走 launcher trait**。

**改动**：

1. `SpawnSubagentLauncher` trait 加一个方法：

```
async fn launch_teammate(
    &self,
    request: SpawnSubagentRequest,
    context: SpawnSubagentContext,
    teammate_extras: TeammateLaunchExtras {
        agent_id, agent_name, employee_id, team, tool_whitelist,
        sys_prompt_extra, inbox, conv_dir,
        agent_names, inbox_registry, cancellation_registry,
        teammate_cancel,
    },
) -> Result<(), anyhow::Error>;
```

2. `llm/tool_executor/spawn_subagent.rs::SpawnSubagentLauncherImpl::launch_teammate` 实现：
   - 调 `build_run_components()` 拿到 `Arc<LlmGateway>` / `Arc<ToolRegistry>` / `AppSettings`
   - 构造完整 `TeammateWorkerCtx`（含 gateway / tool_registry / runtime_deps / settings 字段）
   - `tokio::spawn(run_worker(WorkerMode::TeammateIdle { ... }, ctx, Some(initial_prompt)))`

3. `runtime/tools/builtin/spawn_subagent.rs` 的 teammate 分支改成调 `launcher.launch_teammate(...)`，删除原来直接 spawn 的代码

4. `TeammateWorkerCtx` 扩字段（在 `worker_runtime.rs`）：
   - `gateway: Arc<LlmGateway>`
   - `tool_registry: Arc<ToolRegistry>`
   - `runtime_deps: SubAgentRuntimeDeps`（复用 subagent 现有的 deps 类型）
   - `settings: AppSettings`

5. `run_teammate_idle` 用 ctx 的这些字段构造循环依赖，跑 §4.1 精简循环

**删除**：`teammate_stub_turn` 函数整段删（1242-1292 行）。

**文件**：`src-tauri/src/runtime/agent/llm_turn.rs`（新建）

**职责**：跑一段完整的 agentic turn —— LLM stream + tool round + messages 累积，直到 LLM 不再调工具（EndTurn）或达到 `max_iterations` 上限或 cancel。

**接口签名**（伪代码）：

```
pub async fn run_llm_iterations(
    messages: &mut Vec<ChatMessage>,   // 借走，函数 append，跑完留给调用方
    cfg: LlmTurnConfig {
        system_prompt: String,
        tool_defs: Vec<ToolDefinition>,
        allowed_tools: Vec<String>,
        max_iterations: usize,
        settings: AppSettings,        // 含 primary_model
        sub_conv_id: String,           // 用于 gateway.cancel_conversation
    },
    deps: LlmTurnDeps {
        gateway: &LlmGateway,
        tool_round_driver: ToolRoundDriver,
        cancel: CancellationToken,
        event_bus: &RuntimeEventBus,
        on_message_appended: Option<Box<dyn Fn(&ChatMessage) + Send + Sync>>,
            // 每追加一条 message 触发；subagent 传 None；teammate 传一个 closure 实时写 transcript JSONL
    },
) -> Result<LlmTurnOutcome { iterations_used, stop_reason, cancelled, terminal_tool_results }, ...>
```

**实现来源**：把 `worker_runtime.rs:287-617` 的 `'agent_loop: for iteration in 0..max_iterations` 整段搬过来，去掉 SubagentWorkerRuntime 专属的事件 emit / 转录最终汇总（那些留在调用方）。

**关键性质**：
- 无状态（不持有 messages）→ 调用方爱怎么管 messages 就怎么管
- cancel 在 deps 里 → 调用方可以构造 child token / 触发外部 cancel
- 不写 transcript → 调用方决定持久化策略（subagent 一次性整写、teammate 实时 append）

### 4.2 SubagentWorkerRuntime 改调共享函数

**文件**：`src-tauri/src/runtime/agent/worker_runtime.rs`

**改动**：
- `run_worker_turn` 287-617 行那段循环替换成调 `run_llm_iterations`
- messages 是函数局部变量
- 跑完 outcome 拿来生成 `SubAgentResult`，照旧 `store_transcript` 一次性整写
- 行为完全不变 —— 现有的 subagent 测试应该全绿

### 4.3 Teammate idle loop 接 LLM

**文件**：`src-tauri/src/runtime/agent/worker_runtime.rs`

**改动**：

`run_teammate_idle` 启动时构造一次 cfg + deps，进入 select! loop，inbox 收到 ChatMessage 时：

1. 把 StructuredMessage 渲染成一条 user `ChatMessage`，append 到 idle loop 持有的 `messages: Vec<ChatMessage>`
2. **实时 append 到 transcript JSONL**（新 schema 含 tool_*）
3. 调 `run_llm_iterations(&mut messages, &cfg, &deps)`
4. 共享函数内部每完成一次 iteration（push assistant / tool_result）→ 把这一段也 append 到 transcript JSONL
5. 跑完回 select!

注意：第 4 步通过共享函数的 `on_message_appended` 回调实现 —— subagent 传 None（保持一次性整写），teammate 传一个 closure 调 `append_line` 实时写 JSONL。回调签名定死，避免共享函数被特化。

**删除**：`teammate_stub_turn` 函数整段删（1242-1292 行）。

### 4.4 Transcript schema 升级

**文件**：`src-tauri/src/runtime/agent/output_writer.rs`

**改动**：`TranscriptLine` 加三个 `Option` 字段：

```
pub struct TranscriptLine {
    pub role: String,         // user / assistant / tool
    pub content: String,
    pub tool_calls: Option<Vec<TranscriptToolCall>>,   // 新增（assistant 用）
    pub tool_call_id: Option<String>,                  // 新增（tool 结果用）
    pub tool_name: Option<String>,                     // 新增（tool 结果用）
    pub error: Option<String>,                         // 现有
}

pub struct TranscriptToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}
```

**向后兼容**：所有新字段都是 `#[serde(default, skip_serializing_if = "Option::is_none")]`，老 JSONL（只有 role/content）依然能解析。

### 4.5 cancel / shutdown 复用现有机制

**不动**：
- `run_teammate_idle` 的 select! 三路（cancel / inbox / heartbeat）已经处理 cancel
- 共享函数内部跟 SubagentWorkerRuntime 现有逻辑一样 ——`child_cancel.is_cancelled()` 检查 + `gateway.cancel_conversation` + 注入 synthetic tool_results
- shutdown handshake 走现有 StructuredMessage 路径（P2.6 已实现），LLM 看到 shutdown_request XML 自己用 SendMessage shutdown_response 回应

---

## 5. 控制流：完整 round-trip 示例

以今天的会话为例（Lead 派活给小研调研 2026 大模型）：

```
[t=0] 用户: "调研 2026 大模型"
      Lead LLM: 调 spawn_subagent(name=小研, prompt="调研...")
         → 创建 worker_ctx + meta.json
         → tokio::spawn(run_teammate_idle)
         → inbox.send(ChatMessage("调研..."))  ← initial_prompt

[t=1] Teammate idle loop:
      - select! 选中 inbox
      - messages.push(user("调研..."))
      - transcript JSONL append: {"role":"user","content":"调研..."}
      - run_llm_iterations(&mut messages, &cfg, &deps):
          iter 0: LLM stream → tool_call: WebSearch("GPT-5 latest")
                  messages.push(assistant + tool_use)
                  transcript JSONL append: {"role":"assistant","content":"我先查...","tool_calls":[{"id":"tu_abc","name":"WebSearch",...}]}
                  ToolRoundDriver.execute(WebSearch) → "结果..."
                  messages.push(tool_result)
                  transcript JSONL append: {"role":"tool","content":"结果...","tool_call_id":"tu_abc","tool_name":"WebSearch"}
          iter 1-4: WebSearch ×N（DeepSeek / Gemini / 文心 ...）
          iter 5: LLM → tool_call: SendMessage(to="team-lead", message="调研完成，详见...")
                  ToolRoundDriver.execute(SendMessage) → 写 Lead inbox + kick Lead
          iter 6: LLM → 无 tool_call (EndTurn)
                  break
      - 回 select!，messages 留着（含 13 条消息）

[t=2] Lead:
      - 被 P2.4 路径 C kick 唤醒
      - run_chat_turn 继续：Lead inbox 里小研发的消息会被 P2.4 拼成 user message
      - LLM: "收到，我把小算召唤过来分析..."

[t=3] 用户追问: "顺便核实开源协议"
      Lead LLM: SendMessage(to="小研", message="...")
         → 小研 inbox 收到新 ChatMessage

[t=4] Teammate idle loop:
      - select! 选中 inbox（messages 还活着，含 t=1 的 13 条 history）
      - messages.push(user("顺便核实..."))
      - run_llm_iterations → 模型记得之前调研过什么，直接补查开源协议
      ...
```

---

## 6. 未来扩展路径

下面这三个 MVP 都不做，但当前架构必须不堵死它们：

### 6.1 重启续命

**目标**：teammate 进程死了 / app 重启 / session 重启 → 从 transcript JSONL 恢复 messages 继续聊。

**触发点**：spawn_subagent 时检查 `conversations/{conv_id}/teammates/{agent_id}.jsonl` 是否存在 → 存在则按 JSONL 顺序反序列化为 `Vec<ChatMessage>`，作为 idle loop 初始 messages。

**前提**：transcript schema 必须含 tool_call_id（MVP 铁线 4.4 已守住）。

**改动量**：teammate idle loop 入口加一个 `try_restore_from_transcript` 分支，共享函数完全不变。

### 6.2 中途打断（弱打断）

**目标**：teammate 正在跑 5 个 iteration，inbox 来新消息时下一次 iteration 开始前看一眼，把新消息一起 attach 到下一轮 user message。

**改动量**：共享函数加可选 `on_iter_boundary` 回调，teammate 在回调里 `try_recv` inbox。subagent 不传保持现状。

### 6.3 History compact

**目标**：长跑 teammate 累积 messages 超过 token 上限 → 自动调 compact_summary 把老 history 替换成摘要。

**改动量**：共享函数 iter 开头加 token 估算 + 触发 compact，复用现有 CompactSummaryClient。

---

## 7. 测试策略

### 7.1 单元测试

- `run_llm_iterations` 基础测试：mock gateway + mock tool round，验证 messages 累积 / EndTurn 退出 / max_iterations 兜底 / cancel 立即返回
- subagent 现有 25+ 单测全绿（行为不变）
- teammate idle loop 新增测试：mock LLM 模拟工具调用 → 验证 messages 跨 turn 累积 / transcript JSONL 含 tool_call_id / EndTurn 后回 select!

### 7.2 集成测试

新增 `src-tauri/tests/teammate_llm_turn_integration_test.rs`：

- T1: spawn teammate → inbox 发一条消息 → 验证 transcript JSONL ≥ 4 行（user / assistant + tool_use / tool_result / assistant 收尾）
- T2: 跨 turn history 累积 —— 发两条 inbox 消息，验证第二轮 LLM 调用看到第一轮的 messages
- T3: cancel 中断 —— LLM stream 跑到一半 cancel，验证 transcript 含 synthetic tool_result（不留悬空 tool_use）
- T4: shutdown_request handshake（复用 P2.6 现有测试 + 真 LLM mock）
- T5: SubagentWorkerRuntime 行为回归 —— 现有 `subagent_*_test` 全部不退化

### 7.3 端到端冒烟（接住 P2.11）

`docs/test-intents/spec/tasks/ltr-e2e/test-progress.md` 三场景手测：

- 场景 A：单 Teammate 调研 → 看 transcript 产生真消息 + Lead 收到 SendMessage
- 场景 B：多 Teammate swarm → 看 Teammate↔Teammate SendMessage
- 场景 C：plan_approval + shutdown handshake

---

## 8. 风险与缓解

| # | 风险 | 缓解 |
|---|---|---|
| R1 | 抽共享函数动到 subagent，搞坏现有行为 | subagent 现有 25+ 测试全绿是硬门槛；共享函数签名设计为零行为变化（只是搬位置） |
| R2 | TranscriptLine schema 改了破坏老 JSONL 解析 | 新字段全是 `Option` + `#[serde(default)]`，老文件依然可读 |
| R3 | Teammate 长跑导致 messages 无限增长 → context overflow | MVP 用 `max_iterations` 上限兜底；context 真的超了由 gateway 抛错，记 diagnostic；compact 留 P3 |
| R4 | inbox 排队等当前 turn 跑完，Lead 紧急消息延迟可达数十秒 | Spec 明确这是 MVP 行为；未来加"iteration 边界 try_recv"可改善 |
| R5 | 共享函数把 SubagentWorkerRuntime 内部的 event emit / file collection 逻辑切掉，subagent 行为变了 | 那些副作用留在 SubagentWorkerRuntime 调 `run_llm_iterations` 后的包装层；共享函数只负责 messages + outcome |

---

## 9. 验收标准

- [ ] `cd src-tauri && cargo test --tests --no-fail-fast` 全绿（含 review_ 系列）
- [ ] 新增 `teammate_llm_turn_integration_test.rs` T1-T5 全 PASS
- [ ] subagent 现有 25+ 测试零退化
- [ ] 手测：在 ltr-mvp 分支 `pnpm tauri:dev`，新建会话 → 让 Lead 用 spawn_subagent 派活给小研调研 → 看到 teammate transcript JSONL 包含真 LLM 回复 + SendMessage 调用 + Lead 收到回报
- [ ] P2.11 三场景端到端冒烟（`test-progress.md`）全部 PASS
- [ ] ltr-mvp → main 可以 merge（PR）

---

## 10. 文件改动清单

| 文件 | 改动类型 | 说明 |
|---|---|---|
| `src-tauri/src/runtime/agent/llm_turn.rs` | 新建 | 共享 `run_llm_iterations` 函数（估计 250-300 行）|
| `src-tauri/src/runtime/agent/mod.rs` | 加 mod 声明 | `pub mod llm_turn;` + re-export |
| `src-tauri/src/runtime/agent/worker_runtime.rs` | 改 SubagentWorkerRuntime::run_worker_turn | 287-617 行循环替换成调 `run_llm_iterations` |
| `src-tauri/src/runtime/agent/worker_runtime.rs` | 改 run_teammate_idle | 加 messages state；inbox 收到消息→append→调共享函数；删 `teammate_stub_turn` |
| `src-tauri/src/runtime/agent/output_writer.rs` | 扩 TranscriptLine schema | 加 `tool_calls / tool_call_id / tool_name` 三个 Option 字段 + helper 函数 |
| `src-tauri/tests/teammate_llm_turn_integration_test.rs` | 新建 | T1-T5 集成测试 |
| `docs/test-intents/spec/tasks/ltr-e2e/test-progress.md` | 新建（接住 P2.11）| 三场景手测记录 |

---

## 11. 完成后

PR 描述要点：
- 修复 LTR plan 漏写的"Teammate 接 LLM"环节
- 抽出共享 `run_llm_iterations`，subagent + teammate 行为对齐
- 升级 transcript schema 含 tool_call_id（为未来 transcript replay 续命铺路）
- 完成 P2.11 端到端冒烟，LTR MVP 可合 main

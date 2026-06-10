# Human Interaction Priority and Permission Group Design

> 状态：设计稿，待用户 review
> 日期：2026-06-09
> 作者：Codex，基于与项目 owner 的头脑风暴
> 关联工件：
> - `docs/superpowers/specs/2026-06-08-im-pending-interaction-routing-design.md`
> - `docs/superpowers/specs/2026-06-09-human-interaction-control-plane-design.md`
> - `docs/superpowers/specs/2026-06-09-im-run-scoped-interaction-output-design.md`
> - `src-tauri/src/connector/im/shared/ask_coordinator.rs`
> - `src-tauri/src/runtime/pending/queue_manager.rs`
> - `src-tauri/src/runtime/interaction/`
> - `src-tauri/src/runtime/store/pending_permission_request_store.rs`
> - `src-tauri/src/transport/tauri_commands/chat.rs`

## 1. 背景

当前 IM 人机交互还停留在“协调器里补分支”的阶段：

```text
IM 消息进来
  -> ask_coordinator 看状态
  -> permission pending?
  -> askUserQuestion pending?
  -> run busy?
  -> 本地 router / LLM judge / pending queue 各自判断
```

这导致解释权分散：

- 同一句话可能既像审批回复，又像新任务。
- AskUserQuestion 注册晚时，用户回复先进入 busy pending queue。
- pending drain 依赖下一条消息触发。
- permissionAsk 和 AskUserQuestion 看似都在 `ask_coordinator`，但 resolve 路径、UI 路径、IM 回流路径不一致。
- LLM judge 有机会口头确认，但程序没有真正 resume 或 resolve。
- `IMAskCoordinator` 当前仍是 `session_id -> PendingAsk` 单槽位；多个 permissionAsk 会互相覆盖或只 resolve 一个 `tool_call_id`。
- 读取大量同目录文件时，如果逐个审批，会出现不可接受的重复弹窗。

本设计把入口改成硬状态机：

```text
收到用户输入
  -> 先查 HumanInteractionRegistry 是否有 live interaction
  -> 有：必须先由 interaction router 处理
  -> 没有：再看 run active/idle
```

核心原则：

1. 挂起交互拥有优先解释权。
2. 同一 run 的同类权限请求进入 permission group，由一次用户决策批量 resolve。

## 2. 目标

1. 建立统一 `HumanInteractionRegistry`，承载 permissionAsk、AskUserQuestion 和未来其他 HITL 交互。
2. 用户输入先路由到 live interaction；只有没有 live interaction 时，才进入 busy pending queue 或开新 turn。
3. LLM judge 不再是默认解释器，只能作为本地规则无法判断时的辅助 parser。
4. LLM judge 输出必须落成结构化动作，由程序执行 resume、resolve、cancel 或 new turn。
5. 支持 late-registration drain：用户消息早于 interaction 注册时，不丢失、不等待下一条消息、不重复执行。
6. pending queue 只代表 active busy，不代表 suspended waiting user。
7. 将 permissionAsk 从单个 pending ask 升级为 run-scoped `PermissionInteractionGroup`。
8. 多个同风险权限请求合并成一张 App/IM 卡片。
9. 用户一次决策可以批量 resolve 当前 group 中被覆盖的 approval。
10. 目录级或 glob 级长期授权必须由程序校验覆盖范围后落盘。
11. App 与所有 IM 渠道共用同一底层路由语义。

## 3. 非目标

- 不让 LLM 直接执行权限落盘或 interaction resolve。
- 不重写权限落盘格式。
- 不在本阶段重写整个 path permission profile。
- 不把不同风险级别的权限请求强行合并。
- 不删除旧 `/approve`、`/answer` 命令兼容能力。
- 不删除现有单个 `approve_permission_request(tool_call_id)` API；它作为兼容入口保留。
- 不重做所有 IM 渠道的 native 卡片交互；文本回复和 App 操作先走统一底层。

## 4. 核心状态

### 4.1 RunActivity

```rust
enum RunActivity {
    Idle,
    Running {
        run_id: RunId,
    },
    SuspendedForHuman {
        run_id: RunId,
        interaction_id: HumanInteractionId,
    },
}
```

语义：

| 状态 | 含义 | 新输入处理 |
| --- | --- | --- |
| `Idle` | 没有 active run，也没有 suspended run | 开新 turn |
| `Running` | 模型、工具或流式输出正在执行 | 进入 run-scoped buffer 或 pending queue |
| `SuspendedForHuman` | run 等待用户回答或审批 | 默认交给 interaction router |

`SuspendedForHuman` 不等于 busy。它不应该让用户输入框变成“任务忙，消息排队”。

### 4.2 HumanInteraction

```rust
struct HumanInteraction {
    id: HumanInteractionId,
    session_id: SessionId,
    run_id: RunId,
    kind: HumanInteractionKind,
    status: HumanInteractionStatus,
    created_at_ms: i64,
    output_binding: OutputBinding,
}

enum HumanInteractionKind {
    AskUserQuestion {
        interaction_id: InteractionId,
        tool_call_id: ToolCallId,
        payload: serde_json::Value,
    },
    PermissionGroup {
        group_id: HumanInteractionId,
    },
}

enum HumanInteractionStatus {
    Collecting,
    AwaitingUser,
    Resolving,
    Resolved,
    Abandoned,
    Cancelled,
    Expired,
}
```

第一阶段允许 adapter 包装现有 `PendingInteractionControlPlane` 和 `PendingPermissionControlPlane`，不要求一次性迁移所有存储。

### 4.3 HumanInteractionRegistry

```rust
struct HumanInteractionRegistry {
    sessions: HashMap<SessionId, SessionInteractionState>,
}

struct SessionInteractionState {
    live: Vec<HumanInteraction>,
    early_buffer: Vec<InboundUserMessage>,
}
```

约束：

- 同一 session 可以有多个历史 interaction，但 live interaction 需要可排序。
- 默认优先处理最新 live interaction。
- AskUserQuestion 和 PermissionGroup 同时 live 时，优先处理最近注册的交互；如果用户输入明确指向另一项，可按 router 结果切换。
- 已 resolved/cancelled/expired 的 interaction 不能再消费输入。

## 5. 输入路由

统一入口：

```rust
async fn route_user_input(message: InboundUserMessage) -> RouteOutcome
```

路由顺序：

1. 查 `HumanInteractionRegistry.live(session_id)`。
2. 如果存在 live interaction，调用 `HumanInteractionRouter::route_reply(message, interaction)`。
3. 如果 router 返回 `Resolved`，执行结构化 resolve，并 resume 原 run。
4. 如果返回 `AbandonedAndNewTurn`，取消或 abandon 原 interaction，再用当前 message 开新 turn。
5. 如果返回 `NeedClarification`，发送澄清，不开新 turn，不进 pending queue。
6. 如果返回 `NotForInteraction`，才继续检查 run state。
7. 没有 live interaction 时：
   - `Running`：进 run-scoped buffer 或 busy pending queue。
   - `Idle`：开新 turn。
   - `SuspendedForHuman` 但 registry 没有 live interaction：记录诊断并提示用户重试，不能静默排队。

```rust
enum RouteOutcome {
    Resolved {
        interaction_id: HumanInteractionId,
        resolution: HumanInteractionResolution,
    },
    AbandonedAndNewTurn {
        abandoned: HumanInteractionId,
        message: InboundUserMessage,
    },
    NeedClarification {
        interaction_id: HumanInteractionId,
        prompt: String,
    },
    QueuedWhileRunning {
        run_id: RunId,
    },
    StartedNewTurn {
        run_id: RunId,
    },
}
```

## 6. Router 规则

### 6.1 本地规则先行

明确表达必须由本地规则直接处理：

| 输入 | 结果 |
| --- | --- |
| `可以`、`好的`、`允许一次` | permission allow once |
| `先拒绝`、`不行` | permission deny |
| `以后这个目录都可以` | permission allow always candidate |
| `取消`、`算了` | cancel current interaction |
| `问我三个问题` | abandon permission, start new turn |
| AskUserQuestion 下的普通多行文本 | submit answer |

这些不应该先交给 LLM judge。

### 6.2 LLM judge 只做兜底

只有本地规则无法判断时，才调用 LLM judge。

LLM judge 输入是当前 interaction 的结构化摘要和用户原文；输出必须是 schema：

```json
{
  "action": "resolve | abandon_new_turn | clarify | not_for_interaction",
  "kind": "permission | ask_user_question",
  "payload": {},
  "reason": "..."
}
```

程序必须校验：

- action 是否允许用于当前 interaction。
- permission scope 是否覆盖原请求。
- AskUserQuestion answers 是否符合问题 schema。
- abandon/new turn 是否会取消当前 run。

LLM 返回普通文本或 JSON 解析失败时，默认 `NeedClarification`，不能口头确认。

## 7. Late-Registration Drain

这是修复“AskUserQuestion 信号来晚”的关键。

场景：

1. run 仍是 `Running`。
2. 用户消息到达。
3. 当前还没有 live interaction。
4. 几十毫秒后 run 发出 `UserInteractionRequired` 或 `PermissionAskRequired`。

旧行为：用户消息可能进入 pending queue，等待下一条消息才 flush，或被当新 turn 重复执行。

新行为：

1. `Running` 状态下收到消息，先进入 run-scoped `early_buffer`。
2. 如果短窗口内注册了 live interaction，立即 drain early_buffer 给该 interaction。
3. 如果 run 完成且没有注册 interaction，再按 busy pending queue / new turn 规则处理 buffer。
4. 一条 message 一旦被 interaction 消费或转成 new turn，必须打上 consumed/dispatch marker，不能二次 drain。

建议窗口：

| 类型 | 时间 |
| --- | --- |
| run-scoped early buffer debounce | 150-300ms |
| hard cap | 800ms |

## 8. PermissionInteractionGroup

### 8.1 模型

```rust
struct PermissionInteractionGroup {
    group_id: HumanInteractionId,
    session_id: SessionId,
    run_id: RunId,
    status: PermissionGroupStatus,
    items: Vec<PermissionGroupItem>,
    created_at_ms: i64,
    collecting_until_ms: i64,
    output_binding: OutputBinding,
}

enum PermissionGroupStatus {
    Collecting,
    AwaitingUser,
    Resolving,
    Resolved,
    Cancelled,
    Expired,
}

struct PermissionGroupItem {
    tool_call_id: ToolCallId,
    tool_name: String,
    action: PermissionAction,
    requested_paths: Vec<PathBuf>,
    requested_command: Option<String>,
    message: String,
    suggestions: Vec<String>,
    risk_key: PermissionRiskKey,
}
```

`risk_key` 用来决定是否可合并。最小组成：

```text
tool_name + action(read/write/exec/other) + destination policy + broadness class
```

同一 group 只合并同风险项：

- `Read /tmp/a` 和 `Read /tmp/b` 可以合并。
- `Read /tmp/a` 和 `Write /tmp/b` 不能合并。
- `Read /tmp/a` 和 `Bash rm -rf /tmp/b` 不能合并。

### 8.2 收集窗口

第一个 permissionAsk 到达时，不立即展示最终卡片，而是进入短暂 collecting：

| 场景 | 建议窗口 |
| --- | --- |
| App 内 | 150-250ms |
| IM 渠道 | 250-500ms |
| hard cap | 800ms |

窗口内同 `session_id + run_id + risk_key` 的 permissionAsk append 到同一 group。

窗口结束后：

1. group 进入 `AwaitingUser`。
2. App/IM 展示或更新一张权限卡片。
3. 立即 drain `early_buffer`，如果已有用户输入，就直接尝试解析并 resolve。

窗口后又有新 permissionAsk：

- 如果 group 仍 `AwaitingUser`，且同 risk_key，可以 append 并更新卡片。
- 如果 group 已 `Resolving`，不能追加，必须新建下一组或由已落盘 scope 自动放行。

## 9. 批量决策语义

### 9.1 仅本次允许

用户点击“仅本次允许”或回复“好的、可以、本次允许”：

- 对当前 group 内全部同风险 item 执行 `Allow { remember: false }`。
- 不落盘长期规则。
- 如果 group 内存在不同 risk_key，说明分组错误，应先拆组，不允许一次全放。

### 9.2 以后允许目录或范围

用户点击“永久允许”或回复“以后这个目录都可以读”：

1. parser 产出候选 scope。
2. 程序 canonicalize。
3. 程序逐条检查 scope 是否覆盖 item 的 requested_paths。
4. 被覆盖 item 批量 `Allow { remember: true, destination }`。
5. 未覆盖 item 保留 pending，并要求重新确认。

长期规则推荐写入 path scope，而不是给每个文件写一条规则：

```text
Read + /private/tmp/aijia-permission-test/**
```

### 9.3 拒绝

用户点击“拒绝”或回复“先拒绝、不允许、不行”：

- 对当前 group 内全部 item 执行 `Deny`。
- 默认不落盘长期 deny，除非用户明确说“以后都不要允许这个目录”。

### 9.4 取消或换话题

用户说“算了”“不要这个了”“问我三个问题”：

- 当前 permission group `Cancel` 或 `Abandoned`。
- 当前消息如果包含新任务意图，则转为新 turn。
- 这条消息必须标记为已消费或已 dispatch，不能再进入普通 pending queue 二次执行。

## 10. App 与 IM 交互

### 10.1 统一输入

所有入口归一成：

```rust
struct InboundUserMessage {
    session_id: SessionId,
    text: String,
    origin: TurnOrigin,
    output_binding: OutputBinding,
    source_message_id: Option<String>,
    received_at_ms: i64,
}
```

要求：

- App 点击按钮和 IM 文本回复都调用同一个 registry/router。
- App “其他”输入和 IM 自然语言回复都走同一套 parser。
- IM 渠道不再各自决定“这是审批还是新任务”。
- 输出目标由 suspended run 的 `output_binding` 决定，不由 session 历史凭证决定。

### 10.2 App 权限组卡片

卡片展示：

```text
需要你确认后才能继续

工具：Read
范围：/private/tmp/aijia-permission-test/
本次请求：3 个文件

示例：
- secret1.txt
- secret2.txt
- secret3.txt

操作：
1. 仅本次允许这些文件
2. 以后允许读取该目录
3. 拒绝
```

如果只有一个文件，仍可显示单文件，但底层仍是 group。

如果多个文件来自多个目录，展示为多个 group section：

```text
Read
- /tmp/a/ 2 个文件
- /tmp/b/ 1 个文件
```

如果风险不同，必须拆成多张或多段独立确认，不合并按钮。

### 10.3 IM 权限组卡片

IM 侧不展示内部 `tool_call_id` 和备用命令。展示 group 摘要：

```text
🔒 我需要你的确认才能继续

工具：Read
范围：/private/tmp/aijia-permission-test/
本次请求：3 个文件

请选择：
1. 仅本次允许
2. 以后允许读取该目录
3. 拒绝

你也可以直接回复自然语言说明范围。
```

## 11. Pending Queue 边界

pending queue 只服务 active busy。

不允许进入 pending queue 的情况：

- session 有 live AskUserQuestion。
- session 有 live PermissionGroup。
- 当前消息已经被 router 判定为 `AbandonedAndNewTurn`。
- 当前消息已经作为 early_buffer 被 live interaction 消费。

允许进入 pending queue 的情况：

- run 正在模型生成、工具执行或输出流式中。
- 没有 live interaction。
- early buffer 窗口结束后确认没有 interaction 注册。

## 12. 与旧 spec 的关系

本设计合并并取代以下两份拆分草稿：

- `2026-06-09-human-interaction-registry-priority-routing-design.md`
- `2026-06-09-permission-group-and-interaction-priority-design.md`

同时修正和补充旧 spec：

- `2026-06-08-im-pending-interaction-routing-design.md` 的 `PendingAskKind::Permission` 不再代表单 approval，而应代表 `PermissionInteractionGroup`。
- `2026-06-09-human-interaction-control-plane-design.md` 的 `HumanInteractionKind::PermissionAsk` 应具体化为 group-capable interaction。
- `2026-06-09-im-run-scoped-interaction-output-design.md` 的 permission 卡片展示规则应改为 group 摘要卡片。

## 13. 验收标准

1. permission pending 时，用户回复“问我三个问题”，当前 permission 被 abandon，该消息开新 turn 且只执行一次。
2. permission pending 时，用户回复“先拒绝”，程序直接 deny 当前 permission/group，不再先问 LLM judge。
3. AskUserQuestion pending 时，用户直接输入三行答案，程序 submit 原 interaction，原 run 继续。
4. AskUserQuestion 注册晚于用户消息时，注册后立即 drain early_buffer，不等待下一条消息。
5. 没有 live interaction、run 正在生成时，用户消息才进入 pending queue。
6. LLM judge 返回普通文本或坏 JSON 时，不会口头确认成功，而是进入 clarification 或安全失败。
7. App 侧“其他”输入和 IM 文本回复对同一个 pending interaction 产生一致 outcome。
8. 同一条消息不会既 resume interaction 又进入 pending queue。
9. 已 cancel/resolved 的 interaction 不会消费后续消息。
10. IM/App 输出目标不因 session 历史凭证串线。
11. 并发 3 个 `Read` 同目录文件时，只展示一个 permission group。
12. 用户回复“好的”后，当前 group 的 3 个 approval 全部 allow once，run 继续。
13. 用户回复“以后 /tmp/aijia-permission-test 下都可以读”后，写入目录级 read scope，并 resolve 被覆盖的 approvals。
14. 用户回复“先拒绝”后，当前 group 全部 deny，App 和 IM 状态一致。
15. 混合 `Read` 和 `Write` 不合并成一个允许按钮。
16. App 点击 group 按钮和 IM 文本回复走同一底层 batch resolver。
17. 普通 App 新 turn 不继承 IM output binding。

## 14. 第一阶段实现边界

第一阶段做：

- 新增 `HumanInteractionRegistry` 或等价模块。
- 改造 `IMAskCoordinator`，不再用 `session_id -> PendingAsk` 单槽作为唯一真相。
- 新增 `HumanInteractionRouter`，本地规则先行，LLM judge 兜底。
- 新增 run-scoped early buffer 和 drain。
- 将 permissionAsk 和 AskUserQuestion 都接入统一 route/resolve。
- 支持同 run、同 risk_key 的 permission group。
- 支持 allow once、deny、cancel、新 turn、目录级 allow always。
- 支持 App/IM 统一路由和 batch resolve。
- 保留旧单 approval API。
- 增加 focused regression tests。

第一阶段不做：

- 完整重写权限存储。
- 所有 IM native callback 的 UI 重构。
- 历史 interaction 恢复。
- 跨 run 的 permission group 合并。
- 多 risk group 的一键全局确认。
- 复杂 glob 编辑 UI。

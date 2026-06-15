# Human Interaction Control Plane Design

> 状态：设计稿，待用户 review
> 日期：2026-06-09
> 作者：Codex，基于与项目 owner 的头脑风暴、日志排查、本地成熟项目对标和公开 HITL 框架调研
> 关联工件：
> - `docs/superpowers/specs/2026-06-03-permission-approval-surface-design.md`
> - `docs/superpowers/specs/2026-06-08-im-pending-interaction-routing-design.md`
> - `docs/superpowers/specs/2026-06-09-im-run-scoped-interaction-output-design.md`
> - `src-tauri/src/runtime/chat/chat_turn_driver.rs`
> - `src-tauri/src/runtime/interaction/`
> - `src-tauri/src/runtime/store/pending_permission_request_store.rs`
> - `src-tauri/src/runtime/pending/queue_manager.rs`
> - `src-tauri/src/connector/im/`

## 1. 背景

近期 permissionAsk、AskUserQuestion、IM pending queue、APP/IM 双端输出连续暴露问题：

1. permissionAsk 挂起时，用户下一条自然语言消息有时被当成普通 pending，有时被当成审批解释，有时又被模型口头确认但程序没有真正 resolve。
2. AskUserQuestion 挂起时，如果用户回复早于 pending interaction 注册，消息可能先进入 pending queue，之后又被 drain 成新 turn，造成重复执行。
3. busy pending drain 生成的新 run 没有重新绑定 IM 输出目标，导致 APP 内有回复，钉钉侧没有最终回复。
4. `reply_manager` 记住 session 级 IM 凭证后，APP 内普通 run 有机会串到 IM 侧。
5. 现有修补集中在钉钉和 `ask_coordinator`，但飞书、企业微信、Telegram、WhatsApp 等渠道共享同类输入/输出语义风险。

这些不是单个中文解析、单个卡片渲染或单个队列条件的问题。根因是 Lotus 缺少一个统一的 human-in-the-loop control plane：run 进入等待用户交互时没有被建模成可恢复的 `SuspendedForHuman`，用户输入没有统一的 interaction routing，输出也没有绑定到 run 的 origin。

## 2. 对标结论

成熟实现的共同点：

- Human interaction 是 run/graph 的控制流事件，不是普通聊天消息。
- 挂起请求必须有稳定 id、原始 tool call/request、run/session 绑定和可恢复状态。
- 用户回复通过结构化 resolution 恢复原 run，而不是靠 UI 文案或卡片文本猜测。
- 输出目标要跟 turn source/run source 绑定，不能只依赖 session 的 last channel。

本地对标：

- `claude-code-main` remote permission 将普通 SDK message 与 control request/response 分离，并用 request id 管理 pending permission。
- `openclaw` 在 outbound target 里显式使用 `turnSourceChannel`、`turnSourceTo`、`turnSourceAccountId`、`turnSourceThreadId` 防止共享 session 的跨渠道串线。

公开框架对标：

- LangGraph 使用 `interrupt()` 暂停并通过 `Command(resume=...)` 恢复同一个 graph state。
- OpenAI Agents SDK 将 tool approval 暴露为 run interruption，应用 approve/reject 后继续同一个 run state。
- AutoGen 和 Microsoft Agent Framework 都把 human input/approval 当控制流，不当作普通 prompt。

## 3. 目标

1. 为 permissionAsk 和 AskUserQuestion 建立统一的底层 `HumanInteraction` 生命周期。
2. run 状态区分 `Idle`、`Running`、`SuspendedForHuman`，避免把等待用户交互误判为 busy。
3. 所有入口的用户消息先经过统一 interaction router，再决定 resume、abandon/new turn 或 pending queue。
4. pending queue 只用于真正 running 的 run；等待用户交互时的下一条消息默认由 interaction 消费。
5. 消息早于 interaction 注册到达时，先进入 run-scoped input buffer；如果 run 随后挂起，buffer 被交互消费，而不是无脑 drain。
6. 每个 run 携带 `turn_origin` 和 `output_binding`，保证输出回正确渠道，APP-only run 不串到 IM。
7. 所有 IM 渠道通过 shared pipeline 获得一致行为；钉钉只是验收样本，不是专属实现。
8. LLM 只负责自然语言到结构化 intent 的解释，权限落盘、scope 校验、resume 执行都由程序完成。

## 4. 非目标

- 不让 LLM 直接写 `permissions.json`。
- 不在本阶段实现恢复任意历史审批请求。
- 不把所有 IM 渠道的 native button/action callback 重做一遍。
- 不重新设计 APP 视觉样式、IM 卡片品牌样式或消息字体。
- 不删除内部 `/approve`、`/answer` 兼容解析能力，但普通用户卡片不再展示内部 id 和备用命令。

## 5. 核心模型

### 5.1 Run 状态

新增或显式化 run activity state：

```rust
enum RunActivityState {
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

| 状态 | 含义 | 用户下一条输入 |
| --- | --- | --- |
| `Idle` | 没有运行中或挂起 run | 开新 turn |
| `Running` | 模型、工具或 resume 后输出正在执行 | 进入 pending queue 或 run-scoped input buffer |
| `SuspendedForHuman` | run 等待 permissionAsk 或 AskUserQuestion | 默认交给 interaction router 消费 |

`SuspendedForHuman` 不等于 busy。它不应该阻塞用户继续输入，但它保留原 run 的可恢复状态。

### 5.2 HumanInteraction

permissionAsk 和 AskUserQuestion 统一抽象为：

```rust
struct HumanInteractionRequest {
    id: HumanInteractionId,
    session_id: SessionId,
    run_id: RunId,
    tool_call_id: ToolCallId,
    kind: HumanInteractionKind,
    payload: HumanInteractionPayload,
    original_request: RuntimeToolCallRequest,
    turn_origin: TurnOrigin,
    output_binding: OutputBinding,
    status: HumanInteractionStatus,
}

enum HumanInteractionKind {
    PermissionAsk,
    AskUserQuestion,
}

enum HumanInteractionStatus {
    Pending,
    Resolved,
    Cancelled,
    Abandoned,
    Expired,
}
```

现有 `InteractionRequest` 和 `PendingPermissionRequest` 可以先通过 adapter 接入，不要求第一步完全合并存储；但对外路由必须走统一接口。

### 5.3 TurnOrigin 与 OutputBinding

`channel_context` 只能继续作为提示词上下文，不能承担路由职责。每个 turn/run 需要独立携带：

```rust
enum TurnOrigin {
    App,
    Im {
        platform: ImPlatform,
        external_conversation_key: String,
        sender_id: Option<String>,
        sender_label: Option<String>,
        account_id: Option<String>,
        thread_id: Option<String>,
    },
}

enum OutputBinding {
    AppOnly,
    Im {
        platform: ImPlatform,
        target: ImReplyTarget,
        allow_streaming_reply: bool,
    },
}
```

要求：

- IM 入站触发的 run 默认输出回原 IM target。
- APP composer 触发的 run 默认 `AppOnly`。
- APP 处理 IM-origin pending interaction 时，原 suspended run 的后续输出仍按原 run 的 output binding；APP 新 turn 不继承 IM 凭证。
- session 级 credentials 只能用于发送已绑定 run 的输出，不能作为 lazy-create IM reply 的授权依据。

## 6. 输入路由

所有 APP/IM 用户输入先归一成：

```rust
struct InboundUserMessage {
    session_id: SessionId,
    text: String,
    attachments: Vec<ChatAttachmentRef>,
    origin: TurnOrigin,
    output_binding: OutputBinding,
    received_at_ms: i64,
    source_message_id: Option<String>,
}
```

统一路由顺序：

1. 查 session 是否有 live `HumanInteractionRequest`。
2. 如果有，调用 `HumanInteractionRouter::route_reply(message, interaction)`。
3. router 返回：
   - `ResolveInteraction(resolution)`：结构化 resolve，resume 原 run。
   - `AbandonAndStartNewTurn(reason, message)`：取消或 abandon 原 interaction，再用当前消息开新 turn。
   - `ClarifyInteraction(message)`：回复澄清，不开新 turn。
   - `NotForInteraction(message)`：只有在确认不是交互回复时，继续按 run 状态处理。
4. 如果没有 live interaction：
   - `Idle`：开新 turn。
   - `Running`：进入 pending queue 或 run-scoped input buffer。
   - `SuspendedForHuman`：不应出现无 interaction 的状态；记录诊断并回退为 `Idle` 或提示用户重试。

## 7. PermissionAsk 路由

permissionAsk 是安全敏感交互。LLM 可以解释自然语言，但程序必须校验和执行。

### 7.1 结构化 intent

自然语言回复解析为：

```rust
enum PermissionReplyIntent {
    AllowOnce,
    AllowAlways {
        scope: Option<String>,
    },
    Deny {
        reason: Option<String>,
    },
    Cancel {
        reason: Option<String>,
    },
    NewTurn {
        reason: String,
    },
    Clarify {
        message: String,
    },
}
```

示例：

| 用户回复 | intent |
| --- | --- |
| `好的，那就允许你访问一次吧` | `AllowOnce` |
| `以后 /tmp 这个目录下的文件都可以读` | `AllowAlways { scope: "/tmp" }` |
| `不行，不要读这个` | `Deny` |
| `算了，看看别的文件` | `NewTurn` 或 `Cancel` 后新 turn |

### 7.2 程序校验

`AllowAlways` 必须经过：

- canonicalize path。
- scope 覆盖当前请求路径。
- action 与 tool 权限类型匹配，例如 read/write 不混用。
- remember destination 合法。
- broad scope 按现有策略决定是否需要二次确认。

只有校验通过，程序才调用 permission control plane resolve，并由原 permission pipeline 落盘。

## 8. AskUserQuestion 路由

AskUserQuestion 是信息收集交互。普通文本默认作为答案消费。

### 8.1 回答构造

用户直接回复时，程序构造：

```json
{
  "answers": {
    "<question-key>": "<answer>"
  },
  "annotations": {
    "rawText": "...",
    "source": "im|app",
    "answerMode": "freeText|option|multiLine"
  }
}
```

规则：

- 单问题：整段文本作为该问题答案。
- 多问题：按行映射；行数不匹配时保留 rawText，并尽量填充前几个问题。
- 选项文本可匹配 label；无法匹配时按 free text 提交。
- “其他”输入与 IM 自然语言回复同义。

### 8.2 换话题

明显换话题时不强行当答案：

- `算了`
- `别问了`
- `问我三个问题`
- `看看别的文件`
- `先聊别的`

这类消息走 `AbandonAndStartNewTurn`，原 run 被 cancel/abandon，当前消息开新 turn。

## 9. 时序与 pending queue

### 9.1 早到消息

当 run 仍处于 `Running`，但用户消息早于 interaction 注册到达：

1. 消息先进入 run-scoped input buffer，并标记 origin/output binding。
2. 如果 run 在短时间内进入 `SuspendedForHuman`，buffer 内消息交给 interaction router。
3. 如果 run 正常完成且没有挂起，buffer 转成 pending batch 开新 turn。
4. 如果 run 出错或取消，buffer 按现有 pending 失败策略处理。

这可以覆盖 AskUserQuestion 信号来晚导致“部分 pending、部分发送”的问题。

### 9.2 busy pending

pending queue 只处理真正 active busy：

- 模型仍在输出。
- 工具仍在执行。
- resume 后的 run 重新占用 busy marker。
- 当前没有 live human interaction 可消费该消息。

pending queue item 必须携带完整 `InboundUserMessage`，不能只保存 text/source。

### 9.3 drain

pending drain 创建新 run 时：

- 使用 batch 中最后一条或合并策略确定 `turn_origin`。
- 使用同一批消息的一致 output binding；跨 origin batch 需要拆分，不能合并到一个 run。
- dispatch 前先注册 run output binding。
- drain run 的普通输出必须回到对应渠道。

## 10. 输出路由

### 10.1 Run-scoped output

RuntimeEventBus 输出必须按 `(session_id, run_id)` 找 output binding。

禁止：

- 仅凭 `session_id` 的历史 IM credentials 懒创建卡片。
- APP-only run 输出到 IM。
- 新 run 改写旧 run 已完成卡片。

允许：

- IM-origin run 输出到 IM。
- APP 处理 IM-origin pending interaction 后，原 run resume 的输出继续回 IM。
- APP 对 IM pending 的操作发送简短状态反馈。

### 10.2 卡片内聚

同一 run 中：

- 前置文本和 permissionAsk/AskUserQuestion 卡片可以合并或追加到同一卡片。
- resume 后的新普通输出应进入该 run 的后续卡片或同 run 可更新区域。

跨 run：

- 不复用旧卡片。
- 不修改其他 run 的文本。

## 11. 多 IM 渠道要求

本设计必须覆盖所有 shared IM 渠道：

- 钉钉
- 飞书
- 企业微信
- 个人微信
- Telegram
- WhatsApp

渠道职责只剩两类：

1. 入站适配：平台消息转 `InboundUserMessage`，填充 `TurnOrigin` 和 `OutputBinding`。
2. 出站适配：根据 `OutputBinding` 将 runtime 输出发回本平台。

渠道不得各自实现 permissionAsk、AskUserQuestion、pending 消费语义。若某渠道暂时没有完整 output sink，也必须在 spec/plan 中标为未覆盖，不能默默走钉钉专属分支。

## 12. APP 与 IM 同步规则

| 场景 | APP 展示 | IM 展示 |
| --- | --- | --- |
| APP composer 新 turn | 展示完整流式回复 | 不发送 |
| IM 入站新 turn | APP 可同步展示会话记录 | IM 发送完整回复 |
| IM-origin permission 在 APP 点同意 | APP 更新 pending 状态 | IM 发送短反馈，原 run 后续按 output binding 回复 |
| IM-origin AskUserQuestion 在 APP 提交 | APP 更新 pending 状态 | IM 发送短反馈，原 run 后续按 output binding 回复 |
| APP 新消息与旧 IM session 同 session | APP 展示 | 不因历史 credentials 发 IM |

## 13. 迁移策略

分阶段落地，避免一次性大改失控：

1. 增加数据模型：`TurnOrigin`、`OutputBinding`、`InboundUserMessage`、`HumanInteractionRequest` adapter。
2. 改 IM shared 入站，将各渠道消息统一包装成 `InboundUserMessage`。
3. 改 run registry/activity state，区分 `Running` 和 `SuspendedForHuman`。
4. 建 unified `HumanInteractionRouter`，先接 permissionAsk 和 AskUserQuestion。
5. 改 pending queue，使 queued item 保存完整 envelope，并支持 early input buffer。
6. 改 output binding 注册，reply manager 按 `(session_id, run_id)` 出站。
7. 移除或降级 `ask_coordinator` 中只针对钉钉/文本的局部补丁。
8. 清理 IM 卡片文案，隐藏内部备用指令。

## 14. 验收矩阵

### 14.1 PermissionAsk

- IM pending 后回复 `允许一次`，resume 原 run，最终回复回原 IM 渠道。
- IM pending 后回复 `以后 /tmp/aijia-permission-test 都可以读`，程序校验 scope、写入 `permissions.json`，resume 后不再重复问同一路径。
- IM pending 后回复 `问我三个问题`，abandon 原 permission，并开新 turn；该消息不再 pending drain 二次执行。
- APP 点同意 IM-origin permission，IM 只收到短反馈和原 run 后续输出，不收到 APP-only 普通回复。
- stale permission id 不能被口头“同意”伪装成已 resolve。

### 14.2 AskUserQuestion

- IM pending 后普通文本直接构造成 answers 并 resume 原 run。
- 多行回复按问题顺序填充，rawText 保留。
- 回复早于 pending 注册时，消息被 early buffer 吸收，不被 drain 成新 turn。
- `算了，聊别的` abandon 原 interaction 并开新 turn。
- APP “其他”输入和 IM 自然语言输入语义一致。

### 14.3 Pending Queue

- 真正 Running 时连续 IM 消息进入 pending queue，run 完成后合并发送。
- SuspendedForHuman 时下一条消息不进普通 pending queue。
- pending drain run 创建前完成 output binding 注册，非钉钉 shared IM 也能收到回复。
- 跨 origin pending batch 不合并，避免 APP/IM 串线。

### 14.4 Output Binding

- IM-origin run 回复原 IM 渠道。
- APP-origin run 不发 IM。
- 同 session 历史 IM credentials 不触发 lazy IM card。
- 新 run 不改写旧 run 卡片。
- 同 run 的前置文本和 interaction 卡片内聚展示。

### 14.5 多渠道覆盖

- shared IM fake connector 单测覆盖 permissionAsk、AskUserQuestion、pending drain、output binding。
- 钉钉真实路径手测覆盖。
- 至少抽一个非钉钉渠道路径验证它使用 shared `InboundUserMessage` 和 shared output binding。
- 如果飞书、企业微信、Telegram、WhatsApp 任一渠道未走 shared pipeline，必须在实现计划中列为适配任务。

## 15. 诊断日志

新增关键日志，方便以后定位而不是猜：

- inbound message envelope：session、origin、source message id。
- run state transition：Running -> SuspendedForHuman -> Running -> Idle。
- interaction registered/resolved/abandoned：kind、run_id、interaction id。
- early buffer consumed/drained：message ids、target interaction 或 new run。
- output binding registered/used/missing：session、run、target。
- pending drain split/merge：batch ids、origin summary。

日志不能打印敏感 token、webhook secret 或完整权限文件内容。

## 16. 风险与约束

- PermissionAsk 的自然语言解析必须 fail closed。LLM 不确定时只能 clarify，不能默认 allow。
- output binding 的跨渠道抽象不能把钉钉卡片能力强行要求到所有渠道；不同渠道可以使用 text/card/markdown 的各自 sink。
- early buffer 需要 TTL，避免 run 永久卡住导致消息也永久不见。
- APP 与 IM 同步规则要清楚，否则用户会误以为 APP 聊天也会广播到 IM。
- 现有未提交局部补丁需要在实现时重新审视，不能盲目叠加。

## 17. 成功标准

这次改造完成后，以下说法必须成立：

1. permissionAsk 和 AskUserQuestion 是同一套 human interaction lifecycle 的两个类型。
2. 等待用户交互时，下一条消息默认被 interaction 消费，而不是进入普通 pending。
3. 只有真正 busy 时才 pending；pending drain 后也能正确回复原渠道。
4. output binding 是 run-scoped，不是 session-scoped。
5. 钉钉、飞书、企业微信、个人微信、Telegram、WhatsApp 不需要各自写审批/提问判断逻辑，只需要接入统一 envelope 和 sink。
6. 如果未来再出问题，可以从 run state、interaction state、output binding 三条日志定位，而不是继续猜 UI 或中文解析。

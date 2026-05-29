# IM 通道硬交互降级（A 档）设计

- **日期**：2026-05-08
- **范围**：仅 A 档——`AskUserQuestion` 工具与 `permission:ask` 闸门在钉钉 IM 通道的死锁问题
- **不在范围**：工具执行状态可见化（P1）、生成文件回传钉钉（P2）、子 Agent 执行摘要（P2）

## 背景

钉钉 IM 通道入口和应用内 chat 共用同一套 `SessionRuntime / QueryEngine`。但出口侧的 `DingtalkReplyManager` 当前只订阅了 `StreamDelta / StreamDone / StreamError` 三种 RuntimeEvent，**没有处理 `PermissionAskRequired` 与 `UserInteractionRequired`**。

后端 `await_permission_resolution` 与 interaction 等待都是无限期 oneshot，没有 deadline。结果：

- LLM 在钉钉群对话里调 `AskUserQuestion`，或触发 `write_file / bash` 这类需要权限询问的工具时，对话**永远死锁**——直到某个 app 客户端进来手动批一下。

App 内对这两类硬交互有 modal/dialog UI 处理：
- `permission:ask` → `PermissionAskDialog`（允许/拒绝 + 记住策略）
- `AskUserQuestion` → `AskUserQuestionDialog`（结构化选项 + multiSelect + Other 自由文本）

IM 通道无 UI，必须降级成纯文字沟通。

## 行为规约

### `AskUserQuestion`

- 把问题文本化发到钉钉群（"我有几个问题想问你：1. ... 2. ..."），LLM 暂停等待。
- 用户每条钉钉新消息进来时，**调主模型做判断**，三档结果之一：
  - `answered`：解析出选中项（含 multiSelect、Other 自由文本）→ resolve 当前 ask，bot 继续这一轮。
  - `abandoned`：用户在干别的（"算了，帮我查天气"）→ 静默 cancel + 关闭这一轮���新消息作为下一轮 user input 起新 turn。
  - `ambiguous`：看不出在不在回答 → 把这条新消息作为结构化 tool_result（`{kind: user_did_not_answer, user_said, guidance}`）resolve 回 LLM，让 LLM 自己决定重问 / 换说法 / 放弃换方向。
- 多条连发：**先到先得**，每条独立判一次。

### `permission:ask`

- 把权限询问文本化发到钉钉（"我打算执行：bash `ls /tmp`，是否允许？"），LLM 暂停。
- 用户回复进来时调主模型判断：
  - `answered` → allow / deny。
  - `abandoned` → 静默 deny + 关闭这一轮；新消息走新 turn。
  - `ambiguous` → 静默 deny，reason 携带用户原话（permission 没法塞结构化 tool_result，LLM 看到拒绝原因后自适应换路径）。

### Deadline

- 每个挂起的 ask 设 **10 分钟** deadline，到点静默关闭：Permission → Deny；UserQuestion → Cancelled。
- **不发任何超时通知**；用户隔天回复就当新一轮自然处理。

### multiSelect / Other

- 由 LLM 判断器解析（不再人为规约语法），多选 + 自由文本自动支持。

### 影响面

- **后端**：新增 `IMAskCoordinator`；改造 `DingtalkReplyManager` 与 `ChannelManager`。
- **前端**：零改动（频道页本来就走通用 chat 渲染）。

## 设计

### §1 模块划分与边界

新增 `src-tauri/src/connector/channel/ask_coordinator.rs`，对外公开 `IMAskCoordinator`。它是 IM 通道里专管"挂起 ask + 等回复 + LLM 判断 + resolve"的状态机。

```
┌─────────────────────┐    PermissionAskRequired      ┌──────────────────────┐
│  RuntimeEventBus    │  /  UserInteractionRequired   │ IMAskCoordinator     │
│  (后端事件)          │ ────────────────────────────► │ (新)                 │
└─────────────────────┘                                │                      │
                                                       │  • pending_asks      │
┌─────────────────────┐    收到钉钉消息                 │    (per session_id)  │
│ ChannelManager      │ ───── try_handle_reply ──────► │  • LLM 判断器        │
│ (manager.rs)        │ ◄──── 已消化 / 未消化 ────────  │  • deadline timer    │
└─────────────────────┘                                │                      │
                                                       │                      │
┌─────────────────────┐  文本化 ask 发到钉钉            │                      │
│ DingtalkReplyManager│ ◄──── 委托文本输出 ──────────  │                      │
└─────────────────────┘                                └──────────────────────┘
                                                              │
                                          ┌───────────────────┼─────────────────┐
                                          ▼                   ▼                 ▼
                              PendingPermissionControlPlane   InteractionControlPlane
                              (resolve permission ask)        (resolve AskUserQuestion)
```

**职责划分**：

| 模块 | 职责 |
|---|---|
| `IMAskCoordinator`（新） | 挂起态状态机、LLM 判断、resolve、deadline |
| `DingtalkReplyManager` | 实现 `AskOutputSink` trait，把"协调器决定要发到钉钉的文本"实际推到 AI Card / 群消息；不再持有 ask 状态 |
| `ChannelManager` | 收到钉钉消息时先调 `coordinator.try_handle_reply()`，未命中再走 `send_chat_request` |
| `RuntimeEventBus` | 协调器多订阅一个 subscriber，关心 `PermissionAskRequired` + `UserInteractionRequired` |
| `ChannelSessionRegistry`（新 trait） | 仅暴露 `is_channel_session(&SessionId) -> bool`；由 `ChannelSessionRouter` 实现，协调器入口用它过滤事件来源 |

**关键约束**：

- 协调器**不依赖** `tauri::*`（CLAUDE.md 硬约���），只依赖 `LlmGateway`、两个 ControlPlane、`AskOutputSink` trait（钉钉实现）、`ChannelSessionRegistry` trait（路由实现）。
- 协调器**单测可独立运行**：mock LlmGateway、mock 输出器、mock registry、构造假事件即可。
- 协调器**单实例 per app**（`lib.rs` 启动期 `app.manage()` 注入）；内部 `Mutex<HashMap<SessionId, PendingAsk>>` 管状态。
- 协调器**只处理 IM session 来源的事件**（硬约束，见下文"§1.1 事件来源过滤"）。这是本设计不影响 app 内普通对话的关键。

#### §1.1 事件来源过滤（硬约束 · 不影响 app 内对话）

`RuntimeEventBus` 是**全局**的——app 内对话和 IM 通道共用同一条事件总线。协调器若无差别处理所有 `PermissionAskRequired` / `UserInteractionRequired`，会把 app 内 `PermissionAskDialog` / `AskUserQuestionDialog` 正在等用户操作的 ask 也挂进自己的状态机，10 分钟 deadline 后**静默 deny / cancel**，前端 dialog 上点"允许"会失效。这是 must-fix。

**规则**：

1. 协调器 `on_event` 入口收到 `PermissionAskRequired` / `UserInteractionRequired` 后，**第一步**调 `registry.is_channel_session(&event.session_id)`。返回 `false` → **完全 no-op**：不建 `PendingAsk`、不起 deadline、不调 `AskOutputSink`、不订阅 resolve、不写日志（仅 trace 级）。
2. `AskOutputSink` 实现侧（`DingtalkReplyManager::deliver_ask_card` / `force_finish_current_card`）保留现有"未 register 的 session 直接忽略"的二次保护（参见 reply_manager.rs 现有 `on_event` 实现），作为 defense-in-depth。
3. `ChannelSessionRouter` 负责实现 `is_channel_session`：内部需要一个 O(1) 查询，建议在 `SessionsState` 旁补一个 `HashSet<String>` 作为 session_id 集合的反向索引，create / remove session 时同步更新。
4. **事件流转向**保持不变——app 内的 `PermissionAskRequired` 仍由 `transport/tauri_event_adapter.rs` 转成前端 `permission:ask` 事件给 dialog；协调器只是"额外"被触发，过滤条件不命中就什么都不做。

**结果**：app 内对话的 permission / interaction 等待路径完全无感知协调器存在；IM 通道的事件按本设计处理。

### §2 状态机定义

每个 `SessionId` 在协调器里独立持有一个 `PendingAsk` 槽位（`Option`，最多一个挂起的 ask）。

#### 数据结构

```rust
struct PendingAsk {
    session_id: SessionId,
    run_id: RunId,
    kind: PendingAskKind,
    deadline_at: Instant,           // 创建时 + 10min
    deadline_handle: AbortHandle,   // tokio::spawn 的超时任务句柄
    primary_model: String,          // 从事件 emit 时一并塞入，避免协调器耦合 settings store
}

enum PendingAskKind {
    Permission {
        tool_call_id: ToolCallId,
        tool_name: String,
        message: String,
        suggestions: Vec<String>,
    },
    UserQuestion {
        tool_call_id: ToolCallId,
        questions: Vec<AskUserQuestionItem>, // 1-4 题，每题 2-4 选项
    },
}
```

**为什么"每 session 最多一个 ask"？**
后端 ask/interaction 是阻塞 turn 的——LLM 在 ask 没 resolve 之前不会发起下一个工具调用，同一时刻 per session 物理上不可能有两个挂起的 ask。简化：用 `Option` 而不是 `Vec`，回复来了直接对当前唯一槽位判断，不用做"哪条回复对应哪个 ask"的匹配。

#### 状态转换

```
                       ┌────────────┐
                       │   Empty    │ ◄────────────┐
                       └─────┬──────┘              │
                             │ event arrives       │
                             │ (Permission/Question)│
                             ▼                     │
              ┌──────────────────────────┐         │
              │ Pending                  │         │
              │  • 文本化发钉钉           │         │
              │  • 起 10min deadline     │         │
              └─────┬────────────────────┘         │
                    │                              │
       ┌────────────┼────────────┬───────────┐    │
       │ reply 进来  │ deadline   │ session   │    │
       │            │ 触发        │ cancel   │    │
       ▼            ▼            ▼           ▼    │
   judge_with_llm  silent      cancel        │    │
       │           deny        ask           │    │
       │            │            │           │    │
   ┌───┴────┐       │            │           │    │
   │        │       │            │           │    │
answered  ambiguous abandoned    │           │    │
   │        │       │            │           │    │
   │        │       └─resolve────┴───────────┴────┤
   │        │              (deny + clear)         │
   │        │                                     │
   │        └─resolve toolResult(             ────┤
   │           "用户没直接答，他说的是 X")           │
   │                                              │
   └─resolve allow / parsed selections ───────────┘
                    (clear)
```

#### 三档 LLM 判断的具体落地

| 判断结果 | Permission ask | UserQuestion ask |
|---|---|---|
| `answered` | resolve `Allow` 或 `Deny`（带 LLM 提取的理由） | resolve interaction 携带 `selections: Vec<Selection>` (含 multiSelect / Other 文本) |
| `abandoned` | resolve `Deny`（reason="user changed topic"），新消息走 `send_chat_request` 起新 turn | resolve interaction with `Cancelled`（reason 同上），新消息走新 turn |
| `ambiguous` | resolve `Deny`（reason 包含用户原话） | resolve interaction with **结构化 toolResult**：`{ "kind": "user_did_not_answer", "user_said": "...", "guidance": "..." }`，让 LLM 看到后自己重问/换路径 |

**关键点**：所有路径上 `PendingAsk` 都会被**清空**——没有"挂着不动"这一档。`ambiguous` 也清，因为已经 resolve（只是 resolve 用的是"非答案"载荷）。LLM 收到这个 toolResult 自己决定要不要再起一个新 ask。

#### Deadline 行为

- 创建 `PendingAsk` 时 `tokio::spawn` 一个 `sleep(10min)` task，存其 `AbortHandle`。
- 如果 reply 先到，调 `handle.abort()` 取消超时任务。
- 如果 deadline 先到，触发同一份 resolve 逻辑：Permission → Deny；UserQuestion → Cancelled。
- 全程**不发任何钉钉消息**。

#### Session cancel 兼容

`turn_cancel` 触发时，runtime 已有的 `await_permission_resolution` / interaction 的 cancel 逻辑会调用 control plane 的 resolve，协调器需要订阅一个清理钩子（或在 resolve 时反向通知协调器清空槽位），避免槽位泄漏。具体实现：协调器在每次 emit 文本到钉钉前用 `control_plane.is_pending(tool_call_id)` 验真，resolve 时也再 check 一次幂等。

### §3 LLM 判断器协议

协调器收到钉钉消息后，对当前挂起的 ask 调一次主模型，让 LLM 判定回复是哪一档。这是一个**短、廉价、单轮、强结构化**的调用。

#### 调用通路

复用 `LlmGateway`（runtime 注入的依赖）：

- **不**走 `SessionRuntime` / `QueryEngine`（避免触发新 turn / 工具系统）。
- **不**写入对话历史（带外判断，不污染原 session 的 message log）。
- 直接构造一次 single-turn `chat_completion` 请求，强制 JSON 输出。

#### 模型选择

**用主模型**（用户当前 session 在用的 primary）。在 `RuntimeChatTurnDriver` emit ask 事件时把 primary_model 一并塞进 `PendingAsk`，协调器不直接耦合 LlmSettings store。

#### Prompt 模板

两个变体共用一个 schema 输出，prompt 里只换上下文。

**Permission 变体**

```
[system]
你是一个分诊器。AI 助手刚向用户请求了一个高风险操作的授权，
现在用户在钉钉里发了一条新消息。判断这条消息属于哪一档：

- "answered": 用户在明确允许或拒绝当前操作
- "abandoned": 用户在转换话题、提出新需求、或闲聊
- "ambiguous": 既不是清楚的允许/拒绝，也不是明显的新话题

只输出 JSON，遵循下面的 schema。

[user]
AI 想做的操作：
{tool_name}: {ask_message}
建议参数（如有）：{suggestions}

用户的回复：
"""
{user_reply}
"""

输出 JSON：
{
  "verdict": "answered" | "abandoned" | "ambiguous",
  "decision": "allow" | "deny",
  "reason": "<<= 一句话解释你的判断>>"
}
```

**UserQuestion 变体**

```
[system]
你是一个分诊器。AI 助手刚通过 AskUserQuestion 工具向用户问了一组问题，
现在用户在钉钉里发了一条新消息。判断这条消息属于哪一档：

- "answered": 用户在回答这些问题（即使表达自然、用别的措辞、或选择"其他"）
- "abandoned": 用户在转换话题、放弃当前问题、或闲聊
- "ambiguous": 既不是清楚的答案，也不是明显的新话题

answered 时，按下面 schema 给出每一题的选择。
- 如果该题是 multiSelect，可以列多个选项（按 label 文本完整复制）
- 如果用户给的是自由文本/"其他"，写到 freeText 字段
- 必填字段全部要给

只输出 JSON。

[user]
AI 提的问题：
{questions_json}

用户的回复：
"""
{user_reply}
"""

输出 JSON：
{
  "verdict": "answered" | "abandoned" | "ambiguous",
  "selections": [
    { "questionIndex": 0, "labels": ["..."], "freeText": null },
    ...
  ],
  "reason": "<<= 一句话解释>>"
}
```

#### 输出解析与降级

- **强制 JSON 模式**：用 LlmGateway 的 `response_format: json_object`（如果主模型不支持 JSON 模式则降级为 prompt 末尾追加"必须只输出 JSON"+ 解析时 strip code fence）。
- **解析失败**（JSON 解析不出 / verdict 字段缺失 / answered 但无 selections）→ 一律按 `ambiguous` 处理，避免误判。
- **schema 校验**：用 serde 反序列化到固定 struct。

#### 健壮性参数

| 项 | 值 |
|---|---|
| max_tokens | 256 |
| temperature | 0 |
| 调用超时 | 30 秒（超时按 ambiguous 处理） |
| 重试 | 不重试（用户已在等了，宁可走 ambiguous 也别再多花时间） |

#### 防滥用边界

- **空消息 / 纯空格**：不调 LLM，直接按 ambiguous 处理。
- **同一 ask 收到 N 条消息**：先到先得，每条独立判一次。第一条如果判出 answered，槽位被清空；后续消息进来发现槽位已空，走正常 `send_chat_request` 起新 turn。

#### 不做的事

- 不缓存判断结果（每条消息都新判）。
- 不做 prompt cache（短 prompt，不值得）。
- 不发 LLM 的 `reason` 给钉钉用户看（全程静默处理）。
- 不在 LLM 调用失败/超时时给用户发提示。

### §4 ChannelManager 接收侧分流

#### 当前路径（改造前）

```
钉钉 WebSocket 收到消息
 → manager.rs 消息处理 task
   → ChannelSessionRouter.get_or_create_session()
   → reply_manager.register(...)
   → chat_adapter.send_chat_request(ChatTurnRequest::new(session_id, content, []))
```

#### 改造后

```
钉钉 WebSocket 收到消息
 → manager.rs 消息处理 task
   → ChannelSessionRouter.get_or_create_session()
   → ask_coordinator.try_handle_reply(session_id, content)
       ├─ NotPending  (无挂起 ask)
       │    → reply_manager.register(...)                  ← 原路径
       │    → chat_adapter.send_chat_request(...)
       │
       ├─ Consumed    (有挂起 ask，answered/ambiguous 已 resolve)
       │    → 不做任何事
       │
       └─ Reroute { content }  (abandoned: 协调器先 deny 了原 ask)
            → reply_manager.register(...)
            → chat_adapter.send_chat_request(...)
```

#### `try_handle_reply` 返回值

```rust
enum HandleOutcome {
    NotPending,
    Consumed,
    Reroute { content: String },
}
```

**为什么要 `Reroute`？**
abandoned 场景下用户消息是一条新的有效 user input，不能丢。协调器负责关掉旧 ask（deny + 清槽），但不能自己去 `send_chat_request`（会形成循环依赖——协调器依赖 chat_adapter，chat_adapter 通过事件依赖协调器）。所以把"起新 turn"还给 manager.rs。

#### 上下文：abandoned 的"新 turn"

`manager.rs` 现有 `send_chat_request` 路径会：
1. 通过 `reply_manager.register()` 先创建一张 AI Card；
2. 然后 `send_chat_request` 传入新构造的 `run_id`。

abandoned 场景：

- 旧 ask 属于**老的 run_id** → 协调器 resolve 时用老 run_id 的 control_plane 句柄，老 run_id 自然 finish。
- 新消息作为**新的 run_id** → 正常 register + send_chat_request，开新 AI Card。

旧 AI Card 在协调器 resolve 的瞬间会收到 `StreamDone`（来自 LLM 对老 run 的完结回复），`DingtalkReplyManager` 正常 finish。用户在钉钉群里看到**两张卡片**：旧的"用户拒绝了执行"之类结束，新的"天气查询中..."开始。这个行为是符合直觉的。

#### 并发与顺序

消息处理在 `connect_dingtalk()` 内 `tokio::spawn` 任务里**单线程循环**消费 mpsc，同 session 天然串行。`try_handle_reply` 内部 `await` LLM 判断（最多 30s）期间，下一条消息会阻塞在 channel 等前一条处理完——可接受。

#### 失败兜底

- 协调器 panic / LlmGateway 不可用 → `try_handle_reply` 返回 `Err`。
- manager.rs 收到 Err 时降级为"NotPending 处理"——按正常 send_chat_request 起新 turn。坏处是旧 ask 被孤立（永远等 deadline），但至少对话不中断、不丢消息。

### §5 `DingtalkReplyManager` 改造与卡片策略

#### 钉钉 AI Card 关键事实

- `register()` 调 `create_and_deliver_card()` 创建一张卡（拿到 `card_instance_id`）。
- LLM streaming 期间，`StreamDelta` 持续 PUT `/v1.0/card/streaming`，**累加文本流式刷同一张卡**。
- `StreamDone` PUT `flowStatus=3` 把卡片**封口**，之后再 PUT 会被钉钉 reject。
- 卡片一旦封口必须**新建一张卡**才能发新东西。

#### 三种发 ask 文本的策略

| 策略 | 行为 | 视觉效果 |
|---|---|---|
| A. 拼到当前卡片 | ask 文本作为附加 StreamDelta 拼到当前 LLM 输出尾部 | 卡片底部多一段 ask 文本；用户回复后 LLM 新内容继续拼，整张卡变得无限长且无视觉结构 |
| B. 当前卡片封口 + 另起新卡片 | finish 当前卡，`create_and_deliver_card` 一张新卡装 ask 文本 | 群里出现两张卡：一张 LLM 此前输出，一张单独 ask 询问 |
| C. 当前卡片封口 + 发普通群消息 | finish 当前卡，ask 用普通 markdown 消息（非 AI Card）发出 | 卡片和普通气泡混着，用户搞不清"思考"还是"询问" |

**采用 B**：每张卡承载一个语义单元——"LLM 一段输出"、"一个 ask 询问"、"用户回复后的 LLM 新输出"各占一张。卡片之间天然有视觉分隔。

#### `DingtalkReplyManager` 新增方法

```rust
impl DingtalkReplyManager {
    /// 协调器调用：当前 run 的卡封口（如果还开着），新建一张卡装 ask 文本
    pub async fn deliver_ask_card(
        &self,
        session_id: &SessionId,
        ask_kind: &PendingAskKind,
    ) -> Result<()>;

    /// 协调器调用：当前 run 因 abandoned/timeout 被强制中止时，
    /// 把当前卡封口（如果还开着），不新建新卡
    pub async fn force_finish_current_card(
        &self,
        session_id: &SessionId,
        reason_for_log: &str,
    ) -> Result<()>;
}
```

`deliver_ask_card` 内部：

1. 拿 `ReplyContext`，如果有 `card_instance_id` 且没 finish 过 → 调 `finish_card(flowStatus=3, accumulated_text)`。
2. 调 `create_and_deliver_card(...)` 新建一张，文本是协调器格式化好的 ask 内容。
3. 这张 ask 卡**直接 finish**（flowStatus=3，一次性，不进入 streaming），ask 文本是确定的不需要流式刷。
4. 不更新 ReplyContext 的 `card_instance_id`——因为这张卡已 finish，未来 LLM 的 StreamDelta 应该新开一张卡（见下）。

#### 用户回复后的卡片：按需开卡

用户在钉钉回复 → 协调器判 answered → resolve 原 ask → 老 run 的 LLM 收到 toolResult 继续 streaming。这时 `DingtalkReplyManager` 收到 StreamDelta，但当前 ReplyContext 的卡片已被 `deliver_ask_card` finish 掉。

需要在 `on_event` 处理 StreamDelta 时加判断：**如果当前 ReplyContext 没有可用的 streaming 卡（card_instance_id 为空或已 finish），自动 create + deliver 一张新卡再开始 stream**。这是个新增的"按需开卡"逻辑——以后任何让卡片中途封口的需求都能复用。

为做到幂等且无锁竞争，`ReplyContext` 增加一个明确的 `card_lifecycle: CardLifecycle` 字段（`None | Streaming { instance_id } | Finishing | Finished`），代替靠 Option 判空。

#### Ask 文本格式

协调器把 PendingAskKind 格式化为 markdown。两个变体：

**Permission**

```
🔒 我需要你的确认才能继续

打算执行：**bash**
> 命令：`ls -la /tmp`

是否允许？请直接回复（如"可以"/"不要"，自然语言即可）。
```

**UserQuestion** (1 题示例)

```
❓ 我有个问题想问你

**1. 你想用哪个数据源？**
- A. 销售明细表
- B. 月度汇总表
- C. 实时 OLAP 视图

请直接回复你的选择（自然语言即可，例如"用 B"或"销售明细表"）。
```

multi 题接着 `**2. ...**`，multiSelect 在题目下加注 `（可多选）`，"Other" 选项一律不在文本里展示——LLM 判断器看到自由文本会自己塞进 `freeText` 字段。

#### `force_finish_current_card` 用途

abandoned / deadline / session cancel 时，协调器要把当前老 run 的卡片封口，但**不**创建 ask 卡（这一轮已经废了，没必要再发文字给用户看）。等用户的新消息走 `Reroute` → 正常 register + 创建新 run 的卡片。

```
   abandoned 场景:
   旧卡(LLM输出片段) ─── force_finish ──┐
                                       ▼
   新消息进 manager.rs → register → 新卡(LLM 处理新需求)
```

#### 不做的事

- 不重发 ask 卡（如果钉钉 API 失败）：失败就当作 ask 没发出去，协调器 PendingAsk 继续挂着，靠 deadline 兜底关闭。
- 不在 ask 卡里加按钮（钉钉 AI Card 按钮需要回调 webhook，工程量大）。
- 不展示用户原始回复回声（钉钉群里他自己已经能看到）。

### §6 测试策略

#### 单元测试（`ask_coordinator.rs`）

依赖三个抽象，全部 mock：

| 依赖 | Mock 实现 |
|---|---|
| `LlmGateway` | `MockLlmJudge`：预设 (input → verdict JSON) 表，可注入 panic / timeout / 非法 JSON |
| 文本输出器（`AskOutputSink` trait） | `RecordingSink`：把 `deliver_ask_card` / `force_finish_current_card` 调用记到 `Vec<SinkCall>` |
| 两个 ControlPlane | 用真的 `PendingPermissionControlPlane` / `InteractionControlPlane`（in-memory + tokio::oneshot） |

需要覆盖的意图：

1. **基础挂起**：`PermissionAskRequired` event → 协调器调 sink 发卡 + 槽位为 Pending。
2. **基础挂起 (UserQuestion)**：`UserInteractionRequired` event → 同上。
3. **Answered (Permission allow/deny)**：reply → judge 返回 answered + allow/deny → control_plane 收到 resolution + 槽位清空 + `Consumed`。
4. **Answered (UserQuestion 单选)**：reply → judge answered + 单选 → control_plane 收到 selections + 清空 + Consumed。
5. **Answered (UserQuestion multiSelect + Other freeText)**。
6. **Abandoned**：reply → judge abandoned → permission/interaction 都 resolve 成 deny/cancelled + `Reroute { content }` + 清空。
7. **Ambiguous (Permission)**：reply → judge ambiguous → resolve deny + reason 含用户原话 + Consumed + 清空。
8. **Ambiguous (UserQuestion)**：reply → judge ambiguous → resolve interaction with `kind: user_did_not_answer` 结构化载荷 + Consumed + 清空。
9. **Deadline 触发**：注入虚拟时钟 (tokio::test 的 `pause()`)，advance 10min → 槽位清空 + 对应 resolution + **未调用 sink**。
10. **Reply 在 deadline 之前到**：advance 9:59 → reply → 判定走完 → deadline 任务被 abort，advance 11min 没有副作用。
11. **NotPending 路径**：从未 emit 过 ask，try_handle_reply → `NotPending`。
12. **协调器自身 panic / Err 兜底**：mock LLM 抛错 → try_handle_reply 返回 Err；槽位继续挂着等 deadline。
13. **空消息 / 纯空格**：try_handle_reply 收到 `""` → 不调 LLM，按 ambiguous 路径。
14. **JSON 解析失败**：mock LLM 返回非法 JSON → ambiguous 兜底。
15. **多条连发顺序处理**：连续两条 reply，第一条 answered（清空槽位），第二条进来时槽位为空 → 第二条返回 NotPending。
16. **Session cancel 兼容**：协调器观测到 control_plane 已被外部 resolve → 后续 reply 进来发现槽位但 control_plane 不再 pending → 清空槽位 + 返回 NotPending（幂等）。
17. **非 IM session 事件 no-op（防回归 · 关键）**：mock `ChannelSessionRegistry` 对任意 `session_id` 返回 `false`（模拟 app 内对话）→ emit `PermissionAskRequired` 与 `UserInteractionRequired` 各一条 → 断言：协调器内部槽位仍为空、未起 deadline 任务、`AskOutputSink` 零调用、两个 ControlPlane 都没有被 resolve；advance 11min 后协调器仍无副作用（验证 deadline 没被注册）。
18. **同一 app 中 IM 与非 IM 事件混合**：registry 对 IM session_id 返回 true、对 app session_id 返回 false → 同时 emit 两条事件 → 断言只有 IM session 走完整流程（建槽位、发卡、起 deadline），app session 完全无感。

#### 集成测试（`src-tauri/tests/`）

新增 `im_ask_coordinator_integration_test.rs`：

- **场景 A：完整 happy path**——构造真的 `SessionRuntime` + fake LLM gateway（按脚本返回 tool_call AskUserQuestion → 收到 toolResult 后返回纯文本），通过协调器接收"用户回复"，断言 LLM 收到正确的 selections + 最终 finish。
- **场景 B：abandoned 起新 turn**——同上，但用户回复跑题，断言旧 ask 被 deny + 新 turn 用新消息正常起来。
- **场景 C：deadline 关闭**——挂起后 advance 10min，断言旧 turn 被关，下条用户消息正常起新 turn。

#### 架构约束测试 (`review_*`)

新增 `src-tauri/tests/review_im_ask_coordinator.rs`，验证：

- 协调器模块**不 use tauri::***（grep 字符串断言）。
- 协调器**只通过 trait** 访问钉钉输出（不直接调 `dingtalk_card::*`）。

#### 前端测试

前端零改动，无需新增。但需确认 `useStreaming.integration.test.tsx` 不被破坏（频道页消息渲染不变）。

### §7 落地步骤梗概

#### 改动面总览

| 位置 | 改动类型 | 要点 |
|---|---|---|
| `src-tauri/src/connector/channel/ask_coordinator.rs` | **新增** | 状态机 + LLM 判断器 + deadline + `AskOutputSink` trait + `ChannelSessionRegistry` trait + 事件入口处的 `is_channel_session` 过滤 |
| `src-tauri/src/connector/channel/router.rs` | **修改** | `ChannelSessionRouter` 实现 `ChannelSessionRegistry`：补 `HashSet<String>` 反向索引 + `is_channel_session(&SessionId) -> bool` O(1) 查询；create / remove session 时同步索引 |
| `src-tauri/src/connector/channel/reply_manager.rs` | **修改** | 实现 `AskOutputSink`；`on_event` 处理 StreamDelta 时增加"按需开卡"逻辑 |
| `src-tauri/src/connector/channel/manager.rs` | **修改** | 消息处理 task 接入 `coordinator.try_handle_reply` 分流；`connect_dingtalk` 时把协调器订阅挂到 event bus |
| `src-tauri/src/connector/channel/mod.rs` | **修改** | 新模块 `pub use` |
| `src-tauri/src/lib.rs` | **修改** | 启动期构造 `IMAskCoordinator`，`app.manage()` 注入；把 `ChannelSessionRouter` 作为 `ChannelSessionRegistry` 注入协调器；ChannelManager 构造时把协调器句柄传进去 |
| `src-tauri/tests/im_ask_coordinator_integration_test.rs` | **新增** | §6 场景 A/B/C |
| `src-tauri/tests/review_im_ask_coordinator.rs` | **新增** | 约束 review 测试 |
| 前端 | **零改动** | 频道页本来就走通用 chat 渲染 |

#### 建议的提交顺序

每步独立可编译 + 测试通过：

1. **Step 1 — 协调器骨架 + `AskOutputSink` / `ChannelSessionRegistry` trait**：新增 `ask_coordinator.rs`，定义数据结构、两个 trait、空方法；`on_event` 入口先调 `registry.is_channel_session()` 过滤。同步给 `ChannelSessionRouter` 加 `is_channel_session` 实现 + 反向索引。`cargo check` 通过 + 单测 #17-18 通过。
2. **Step 2 — 协调器挂起 + deadline 状态机**：实现 register + deadline 定时器。写单测 #1-2、#9-10。
3. **Step 3 — LLM 判断器**：实现 `judge_reply`（调 LlmGateway、JSON 解析、失败兜底）。写单测 #12-14。
4. **Step 4 — `try_handle_reply` 整合判断器 + resolve 三档**：写单测 #3-8、#11、#13、#15-16。
5. **Step 5 — `DingtalkReplyManager` 改造**：实现 `AskOutputSink`；`on_event` StreamDelta 路径加"按需开卡"；新增 `force_finish_current_card`。
6. **Step 6 — `manager.rs` 接线**：消息处理 task 接入分流；`connect_dingtalk` 把协调器 subscriber 挂上。
7. **Step 7 — `lib.rs` 注入 + 集成测试**：协调器的 app.manage()；写 §6 场景 A/B/C 集成测试；写 review 架构测试。
8. **Step 8 — 手动冒烟**：在自己钉钉里跑一遍：
   - 触发 AskUserQuestion → 群里回复选项 → 看到正常 resolve
   - 触发 AskUserQuestion → 群里跑题 → 看到旧 turn 关 + 新 turn 起
   - 触发 AskUserQuestion → 群里模糊回复 → 看 LLM 是否会继续重问
   - 触发 write_file → 群里回复"可以" → 看到文件写入成功
   - 触发 write_file → 群里不回 → 10min 后自动 deny（看后端日志确认）
   - **app 内对话回归（必做）**：app 内触发 `write_file` permission ask → 不点允许放置 11min → 回来点"允许"，**仍然成功执行**（验证协调器没误触 app session 的 control plane）；同样跑一遍 `AskUserQuestion`。

#### 回滚策略

每一步独立提交，单步可 revert。最早可独立上线的是 Step 5 结束（协调器已完整，但 manager.rs 没接入，相当于 dark launch）。Step 6 才真正接入流量。

#### 风险点

1. **`ReplyContext` 并发**：协调器和 reply_manager 都要访问。设计 trait 方法时让锁持有期最短，不要在持锁时调 HTTP API。
2. **按需开卡的幂等性**：判断"当前没活卡"的条件要严谨——同时存在"卡正在被其他任务 finish"和"卡已被 finish 未清指针"两种过渡态，需要在 ReplyContext 增加明确的 `card_lifecycle` 状态字段。
3. **LLM 判断器�� token 成本**：每条 IM 消息都是一次主模型 call。后续可加监控看平均每 session 多出多少 token，必要时再切到小模型（不在本期范围）。

## 不在范围（明确排除）

- 工具执行状态（`tool:executing`/`tool:completed`）的文字滚动 → P1。
- 生成文件回传钉钉 → P2。
- 子 Agent 执行摘要 → P2。
- 前端频道页的任何额外 UI → 不需要。
- 钉钉以外的 IM 平台（飞书 / 微信 / 企微）→ 当前是占位，本期不动。

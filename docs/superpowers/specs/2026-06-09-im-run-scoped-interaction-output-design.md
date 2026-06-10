# IM Run-Scoped Interaction Output Design

> 状态：设计稿，待用户 review
> 日期：2026-06-09
> 作者：Codex，基于与项目 owner 的头脑风暴
> 关联工件：
> - `docs/superpowers/specs/2026-06-08-im-pending-interaction-routing-design.md`
> - `docs/superpowers/specs/2026-06-03-permission-approval-surface-design.md`
> - `src-tauri/src/connector/im/shared/ask_coordinator.rs`
> - `src-tauri/src/connector/im/shared/reply_manager.rs`
> - `src-tauri/src/connector/im/manager.rs`
> - `src-tauri/src/runtime/pending/`

## 1. 背景

IM pending interaction 近期已经从“审批期间一律拦截消息”调整为更自然的模型判断和 run resume 语义，但实际联调又暴露出一组相关问题：

1. 用户在权限审批 pending 时发送新任务，例如“问我三个问题”，模型可以跳出审批并执行新 turn；但这条消息仍可能留在 pending queue，导致当前回复完成后被二次执行。
2. 新 turn 触发 `AskUserQuestion` 时，前置文本和问题卡片被拆成两张 IM 卡片，或者后续输出改写了前一个 run 的卡片内容。
3. 权限卡片和 AskUserQuestion 卡片展示 `/approve call_00_xxx ...`、`/answer interaction_xxx ...` 这类内部备用指令。普通用户无法理解这些 id，视觉上也像乱码。
4. APP 内继续发送普通消息时，因为 IM reply manager 记住了 session 凭证，同 session 的 AI 回复也可能被懒创建卡片同步到 RM/IM 频道。原始产品意图不是“APP 对话直播到 IM”，而是只在 APP 处理 IM 留下的交互时给 IM 一个简短状态反馈。
5. 用户后续说“刚刚那个权限我想好了，同意”时，模型可能口头确认，但如果原 permission request 已经被新 turn abandon，程序层并没有真正 resolve 原审批，随后仍会再次询问权限。

这些现象不能分散修补。它们共同说明 IM 侧缺少一条清晰原则：run 来源、输出卡片、pending queue、pending interaction 生命周期必须有明确边界。

## 2. 目标

1. 隐藏 IM 用户不可理解的内部备用指令。
2. 同一个 run 内，普通文本输出和随后触发的 pending interaction 卡片应内聚展示。
3. 跨 run 不复用、不改写旧卡片。
4. 已经作为新 turn 执行的 IM 消息不能再被 pending queue flush 二次投递。
5. APP 触发的普通 run 不自动同步 AI 回复到 RM/IM。
6. APP 处理 RM/IM 留下的 permission 或 AskUserQuestion 时，只向 RM/IM 发送简短状态反馈。
7. 被 abandon 的权限请求不能被模型口头“同意”伪装成已 resolve。

## 3. 非目标

- 不删除内部显式命令解析能力。`/approve <id> ...` 和 `/answer <id> ...` 可以保留为日志、开发调试或兼容入口，但不展示给普通 IM 用户。
- 不实现“恢复任意历史权限审批”。本设计优先避免误导；历史审批恢复需要单独设计最近审批缓存和安全约束。
- 不重做所有 IM channel 的 native button/action callback。
- 不改 APP 里 pending action surface 的视觉细节。
- 不让 LLM 直接写 `permissions.json`。LLM 只能产出结构化意图，仍由 runtime permission control plane 校验和落盘。

## 4. 与旧 spec 的关系

本设计是 `2026-06-03-permission-approval-surface-design.md` 和 `2026-06-08-im-pending-interaction-routing-design.md` 的交互展示与输出路由补充。

旧 spec 中“IM fallback commands should be displayed”的要求被本设计收窄为：

- 内部命令解析能力保留。
- 普通 IM 用户卡片不展示内部命令和内部 id。
- 如果未来需要调试显示，只能在开发模式或日志里展示，不能作为默认用户文案。

## 5. 核心原则

一句话原则：

```text
谁触发的 run，输出回谁；同一个 run 的文本和交互卡片内聚；跨 run 不改写；已经消费的消息不再排队；用户看不到内部 call id。
```

### 5.1 Run 来源决定输出目的地

每个 run 需要携带输出来源语义：

| run 来源 | AI 普通回复 | pending interaction 状态反馈 |
| --- | --- | --- |
| IM/RM 入站消息触发 | 输出到原 IM/RM 会话 | 输出到原 IM/RM 会话 |
| APP composer 触发 | 只输出到 APP | 不需要发 IM/RM |
| APP 处理 IM/RM 留下的 pending interaction | 后续普通 AI 回复默认仍在 APP 展示；是否继续发 IM/RM 由原 run 来源决定 | 向原 IM/RM 会话发送短反馈 |

这意味着 `reply_manager` 不能仅凭 `session_id` 有历史 IM 凭证就懒创建 IM 卡片。它必须知道当前输出属于哪个 run，以及该 run 是否允许 outbound IM reply。

### 5.2 Pending interaction 是 run 内交互，不是新卡片强制边界

当一个 run 先输出文本，然后触发 permission 或 AskUserQuestion：

- 如果当前 streaming card 属于同一个 run，pending card 应追加或融合到当前 card。
- 如果当前 streaming card 属于其他 run，必须新建或使用当前 run 的 card。
- pending card 不应该覆盖其他 run 已完成的文本。

示例：

```text
用户：问我三个问题
AI card:
好的，我来问你三个问题，更好地了解你的需求。

❓ 我有几个问题想问你

1. ...
2. ...
3. ...
```

反例：

```text
旧 run card: 好的，我来查看文件。
新 run 输出后，旧 run card 被改成：好的，我来问你三个问题。
```

## 6. IM 卡片展示规则

### 6.1 Permission 卡片

普通 IM 用户看到：

```text
🔒 我需要你的确认才能继续

工具：Read
路径：/private/tmp/aijia-permission-test/secret3.txt
当前授权范围：/private/tmp/aijia-permission-test

请选择以下操作之一：

1. 仅本次允许
2. 永久允许
3. 拒绝
4. 取消当前任务

你也可以直接回复自然语言说明授权范围或调整要求。
```

不展示：

```text
/approve call_00_xxx allow
/approve call_00_xxx deny
/approve call_00_xxx cancel
```

### 6.2 AskUserQuestion 卡片

普通 IM 用户看到：

```text
❓ 我有几个问题想问你

1. 专业领域
- HR/人事
- 财务
- 销售

2. 最需要协助
- 数据处理与分析
- 文案整理

你可以按选项回复，也可以直接用自然语言回答。
```

不展示：

```text
/answer interaction_xxx <你的答案>
/answer interaction_xxx cancel
```

### 6.3 内部命令保留方式

内部命令解析可以继续存在，但只作为隐藏能力：

- 日志排查。
- 开发调试。
- native button callback 的兼容目标。
- 历史用户输入了旧命令时仍能工作。

普通 IM markdown/card 文案不得包含这些命令和内部 id。

## 7. Pending Queue 规则

pending queue 只解决 active busy，不解决 waiting user。

### 7.1 进入 queue 的条件

只有满足以下条件时，IM 消息才进入 pending queue：

1. 当前 session 有 active run 正在执行模型、工具或流式输出。
2. 这条消息没有被 pending interaction 路由消费。
3. 这条消息没有被判定为 abandon 当前 interaction 后立即开新 turn。

### 7.2 NewTurn fallthrough 的去重要求

当权限 pending 时，用户发“问我三个问题”：

1. `ask_coordinator` 判断为 `NewTurn`。
2. 当前 permission 被 abandon 或 cancel。
3. pending ask 清理。
4. 同一条消息 fallthrough 到正常 dispatch，创建新 run。
5. 该消息必须标记为已经 dispatch，不能再作为 pending item 留在 queue。

验收标准：

- 用户只看到一次“三个问题”。
- 后续“我就随便问问”只影响当前 AskUserQuestion 或当前新 turn，不触发之前那条“问我三个问题”重复执行。

## 8. APP 与 RM/IM 同步规则

### 8.1 禁止普通 APP 回复自动同步到 RM/IM

当前 `DingtalkReplyManager` 通过 `remember_credentials` 保存 IM session 凭证，并允许后续 `dispatch_chunk` 在没有 active context 时 lazy-create card。这个能力要收紧：

- APP composer 创建的新 run 不允许 lazy-create IM card。
- 只有 IM/RM 入站消息创建的 run 才注册可输出到 IM/RM 的 reply context。
- run 完成后，对应 context 清理；仅保留凭证不能代表后续 run 也可输出到 IM/RM。

### 8.2 APP 处理 pending interaction 的反馈

当 pending interaction 原本来自 IM/RM run，但用户在 APP 里点击或提交：

- APP 正常 resolve permission 或 interaction。
- 向原 IM/RM 会话发送短反馈。
- 反馈不应包含模型长回复，也不应创建新的 streaming AI card。

建议反馈文案：

| APP 操作 | RM/IM 反馈 |
| --- | --- |
| 权限仅本次允许 | 已允许本次操作，任务继续执行。 |
| 权限永久允许 | 已记录授权范围，任务继续执行。 |
| 权限拒绝 | 已拒绝本次权限请求。 |
| 取消当前任务 | 已取消当前任务。 |
| AskUserQuestion 提交 | 已提交你的回答，任务继续执行。 |
| AskUserQuestion 取消 | 已取消这次提问。 |

这些反馈用于让 IM/RM 侧知道用户已经在 APP 处理了卡住的交互，不代表 APP 的普通对话要回流到 IM/RM。

## 9. Abandoned Permission 规则

如果 permission pending 已经因为新 turn 被 abandon：

- 后续“刚刚那个权限我同意”不能直接 resolve 已失效的 tool call。
- 模型不能只口头确认“好的，已同意”。
- 程序层必须给模型或用户一个明确事实：原审批已失效。

第一阶段推荐行为：

```text
刚才那次权限请求已经被新任务打断了。请重新发起读取，我会基于新的请求再次确认权限。
```

如果模型选择重新读取同一个文件，runtime 会产生新的 permission request。用户再次同意后，新的 request 正常 resolve 和落盘。

不在本阶段实现“恢复旧权限请求”，原因：

- 旧 tool call 可能已经被取消。
- 旧 run 可能已经完成或失效。
- 用户的“同意”可能缺少具体 scope。
- 复活历史审批需要额外的最近审批缓存、过期时间和安全校验。

## 10. 数据结构与实现边界

### 10.1 Run 输出目标

为 IM reply manager 或 runtime event 增加可判定的输出目标：

```rust
enum RunOutputTarget {
    AppOnly,
    Im {
        platform: Platform,
        external_conversation_key: String,
    },
}
```

或者等价地，在现有 request/context 中携带：

```rust
struct ChannelReplyRoute {
    session_id: SessionId,
    run_id: RunId,
    platform: Platform,
    target: ReplyTarget,
    allow_streaming_reply: bool,
}
```

关键不是具体类型名，而是 reply manager 判断输出时必须 run-aware，不能只 session-aware。

### 10.2 Card context key

当前 `contexts: HashMap<String, ReplyContext>` 以 `session_id` 为 key，容易跨 run 复用。目标 key 应包含：

```text
session_id + run_id
```

派生要求：

- stream delta 必须落到同一个 run 的 card。
- ask card delivery 必须检查 pending event 的 `run_id` 与 card context 的 `run_id`。
- run completed/fail/cancel 只清理对应 run 的 context。

### 10.3 Ask card 合并策略

`deliver_ask_card(session_id, markdown)` 需要能够找到同 run card。建议签名或内部调用链携带 run：

```rust
async fn deliver_ask_card(
    &self,
    session_id: &SessionId,
    run_id: &RunId,
    markdown: String,
) -> Result<()>;
```

合并规则：

1. 有同 run streaming card：把 pending markdown 追加到该 card 并 finish。
2. 同 run accumulated text 为空：用 pending markdown 填充该 card。
3. 没有同 run card，但 run 允许 IM 输出：创建 ask card。
4. 没有同 run card，且 run 不允许 IM 输出：不向 IM 输出 ask card；如果这是 APP 处理 IM pending 的反馈，走短反馈通道。

### 10.4 Pending item 消费标记

IM manager 在 `handle_pending_action_pre_dispatch` 返回 `NotPending` 后继续 dispatch 时，需要区分两种 `NotPending`：

```rust
enum HandleOutcome {
    NotPending,
    NewTurnAfterAbandon,
    ApprovalResolved,
    AnswerResolved,
    InvalidApprovalAction { message: String },
}
```

或者在调用侧使用等价标记，确保 `NewTurnAfterAbandon` 的这条消息不会被 pending queue 再次 enqueue。

如果不新增 enum variant，也必须在 pending adapter/build enqueue 前记录这条 inbound message id 已经进入 direct dispatch。

## 11. 测试计划

### 11.1 Rust 单元测试

`ask_coordinator.rs`：

- permission pending + “问我三个问题” => abandon permission，返回可新 turn 的 outcome。
- NewTurn fallthrough 后 pending map 清空。
- permission card markdown 不包含 `/approve`、`call_00`。
- AskUserQuestion card markdown 不包含 `/answer`、`interaction_id`。
- abandoned permission 后再次自然语言“同意刚刚那个权限”不 resolve 旧 tool call。

`reply_manager.rs`：

- 同 run 前置文本 + ask card 合并到同一 card。
- 不同 run 的 ask card 不覆盖旧 run card。
- APP-only run 不 lazy-create IM card。
- IM-origin run 仍正常创建和更新 IM card。
- run completed 只清理对应 run 的 context。

`pending_manager` 或 IM manager 相关测试：

- NewTurn fallthrough 消息不会进入 queue。
- active busy 时新的未消费消息仍进入 queue。

### 11.2 集成/手工验收

场景一：跳出权限审批

1. IM 发“读取 `/tmp/aijia-permission-test/secret3.txt` 并总结”。
2. 出现文件读取前置文本和权限确认。
3. IM 发“问我三个问题”。
4. 旧读取卡片不被改写。
5. 新 run 只输出一次三个问题。
6. pending queue 不重复 flush “问我三个问题”。

场景二：APP 普通消息不回流 IM

1. 从 IM 创建会话。
2. 回到 APP 同 session 普通发问。
3. APP 显示 AI 回复。
4. IM/RM 不收到这条普通 AI 回复。

场景三：APP 处理 IM pending

1. IM 触发权限或 AskUserQuestion。
2. APP 里点击允许或提交答案。
3. IM/RM 收到短反馈。
4. 后续长回复按原 run 输出目标处理，不因 session 凭证误发。

场景四：失效权限不口头伪确认

1. IM 触发权限。
2. 用户发新任务导致旧权限 abandon。
3. 用户说“刚刚那个权限我同意”。
4. 系统不显示“已同意并继续旧请求”的假象；要么提示旧请求已失效，要么重新发起读取并生成新 permission request。

## 12. 风险与取舍

1. 隐藏备用指令会减少普通用户困惑，但调试时少一个可见入口。用日志和开发模式补足。
2. run-aware card key 会触及 reply manager 的状态结构，改动比单纯清空 `accumulated_text` 更大，但能解决跨 run 污染的根因。
3. APP-only 与 IM-origin 输出隔离需要确定每个 run 的来源。若现有 request 没有稳定来源字段，需要从 `build_channel_chat_request` 和 APP composer request 创建处补齐。
4. abandoned permission 不做历史恢复会让用户需要重新发起一次读取，但比口头确认却没执行更诚实、安全。

## 13. 完成标准

1. 普通 IM 卡片不显示内部备用指令和内部 id。
2. 同 run 文本和 pending interaction 卡片内聚展示。
3. 跨 run 不改写旧卡片。
4. NewTurn fallthrough 消息不再二次执行。
5. APP 普通 AI 回复不再自动同步到 RM/IM。
6. APP 处理 IM pending interaction 时，RM/IM 只收到短状态反馈。
7. 已失效 permission 不会被自然语言“同意”伪装成成功 resolve。
8. 相关 Rust 测试覆盖上述行为。

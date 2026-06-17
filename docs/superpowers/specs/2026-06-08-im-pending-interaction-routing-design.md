# IM Pending Interaction Routing Design

> 状态：设计稿，待用户 review
> 日期：2026-06-08
> 作者：Codex，基于与项目 owner 的头脑风暴
> 关联工件：
> - `src-tauri/src/connector/im/shared/ask_coordinator.rs`
> - `src-tauri/src/runtime/tools/builtin/ask_user_question.rs`
> - `src-tauri/src/runtime/interaction/control_plane.rs`
> - `docs/superpowers/specs/2026-06-03-permission-approval-surface-design.md`

## 1. 背景

6 月 3 日的 permission approval 设计把“等待权限审批”和“忙时消息排队”混在了一起。实际产品语义应该更简单：

- run 正在执行模型或工具时，用户输入进入 pending queue。
- run 等待用户交互时，不是 active busy，而是 suspended waiting user。
- 等待用户交互期间的下一条输入，默认是对这个 pending interaction 的回复。
- 只有用户明确表示换话题、取消当前任务，才取消 suspended run 并开新 turn。

这次对话 `f25ed287-2d64-4708-9798-ab57f1038abc` 暴露了 AskUserQuestion 路由问题：

1. 用户让模型“问我三个问题”。
2. 模型调用 `AskUserQuestion`，IM 侧注册了 `kind=user_question` pending。
3. 用户直接回复三行答案：

   ```text
   HR/人事
   数据处理与分析
   结论优先
   ```

4. `ask_coordinator.rs` 只接受显式 `/answer <interaction-id> ...`，普通文本返回 `NotPending`。
5. 这三行答案被当成新 turn，模型又调用 `WriteMemory` 记录偏好。
6. 新 turn 完成时还删除了旧 run 的 pending ask，说明 pending 生命周期只按 session 清理也有风险。

这不是模型理解问题，而是 IM pending interaction 的路由语义错了。

## 2. 目标

1. 明确区分 active busy 和 suspended waiting user。
2. 让 AskUserQuestion 的普通文本回复默认 resume 原 interaction。
3. 让 permission approval 的自然语言输入成为审批解释文本，而不是普通新聊天。
4. 只有 active busy 时才使用 pending queue。
5. pending ask 的生命周期必须 run-aware，不能被其他 run 完成事件误删。
6. 保留显式命令和按钮作为确定性快速路径。

## 3. 非目标

- 不让普通 LLM 直接写 permission store。
- 不删除 `/approve`、`/answer` 等显式命令。
- 不把所有 pending 输入都交给一个自由的 LLM Router。
- 不在本设计里改 IM 卡片样式、前端字体、消息气泡布局。
- 不重新定义普通忙时 pending queue 的合并和防抖机制。

## 4. 核心语义

### 4.1 Run 状态

run 至少需要区分三类状态：

| 状态 | 含义 | 新输入处理 |
| --- | --- | --- |
| idle | 当前 session 没有运行中或挂起的 run | 开新 turn |
| active busy | run 正在执行模型、工具或恢复流式输出 | 进入 pending queue |
| suspended waiting user | run 等待 AskUserQuestion 或 permission approval | 默认回复 pending interaction |

suspended waiting user 不占 active run busy marker。这样 app 输入框和 IM 入口都可以继续接收用户文本。

当用户回复 pending interaction 后，runtime 再 reacquire busy marker，继续原 run。

当用户明确取消或换话题时，runtime cancel suspended run，清理对应 pending interaction，再开新 turn。

### 4.2 输入路由顺序

IM 或 app composer 收到用户文本时，按以下顺序处理：

1. 查当前 session 是否有 live pending interaction。
2. 如果没有 pending interaction，正常开新 turn 或按 active busy 进入 queue。
3. 如果有 AskUserQuestion pending，先走 AskUserQuestion 回复规则。
4. 如果有 permission pending，先走 permission 审批回复规则。
5. 如果当前 run 是 active busy，才进入 pending queue。

这意味着 pending interaction 优先于“新 turn”判断，也优先于“忙时 queue”判断。

## 5. AskUserQuestion 路由

AskUserQuestion 是确定性的人机交互，不应该要求 IM 用户一定输入 `/answer`。

### 5.1 默认规则

当 session 有 `PendingAskKind::UserQuestion`：

- 普通文本默认作为 answer 提交给原 `interaction_id`。
- 多问题表单优先按行映射。
- 如果问题有稳定 id，按 id 生成 answers。
- 如果没有稳定 id，按 question 顺序或 option label 映射。
- 行数不匹配时仍提交原文，作为 `rawText` 或 `freeText`，让原 run 继续判断。
- `/answer <interaction-id> ...` 继续作为显式快速路径。

示例：

```text
HR/人事
数据处理与分析
结论优先
```

应转换为类似：

```json
{
  "answers": [
    "HR/人事",
    "数据处理与分析",
    "结论优先"
  ],
  "rawText": "HR/人事\n数据处理与分析\n结论优先"
}
```

然后通过原 `InteractionResolution::Submit` resume 原 interaction。原 `AskUserQuestionRuntimeTool` 读取 `ctx.interaction_resolution` 后返回给模型，让同一个 run 继续执行。

### 5.2 取消和换话题

只有明确取消或换话题时，才不把文本当答案。

候选表达包括：

- `算了`
- `别问了`
- `取消`
- `不用了`
- `换个事`
- `看看别的文件`
- `先不回答这个`

这类输入应 resolve 为 cancel，随后根据文本是否包含新任务意图决定是否开新 turn。

### 5.3 UI 和 IM 的一致性

AskUserQuestion 卡片或 pending surface 的“其他”必须有意义：

- 选择题按钮提交 structured answer。
- “其他”输入提交 free text answer。
- IM 直接回复普通文本也提交 free text answer。

“其他”不是忙时 pending，也不是普通新聊天。

#### 5.3.1 后续：AskUserQuestion option schema 需要表达补充输入

2026-06-17 真实会话暴露了一个产品语义问题：`AskUserQuestion` 现在只有
`label` / `description` 这类展示字段，无法表达“某个选项被选中后需要用户
补充输入”。短期先在工具定义中明确要求模型不要把“其他 / Other / 请说明”
这类自定义回答入口作为普通 option 传入；前端也不应继续扩展基于 label
关键词的兜底判断。

后续如果产品需要“选中某项后补充信息”，应系统扩展 `AskUserQuestion` 参数，例如：

- option 支持 `requiresInput: true`。
- option 支持 `inputLabel` / `inputPlaceholder`，例如“请输入手机号”“请说明测试目标”。
- option 支持基本校验元数据，例如 phone / url / text / number 或 required。
- 前端只在 option schema 声明需要输入时展示输入框；内置 custom row 是否存在也应由工具参数或统一默认策略决定。
- IM 文本回复与 App 表单提交应落到同一套 answer shape，避免 App 有输入框、IM 只能传裸文本导致语义不一致。

验收口径：模型不通过“其他（请说明）”这类 label 暗示输入框；UI 根据结构化
字段决定是否渲染输入框，提交结果中能区分“选择了哪个选项”和“补充输入是什么”。

## 6. Permission 路由

permission approval 也是 pending interaction，但它多了安全边界，不能把自然语言直接落盘成权限。

### 6.1 规则优先

当 session 有 permission pending：

- `允许`、`可以`、`本次允许` 解析为 `allow_once`。
- `永久允许`、`以后都允许` 解析为 `allow_remember`，但 scope 仍需校验。
- `拒绝`、`不允许` 解析为 `deny`。
- `取消`、`算了` 解析为 `cancel`。
- `/approve <request-id> allow|deny|cancel` 继续作为显式快速路径。

### 6.2 自然语言审批解释

复杂文本不能 fallthrough 成普通新 turn，例如：

```text
以后 /tmp 这个目录下的文件都可以读
```

它应该进入 permission natural-language parser，产出结构化候选：

```rust
enum PermissionReplyCandidate {
    AllowOnce,
    AllowRemember { scope: PermissionScope },
    Deny,
    CancelAndMaybeNewTurn { user_text: String },
    NeedsClarification { reason: String },
}
```

parser 只产出候选，不直接写权限。候选必须经过 runtime permission control plane 校验：

- path canonicalize
- requested tool/action match
- scope 是否覆盖原请求
- remember destination 是否合法
- broad scope 是否需要更明确的确认

### 6.3 “其他”输入的语义

permission pending surface 的“其他”也必须有意义：

- 可以扩大或缩小授权范围。
- 可以说明拒绝原因。
- 可以表达取消当前任务。
- 可以表达“不要这个了，做另一个事”，这时 cancel 当前 pending，再开新 turn。

它不是普通聊天输入，也不是只能按按钮。

## 7. Pending 生命周期

pending interaction 不能只按 session 管。最小 key 应包含：

```text
session_id + run_id + interaction_id/tool_call_id
```

清理规则：

- submit 成功后，删除对应 interaction。
- cancel 成功后，删除对应 interaction。
- run cancelled 后，删除该 run 下所有 interaction。
- run completed 后，只删除该 run 下仍存在的 interaction。
- 其他 run completed 不能删除当前 run 的 pending interaction。
- 新 turn completion 不能删除 suspended run 的 pending interaction。

这可以避免新 run `6a1ad142-301f-4361-be14-377e5ff476a9` 完成时，误删旧 run `0d68cfc2-30cb-4761-a3f5-adb0885df7ea` 的 AskUserQuestion pending。

## 8. Busy Queue 的边界

pending queue 只处理 active busy 场景：

- 模型还在输出。
- 工具还在执行。
- resume 后的 run 重新占用 busy marker。
- 用户连续发送多条消息，而当前 run 没有进入 suspended waiting user。

pending queue 不处理 suspended waiting user 的第一条回复。那条回复应该优先 resolve pending interaction。

如果 suspended run resume 后又进入 active busy，后续消息才进入 pending queue。

## 9. 状态展示

turn stage 和前端 pending surface 需要 run-aware：

- `waitingPermission` 只展示当前 conversation 的 live permission。
- `waitingInteraction` 只展示当前 conversation 的 live AskUserQuestion。
- 旧 run heartbeat 不能覆盖新状态。
- 新 turn 完成不能把旧 pending 的 UI 清掉。
- app 侧和 IM 侧共用同一份 pending interaction 状态。

## 10. 测试口径

### 10.1 AskUserQuestion

- IM pending AskUserQuestion 后，普通三行答案应 resolve 原 interaction，不开新 run。
- app pending AskUserQuestion 的“其他”输入应作为 free text answer 提交。
- `/answer <interaction-id> ...` 仍能显式 resolve。
- `算了` 应 cancel 原 interaction。
- `算了，看看别的文件` 应 cancel 原 interaction，并开新 turn。

### 10.2 Permission

- permission pending 后，`可以` 应解析为 allow once。
- `永久允许` 应解析为 remember candidate，并由 permission layer 校验和落盘。
- `以后 /tmp 这个目录下的文件都可以读` 应进入自然语言审批解释，不开普通新 turn。
- invalid request id 的 `/approve` 不应 fallthrough 成普通聊天。
- permission store 的写入只能发生在 runtime validation 之后。

### 10.3 Lifecycle

- 不同 run 的 `TurnCompleted` 不能删除旧 run 的 pending interaction。
- run cancelled 后清理该 run 的 pending interaction。
- suspended waiting user 不占 active busy。
- resume 后能重新 acquire busy。
- active busy 时连续消息仍进入 pending queue。

## 11. 实施建议

推荐分三步落地：

1. 修 AskUserQuestion 确定性 resume。
   - 调整 `IMAskCoordinator::try_handle_reply`。
   - 普通文本命中 `PendingAskKind::UserQuestion` 时构造 `InteractionResolution::Submit`。
   - 补 IM 三行答案不新开 run 的测试。

2. 修 pending 生命周期 key。
   - pending ask map 加 `run_id` 维度。
   - cleanup 事件必须携带 run id 并精确删除。
   - 补新 run completed 不删除旧 pending 的测试。

3. 修 permission natural-language path。
   - 规则优先解析 allow/deny/cancel。
   - 增加自然语言候选 parser。
   - 所有 remember/scope 都走 permission validation 和 store 写入。

这三步可以顺序提交。第一步能立刻修复 `f25ed287-2d64-4708-9798-ab57f1038abc` 这种 AskUserQuestion 误开新 turn 的问题。

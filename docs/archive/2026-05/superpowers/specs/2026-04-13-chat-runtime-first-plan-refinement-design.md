# Chat Runtime-First 收口计划细化设计（执行导向）

## 背景

当前已有专项计划 `docs/superpowers/plans/2026-04-13-chat-runtime-first-closure-plan.md`，目标、验收标准和高层分期是清楚的，但仍存在一个执行问题：计划还不足以被稳定地按阶段、按任务直接实施。

主要症结不是方向错误，而是“从高层专项计划到可执行实施单元”的中间层还缺了一步。具体表现为：

- `R2` / `R3` 粒度偏大，单个阶段内部仍混合多种迁移动作
- runtime ownership、dispatcher contract、event compatibility 三类变化交叉，阶段边界还不够硬
- 测试虽然提了方向，但还没有把“每一阶段必须先证明什么、改完后必须锁住什么”写成固定锚点
- 当前仓库存在大量未提交改动，如果实施单元不够小，容易把本专项和别的进行中改动混在一起

因此，这次细化的目标不是改专项方向，而是把它改写成**可直接派发执行、可阶段性验收、可在当前工作区控制修改边界**的计划。

---

## 目标

把现有专项计划细化为一份**执行级 implementation plan**，满足下面 5 个要求：

1. 保留原计划的主结构（仍以 `R1 ~ R4` 为主线）
2. 每个 phase 再拆成更小的执行单元，能逐项实施
3. 每个执行单元都绑定清晰的测试锚点与退出条件
4. 明确本轮采用“**分两步迁移**”而不是一步到位重写
5. 让后续执行时能稳定回答 3 个问题：
   - 现在该改哪几个文件？
   - 这一步必须先写什么失败测试？
   - 改完怎样证明 ownership 已经迁移，而不是只是换了包装？

---

## 不采用的细化方式

这次不采用以下方式：

### 1. 不改成纯设计文档驱动

不单独停留在“如何细化计划”的抽象设计，而是直接服务于计划重写。因为当前问题不是缺概念，而是缺执行粒度。

### 2. 不改成完全重排的能力矩阵计划

虽然按“turn ownership / dispatch ownership / event compatibility”重新组织会更理论化，但会打断现有 `R1 ~ R4` 的计划结构，也不利于承接已有 review 语境。

### 3. 不做一步到位迁移计划

不要求在同一轮计划里把 `chat_runtime_impl.rs` 的 orchestration 与 helper 角色同时清空。这样会把结构迁移、逻辑迁移、回归验证三件事叠在一起，风险过高。

---

## 推荐方案

采用：**保留 `R1 ~ R4` 主结构 + 每阶段再拆执行单元 + 分两步迁移 + 测试锚点驱动**。

这是最适合当前代码库状态的细化方式，因为它同时满足：

- 与原专项计划连续
- 能直接转成实施步骤
- 能适配当前已有大量未提交改动
- 能为后续子代理执行提供稳定边界

---

## 核心设计

### 一、迁移策略：分两步迁移

本轮计划细化明确采用“**分两步迁移**”策略。

#### 第一步：ownership 先迁移

优先要求 runtime 拿回真实聊天主链路的 ownership，包括：

- `SessionRuntime::run_chat_request()` 成为真实入口
- runtime 明确持有 turn lifecycle
- runtime 决定何时开始流式输出、何时进入工具循环、何时写回消息、何时结束 turn
- transport 不再拥有 full orchestration 的控制权

在这一步里，允许 `chat_runtime_impl.rs` 继续保留一部分 **compatibility helper** 或 **host-bound helper**，前提是它们不再决定聊天主循环。

#### 第二步：legacy / helper 进一步收缩

在 ownership 已迁回 runtime 之后，再继续收缩：

- `TauriLegacyTurnExecutor` 的角色
- `legacy_send_message_impl()` 的 orchestration 残留
- transport 侧直接决定 tool dispatch/schema surface 的逻辑
- `chat_runtime_impl.rs` 中仍带有业务编排味道的 helper

这样做的核心目的是把“谁拥有主链路”与“helper 还剩多少”拆开处理，先锁真相源，再做瘦身。

---

### 二、阶段拆分原则

保留原计划的 `R1 ~ R4`，但每个阶段都必须细化成“**单一迁移目标 + 单一验证目标**”的执行单元。

#### R1：证明旧问题仍存在，并锁住切换目标

R1 不只是“补测试”，而是要把当前错误结构变成**不能回避的红灯约束**。

R1 应至少拆成以下执行单元：

1. **锁住 executor-backed bypass 现状**
   - 证明当前路径还是 `preflight + delegate`
   - 证明 runtime 没拥有完整 turn orchestration

2. **锁住 runtime chat driver 的目标入口**
   - 测试层明确要求 `send_message` 主链路由 runtime 入口驱动
   - 不允许 transport 侧 full orchestration 继续被视为通过

3. **锁住 event compatibility 不变**
   - 在迁移前先把 legacy-compatible 事件序列作为观测面固定下来

R1 的作用是建立“迁移后必须满足什么”，而不是仅仅说明“现在有问题”。

#### R2：先迁 turn ownership 与 orchestration entry

R2 应聚焦在：**runtime 成为聊天 orchestrator**。

这个阶段不要求一次性完成所有 tool/dispatcher 收口，而是优先迁移：

1. `SessionRuntime::run_chat_request()` 的真实驱动权
2. runtime-owned chat driver 的入口与职责边界
3. transport 从 orchestrator 降级为 host adapter / helper provider

R2 必须明确：

- 哪些 orchestration 逻辑本轮必须迁出 transport
- 哪些 helper 可以暂留在 `chat_runtime_impl.rs`
- 什么叫“helper 可以保留，但 orchestration ownership 不能保留”

#### R3：收 tool dispatch、schema surface 与 workspace-first 注入

R3 只处理一个重点：**真实聊天里的工具执行合同必须归 runtime**。

它应拆成：

1. **dispatch path 收口**
   - 不再以 transport 侧直接 `ToolRegistry::execute(...)` 作为真实主路径

2. **schema surface 真相源收口**
   - runtime 决定聊天轮次里暴露给模型的 tool surface

3. **workspace-first 注入保留**
   - 授权目录继续进入 runtime tool execution context
   - request-scoped tools 继续可达

R3 的退出条件不是“工具能跑就行”，而是“**工具能跑且走的是 runtime 合同**”。

#### R4：锁兼容事件与关键回归闭环

R4 才处理最终兼容性收尾：

1. `TauriEventAdapter` 映射保持兼容
2. 不发生 double emit
3. Workspace-First / Atomic Tool 相关关键回归持续通过
4. targeted regression 套件形成“主链路已切换”的最终证据

R4 不应回头再承担 ownership 迁移主工作；它只负责证明迁移后没有破坏外部行为。

---

### 三、执行单元模板

细化后的正式计划中，每个执行单元都应使用统一模板。建议至少包含以下字段：

#### 1. 目标
说明这一小步唯一要解决的结构问题，例如：
- “禁止 executor-backed chat path 完整委托 legacy executor”
- “让 SessionRuntime 直接驱动 runtime chat driver”
- “把真实 tool execution 主路径收敛到 runtime dispatcher”

#### 2. 修改文件
列出本步真正允许改动的文件，不鼓励超范围顺手改。

#### 3. 不做什么
明确本步不承担的重构，例如：
- 不顺手重构全部 helper
- 不改前端协议
- 不处理所有 legacy tool handler

#### 4. 红灯测试
列出必须先写或先改的测试，以及这些测试需要证明的事实。

#### 5. 实现动作
只写实现动作，不夹杂额外架构发散。

#### 6. 绿灯验证
列出最小验证命令，不要求一上来跑全量，但要能证明本步达标。

#### 7. 退出条件
只有满足退出条件，才能进入下一执行单元。

这个模板的意义是：让每一步都具备“可实施、可回退判断、可 review”的最小闭环。

---

### 四、helper 与 orchestrator 的边界定义

这是本次细化里最关键的部分，必须在正式计划里写死。

#### 可以暂留在 `chat_runtime_impl.rs` 的内容

第一步迁移里，允许保留以下类型的 helper：

- 纯 Tauri host 绑定逻辑
- 与旧事件协议映射强绑定的兼容辅助函数
- 已有实现中被 runtime 调用的上下文装配辅助，但其调用时机与主循环控制权必须由 runtime 决定

#### 本轮必须迁出 transport ownership 的内容

以下内容不能再继续由 transport 拥有控制权：

- 聊天主循环的起止控制
- 何时进入工具调用回合
- 何时结束 streaming / terminal state
- 何时持久化 assistant/tool 结果
- 真实 tool execution 主路径的选择权

换句话说：

- helper 可以保留“怎么做”的局部实现
- 但 transport 不能再保留“什么时候做、做哪一步、主链路下一步去哪”的决定权

这是判断“到底有没有 runtime-first 收口”的核心标准。

---

### 五、测试锚点设计

细化后的正式计划必须把测试从“推荐方向”升级为“阶段锚点”。

#### R1 锚点

必须把下面三类约束写死：

1. **bypass 不再允许存在**
   - executor-backed path 不能只是 preflight 后完整委托 legacy executor

2. **runtime driver 成为真实主链路入口**
   - `send_message` runtime path 测试必须能识别主 orchestrator 已切换

3. **compatibility 观测面固定**
   - 至少锁住关键 legacy events 的序列或关键点位

#### R2 锚点

必须新增或升级测试，证明：

- `SessionRuntime::run_chat_request()` 不再只是 preflight wrapper
- runtime-owned driver 被真实调用
- transport 不是 full orchestrator

#### R3 锚点

必须证明：

- tool execution 主路径进入 runtime dispatcher
- request-scoped tools 仍然可达
- authorized workspace directory 仍被注入主链路
- tool events 不会双发或漏发

#### R4 锚点

必须证明：

- 旧前端协议仍可消费
- workspace-first golden path 无回归
- targeted tests 组合起来可以作为“真实主链路已切换”的最终证据包

---

## 设计后的正式计划应如何改写

基于上面的设计，原计划改写时应满足以下结构变化：

### 1. 保留原文的 Goal / AC / Truth Source / Kill List

这些高层内容已经足够清楚，不需要推翻。

### 2. 把 `R1 ~ R4` 从“阶段建议”升级为“执行阶段”

每个阶段必须变成：

- 目标
- 前置条件
- 执行单元 A / B / C
- 每个单元的测试锚点
- 最小验证命令
- 阶段退出条件

### 3. 增加“迁移边界”段落

专门说明：

- `chat_runtime_impl.rs` 本轮可保留什么
- `TauriLegacyTurnExecutor` 允许暂存到什么程度
- 什么情况仍然算没完成 runtime-first 收口

### 4. 增加“实施纪律”段落

明确要求：

- 必须先写红灯测试再迁移 ownership
- 不允许先靠 prompt/包装绕过测试
- 不允许把 transport 中的 orchestrator 逻辑只是挪名不挪权
- 不允许在一个执行单元里同时做 ownership 迁移和大规模 unrelated cleanup

### 5. 增加“最终证据包”段落

定义本专项关闭前必须同时满足的证据：

- runtime mainline tests 通过
- dispatcher production path tests 通过
- workspace-first 关键回归通过
- 兼容事件测试通过
- 测试断言能够说明主 orchestrator 已切换，而不是仅仅输出没变

---

## 预期收益

如果按这个设计细化原计划，后续实施会有 4 个直接收益：

1. **子任务可派发**
   - 每一小步都有清晰边界，不会让执行者面对一个“大而正确但难下手”的阶段

2. **review 可对焦**
   - reviewer 能判断这一步到底是在迁 ownership，还是只是在重命名/包一层

3. **现有改动更容易共存**
   - 当前工作区已有大量未提交更改，越小的实施单元越容易控制风险

4. **验收标准更硬**
   - 不再出现“runtime 模块有了，但真实主链路还在 legacy full delegate”这种伪完成状态

---

## 建议的下一步

下一步不是直接实施，而是：

1. 依据本设计，直接重写原计划文件 `docs/superpowers/plans/2026-04-13-chat-runtime-first-closure-plan.md`
2. 在原计划中保留高层目标与验收标准
3. 重写 `R1 ~ R4` 为可执行阶段
4. 明确分两步迁移边界与每步测试锚点
5. 让该计划可以直接作为后续 implementation session 的唯一执行依据

---

## 一句话结论

这次细化的本质不是“再解释一遍 runtime-first”，而是把现有专项计划升级成：**每一步都知道先写什么红灯、改哪几个文件、怎样证明 ownership 真的迁走了** 的执行计划。

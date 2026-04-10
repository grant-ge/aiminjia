# 第 0 期：现状审计与迁移护栏

> 文档角色：正式重构前的审计与护栏文档
> 目标：建立对现有系统的可验证认知，避免后续出现新旧双栈长期并存、状态真相源不清、事件语义漂移

---

## 一、本期目标

在不改变现有功能的前提下，完成以下审计资产：

1. `chat.rs` 职责地图
2. 系统状态拥有者矩阵
3. 事件发射/消费矩阵
4. tool 生命周期与调用路径审计
5. 回归基线与 golden trace 回放样本

### 本期解决的挑战
- 为后续 C1-C10 建立事实基础
- 提前识别 busy/run.lock/streaming/tool 执行的多真相源问题
- 防止第 1 期开始后对旧系统认知不足导致返工

---

## 二、现状问题假设（待审计确认）

### 2.1 chat.rs 过胖
重点关注：`src-tauri/src/commands/chat.rs:540` 附近的 `send_message()` 及其周边 helper。

可能职责：
- input 解析
- 会话/消息加载
- settings 读取
- 鉴权
- prompt/context 组装
- 模型选择/路由
- skill 激活 / precompute / step note
- tool loop
- sub-agent
- streaming emit
- state 清理
- message persist
- error/fallback

### 2.2 多真相源状态
初步怀疑：
- busy：`llm/gateway.rs` 内存态
- run.lock：`storage/file_store/mod.rs:545`
- 前端 streaming/busy：前端 hook / store
- python session：`python/session.rs`
- auth：`auth/mod.rs`

### 2.3 Current Event Contract
当前真实后端事件基线必须按现状审计，不得用未来命名替代现状协议。至少按以下 legacy Tauri events 作为前端兼容基线：

- `streaming:delta`
- `streaming:done`
- `tool:executing`
- `tool:completed`
- `message:updated`
- `agent:idle`

要求：
- 所有审计、trace、兼容边界都以这些真实事件名为准
- 如果需要记录未来 Runtime 内部事件，如 `StreamStarted`，必须明确标记为“内部 runtime event”
- 审计时要确认这些事件的 payload 结构与顺序依赖

如果顺序或字段变动，可能出现“功能正常但 UI 状态漂移”。

---

## 三、审计任务

### 3.1 chat.rs 职责地图
输出一张职责表：

| 职责 | 入口函数/代码区段 | 依赖对象 | 后续归属层 |
|------|------------------|----------|-----------|
| 输入规范化 | | | Transport / Input Layer |
| 会话加载 | | | SessionRuntime |
| 鉴权 | | | Auth Adapter / Permission |
| 上下文构建 | | | QueryEngine |
| 技能策略 | | | Skill Strategy |
| 工具调度 | | | Tool Runtime |
| 子代理 | | | Agent Runtime |
| 流式事件 | | | RuntimeEvent + Adapter |
| 持久化 | | | Store / Repository |
| 清理/取消 | | | Run Control |

要求：
- 覆盖 `send_message()` 内所有显著步骤
- 标记每一步当前依赖的全局状态与副作用
- 标记未来迁移到的目标层

### 3.2 状态拥有者矩阵
输出表：

| 状态 | 当前真相源 | 读取方 | 写入方 | 风险 | 目标真相源 |
|------|-----------|--------|--------|------|-----------|
| busy | | | | | RunState |
| streaming | | | | | RunState |
| current tool call | | | | | ToolCallState |
| python session | | | | | RunScoped PythonSession |
| auth | | | | | AuthState |
| run lock | | | | | RunStore |
| messages | | | | | SessionStore |

要求：
- 每个状态必须只有一个“当前真相源”结论
- 如果实际是多真相源，明确指出冲突点

### 3.3 事件发射/消费矩阵
输出表：

| 事件名 | 当前发射位置 | 当前消费位置 | 关键字段 | 顺序依赖 | 是否需兼容 |
|--------|-------------|-------------|----------|----------|-----------|
| streaming:delta | | | | | 是 |
| streaming:done | | | | | 是 |
| tool:executing | | | | | 是 |
| tool:completed | | | | | 是 |
| message:updated | | | | | 是 |
| agent:idle | | | | | 是 |

要求：
- 明确哪些事件顺序是 UI 的隐含 contract
- 记录 payload shape
- 区分“真实 legacy Tauri events”和“未来内部 runtime event”

### 3.4 Tool 生命周期审计
至少识别：
- tool 定义来源（`llm/tools.rs` / `plugin/builtin/tools/*` / `registry.rs`）
- tool 调用入口
- tool 执行时能拿到哪些上下文
- tool 返回结果怎么映射回消息流
- 哪些 tool 已经依赖 service locator 风格的 PluginContext

### 3.5 Golden Trace 基线
挑选至少 5 条真实用户链路，产出事件时序：

1. 普通聊天，无工具
2. 单工具调用
3. 多工具调用
4. sub-agent 调用
5. 取消中断

每条 golden trace 至少记录：
- 输入
- 关键状态变化
- 发出的事件序列
- 最终持久化结果

---

## 四、新增文件（建议）

本期新增的审计文件：

```
docs/
├── architecture-audit/
│   ├── chat-responsibility-map.md
│   ├── state-owner-matrix.md
│   ├── event-contract-matrix.md
│   ├── tool-lifecycle-audit.md
│   └── golden-traces.md
```

可选新增代码侧辅助文件：

```
src-tauri/src/runtime_audit/
├── mod.rs
└── trace_capture.rs   # 仅用于抓取当前事件序列，不进入正式 runtime
```

---

## 五、迁移文件

本期不做正式迁移，只做审计与基线采集。

允许的临时改动：
- 增加 trace capture / logging 帮助提取事件序列
- 增加注释或临时文档定位职责区段

禁止：
- 正式抽 SessionRuntime
- 正式改 ToolPlugin trait
- 正式替换 store

---

## 六、Compatibility Boundary

本期必须保持：
- 所有 Tauri command 对前端接口不变
- 所有事件名与 payload 不变
- 所有持久化格式不变
- Python session 复用逻辑不变

本期只是建立护栏，不引入新行为。

---

## 七、Kill List

本期不废弃任何线上路径。

但要产出“未来 kill list 候选”：
- `chat.rs` 内未来将被迁出的职责段
- `llm/gateway.rs` 中未来将被 runtime 接管的控制逻辑
- `PluginContext` 中未来将被移除的全量依赖注入

---

## 八、Truth Source（本期只做认领，不改实现）

本期要在审计文档中明确未来的真相源：

| 状态 | 未来真相源 |
|------|-----------|
| run/busy | `RunState` |
| streaming | `RunState` |
| tool executing | `ToolCallState` |
| agent status | `AgentInvocationState` |
| message history | `SessionStore` |
| python session scope | `RunScoped PythonSession` |

要求：即使当前实现还没改，也要在文档里先拍板。

---

## 九、Golden Trace 验收

### 9.1 验收标准
本期完成的标志不是写完文档，而是：
- 可以复放 5 条 golden trace
- 能证明每条 trace 的事件序列、状态变化、持久化结果一致
- 能从 trace 定位到对应代码段

### 9.2 Trace 输出格式
建议格式：

```markdown
## Trace 01 - 普通聊天
- Input: ...
- SessionId: ...
- conversation_id: ...
- Event sequence:
  1. streaming:delta
  2. streaming:delta
  3. message:updated
  4. streaming:done
- State transitions:
  - busy false -> true
  - busy true -> false
- Persist:
  - user message saved
  - assistant message saved
```

---

## 十、Cutover Strategy

本期无生产切换，只做审计与基线采集。

策略：
- 不改线上主路径
- 允许增加 trace capture / logging 辅助代码
- 所有审计产物必须可被下一期直接引用

## 十一、Rollback Strategy

本期不存在正式切流，因此也不存在业务级回滚。

若审计辅助代码影响现有行为：
- 直接移除 trace capture / logging 改动
- 回到原始 command/tool/store 路径
- 审计文档可以保留，但代码侧辅助开关必须关闭

---

## 十二、Not Doing

本期明确不做：
- 不引入新 ID 模型到生产逻辑
- 不抽 Runtime 模块
- 不修改前端 hooks
- 不统一 tool 注册表
- 不修改 Python session 作用域
- 不引入新 repository trait

---

## 十三、本期完成定义

第 0 期完成的标志：

1. 有完整职责地图，覆盖 `chat.rs` 主流程
2. 有状态拥有者矩阵，并指出冲突状态
3. 有事件协议矩阵，并指出必须兼容的隐式 contract
4. 有 tool 生命周期审计
5. 有 5 条可复放的 golden trace
6. 有下一期可直接使用的 kill list 候选

完成后才能进入第 1 期。

# lotus-app 剩余架构改造清单

**日期**：2026-04-15  
**目的**：基于当前代码状态，整理对标 `claude-code-best` 仍未完成的改造点，供后续继续派发给 Claude 执行。  
**使用方式**：这不是问题复盘文档，而是后续施工 backlog。优先按“致命 / 高优先级 / 可后置”执行，不要把所有项揉成一轮混做。

---

## 一、当前总体判断

当前系统已经不再是“完全 legacy chat + legacy tool”的状态，runtime-first、workspace 授权、前端事件消费链路都已有实质进展。

但对标 `claude-code-best`，当前仍处于：

> **runtime 外壳已形成，但主链路 ownership、权限边界、工具边界、取消模型、前后端状态模型还没有一起闭合。**

因此，剩余问题的重点不再是“补功能”，而是“把系统边界收干净”。

---

## 二、致命项

这些项不闭合，不能认为已达到 `claude-code-best` 级别。

### D1. 权限系统没有真正成为生产主路径的唯一裁决边界

**现状**

- 新权限模型已经实现，但生产 dispatcher 仍未完全切到新 policy boundary
- 当前仍存在“代码里有新权限系统，但真实运行主链路主要吃旧权限语义”的风险

**为什么致命**

- 这会直接影响工具调用的 allow / deny / ask / persisted decision 是否真实生效
- 一旦权限系统不是唯一边界，多会话、工具调用、workspace 访问、Python 执行都会留下灰区

**改造目标**

- 生产工具执行链路统一经过一个 runtime-owned policy engine
- persisted permission、unknown scope、session capability 都在同一条主路径裁决
- 不再存在“测试里走新权限，生产里还在走旧权限”的双轨状态

---

### D2. 工具体系仍然由 legacy bridge 主导，不是 runtime-native contract 主导

**现状**

- RuntimeTool 已存在，但 legacy adapter 仍大量参与真实生产工具执行
- 工具上下文仍未彻底摆脱旧式 service locator 语义

**为什么致命**

- 只要工具系统还是双轨，权限、审计、取消、tool_call 生命周期就难以形成单一真相源
- 即使功能能跑，系统也很难达到 `claude-code-best` 那种原子、可组合、可治理的工具模型

**改造目标**

- 默认主链路上的核心工具全部转为 runtime-native contract
- legacy adapter 退化为过渡兼容层，而不是继续承担主要生产流量
- 工具执行路径统一为：lookup -> permission -> execution -> event/audit

---

### D3. 取消传播模型不完整，长任务 / 工具 / 子进程 / 子 agent 没有统一收口

**现状**

- 当前取消能力还没有成为系统级统一机制
- 部分路径仍带有 fire-and-forget、gateway 兜底取消、子路径单独回收等旧语义

**为什么致命**

- 单会话 happy path 下不一定明显
- 但长任务、多工具、多会话并发、子 agent、Python 子进程场景会持续暴露中断不彻底、误清理、残留执行等问题

**改造目标**

- 建立统一的 run-scoped cancellation model
- LLM stream、tool execution、subprocess、background agent 都接受同一取消源
- cancel 后的状态、事件、store 记录、前端状态全部一致

---

## 三、高优先级项

这些项不一定立即致命，但会持续阻塞主链路收口。

### H1. chat 主链路仍是过渡态，runtime 还不是唯一 owner

**现状**

- runtime 已进入入口层
- 但聊天主编排 ownership 仍没有完全从 legacy chat 实现回收到 runtime driver

**问题**

- 这会导致后续所有改造继续被迫兼容 legacy 编排语义
- chat 生命周期相关能力无法真正 runtime-owned

**目标**

- runtime driver 成为聊天主链路唯一 owner
- legacy transport 退化为 host bridge / compat adapter，而不是继续持有聊天主循环

---

### H2. 正向主链路“能跑”，但还不是“完全可信的 runtime 主链路”

**现状**

- 现在正向主链路基本可用
- 但“主流程可用”不等于“真实生产路径已完全按目标架构闭合”

**问题**

- targeted green tests 仍可能高估主链路收口程度
- 一些 legacy 语义仍可能在真实 adapter path 中继续生效

**目标**

- 增补 production-path gating
- 明确证明真实 send_message path、真实 tool round、真实 run/task/tool_call 状态更新都走 runtime 真相源

---

### H3. 会话隔离仍是部分隔离，不是强隔离

**现状**

- 目前已经有 conversation/run 级别状态组织
- 但还不是严格的 per-session / per-run 封闭边界

**问题**

- 多会话并发时，busy、cancel、tool lifecycle、child agent、workspace capability 更容易出现串扰风险

**目标**

- 强化 run/session 边界
- 清理所有会弱化隔离的共享上下文和隐式全局依赖

---

### H4. `PluginContext` / `AppStorage` 仍未完全退出主路径

**现状**

- facade 化和 trait 化已经开始
- 但旧存储边界、旧上下文模型仍然广泛存在于主路径

**问题**

- 这会让能力边界散落在业务实现里
- 影响工具 contract、store contract、测试可信度和后续演进速度

**目标**

- 主路径改为依赖 runtime/store facade 与窄上下文
- `PluginContext` 降级为兼容层
- `AppStorage` 不再被大量业务路径直接消费

---

## 四、能力模型专项

这些项决定系统上限，不应长期后拖。

### M1. Workspace-First 仍未真正完成

**现状**

- “连接本地目录”能力已经接上
- 前端也能展示当前授权目录

**但还没完成的点**

- 文件能力整体仍未完全从 `upload-first` 转为 `workspace-first`
- 仍有一部分能力、规则、习惯路径默认围绕 uploads/workspace 副本展开

**目标**

- “授权本地目录”成为一等工作对象
- 目录读取、搜索、分析、输出形成完整工作流
- 上传文件流程继续保留，但不再是文件能力默认中心

---

### M2. Atomic Tool 仍未完成

**现状**

- 工具体系已有 runtime 化进展
- 但默认工具面和复合工具语义问题仍未根治

**问题**

- 默认暴露面偏宽
- 大量工具依然承担多段复合动作
- 工具失败时局部恢复能力不足

**目标**

- 收缩默认工具面
- 基础工具保持原子语义
- 复杂流程上移到 skill / workflow / orchestration 层

---

### M3. Prompt Slimming 仍未完成

**现状**

- prompt 仍承担一部分本应由 runtime 负责的规则

**问题**

- 目录语义、工具顺序、数据传递、步骤推进等规则仍可能依赖 prompt
- 这会让 prompt 继续充当隐藏编排层

**目标**

- prompt 只保留角色、风格、少量行为边界
- 工具协议、目录规则、轮次规则、workflow 规则收回到 runtime / policy / schema / workflow 配置

---

### M4. Skill 本地导入 / 打包导入模型仍需统一

**现状**

- skill 体系还未形成统一的本地目录导入 / 本地包导入模型

**问题**

- 用户心智不统一
- UI、后端能力、导入路径之间仍有歧义

**目标**

- 统一 skill 本地安装模型
- 明确区分源码目录安装与打包文件安装

---

## 五、前后端未完全对齐项

这部分不是“前端完全没接”，而是“关键链路已接，但还停留在兼容层或未完全承接新 runtime 语义”。

### F1. 前端主要消费的仍是 legacy 兼容事件，不是完整 runtime state model

**现状**

- `task:status-changed`、`agent:idle`、workspace 授权状态等关键联调项已接上
- 这部分不能再按“前端未接入”处理

**剩余问题**

- 前端当前更多是在消费兼容事件，而不是一套完整的 runtime 状态真相源
- 一旦后端进一步收口 run / task / tool_call / permission state，前端模型还需要同步升级

**目标**

- 前端状态模型和 runtime 状态模型逐步对齐
- 不是只接 legacy 事件名，而是接 runtime 语义本身

---

### F2. 权限交互模型还没有完整前后端闭环

**现状**

- 后端权限能力在演进
- 但前端还没有形成完整的 allow / deny / ask / persisted policy 交互模型

**问题**

- 即使后端把 policy engine 做好了，前端若没有对应交互与状态展示，也无法形成完整产品能力

**目标**

- 建立统一的权限请求、权限记忆、权限撤销、权限状态可见性模型

---

### F3. 多会话 / 多任务 / 子 agent / 中断状态还没有被前端完全证明

**现状**

- 当前关键联调 happy path 已基本接通

**剩余问题**

- 复杂并发态下，前端 store 和 UI 是否与 run/task/tool_call 真相源完全一致，仍缺更系统的证明

**目标**

- 对多会话并行、任务嵌套、子 agent 完成、取消/重试等复杂路径补齐前后端联调验证

---

## 六、已不应继续作为主问题重复推进的项

这些项按当前代码状态看，已不应再作为“主要残留问题”继续推动。

- Claude provider 多轮 tool calling 损坏：大概率已修
- `authorized_workspace_store` 初始化顺序错误：大概率已修
- 前端完全没有消费 `task:status-changed`：已修
- child agent `agent:idle` 误清 parent conversation：已修
- “连接本地目录”只有后端没有前端：不成立，前端已接入基础链路

说明：这些项不代表永远没有回归风险，而是**不应再作为当前 backlog 的主叙事**。

---

## 七、建议执行顺序

后续如果继续派 Claude 改造，建议按这个顺序拆任务，不要一轮混做：

1. `D1 权限主路径闭合`
2. `D2 工具主路径去 legacy bridge`
3. `D3 取消传播统一`
4. `H1 chat ownership 回收`
5. `H3/H4 会话隔离 + 旧边界退出`
6. `M1 Workspace-First`
7. `M2 Atomic Tool`
8. `M3 Prompt Slimming`
9. `M4 Skill 导入模型统一`
10. `F1-F3 前端状态模型与联调补齐`

---

## 八、一句话结论

当前剩余改造点的核心，不是“再补几个功能”，而是：

> **把 runtime ownership、权限边界、工具边界、取消机制、会话隔离、前端状态模型这几条主线真正收成一套系统。**

只要这些边界没有闭合，就还不能说 lotus-app 已经达到 `claude-code-best` 的架构层级。

# 项目长期执行约束

## 基本要求

- 全程中文回答。
- 只在当前分支 `pzc` 开发，不创建 worktree，不切到其他分支。
- 优先使用 superpowers 相关 skill；按场景使用 `brainstorming`、`subagent-driven-development`、`test-driven-development`、`systematic-debugging`、`verification-before-completion` 等。
- 后台智能体按任务难度选择模型：机械型/小范围实现用快模型；跨模块集成/调试用标准模型；架构对标/审查用高能力模型。
- 只有在任务彼此独立、写集不冲突时才并行；并行不能以牺牲验证质量为代价。

## 总目标

- 按计划文件顺序持续执行：`Plan-U -> Plan-V -> Plan-AA -> Plan-AB -> Plan-AC -> Plan-AD -> Plan-AE -> Plan-W -> Plan-X -> Plan-Y -> Plan-Z -> Plan-AF`，直到全部执行完。
- 如果某个计划在执行过程中已经不合理，先直接修改对应计划文件，再继续执行修改后的计划。
- 不要随意跳过计划顺序；如果要调整执行顺序，必须先把计划本身改清楚。
- 中途不需要停下来征求确认；按 TDD 与验证流程连续推进。

## 架构对标

- 所有后端架构设计持续对标 `/Users/a20250311/github/claude-code-best`。
- 遇到设计决策不确定、控制流不清楚、取消/权限/subagent/工具迁移、prompt caching、thinking、settings layering 等问题时，优先去 `claude-code-best` 找对应实现再决定 lotus-app 的改法。
- 遇到计划本身不合理、实现边界不合理、或当前方案与 `claude-code-best` 架构明显背离时，先修改对应计划，再按调整后的方案继续执行，不要硬按错误设计推进。
- 若某项能力是 lotus 自定义扩展而不是对标仓库已有设计，必须在计划和实现说明里明确标注。

## 当前执行策略

- 当前仓库：`/Users/a20250311/IdeaProjects/lotus-app`
- 当前架构对标基线：`/Users/a20250311/github/claude-code-best`
- 当前主线目标：先完成对 `Plan-U` 到 `Plan-AF` 的执行，不再沿用旧的 `Plan-J -> Plan-T` 目标。
- 当前执行要求：持续使用 superpowers 相关 skill，在当前分支 `pzc` 连续推进；如发现计划边界与对标实现不一致，先修计划再落地实现。
- 当前并行策略：只并行无共享写集的任务；`Plan-Z/Z3` 与 `Plan-AF/AF1/AF2` 统一视为同一个 `Sidebar` 写集，必须合并处理。

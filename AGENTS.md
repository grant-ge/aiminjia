# 项目长期执行约束

## 基本要求

- 全程中文回答。
- 只在当前分支 `pzc` 开发，不创建 worktree，不切到其他分支。
- 优先使用 superpowers 相关 skill；按场景使用 `brainstorming`、`subagent-driven-development`、`test-driven-development`、`systematic-debugging`、`verification-before-completion` 等。

## 总目标

- 按计划文件顺序持续执行：`Plan-C -> Plan-D -> Plan-E -> Plan-F -> Plan-G -> Plan-H`，直到全部执行完。
- 如果某个计划在执行过程中已经不合理，可以先直接修改对应计划文件，再继续执行修改后的计划。
- 不要随意跳过计划顺序；如果要调整执行顺序，必须先把计划本身改清楚。

## 架构对标

- 所有架构设计持续对标 `/Users/a20250311/github/claude-code-best`。
- 遇到设计决策不确定、控制流不清楚、取消/权限/subagent/工具迁移等问题时，优先去 `claude-code-best` 找对应实现再决定 lotus-app 的改法。
- 遇到计划本身不合理、实现边界不合理、或当前方案与 `claude-code-best` 架构明显背离时，先修改对应计划，再按调整后的方案继续执行，不要硬按错误设计推进。

## 当前执行策略

- 当前仓库：`/Users/a20250311/IdeaProjects/lotus-app`
- 当前主线目标：严格按计划顺序推进，而不是按临时优先级插入其他 Plan。
- 当前已在执行：`Plan-C / Task 1 (write_file)`

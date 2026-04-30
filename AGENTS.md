# 项目长期执行约束

## 基本要求

- 全程中文回答。
- 全套使用 superpowers 相关能力：每次任务开始先检查适用 skill，并按场景组合使用 `brainstorming`、`subagent-driven-development`、`test-driven-development`、`systematic-debugging`、`verification-before-completion`、`requesting-code-review` 等。
- 优先拆解问题并多用子 agent 协作：探索、架构对标、实现、审查、验证等职责尽量拆成边界清晰的任务交给合适的子 agent。
- 子 agent 任务必须写清楚目标、上下文、写集、验收标准和禁止事项；多个子 agent 不能写同一批文件或覆盖彼此产出。
- 主 agent 负责统筹计划、分配任务、集成结果、处理冲突、复核子 agent 产出，并完成最终验证。
- 后台智能体按任务难度选择模型：机械型/小范围实现用快模型；跨模块集成/调试用标准模型；架构对标/审查用高能力模型。
- 只有在任务彼此独立、写集不冲突时才并行；涉及同一模块、同一路由、同一状态层或同一 UI 区域的任务必须合并处理或串行推进。

## 执行流程

- 先理解需求、现有代码和相关文档；复杂需求先用 `brainstorming` 或 `writing-plans` 明确方案与计划。
- 修复缺陷时优先走 TDD：先补测试或明确可复现验证，再实现代码；实现新功能时不强制先写 TDD，允许先按计划实现，完成后再补充必要测试和验证。
- 遇到测试失败、行为异常或根因不明的问题，先用 `systematic-debugging` 定位根因，不要凭猜测改代码。
- 完成前必须用 `verification-before-completion` 做验证；不能在没有证据的情况下声称完成、通过或修复。
- 验证 Rust 后端时避免默认使用 `cargo test <filter>` 这类会编译并启动全量 test binary 的耗时过滤方式；优先采用更可控的验证组合，例如 `cargo check`、小范围独立验证脚本、具体集成测试命令，只有必要时再运行完整或过滤后的 Cargo 测试。
- 需要审查重大实现、跨模块改动或子 agent 产出时，使用 `requesting-code-review` 或安排独立子 agent 审查。
- 如果计划或实现边界已经不合理，先修改对应计划/文档，再继续执行修改后的方案。

## 架构对标

- 所有后端架构设计持续对标 `/Users/a20250311/github/claude-code-best`。
- 遇到设计决策不确定、控制流不清楚、取消/权限/subagent/工具迁移、prompt caching、thinking、settings layering 等问题时，优先去 `claude-code-best` 找对应实现再决定 lotus-app 的改法。
- 遇到计划本身不合理、实现边界不合理、或当前方案与 `claude-code-best` 架构明显背离时，先修改对应计划，再按调整后的方案继续执行，不要硬按错误设计推进。
- 若某项能力是 lotus 自定义扩展而不是对标仓库已有设计，必须在计划和实现说明里明确标注。

## 当前执行策略

- 当前仓库：`/Users/a20250311/IdeaProjects/lotus-app`
- 当前架构对标基线：`/Users/a20250311/github/claude-code-best`
- 当前执行要求：持续使用完整 superpowers 工作流；如发现计划边界与对标实现不一致，先修计划再落地实现。
- 当前子 agent 策略：优先把可独立验证的问题交给子 agent 处理，并保持任务边界、写集、验证口径清晰。
- 当前并行策略：只并行无共享写集的任务；共享上下文或共享写集的任务必须合并处理或串行推进。

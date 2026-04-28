# sales-analysis migration notes

## 来源

- 旧目录：`src-tauri/plugins/sales-analysis`
- 已人工阅读：`plugin.toml`、`workflow.toml`、`prompts/base.md`、`prompts/step0.md`、`prompts/step1.md`、`prompts/step2.md`、`prompts/step3.md`、`scripts/knowledge/*`、`scripts/step*.py`
- 新入口：`docs/skills-migration/sales-analysis/SKILL.md`

## 迁移取舍

- 保留旧 skill 的角色定位、触发关键词、文件依赖、分析框架和四阶段业务流程。
- 将旧 workflow 的状态机步骤改写为无状态推荐流程，不再要求 runtime 维护步骤状态或自动推进。
- 将旧提示词中的 `[precompute_result]` 改写为“实际读取或计算得到的结果”，避免暗示系统会自动注入预计算结果。
- 旧保存类工具已移除；`SKILL.md` 改为在当前回复内复述阶段摘要、关键假设和待确认事项，并请用户确认后进入下一阶段。
- `model: opus`、`effort: high`、`context: inline` 按 Task 2 新规范和旧 `deep_reasoning` 偏好迁移。

## 保留资源

### references/knowledge

- `references/knowledge/benchmarks.json`：销售和转化相关基准
- `references/knowledge/playbooks.json`：增长、客户经营和销售改进动作

## 旧机制处理

- 未随包保留旧版计算资产；其中有价值的识别与计算口径已转写到 `SKILL.md` 分析流程，复制后不依赖额外运行环境。

## 人工复核点

- 已确认 `SKILL.md` frontmatter 中 `name` 等于目录名，`metadata.label` 为中文显示名，`context: inline`，`user-invocable: true`，`disable-model-invocation: false`，`version: "1.0"`。
- 已确认正文是 standalone 指南，没有把 `plugin.toml` / `workflow.toml` 描述为运行机制。
- 已确认历史计算口径已转写到 `SKILL.md` 的分析流程与质量检查。
- 已确认知识库 JSON 与旧 `scripts/knowledge/` 内容逐字节一致。

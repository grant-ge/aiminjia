# engagement-survey migration notes

## 来源

- 旧入口：`src-tauri/plugins/engagement-survey/plugin.toml`
- 旧流程：`src-tauri/plugins/engagement-survey/workflow.toml`
- 旧提示词：`src-tauri/plugins/engagement-survey/prompts/base.md`、`step0.md`、`step1.md`、`step2.md`、`step3.md`、`step4.md`
- 旧知识库：`src-tauri/plugins/engagement-survey/scripts/knowledge/benchmarks.json`、`rules.json`

## 迁移取舍

- 将旧的“数据识别 -> 整体指标 -> 分组差异 -> 根因诊断 -> 行动计划报告”改写为 `SKILL.md` 的无状态数据分析指南。
- 保留 eNPS 计算、维度得分、分组差异、驱动因素和行动计划框架。
- 强化数据口径、缺失值、小样本、匿名化、相关不等于因果等分析风险提示。
- 删除旧 workflow 中系统自动完成计算或自动重新执行分析的表述；改为由模型明确读取文件、计算、复核并转述。

## 保留资源

- `references/knowledge/benchmarks.json`：从旧 `scripts/knowledge/benchmarks.json` 原样迁入。
- `references/knowledge/rules.json`：从旧 `scripts/knowledge/rules.json` 原样迁入。

## 人工复核点

- 已确认 `SKILL.md` frontmatter 的 `name` 等于目录名，`when_to_use` 明确说明通常需要文件或结构化数据。
- 已确认正文不再声称系统会自动注入计算结果或自动执行脚本。
- 已确认旧预计算脚本未随包保留，eNPS、维度、分组差异和驱动因素等关键口径已转写到 `SKILL.md` 流程，避免依赖旧运行时变量。

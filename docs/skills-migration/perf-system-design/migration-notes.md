# perf-system-design migration notes

## 来源

- 旧入口：`src-tauri/plugins/perf-system-design/plugin.toml`
- 旧流程：`src-tauri/plugins/perf-system-design/workflow.toml`
- 旧提示词：`src-tauri/plugins/perf-system-design/prompts/base.md`、`step0.md`、`step1.md`、`step2.md`、`step3.md`、`step4.md`
- 旧知识库：`src-tauri/plugins/perf-system-design/scripts/knowledge/frameworks.json`、`templates.json`

## 迁移取舍

- 将旧的“企业画像收集 -> 绩效工具推荐 -> 体系结构设计 -> KPI/OKR 模板定制 -> 实施路线图与报告”改写为 `SKILL.md` 的无状态设计指南。
- 保留 KPI、OKR、BSC、MBO、360 等工具选择逻辑，以及分层方案、考核流程、角色职责和路线图要求。
- 强化薪酬挂钩、强制分布、末位淘汰、申诉机制等高风险设计提示。
- 删除旧 workflow 的自动知识库加载和运行结果注入机制；改为按需读取参考资料，并把绩效工具推荐逻辑写入正文流程。

## 保留资源

- `references/knowledge/frameworks.json`：从旧 `scripts/knowledge/frameworks.json` 原样迁入。
- `references/knowledge/templates.json`：从旧 `scripts/knowledge/templates.json` 原样迁入。

## 人工复核点

- 已确认 `SKILL.md` frontmatter 的 `name` 等于目录名，`metadata.label` 为“绩效体系设计向导”。
- 已确认正文不依赖旧 stateful workflow，且每阶段都要求用户确认。
- 已确认旧预计算脚本未随包保留，关键推荐口径已转写到 `SKILL.md` 流程，避免依赖旧运行时变量。

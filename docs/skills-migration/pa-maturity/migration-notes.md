# pa-maturity migration notes

## 来源

- 旧入口：`src-tauri/plugins/pa-maturity/plugin.toml`
- 旧流程：`src-tauri/plugins/pa-maturity/workflow.toml`
- 旧提示词：`src-tauri/plugins/pa-maturity/prompts/base.md`、`step0.md`、`step1.md`、`step2.md`、`step3.md`、`step4.md`
- 旧知识库：`src-tauri/plugins/pa-maturity/scripts/knowledge/maturity_model.json`、`toolstack.json`

## 迁移取舍

- 将旧的“基本信息收集 -> 16 题自评 -> 成熟度定级 -> 差距分析 -> 升级路线图与报告”改写为 `SKILL.md` 的无状态评估指南。
- 保留 L1-L4 People Analytics 成熟度模型、四大维度 16 题问卷、24 个月路线图结构。
- 删除旧 workflow 的自动步骤推进、自动知识库加载和运行结果注入依赖；改为按需读取 `references/knowledge`，并把 16 题评分、维度均分和 L1-L4 定级口径写入正文流程。
- `allowed-tools` 保留信息记录、分析记录、外部最佳实践检索、报告和 PPT 生成工具。

## 保留资源

- `references/knowledge/maturity_model.json`：从旧 `scripts/knowledge/maturity_model.json` 原样迁入。
- `references/knowledge/toolstack.json`：从旧 `scripts/knowledge/toolstack.json` 原样迁入。

## 人工复核点

- 已确认 `SKILL.md` frontmatter 的 `name` 等于目录名，触发描述覆盖中英文 People Analytics/HR analytics 场景。
- 已确认正文强调不得替用户打分，需先收集事实再评估。
- 已确认旧预计算脚本未随包保留，关键评分和定级口径已转写到 `SKILL.md` 流程，避免依赖旧运行时变量。

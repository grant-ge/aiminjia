# org-diagnosis migration notes

## 来源

- 旧入口：`src-tauri/plugins/org-diagnosis/plugin.toml`
- 旧流程：`src-tauri/plugins/org-diagnosis/workflow.toml`
- 旧提示词：`src-tauri/plugins/org-diagnosis/prompts/base.md`、`step0.md`、`step1.md`、`step2.md`、`step3.md`、`step4.md`
- 旧知识库：`src-tauri/plugins/org-diagnosis/scripts/knowledge/frameworks.json`、`interventions.json`

## 迁移取舍

- 将旧的“症状收集 -> 诊断方法推荐 -> 根因分析 -> 方案设计 -> 报告生成”流程改写为 `SKILL.md` 中的无状态操作指南。
- 保留六盒子、7S、Burke-Litwin 的框架选择逻辑和红黄绿/1-5 分诊断口径。
- 删除旧 workflow 的自动推进、步骤状态和运行结果注入等机制表述；正文只说明可按需读取资源，并把组织症状摘要、框架选择和评分口径写入操作流程。
- `allowed-tools` 保留旧流程实际需要的记忆、分析记录、搜索、报告和 PPT 工具。

## 保留资源

- `references/knowledge/frameworks.json`：从旧 `scripts/knowledge/frameworks.json` 原样迁入。
- `references/knowledge/interventions.json`：从旧 `scripts/knowledge/interventions.json` 原样迁入。

## 人工复核点

- 已确认 `SKILL.md` frontmatter 的 `name` 等于目录名，`metadata.label` 为中文显示名，`context` 为 `inline`。
- 已确认正文不把 `plugin.toml`、`workflow.toml` 或自动计算结果注入当作运行机制。
- 已确认旧预计算脚本未随包保留，关键口径已转写到 `SKILL.md` 流程，避免依赖旧运行时变量。

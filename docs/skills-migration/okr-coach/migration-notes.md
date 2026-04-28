# okr-coach migration notes

## 来源

- 旧目录：`src-tauri/plugins/okr-coach`
- 已人工复核：`plugin.toml`、`workflow.toml`、`prompts/base.md`、`prompts/step0.md`、`prompts/step1.md`、`prompts/step2.md`、`scripts/knowledge/*.json`、`scripts/step0.py`
- 读取方式：当前工作树旧目录已清理，旧源以 `git show HEAD:src-tauri/plugins/okr-coach/...` 复核。

## 迁移取舍

- 将旧“背景与目标确认 -> OKR 初稿制定 -> 对齐检查与优化”改写为无状态 OKR 教练指南。
- 保留 Objective / Key Result 原则、SMART 检验、常见错误、对齐检查和评分理念。
- 删除正文中旧预计算占位符、自动 step 跳转、`plugin.toml` / `workflow.toml` 运行机制描述。
- 保留 OKR 原则、案例库、指标库 JSON 作为按需读取参考。
- 未随包保留旧预计算脚本；其层级/职能识别、指标推荐和质量评分口径已转写到 SKILL.md 流程，避免复制后依赖旧运行时变量。

## 保留资源

- `references/knowledge/okr_principles.json`
- `references/knowledge/okr_library.json`
- `references/knowledge/metrics_library.json`

## 人工复核点

- frontmatter `name` 与目录名一致，`metadata.label` 为中文显示名。
- `allowed-tools` 对应旧流程中读取上级 OKR、记录背景、导出、生成报告和幻灯片的需求。
- SKILL.md 正文可 standalone 使用，不依赖旧运行时状态。
- 旧路径只在本迁移说明中作为历史来源出现。

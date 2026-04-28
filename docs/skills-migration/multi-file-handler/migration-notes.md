# multi-file-handler migration notes

## 来源

- 旧目录：`src-tauri/plugins/multi-file-handler`
- 已人工复核：`plugin.toml`、`workflow.toml`、`prompts/base.md`、`prompts/step0-intent.md`、`prompts/step1-load.md`、`prompts/step2-compare.md`、`prompts/step2-merge.md`、`prompts/step2-batch_translate.md`、`prompts/step2-cross_ref.md`、`prompts/step2-summarize.md`、`prompts/step3-report.md`
- 读取方式：当前工作树旧目录已清理，旧源以 `git show HEAD:src-tauri/plugins/multi-file-handler/...` 复核。

## 迁移取舍

- 将旧四步动态 workflow 改写为无状态指南：意图分流、schema 对齐、按模式执行、生成交付物。
- 保留五种处理模式：`compare`、`merge`、`batch_translate`、`cross_ref`、`summarize_all`。
- 删除旧 `prompt_router`、自动读取 `step0_intent.mode` 选择分支等运行机制描述；现在由模型按用户确认的模式执行。
- 旧技能无 `scripts/knowledge` 和可直接迁移的预计算脚本，因此未创建 references/knowledge 或 scripts 资源；模式识别和 schema 判断已写入 SKILL.md 流程。

## 保留资源

- 无知识库 JSON。
- 无需随包保留预计算脚本。

## 人工复核点

- frontmatter `name` 与目录名一致，`metadata.label` 为中文显示名。
- `allowed-tools` 覆盖旧流程实际用到的读取、记录、Python 计算、数据导出、图表、报告和幻灯片生成。
- SKILL.md 正文保留 strict intent JSON 作为推荐记录格式，但不宣称旧 workflow 会自动路由。
- 旧路径只在本迁移说明中作为历史来源出现。

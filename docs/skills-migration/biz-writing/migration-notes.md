# biz-writing migration notes

## 来源

- 旧目录：`src-tauri/plugins/biz-writing`
- 已人工复核：`plugin.toml`、`workflow.toml`、`prompts/base.md`、`prompts/step0.md`、`prompts/step1.md`、`prompts/step2.md`、`scripts/knowledge/*.json`、`scripts/step0.py`
- 读取方式：当前工作树旧目录已清理，旧源以 `git show HEAD:src-tauri/plugins/biz-writing/...` 复核。

## 迁移取舍

- 将旧三步流程改写为无状态 SKILL.md 指南：明确任务、选择结构、输出初稿、修改定稿。
- 删除正文中旧预计算占位符、自动 step 跳转、`plugin.toml` / `workflow.toml` 运行机制描述。
- 保留旧知识库作为可选参考资料，不要求自动加载。
- 未随包保留旧预计算脚本；其文档类型识别、模板匹配和质量判断口径已转写到 SKILL.md 流程，避免复制后依赖旧运行时变量。

## 保留资源

- `references/knowledge/doc_types.json`
- `references/knowledge/templates.json`
- `references/knowledge/writing_rules.json`

## 人工复核点

- frontmatter `name` 与目录名一致，`metadata.label` 为中文显示名。
- `allowed-tools` 只保留旧流程实际需要的读取、记录、导出、报告和幻灯片工具。
- SKILL.md 正文可 standalone 使用，不依赖旧运行时状态。
- 旧路径只在本迁移说明中作为历史来源出现。

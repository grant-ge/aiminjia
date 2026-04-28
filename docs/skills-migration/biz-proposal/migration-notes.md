# biz-proposal migration notes

## 来源

- 旧目录：`src-tauri/plugins/biz-proposal`
- 已人工复核：`plugin.toml`、`workflow.toml`、`prompts/base.md`、`prompts/step0.md`、`prompts/step1.md`、`prompts/step2.md`、`scripts/knowledge/*.json`、`scripts/step0.py`
- 读取方式：当前工作树旧目录已清理，旧源以 `git show HEAD:src-tauri/plugins/biz-proposal/...` 复核。

## 迁移取舍

- 将旧“需求确认与框架 -> 方案主体撰写 -> 完善与定稿”改写为无状态方案撰写指南。
- 保留 SCQA、MECE、金字塔、执行摘要、风险/预算/里程碑等核心方法，但改成可直接执行的操作说明。
- 删除正文中旧预计算占位符、自动 step 跳转、`plugin.toml` / `workflow.toml` 运行机制描述。
- 保留知识库 JSON 作为按需读取的参考资料。
- 未随包保留旧预计算脚本；其方案类型识别、章节结构匹配和材料质量判断口径已转写到 SKILL.md 流程，避免复制后依赖旧运行时变量。

## 保留资源

- `references/knowledge/proposal_types.json`
- `references/knowledge/structures.json`
- `references/knowledge/writing_rules.json`

## 人工复核点

- frontmatter `name` 与目录名一致，`metadata.label` 为中文显示名。
- `allowed-tools` 对应旧流程中参考材料读取、需求记录、导出、报告和幻灯片生成。
- SKILL.md 正文不把旧 workflow 当作运行机制。
- 旧路径只在本迁移说明中作为历史来源出现。

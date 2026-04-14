# Step 1：生成 plugin.toml

> ⚠️ M2 骨架阶段：此 prompt 为占位。

## 本步目标（M3 真实实施）

基于 step0 收集的意图，生成 `plugin.toml` 文件，调用：
- `write_skill_draft_file(draftId, "plugin.toml", <TOML 内容>)`
- `validate_skill_draft(draftId)` 验证结构
- 若 error → 根据 `fix_hint` 修复后重写

## 字段生成指南

- `plugin.id`：从用户场景推断，例如"离职访谈分析" → `exit-interview-analysis`
  - 小写字母开头，3-40 字符，字母/数字/连字符
  - 不与内置 22 个技能冲突（查全局索引 L1）
- `plugin.name`：中文名称，2-40 字符
- `trigger.keywords`：从用户描述里提取 3-20 个，中英文混合
- `display.category`：按业务领域分类（hr/finance/legal/sales/ops/general）
- `display.icon`：选一个贴合场景的 emoji
- `model.preference`：涉及分析推理用 `deep_reasoning`，纯对话用 `balanced`

## 输出预览

生成后向用户展示 plugin.toml 关键字段（折叠 TOML 语法，只显示字段含义），
询问"有什么要调整的吗？"。用户确认后进入 step2。

## M2 骨架期的行为

简单回应"plugin.toml 生成模拟完成，点击下一步"。

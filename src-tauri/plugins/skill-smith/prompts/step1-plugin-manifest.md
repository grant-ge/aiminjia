=== 当前任务：Step 1 — 生成 plugin.toml ===

基于 step0 收集的意图（查看 save_analysis_note 中 key=skill_intent 的内容），生成技能的基本信息配置文件。

## 执行流程

1. 根据用户需求构造 plugin.toml 内容
2. 调用 `skill_smith_write_file(relative_path="plugin.toml", content=<TOML内容>)`
3. 调用 `skill_smith_validate()` 验证格式
4. 如果有 error → 根据 fix_hint 修改内容 → 重新 write_file → 再次 validate
5. 验证通过后，向用户展示关键配置并确认

## plugin.toml 模板

```toml
[plugin]
id = "技能ID"
name = "技能名称"
type = "skill"
description = "技能的详细描述"
priority = 20

[trigger]
keywords = ["关键词1", "关键词2", "关键词3"]
requires_files = false

[model]
preference = "deep_reasoning"

[prompts]
include_app_base = true

[defaults]
max_iterations = 5
token_budget = 8192

[display]
category = "general"
icon = "🎯"
short_description = "一句话描述"
short_description_en = "One line description in English"
trigger_text = "我想做XX分析"
name_en = "English Name"
```

## 字段生成规则

- `plugin.id`：从场景推断，如"离职分析" → `exit-interview-analysis`。小写字母开头，3-40字符，只含字母/数字/连字符
- `plugin.name`：中文名，2-40 字符
- `trigger.keywords`：3-20 个，中英文混合，从用户描述提取
- `requires_files`：需要上传文件则 true，纯对话则 false
- `display.category`：hr / finance / legal / sales / ops / general
- `display.icon`：选一个贴合场景的 emoji
- `model.preference`：涉及数据分析/推理用 `deep_reasoning`，纯对话用 `balanced`，简单查询用 `fast`

## 向用户展示（不展示 TOML 语法）

- 技能名称：XX
- 触发词：XX、XX、XX
- 分类：XX
- 图标：XX
- 是否需要上传文件：是/否

"以上配置是否需要调整？确认后我会设计工作流程。"

⚠️ validate 未通过前不要告诉用户"已完成"。
⚠️ 不要向用户展示 TOML 原文。

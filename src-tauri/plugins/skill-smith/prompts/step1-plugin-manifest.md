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

## 内置技能参考（few-shot）

以下是 AI小家 已有的内置技能配置，用作结构和风格参考：

**示例 1：对话咨询类（OKR 辅导）**
```toml
[plugin]
id = "okr-coach"
name = "OKR 制定辅导"
type = "skill"
description = "OKR coaching: objective setting, key results design, alignment check"
priority = 20

[trigger]
keywords = [
    "OKR", "目标制定", "关键结果", "目标管理",
    "OKR辅导", "目标拆解", "KPI", "目标对齐",
    "OKR coaching", "objective setting", "key results",
    "goal setting", "KPI design",
]
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
short_description = "目标制定、KR 设计、对齐检查"
short_description_en = "Guide OKR setting with SMART criteria and alignment check"
trigger_text = "帮我制定 OKR"
name_en = "OKR Goal Coaching"
```

**示例 2：数据分析类（薪酬分析）**
```toml
[plugin]
id = "comp-analysis-v2"
name = "薪酬公平性分析 v2"
type = "skill"
description = "Precompute-based compensation equity analysis workflow"
priority = 20

[trigger]
keywords = [
    "薪酬分析", "薪酬诊断", "公平性分析", "薪酬公平",
    "薪资分析", "薪资诊断", "薪酬对标", "薪酬体系分析",
    "compensation analysis", "pay equity", "salary analysis",
]
requires_files = true

[model]
preference = "deep_reasoning"

[prompts]
include_app_base = true

[defaults]
max_iterations = 5
token_budget = 8192

[display]
category = "hr"
icon = "💰"
short_description = "薪酬公平性诊断、离群值识别、调薪建议"
short_description_en = "Compensation equity diagnosis, outlier detection, and salary adjustment recommendations"
trigger_text = "帮我做薪酬公平性分析"
name_en = "Compensation Equity Analysis"
```

**示例 3：写作类（商务文档）**
```toml
[plugin]
id = "biz-writing"
name = "商务文档撰写"
type = "skill"
description = "Business writing: emails, reports, memos, presentations"
priority = 20

[trigger]
keywords = [
    "商务写作", "商务邮件", "工作报告", "会议纪要",
    "汇报材料", "总结报告", "商务文档", "PPT大纲",
    "business writing", "business email", "report writing",
]
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
icon = "✍️"
short_description = "邮件、报告、纪要、PPT 大纲"
short_description_en = "Write emails, meeting summaries, reports, and other business documents"
trigger_text = "帮我写商务文档"
name_en = "Business Document Writing"
```

**已有技能 ID 列表（不可重复）**：
comp-analysis-v2, engagement-survey, talent-9box, recruitment-funnel, salary-benchmarking, org-diagnosis, perf-system-design, pa-maturity, okr-coach, budget-analysis, finance-analysis, contract-review, labor-compliance, policy-compliance-audit, sales-analysis, customer-segmentation, ops-analysis, biz-proposal, biz-writing, survey-analysis, user-behavior, skill-smith, multi-file-handler

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

"以上配置是否需要调整？没问题的话请说「继续」，我会为你设计工作流程。"

## 如果用户要求修改

用户可能说"换个图标"、"关键词加上XX"、"分类改成 finance"等。处理方式：
1. 调用 `skill_smith_read_file(relative_path="plugin.toml")` 读取当前内容
2. 仅修改用户要求的字段，保持其余不变
3. 调用 `skill_smith_write_file` 写回修改后的完整内容
4. 调用 `skill_smith_validate()` 确认修改合法
5. 向用户展示修改后的配置摘要

不要因为一个字段的修改而重新生成整个文件。

⚠️ validate 未通过前不要告诉用户"已完成"。
⚠️ 不要向用户展示 TOML 原文。

# Step 0: 需求确认与框架

[precompute_result] 已加载商务方案知识库，包含：方案类型定义（proposal_types，含用途/页数/受众/核心章节）、详细章节结构（structures，含每章要点清单）、写作规则（writing_rules，含 SCQA/MECE/金字塔/数据论证）。根据用户选择的方案类型，从 proposal_types 匹配推荐章节，从 structures 获取每章要点作为大纲基础；撰写时遵循 writing_rules 中的方法论。

1. 了解方案背景：解决什么问题？给谁看？
2. 确认方案类型：项目立项/解决方案/商业计划/投标方案
3. 确认关键信息：预算范围、时间要求、决策者关注点
4. 如有参考资料，使用 `load_file` 加载
5. 输出方案大纲框架，征求用户意见
6. 使用 `save_analysis_note` 记录需求

确认后进入下一步。

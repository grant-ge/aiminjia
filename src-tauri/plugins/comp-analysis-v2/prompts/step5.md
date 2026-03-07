=== Step 5 — 行动方案与报告 ===

系统已自动完成调薪方案计算，结果在 [precompute_result] 中。

你的任务：
1. 用简洁的中文向用户展示调薪方案：
   - 方案概述（保守/平衡/激进等场景对比）
   - 各方案的预算影响（总成本、人均调幅）
   - 受影响人员的分布
2. 告知用户已导出 step5_scenarios.xlsx（方案对比明细）
3. 调用 generate_report 生成完整的薪酬诊断报告
   - 报告应包含所有步骤的关键发现和建议
   - 使用 [precompute_result] 中的 report_sections 作为报告各节内容
4. 询问用户：
   - "方案看着合理吗？"
   - "需要调整哪个方案的参数？"
   - "报告内容有需要修改的吗？"

你可以使用 generate_report 和 export_data 工具。

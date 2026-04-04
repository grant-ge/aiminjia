=== Step 2 — 滚动预测与风险评估 ===

系统已自动完成滚动预测，结果在 [precompute_result] 中：
- `actual_to_date`：已实现累计金额
- `periods_realized`：已实现期间数
- `full_year_estimate`：全年预测金额
- `achievement_rate`：预测达成率（%）
- `gap_vs_budget`：预测 vs 预算的缺口
- `risk_flags`：自动识别的风险项

你的任务：
1. **全年预测展示** — 以表格形式展示：已实现 + 预测 + 全年预算 + 预测达成率
2. **风险评估** — 展示 risk_flags，并判断严重程度
3. **敏感性分析** — 用 `execute_python` 做情景分析：如收入下降10%/成本增加10% 对利润的影响
4. **行动建议** — 对预测超支的科目，提出需要立即关注的行动项

用 `export_data` 导出全年预测表（`step2_forecast.xlsx`）。

展示预测结果后等待用户确认，告知下一步将生成完整管控建议报告。

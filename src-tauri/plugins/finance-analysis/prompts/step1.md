=== Step 1 — 收入成本结构分析 ===

系统已自动完成初步计算，结果在 [precompute_result] 中：
- `gross_margin_trend`：各期毛利率趋势
- `anomalies`：自动检测到的异常变化
- `summary`：识别到的科目列表

你的任务：
1. **收入构成** — 按产品线/业务线拆分收入占比（用 `execute_python` 做更细粒度分析）
2. **毛利率趋势** — 基于 precompute_result 展示趋势，标注拐点
3. **费用率分析** — 销售/管理/研发费用率的变化趋势
4. **异常标注** — 对 anomalies 中的项目追溯原因

用 `export_data` 导出收入成本结构表（`step1_income_cost.xlsx`）。

展示关键发现（3-5条）后等待用户确认，告知下一步将进行财务比率计算和杜邦分解。

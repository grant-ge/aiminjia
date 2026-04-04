# Step 1: 描述统计与分布

系统已自动完成描述统计计算，结果在 [precompute_result] 中。

**如果 [precompute_result] 有 `question_stats`：**
- 展示各题目统计结果（频率分布、均值、Top2Box等）
- 如有 `nps_result`，展示NPS得分及分布
- 对标行业基准（如有 `industry_benchmarks`）

**降级情况（有 `error` 字段）：**
- 使用 `execute_python` 手动计算

1. **各题目回答分布** — 频次、占比、均值（量表题）
2. **整体满意度/NPS** — 如果是满意度调查
3. **各维度得分排名** — 哪些维度得分最高/最低
4. **异常检测** — 极端值、双峰分布等

展示结果后等待用户确认。

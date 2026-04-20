# Step 2：执行对比分析（mode=compare）

## 目标

基于 step1 的 schema 对齐结果，计算两文件在各维度上的**差异**。产出差异明细表 + 关键指标对比图。

## 执行要点

### 1. 从前面的 note 读上下文

- `step0_intent`：确认 mode=compare，primary_key，dimensions
- `step1_schema`：知道哪些字段是共有的、哪些是单边的

### 2. 三类差异分别处理

#### A. 两边都有的记录（按 primary_key 匹配）—— 字段级差异

对每个 common_field 计算：
- 变化了多少条：`(A[field] != B[field]).sum()`
- 平均变化幅度（数值字段）：`(B[field] - A[field]).mean()`
- 分布变化（分类字段）：各类别计数变化

### B. A 独有记录

列出在 A 但不在 B 的 primary_key 对应行，给个 summary：
- 数量
- 代表性示例（5 条）

### C. B 独有记录

同上。

### 3. 产出差异明细 Excel

用 `export_data` 导出：
- Sheet 1: "字段级差异" —— primary_key + 每个 common_field 的 A/B/差异值
- Sheet 2: "A 独有" —— 仅 A 有的完整记录
- Sheet 3: "B 独有" —— 仅 B 有的完整记录

### 4. 关键指标图（选做，如有数值字段）

对每个数值 dimension 画分布对比图（直方图 / 箱线图）：

```python
# generate_chart with format=html + plotly
```

### 5. chat 总结

给用户一个简洁的 Markdown 摘要，不要全文铺陈：

```
## 对比完成

- 匹配记录：1,083 条
  - 发生变化的记录：X 条
  - 主要变化维度：基本工资（平均 +5.2%）、部门（12 人转岗）
- A 独有：12 条（离职或调出）
- B 独有：120 条（新入职或调入）

📁 已导出：
- 差异明细.xlsx（3 sheets）
- 基本工资分布对比.html

继续生成完整 HTML 对比报告？
```

## 陷阱

- **primary_key 重复**：如果在 A 或 B 里 primary_key 不唯一，先 dedup 并告知用户
- **数据类型不一致**：比如工号 A 是字符串 "001"，B 是数值 1 —— 对齐时注意转换
- **缺失值**：NaN vs 空字符串对比会混乱，先 `fillna("")` 统一处理
- **日期字段**：2025-01-01 vs 2025/1/1 需要 parse 成同一格式再比较

## 用户 confirm 后进 step3

询问"对比结果满意吗？继续生成报告 / 要调整维度可以说。"

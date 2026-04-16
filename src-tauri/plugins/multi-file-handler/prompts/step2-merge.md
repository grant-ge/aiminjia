# Step 2：执行合并（mode=merge）

## 目标

把 2 个以上文件合并成一份整体，做整体统计。区分"**追加式**合并"（纵向 union，schema 必须一致）和"**关联式**合并"（横向 join，用 primary_key）。

## 执行要点

### 1. 确定合并方向

从 step0_intent.dimensions 或用户原话推断：
- "把各渠道销售合起来看总量" → 纵向 union
- "员工表和薪资表合到一起看每人完整信息" → 横向 join

如果歧义，问用户。

### 2A. 纵向 union

```python
# 确保 schema 一致
common_cols = list(set(df_a.columns) & set(df_b.columns))
merged = pd.concat([df_a[common_cols], df_b[common_cols]], ignore_index=True)
# 加"来源"列标注每行来自哪个文件
merged['_source'] = ['A'] * len(df_a) + ['B'] * len(df_b)
```

**独有字段处理**：默认丢弃；如用户想保留，设 NaN 填充。

### 2B. 横向 join

```python
merged = df_a.merge(df_b, on=primary_key, how='outer',  # 或 inner, left, right
                    suffixes=('_a', '_b'))
```

four join 模式按需选：
- `inner`：两边都有的
- `outer`：全部（NaN 填充缺失）
- `left`：以 A 为准
- `right`：以 B 为准

默认 `outer` + 报告各 join 形态的命中数。

### 3. 对合并结果做基本统计

- 总行数、独立 primary_key 数
- 各数值字段：均值/中位数/分位数
- 各分类字段：top 值分布
- 数据完整性：缺失率

### 4. 导出合并结果

```python
# 用 export_data 导出合并表 Excel
# Sheet 1: 合并主表
# Sheet 2: 统计摘要
```

### 5. chat 总结

```
## 合并完成

- 合并策略：纵向 union（两份销售数据堆叠）
- 合并后：3,205 行，20 列
- 关键指标：
  - 总销售额：¥1,234,567（月增长 12%）
  - 客户数：890（新增 120）

📁 已导出：merged_data.xlsx

继续生成合并分析报告？
```

## 陷阱

- **Schema 不一致**：纵向合并时如果列顺序/类型不一致，pandas 会 NaN 填充。提前标注给用户看
- **Primary_key 冲突**：两边有同一 id 但内容不同 —— 用户可能要 dedup 或保留两份，问清楚
- **超大合并**：百万行量级用 `pd.concat` 占内存，必要时分批写盘

# Step 2：交叉引用 / lookup（mode=cross_ref）

## 目标

用一份主表（A）里的字段到另一份查询表（B）里查数据，补齐 A 的信息。典型是"员工表 + 薪资表 join"、"订单 + 客户 VLOOKUP"、"商品 + 库存对表"。

## 执行要点

### 1. 从 intent 读主表和查询键

- step0_intent.primary_key 是 join 键（A.key = B.key）
- files[0] 一般是主表 A，files[1+] 是查询表

### 2. 查询并合并

```python
# 典型 left join：以 A 为准，用 B 里的数据补齐
result = df_a.merge(df_b, on=primary_key, how='left',
                    suffixes=('_a', '_b'))

# 统计 lookup 结果
matched = result[result['_merge'] == 'both'].shape[0]
unmatched = result[result['_merge'] == 'left_only'].shape[0]
```

### 3. 报告 lookup 质量

- A 有多少行在 B 里找到了匹配
- 有多少行在 B 里**没找到**（用户可能没想到会漏） —— 这常常是数据 bug 的信号
- B 里有多少行 A 里用不上（孤儿数据）

### 4. 按用户目标生成交付物

如果 A 是员工表，B 是薪资表，用户想要"每人工资明细"：
- 在 A 里追加 B 的工资字段
- 按部门/职级分组统计
- 导出完整员工薪资明细表

### 5. chat 总结

```
## 交叉引用完成

- 主表 A（员工花名册）：1,095 人
- 查询表 B（12 月薪资表）：1,087 条
- 命中：1,081 条（A 中 14 人在 B 里找不到薪资，可能离职或系统漏录）
- B 有但 A 里没对应员工：6 条（可能是外包或临时工）

📁 已导出：employee_with_salary.xlsx（含 `_match_status` 列标注命中情况）

⚠️ 建议人工核查 14 名缺失记录（工号已列在报告 appendix）

继续生成详细报告？
```

## 陷阱

- **主键类型不一致**（"001" vs 1 vs 1.0）：strict compare 就 miss，必须先统一 dtype
- **多对多**：A 里一个 key 对应 B 里多条（如一人多个 salary 记录）—— 需明确要取最新 / 所有 / 聚合
- **大小写 / 空格**：主键字符串带 trailing space 会 miss，`.str.strip().str.upper()` 归一化
- **性能**：10 万+ 的 join 用 pandas 会慢，考虑用 indexed merge 或 polars

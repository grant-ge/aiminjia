=== 当前任务：Step 1 — 数据清洗与理解 ===

目标：接收你上传的工资表，完成数据摄入、字段识别、数据清洗和质量评估。

文件使用规则：
- 文件已通过 load_file 加载，在 execute_python 中直接使用 _df 即可
- 如果 _df 不可用，先调用 load_file(file_id) 加载数据
- 在对话中提到文件时使用 originalName（如"你上传的 工资表.xlsx"）

内置分析函数说明：
系统已预注入以下函数，可直接调用（无需自己编写）：
- `_detect_columns(df)` — 自动检测列名语义（姓名/部门/薪资等），返回 `{detected: {}, salary_components: {}, undetected: []}`
- `_normalize_salary(series)` — 处理万/K/货币符号 → float
- `_step1_clean(df, col_map=None)` — 完整的 Step 1 清洗流程，返回结构化结果
- `_print_table(headers, rows, title)` — 输出 Markdown 格式表格
- `_export_detail(df, filename, title, preview_rows, format)` — 导出 DataFrame 到 Excel/CSV 并打印预览

执行步骤（尽量合并执行，减少工具调用次数）：

第一步：数据概览 + 列名检测（合并为一次 execute_python）
  调用 execute_python：使用 _df 查看数据形状、列名、前5行样本，同时调用 _detect_columns(_df) 完成字段映射
  → 输出数据概览 + 字段映射结果

第二步：一键清洗 + 导出排除明细（一次 execute_python）
  调用 execute_python：
  ```python
  results = _step1_clean(_df, col_map)
  # 结果已自动打印和缓存。_df 已自动更新为清洗后的数据。
  if len(results['excluded_df']) > 0:
      _export_detail(results['excluded_df'], 'step1_exclusion_detail', '排除人员明细')
  ```

排除人员展示规则（必须严格遵守）：
- 排除名单必须从 Python 代码的 DataFrame 筛选结果中直接输出
- 使用 _export_detail 导出完整名单到 Excel，消息中仅展示前 15 条预览
- 预览内容直接来自 Python stdout（_print_table 输出），不要另行重新列举
- 脱敏后你看到的是占位符（如 [PERSON_1]），不要试图还原或猜测真实姓名
- 严禁在消息中手动编写人员名单，所有名单必须是代码执行结果的直接引用

第三步：保存并汇总
  ⚠️ 必须调用 save_analysis_note：
  key: "step1_summary"
  content: 必须包括：分析人数、排除人数及原因分布、字段映射表（关键字段→语义）、薪酬结构（固定/浮动比例）、数据质量问题
  调用 update_progress 标记步骤完成
  → 输出完整汇总报告

重要：每一步都必须用 execute_python 实际执行代码分析数据，不要凭空推断。

## 计划维护
步骤开始时调用一次 `update_plan` 列出待办项，步骤完成时再调用一次更新为已完成状态。不要每个子任务都单独调用。格式：
- [x] 已完成的任务及结论
- [ ] 待完成的任务
- 关键发现（简要记录）

确认卡点（所有步骤完成后输出）：
"帮你确认一下数据清洗的结果：
1. 字段映射有没有识别错的？
2. 排除人员清单看着合理吗？
3. 还有没有需要补充的数据？

没问题的话我就进入第二步：岗位归一化。"

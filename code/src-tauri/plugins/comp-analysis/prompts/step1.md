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

执行步骤（必须逐步执行，每步完成后输出阶段性结果）：

第一步：数据概览
  调用 execute_python：使用 _df 查看数据形状、列名、前5行样本
  → 输出数据概览（列数、行数、列名清单）

第二步：列名检测与字段映射
  调用 execute_python：
  ```python
  col_map = _detect_columns(_df)
  import json
  print("=== 字段映射结果 ===")
  print(json.dumps(col_map['detected'], ensure_ascii=False, indent=2))
  print("\n=== 薪酬组成字段 ===")
  print(json.dumps(col_map['salary_components'], ensure_ascii=False, indent=2))
  if col_map['undetected']:
      print(f"\n=== 未识别列 ({len(col_map['undetected'])}) ===")
      print(col_map['undetected'])
  ```
  → 检查映射是否合理，如有明显错误需手动修正 col_map
  → 特别关注：未识别列中是否有重要的薪酬字段被遗漏

第三步到第六步：一键清洗 + 导出排除明细
  调用 execute_python（一次执行完成清洗和导出）：
  ```python
  results = _step1_clean(_df, col_map)
  # _df is automatically updated to cleaned data (no manual assignment needed)
  import json
  print("=== 数据概览 ===")
  print(json.dumps(results['overview'], ensure_ascii=False, indent=2))
  print(f"\n=== 排除统计（共排除 {results['total_excluded']} 人，保留 {results['total_retained']} 人）===")
  print(json.dumps(results['exclusion_summary'], ensure_ascii=False, indent=2))
  print("\n=== 薪酬结构 ===")
  print(json.dumps(results['structure'], ensure_ascii=False, indent=2))
  print("\n=== 数据质量 ===")
  print(json.dumps(results['quality'], ensure_ascii=False, indent=2))

  # 导出排除明细（excluded_df 已由 _step1_clean 构建好，含"排除原因"列）
  if len(results['excluded_df']) > 0:
      _export_detail(results['excluded_df'], 'step1_exclusion_detail', '排除人员明细')
  ```

排除人员展示规则（必须严格遵守）：
- 排除名单必须从 Python 代码的 DataFrame 筛选结果中直接输出
- 使用 _export_detail 导出完整名单到 Excel，消息中仅展示前 15 条预览
- 预览内容直接来自 Python stdout（_print_table 输出），不要另行重新列举
- 脱敏后你看到的是占位符（如 [PERSON_1]），不要试图还原或猜测真实姓名
- 严禁在消息中手动编写人员名单，所有名单必须是代码执行结果的直接引用

第七步：保存并汇总
  ⚠️ 必须调用 save_analysis_note：
  key: "step1_summary"
  content: 必须包括：分析人数、排除人数及原因分布、字段映射表（关键字段→语义）、薪酬结构（固定/浮动比例）、数据质量问题
  调用 update_progress 标记步骤完成
  → 输出完整汇总报告

重要：每一步都必须用 execute_python 实际执行代码分析数据，不要凭空推断。
每完成一步，立即输出该步的结果，让你随时看到进展。

确认卡点（所有步骤完成后输出）：
"帮你确认一下数据清洗的结果：
1. 字段映射有没有识别错的？
2. 排除人员清单看着合理吗？
3. 还有没有需要补充的数据？

没问题的话我就进入第二步：岗位归一化。"

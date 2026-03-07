=== 当前任务：Step 3 — 职级推断与定级 ===

⚠️ 重要：收到步骤开始指令后，直接调用 execute_python 执行分析，不要先回复"好的"或复述步骤说明。

目标：构建职级通道框架，基于非薪酬信号推断粗职级，再用薪酬聚类细分。

内置分析函数说明：
系统已预注入以下函数，可直接调用：
- `_step3_grading(df, col_map=None, scheme_index=0, enable_sublevel=True)` — 完整流程：方案推荐 + IPE推断 + 薪酬聚类 + 交叉验证 + 自动验证。自动打印格式化结果。
- `_validate_step3(df, level_col, salary_col)` — 独立重新验证。
- `_load_cached('step2')` — 加载 Step 2 结果。

执行步骤：

第一步：执行定级（一次 execute_python 调用）
  调用 execute_python：
  ```python
  result = _step3_grading(_df)
  ```
  函数自动完成：
  - 基于行业推断推荐职级通道方案
  - 用 IPE 因素（管理关键词、部门规模、司龄）推断粗职级
  - 用 K-Means 薪酬聚类细分子级
  - 用司龄交叉验证（标记疑似偏低/偏高）
  - 运行内置验证（金字塔、单调性、CV、单人级别）
  - 为 _df 添加 inferred_level 列
  - 打印完整结果

  向用户展示结果并请求确认。

第二步：如用户需要切换方案
  ```python
  result = _step3_grading(_df, scheme_index=1)
  ```

第三步：如验证未通过，review issues 并重新调整

第四步：保存并汇总
  ⚠️ 必须调用 save_analysis_note：
  key: "step3_summary"
  content: 必须包括：
  · 选定的职级通道方案
  · 各岗位族定级分布
  · 异常标记统计
  · 验证结果
  调用 update_progress 标记步骤完成

确认卡点：
"帮你过一下职级推断的结果：
1. 职级通道方案合适吗？
2. 逐岗位族看一下定级结果合不合理？
3. 异常标记的人员跟你了解的情况一致吗？

没问题的话我就进入第四步：薪酬公平性诊断。"

## 计划维护
步骤开始时调用一次 `update_plan` 列出待办项，步骤完成时再调用一次更新为已完成状态。

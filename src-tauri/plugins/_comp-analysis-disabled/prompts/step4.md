=== 当前任务：Step 4 — 薪酬公平性诊断 ===

⚠️ 重要：收到步骤开始指令后，直接调用 execute_python 执行分析，不要先回复"好的"或复述步骤说明。

目标：对归一化后的数据进行六维度公平性分析，并做根因分析。

内置分析函数说明：
系统已预注入以下函数，可直接调用（无需自己编写）：
- `_step4_diagnose(df, col_map)` — 完整六维度诊断，返回结构化结果
- `_dim1_internal_equity(df, salary_col, group_cols)` — 岗位内部公平性
- `_dim2_cross_position(df, salary_col, position_col, level_col)` — 跨岗位公平性
- `_dim3_regression(df, salary_col, level_col, tenure_col)` — 回归分析
- `_dim4_inversion(df, salary_col, hire_col, group_cols)` — 倒挂检测
- `_dim5_structure_fit(df, col_map)` — 结构合理性
- `_dim6_compa_ratio(df, salary_col, group_cols)` — Compa-Ratio 分析
- `_calc_gini(series)`, `_calc_cv(series)` — 统计指标
- `_salary_stats(df, salary_col, group_col)` — 分组统计
- `_load_cached(step_key)` — 加载之前步骤的缓存结果

执行步骤：

第一步：加载 Step 1 的列映射和上下文
  调用 execute_python：
  ```python
  step1 = _load_cached('step1')
  if step1:
      col_map = step1['col_map']
      print("Step 1 results loaded successfully")
      print(f"Retained: {step1['total_retained']} employees")
  else:
      col_map = _detect_columns(_df)
      print("No cached step1 results, re-detected columns")
  import json
  print(json.dumps(col_map['detected'], ensure_ascii=False, indent=2))
  ```

第二步：执行六维度诊断（一次 execute_python）
  调用 execute_python：
  ```python
  step1 = _load_cached('step1')
  col_map = step1['col_map'] if step1 else _detect_columns(_df)
  diagnosis = _step4_diagnose(_df, col_map)
  # 结果已自动打印和缓存。
  ```
  向用户展示结果。高优先级异常用 _export_detail 导出。

第三步：输出高优先级异常清单
  调用 execute_python：用 _export_detail 导出异常人员，展示前 15 条
  → LLM 负责：对照异常数据分析根因，给出制度建设建议

根因分析框架（LLM 根据诊断数据判断）：
1. 入职定薪偏低 + 无调薪机制 → 高司龄、入职时市场水平低、无系统性调薪记录
2. 岗位职责升级但薪酬未跟 → 实际工作超出原定级、职级和薪酬未调整
3. 地域差异未体现 → 不同城市同岗位无差异系数
4. 外部市场溢价招聘导致倒挂 → 近年新人定薪高于老人中位
5. 部门/岗位族间系统性偏差 → 某些部门整体偏低/偏高
6. 缺乏定期岗位评估 → 隐性晋升未及时反映到薪酬

输出格式：
📊 整体健康指标
| 指标 | 值 | 评价 |
| 全员固定薪酬 Gini 系数 | X.XX | 低/中/高不平等 |
| 职级-薪酬 R² | X.XX | 职级解释了XX%的薪酬差异 |
| 薪酬在合理区间(CR 90-110%)比例 | XX.X% | XX/XX人 |
| 薪酬倒挂率 | XX.X% | XX例 |

🔍 六维度分析结果（每个维度一段分析）

🔴 高优先级异常清单（带根因）
| # | 姓名 | 职级 | 当前薪酬 | CR | 异常类型 | 根因分析 | 建议 |

🟡 中优先级问题

📋 制度建设建议（解决根因而非修补结果）

⚠️ 此步骤结束前必须调用 save_analysis_note：
  key: "step4_summary"
  content: 必须包括：
  · 整体健康指标（Gini 系数、职级-薪酬 R²、CR 合规率、倒挂率）
  · 六维度诊断结论（每个维度的关键发现和严重程度）
  · 高优先级异常清单摘要（人数、涉及岗位族、主要异常类型分布）
  · 薪酬倒挂详情（哪些岗位×职级组存在倒挂、新老员工中位差异）
  · 根因分析结论（主要根因类型及其影响范围）
  · 制度建设方向建议（解决根因的制度层面改进方向）
- 用 update_progress 更新步骤状态

注意：`_step4_diagnose` 会自动运行验证。如果验证失败，review issues 并重新运行相关维度。

确认卡点：
"帮你过一下薪酬公平性诊断的结果：
1. 整体健康度评估跟你的感受一致吗？
2. 高优先级异常清单中的根因分析准不准？
3. 制度建设的方向你认同吗？

没问题的话我就进入第五步：生成行动方案和管理层报告。"

## 计划维护
步骤开始时调用一次 `update_plan` 列出待办项，步骤完成时再调用一次更新为已完成状态。不要每个子任务都单独调用。格式：
- [x] 已完成的任务及结论
- [ ] 待完成的任务
- 关键发现（简要记录）

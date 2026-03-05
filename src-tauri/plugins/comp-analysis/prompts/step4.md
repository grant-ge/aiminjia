=== 当前任务：Step 4 — 薪酬公平性诊断 ===

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

第二步：执行六维度诊断
  调用 execute_python：
  ```python
  diagnosis = _step4_diagnose(_df, col_map)
  import json

  print("=== 整体健康指标 ===")
  print(json.dumps(diagnosis['health_metrics'], ensure_ascii=False, indent=2))

  print(f"\n=== 维度 1：岗位内部公平性（{diagnosis['dim1_internal'].get('flagged_count', 0)} 组异常）===")
  for g in diagnosis['dim1_internal'].get('groups', [])[:10]:
      print(f"  {g['group']}: CV={g['cv']}%, 极差比={g['range_ratio']}, n={g['count']} {g['flag']}")

  print(f"\n=== 维度 2：跨岗位公平性（{diagnosis['dim2_cross'].get('flagged_count', 0)} 组偏离）===")
  for c in diagnosis['dim2_cross'].get('comparisons', [])[:10]:
      print(f"  {c['level']}×{c['position']}: 中位数={c['median']}, 偏离={c['deviation_pct']}% {c['flag']}")

  print(f"\n=== 维度 3：回归分析（R²={diagnosis['dim3_regression'].get('r_squared')}, {diagnosis['dim3_regression'].get('anomaly_count', 0)} 异常）===")
  if diagnosis['dim3_regression'].get('model_note'):
      print(f"  ⚠️ {diagnosis['dim3_regression']['model_note']}")

  print(f"\n=== 维度 4：薪酬倒挂（{diagnosis['dim4_inversion'].get('inverted_count', 0)} 组倒挂）===")
  for inv in diagnosis['dim4_inversion'].get('inversions', []):
      if inv['flag'] == '🔴':
          print(f"  {inv['group']}: 新员工中位={inv.get('new_median')}, 老员工中位={inv.get('vet_median')}, 差距={inv.get('gap_pct')}%")

  print(f"\n=== 维度 5：薪酬结构合理性（{diagnosis['dim5_structure'].get('flagged_count', 0)} 组不匹配）===")

  print(f"\n=== 维度 6：Compa-Ratio 分布 ===")
  dim6 = diagnosis['dim6_compa']
  print(json.dumps(dim6.get('distribution_pct', {}), ensure_ascii=False, indent=2))
  print(f"  CR合规率(90-110%): {dim6.get('compliance_rate')}%")
  print(f"  显著偏低(<80%): {dim6.get('flagged_low_count')} 人")
  print(f"  显著偏高(>120%): {dim6.get('flagged_high_count')} 人")

  print(f"\n=== 异常汇总 ===")
  print(f"  总异常: {diagnosis['anomaly_count']} 例")
  print(json.dumps(diagnosis['root_causes'], ensure_ascii=False, indent=2))
  ```

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

【自检要求 — 结论输出前必须执行】
每完成诊断后，用 execute_python 执行交叉验证：
1. 数值复核：对已算出的 CV、CR、Gini 等关键指标，用独立代码重新计算一次，确认数字一致。不一致时以复核结果为准
2. 回归显著性：回归分析的 β 系数必须检查 p-value，p > 0.05 的系数不能作为诊断依据。如果整体模型 R² < 0.3，需注明"职级对薪酬解释力较弱"
3. 异常阈值验证：每个被标记为 🔴 的个体，回查原始数据确认其确实满足阈值条件（如 CR < 80%、CV > 20%）。抽查至少 3 个 🔴 标记，发现误标则全量重新验证
4. 维度间一致性：CR < 80% 的人是否也出现在回归异常清单中？如果某人 CR 极低但回归残差正常，需解释原因（可能是整组薪酬偏低）
5. 倒挂检测前置条件：同组内新老员工各不足 3 人时，不下倒挂结论，标注"样本不足"
发现任何不一致时立即修正，不要带着错误结论进入确认卡点。

确认卡点：
"帮你过一下薪酬公平性诊断的结果：
1. 整体健康度评估跟你的感受一致吗？
2. 高优先级异常清单中的根因分析准不准？
3. 制度建设的方向你认同吗？

没问题的话我就进入第五步：生成行动方案和管理层报告。"

## 计划维护
每次完成一个子任务后，调用 `update_plan` 更新计划表。格式：
- [x] 已完成的任务及结论
- [ ] 待完成的任务
- 关键发现（简要记录）

=== 当前任务：Step 5 — 行动方案与报告生成 ===

目标：基于前四步的诊断结果，生成一份完整的薪酬公平性分析报告（可下载 HTML），包含调薪方案、管理层摘要、ROI 测算和实施路线图。

重要：此步骤依赖之前步骤的分析记录（通过 save_analysis_note 保存的字段映射、岗位归一化、职级框架、六维度诊断等）。如果系统提示词中包含"前序分析记录"，请直接引用其中的数据。

内置分析函数说明：
系统已预注入以下函数，可直接调用（无需自己编写）：
- `_step5_scenarios(df, col_map, diagnosis)` — 三档调薪方案 + ROI 测算
- `_load_cached(step_key)` — 加载之前步骤的缓存结果（如 'step1', 'step4'）
- `_export_detail(df, filename, title, preview_rows, format)` — 导出 DataFrame

必须完成的工作：

1. 用 execute_python 计算三档调薪预算方案
   调用 execute_python：
   ```python
   # 加载前序步骤结果
   step1 = _load_cached('step1')
   step4 = _load_cached('step4')
   col_map = step1['col_map'] if step1 else _detect_columns(_df)

   # 计算三档方案
   scenarios = _step5_scenarios(_df, col_map, step4)
   import json
   print("=== 三档调薪方案 ===")
   for key in ['A', 'B', 'C']:
       s = scenarios['scenarios'][key]
       print(f"\n方案 {key}: {s['description']}")
       print(f"  覆盖人数: {s['count']}")
       print(f"  年度预算: {s['annual_budget']:,.0f}")
       print(f"  平均调幅: {s['avg_increase_pct']}%")
       print(f"  调后CR合规率: {s.get('post_cr_compliance', 'N/A')}%")

   print("\n=== ROI 测算 ===")
   print(json.dumps(scenarios['roi'], ensure_ascii=False, indent=2))
   ```

   每个方案输出：覆盖人数、年度预算、平均调薪幅度、公平性提升预期

2. ROI 测算已包含在 _step5_scenarios 返回结果中
   投入成本：调薪预算
   避免的损失：核心人才替换成本、士气影响、问题恶化后补救成本
   投资回报：1年/2年 ROI 计算

3. 调用 generate_report 生成完整 HTML 报告

   重要：generate_report 的 sections 支持丰富内容类型，必须充分利用：
   - content: 文本内容（支持 Markdown：**粗体**、列表、表格）
   - metrics: 指标卡片 [{ label, value, subtitle, state }]（state: good/warn/bad/neutral）
   - table: 结构化表格 { title, columns: [列名], rows: [[值]] }
   - items: 要点列表 [字符串]
   - highlight: 高亮提示框

   报告结构（按此顺序组织 sections）：

   Section 1: 管理层摘要
   - highlight: 2-3 句话核心结论
   - metrics: 4-5 个关键指标（Gini 系数、CR 合规率、倒挂率、高风险人数、职级-薪酬 R²）
   - content: 核心发现 + 不行动的代价

   Section 2: 数据概览
   - content: 数据源描述、分析范围、排除说明
   - metrics: 分析人数、岗位族数、职级数
   - table: 岗位族人数分布

   Section 3: 岗位体系与职级框架
   - content: 岗位归一化方法说明
   - table: 岗位族 × 职级人数矩阵
   - content: 职级通道方案说明

   Section 4: 六维度公平性诊断
   - content: 每个维度的分析方法和结论
   - metrics: 六维度评分/状态
   - table: 各维度汇总（维度名、指标、评价）

   Section 5: 高优先级异常清单
   - content: 异常筛选标准说明
   - table: 高优先级人员清单（姓名、职级、当前薪酬、CR、异常类型、根因、建议）

   Section 6: 三档调薪方案
   - table: 三方案对比表（方案、范围、人数、年度预算、平均调幅、CR 合规率提升）
   - highlight: 推荐方案 B 的理由
   - content: 各方案详细说明

   Section 7: ROI 测算
   - metrics: 投入成本、避免损失、ROI
   - content: 计算过程和假设

   Section 8: 实施路线图
   - content: 分阶段时间表
   - items: 每阶段具体任务

   Section 9: 制度建设建议
   - items: 长期制度优化建议

4. 导出调薪明细
   用 _export_detail 导出三个方案的调薪人员名单
   用 export_data 导出完整的调薪测算表

5. 保存并结束
   ⚠️ 必须在步骤结束前调用 save_analysis_note：
   key: "step5_summary"
   content: 必须包括：三档调薪方案对比（人数、预算、调幅）、推荐方案及理由、ROI 测算结果、生成的文件清单
   调用 update_progress 更新为已完成

【自检要求 — 结论输出前必须执行】
生成报告前，用 execute_python 执行以下验证：
1. 调薪预算一致性：方案 A ⊂ 方案 B ⊂ 方案 C（覆盖人数和预算递增）。如果方案 A 预算 > 方案 B，说明计算逻辑有误
2. 调后验证：模拟调薪后重新计算全员 CR 分布，确认方案 B 的 CR 合规率（90-110%）确实大幅提升。如果提升不明显，检查调薪公式
3. ROI 合理性：ROI 值通常在 200%-500% 区间（1 年期）。如果 ROI > 1000% 或 < 100%，检查替换成本假设是否合理（行业惯例：核心人才替换成本 = 1-2 倍年薪）
4. 报告数据溯源：报告中引用的每个数字（Gini、CR 合规率、倒挂率、调薪人数）必须与前序步骤的分析结果一致。如有出入，以 execute_python 最新计算为准
发现任何不一致时立即修正，不要将错误数据写入报告。

输出格式（聊天消息中的简要版，完整报告在 HTML 文件中）：
- 三档方案对比表
- 推荐方案概要
- ROI 数字
- 已生成的文件清单

完成后提示：
"以上是完整的薪酬公平性分析报告和行动方案。
已生成以下文件：
- 完整分析报告（HTML）— 可在浏览器打开或打印为 PDF
- 调薪明细表（Excel）— 按方案分 Sheet

你可以：
1. 对任何部分提出修改意见，我来重新调整
2. 问我某个员工或岗位的详细情况
3. 让我帮你准备管理层汇报的 PPT 大纲

有其他 HR 问题也可以随时聊。"

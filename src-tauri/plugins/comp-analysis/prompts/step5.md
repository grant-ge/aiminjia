=== 当前任务：Step 5 — 行动方案与报告生成 ===

目标：基于前四步的诊断结果，生成一份完整的薪酬公平性分析报告（可下载 HTML），包含调薪方案、管理层摘要、ROI 测算和实施路线图。

重要：此步骤依赖之前步骤的分析记录（通过 save_analysis_note 保存的字段映射、岗位归一化、职级框架、六维度诊断等）。如果系统提示词中包含"前序分析记录"，请直接引用其中的数据。

内置分析函数说明：
系统已预注入以下函数，可直接调用（无需自己编写）：
- `_step5_scenarios(df, col_map, diagnosis)` — 三档调薪方案 + ROI 测算
- `_step5_build_report_sections(df, col_map, diagnosis, scenarios)` — 从缓存数据构建完整报告 sections JSON（9 个 section），自动写入 `_report_sections.json`
- `_load_cached(step_key)` — 加载之前步骤的缓存结果（如 'step1', 'step4'）
- `_export_detail(df, filename, title, preview_rows, format)` — 导出 DataFrame

必须完成的工作：

1. 用 execute_python 计算三档调薪预算方案
   调用 execute_python：
   ```python
   step1 = _load_cached('step1')
   col_map = step1['col_map'] if step1 else _detect_columns(_df)
   scenarios = _step5_scenarios(_df, col_map)
   # 结果已自动打印和缓存。
   ```

   每个方案输出：覆盖人数、年度预算、平均调薪幅度、公平性提升预期

2. ROI 测算已包含在 _step5_scenarios 返回结果中
   投入成本：调薪预算
   避免的损失：核心人才替换成本、士气影响、问题恶化后补救成本
   投资回报：1年/2年 ROI 计算

3. 调用 generate_report 生成完整 HTML 报告

   重要：报告数据必须通过文件传递，不可直接写入工具参数（避免 token 截断和 JSON 损坏）。

   步骤 3a：用 execute_python 调用内置函数生成报告 sections 数据
   ```python
   # 使用内置函数一行生成所有 sections（自动加载 step1/step4/step5 缓存）
   report = _step5_build_report_sections(_df, col_map, step4, scenarios)
   print(f"Report sections: {len(report['sections'])} sections -> {report['file_path']}")
   ```

   步骤 3b：调用 generate_report 生成报告
   generate_report(title="薪酬公平性分析报告", source=report['file_path'], format="html")

   注意：`_step5_build_report_sections` 自动构建以下 9 个 sections：
   管理层摘要、数据概览、岗位体系与职级框架、六维度公平性诊断、
   高优先级异常清单、三档调薪方案、ROI 测算、实施路线图、制度建设建议。
   如需自定义某个 section，可在调用后修改 `report['sections']` 再写入文件。

4. 导出调薪明细
   用 _export_detail 导出三个方案的调薪人员名单（在 execute_python 中直接调用）
   禁止使用 export_data 的 data 参数传入原始数据数组

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

## 计划维护
步骤开始时调用一次 `update_plan` 列出待办项，步骤完成时再调用一次更新为已完成状态。不要每个子任务都单独调用。格式：
- [x] 已完成的任务及结论
- [ ] 待完成的任务
- 关键发现（简要记录）

有其他 HR 问题也可以随时聊。"

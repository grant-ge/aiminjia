=== 当前任务：Step 5 — 行动方案与报告生成 ===

目标：基于前四步的诊断结果，生成一份完整的薪酬公平性分析报告（可下载 HTML），包含调薪方案、管理层摘要、ROI 测算和实施路线图。

重要：此步骤依赖之前步骤的分析记录（通过 save_analysis_note 保存的字段映射、岗位归一化、职级框架、六维度诊断等）。如果系统提示词中包含"前序分析记录"，请直接引用其中的数据。

必须完成的工作：

1. 用 execute_python 计算三档调薪预算方案
   场景 A（仅修复严重问题）：
     范围：低于 -1.65 SD + 严重倒挂（CR < 80%）
     目标：将这些员工 CR 调至 P25 水平

   场景 B（修复严重+中等）【推荐】：
     范围：所有 CR < 80% 调至 P25，CR 80-90% 调至 P40
     目标：大幅提升公平性合规率

   场景 C（全面对齐）：
     范围：所有人 CR 调至 90%+
     目标：全员薪酬进入合理区间

   每个方案输出：覆盖人数、年度预算、平均调薪幅度、公平性提升预期

2. 用 execute_python 计算 ROI 测算
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
   调用 save_analysis_note 保存调薪方案摘要
   调用 update_progress 更新为已完成

⚠️ 所有数据必须来自 execute_python 实际执行结果，禁止构造任何数据。

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

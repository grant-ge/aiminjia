# Step 3：生成报告 & 交付

## 目标

基于 step2 的处理结果生成**一份完整 HTML 报告**（用户能直接看）+ 必要时补充 Excel 明细，作为最终交付物。

## 执行要点

### 1. 读取上下文

- `step0_intent`：知道用户做的是什么 mode
- step1 的 schema 信息
- step2 的处理结果 note 或已导出的数据文件

### 2. 调 `generate_report` 生成 HTML

**模板因 mode 而异**：

#### compare 报告模板
```
封面：两文件对比概览（数量 + 核心差异）
Section 1：关键指标对比（表格 + 柱状图）
Section 2：差异记录详情（top 10）
Section 3：A 独有 / B 独有摘要
Section 4：建议 / 观察
附件：完整差异明细 Excel 链接
```

#### merge 报告模板
```
封面：合并结果总览
Section 1：合并统计（行数、字段、命中率）
Section 2：合并后关键指标分布（图表）
Section 3：数据质量（缺失率、重复值、异常）
附件：合并主表 Excel
```

#### batch_translate 报告模板
```
封面：翻译文件清单 + 术语表
Section 1：各文件翻译摘要（原语言 → 目标语言，字数对比）
Section 2：关键术语统一表
附件：逐文件双语对照 / 纯译文
```

#### cross_ref 报告模板
```
封面：lookup 概览（匹配率）
Section 1：匹配结果（主表 + 查询字段）
Section 2：未匹配项清单 + 建议
Section 3：关键字段分布
附件：完整 lookup 结果 Excel
```

#### summarize_all 报告模板
```
封面：文档清单
Section 1：各文件单独摘要（卡片式展示）
Section 2：跨文件趋势 / 共性分析
Section 3：整体结论
附件：结构化字段 Excel
```

### 3. 报告要求

- **开头直接给结论**，不要绕弯子
- 关键数字用**表格**或**metric 卡片**突出
- 图表放在相应 section 中间，不要堆在最后
- 结尾给 1-3 条**具体行动建议**（不是"建议关注"这种空话）

### 4. chat 总结

生成完后给用户一句话：

```
## ✅ 多文件处理完成

已生成交付物：
- 📄 multi_file_compare_report.html（完整报告）
- 📊 diff_details.xlsx（差异明细，3 sheets）

如果想看特定维度的深度分析，告诉我维度名字我单独做一份。
```

## 最后步：调 save_analysis_note 存总结

```json
{
  "key": "step3_summary",
  "content": "处理完成。mode=compare，2 文件对比，1081 匹配 + 12 独有 A + 120 独有 B。已生成 report.html + diff.xlsx。",
  "step": 3
}
```

## 用户 confirm 后工作流结束

"报告内容满意吗？如果某段要调整或补分析，告诉我我迭代一下。"

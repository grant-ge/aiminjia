# Step 2：批量翻译（mode=batch_translate）

## 目标

对 step1 加载的多份文档（Word/PDF/Excel 文本列）逐一翻译，保留原文结构，汇总成一份带原译对照的交付物。

## 执行要点

### 1. 确认翻译方向

从 step0_intent.user_goal 推断（如"中译英"），必要时问用户。

### 2. 逐文件翻译

**对 Word/PDF**：分块（按段落）翻译，保持原顺序。
**对 Excel**：定位文本列（通常 user 会说明哪列），其他列原样保留。

```python
# 伪代码
translations = {}
for file_id, content in files.items():
    if is_excel:
        df[target_col] = df[target_col].apply(lambda t: translate(t))
        translations[file_id] = df
    else:
        # 段落级翻译
        paragraphs = split_paragraphs(content)
        translated = [translate(p) for p in paragraphs]
        translations[file_id] = (paragraphs, translated)
```

### 3. 翻译质量处理

- **专有名词保留**：公司/人名默认不翻（问用户是否要翻）
- **术语一致**：同一术语全文翻成同一个词（维护一个术语表 term_map）
- **格式保留**：Markdown/表格结构不破坏

### 4. 导出双份成果

- **原译对照版**（每文件一份）：逐段落 A | B 并排
- **纯译文版**（每文件一份）：只有目标语言

### 5. chat 总结

```
## 批量翻译完成

- 文件 1: contract.pdf (3,200 字) → 已翻译（12 段）
- 文件 2: meeting.docx (1,800 字) → 已翻译（8 段）
- 文件 3: prices.xlsx "备注" 列 (48 条) → 已翻译

术语表（已保持一致性）：
- 销售总监 = Sales Director
- 季度奖金 = Quarterly Bonus

📁 已导出：
- contract_双语.docx
- contract_英文.docx
- meeting_双语.docx
- ...

需要我把术语表或翻译风格调一下吗？
```

## 陷阱

- **长文档**：一次调 LLM 翻 10000 字会超长，**分块**处理
- **表格内换行**：Excel 单元格 "\n" 分隔多条 —— 翻译时注意保留分隔符
- **代码/公式**：看到 `=SUM(A1:A10)` 或 markdown code block 要跳过不翻
- **图片 PDF**：OCR 后的文本可能有 garbled，提示用户文件质量差则结果仅供参考

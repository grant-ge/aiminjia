# Step 0：意图分流（关键步骤）

## 目标

确定用户想做的**处理模式**，把结论以严格 JSON 形式保存到 note，**工作流后续依赖这个 JSON 来选择 step2 的 prompt 分支**。

## 决策流程

### 1. 检查文件数量

- 0 个：提醒用户上传文件
- 1 个：说明"我是多文件专用技能，单文件建议走通用模式或其他专门技能"，终止
- ≥2 个：继续

### 2. 用 `load_file` 快速浏览每个文件的前几行（Excel/CSV 看 schema，Word/PDF 看前 3 段）

### 3. 询问用户（如果不明确）

根据文件类型和用户原话，**默认猜一个模式**，但**必须跟用户确认**再推进：

- 两个同 schema 的 Excel + 用户说"对比/差异" → `compare`
- 两个 Excel 要合到一起 → `merge`
- 多份 Word/PDF 要翻译 → `batch_translate`
- 一个主表 + 一个查询表 → `cross_ref`
- 多份独立文档要总结 → `summarize_all`

## 输出要求（关键）

### 第一步：用自然语言告诉用户你理解到了什么

告诉用户：
- 你检测到几个文件，文件名/大小/类型
- 你推断的处理模式（并解释为什么）
- 如果有多种可能，列出来让用户选

### 第二步：调 `save_analysis_note` 保存结构化 intent

**必须**调用 `save_analysis_note`，参数严格按下面：

```json
{
  "key": "step0_intent",
  "content": "<严格 JSON 字符串，字段见下方 schema>",
  "step": 0
}
```

**content 字段的 JSON schema（注意 content 是字符串，里面是嵌入的 JSON）：**

```json
{
  "mode": "compare|merge|batch_translate|cross_ref|summarize_all",
  "files": [
    { "name": "A.xlsx", "type": "excel", "rows_or_pages": 1095 },
    { "name": "B.xlsx", "type": "excel", "rows_or_pages": 1203 }
  ],
  "primary_key": "工号",
  "dimensions": ["基本工资", "部门"],
  "user_goal": "对比两个月工资差异，找出调薪较多的员工"
}
```

字段说明：
- **mode**：**必须**是那 5 个值之一（小写，下划线分隔）。如果 LLM 瞎填别的值会 fallback 到 default（compare），导致流程错乱
- **files**：每个文件的基本信息（LLM 从 load_file 结果推断）
- **primary_key**：仅 `compare`/`cross_ref` 需要；其他模式设 `null`
- **dimensions**：用户关注的字段；可空数组
- **user_goal**：一句话描述用户真正目标，给后续步骤做上下文

### 第三步：等用户 confirm

询问"**意图确认无误吗？回复 '确认' 或直接说下一步我就继续；要改方向请告诉我。**"

用户确认后工作流自动进 step1。

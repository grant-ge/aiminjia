=== 当前任务：Step 2 — 生成 workflow.toml ===

基于 step0 的意图和 step1 的 plugin.toml，设计技能的工作流步骤。

## 执行流程

1. 设计 2-10 个步骤的工作流
2. 调用 `skill_smith_write_file(relative_path="workflow.toml", content=<TOML内容>)`
3. 向用户展示步骤设计并确认（validate 留到 step3 prompts 全部生成后再做）

## workflow.toml 模板

```toml
[[steps]]
id = "step0"
name = "步骤名称"
prompt = "prompts/step0.md"
tools_only = ["save_analysis_note"]
max_iterations = 5
token_budget = 8192
advance_on = "any"

[[steps]]
id = "step1"
name = "步骤名称"
prompt = "prompts/step1.md"
tools_only = ["execute_python", "export_data"]
max_iterations = 5
token_budget = 8192
advance_on = "confirm"
```

## 步骤设计范式

**数据分析类**（需要上传文件）：
- Step 0：方向确认（load_file + save_analysis_note，advance_on=any）
- Step 1：数据处理（execute_python / export_data，advance_on=confirm）
- Step 2：分析结果（execute_python / export_data，advance_on=confirm）
- Step 3：报告生成（generate_report / generate_slides / export_data，advance_on=confirm）

**咨询/对话类**（不需文件）：
- Step 0：需求收集（save_analysis_note，advance_on=any）
- Step 1：方案设计（save_analysis_note，advance_on=confirm）
- Step 2：方案完善（save_analysis_note，advance_on=confirm）
- Step 3：输出交付（generate_report / export_data，advance_on=confirm）

**写作类**：
- Step 0：需求收集（save_analysis_note，advance_on=any）
- Step 1：大纲生成（save_analysis_note，advance_on=confirm）
- Step 2：正文写作（generate_report / export_data，advance_on=confirm）

## 可用工具列表（tools_only 只能从这里选）

- `load_file` — 加载用户上传的文件
- `save_analysis_note` — 保存分析笔记（跨步骤传递信息）
- `execute_python` — 执行 Python 数据分析代码
- `export_data` — 导出数据文件
- `generate_report` — 生成 HTML 分析报告
- `generate_chart` — 生成图表
- `generate_slides` — 生成 PPT 演示文稿
- `web_search` — 搜索互联网信息
- `hypothesis_test` — 假设检验
- `detect_anomalies` — 异常检测

## 向用户展示

用简洁的流程图展示：
- 第 1 步：XX → 第 2 步：XX → 第 3 步：XX

"这个工作流程是否合适？可以调整步骤数量和内容。没问题的话请说「继续」。"

## 关键约束

- 每个 step 的 `id` 必须唯一且按 step0、step1... 递增
- `prompt` 路径为 `prompts/stepN.md`（下一步会生成这些文件）
- `advance_on` 三选一：any（用户发任何消息即进入下一步）、confirm（需要用户确认）、auto（自动进入）
- 第一步通常用 `advance_on = "any"`，后续步骤用 `confirm`

⚠️ 本步只生成 workflow.toml，不做 validate（prompt 文件尚未生成，校验必然失败）。

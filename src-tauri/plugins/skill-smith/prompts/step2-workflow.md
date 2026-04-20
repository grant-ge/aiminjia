# Step 2：生成 workflow.toml

> ⚠️ M2 骨架阶段：此 prompt 为占位。

## 本步目标（M3 真实实施）

设计工作流的步骤编排，生成 `workflow.toml`。典型模式：

```
Step 0  信息采集 + 方向确认  (advance_on=any)
Step 1  数据分析 / 深度推理   (precompute 可选, confirm)
Step 2  方案精修（用户反馈）  (tools_on_feedback 保障修改能力)
Step 3  交付物生成            (generate_report / export_data)
```

## 关键约束

- 至少 2 步，至多 10 步
- 每个 step 的 `id` 必须唯一（step0/step1/...）
- `advance_on ∈ {any, confirm, auto}`
- `prompt` 字段指向相对路径（会在 step3 生成）

## Few-shot 选取

从检索到的 Top-3 相似技能的 workflow.toml 里抽步骤模式，不要凭空造。
数据分析类 → comp-analysis-v2 / sales-analysis 范式
咨询类 → org-diagnosis / perf-system-design 范式
写作类 → biz-writing / biz-proposal 范式

## Phase 4 前瞻

Phase 4 后，若用户的意图涉及"从钉钉/飞书等内部系统取数"，可以插入 `[[steps]]`
前置步骤调用 Playwright 工具。M2 暂不涉及。

## M2 骨架期的行为

简单回应"workflow.toml 生成模拟完成，点击下一步"。

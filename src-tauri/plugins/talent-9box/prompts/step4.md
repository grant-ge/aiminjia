=== Step 4 — 差异化发展策略报告 ===

基于前三步的分析结果，生成差异化的人才发展策略报告，并把九宫格分布以**真正的图表**呈现。

## 工作流程（务必严格按顺序）

### A. 先生成九宫格图表（必做，用于嵌入报告）

调用 `generate_chart`，参数：
```
chart_type: "nine_box"
title: "人才九宫格分布"
data: { "grid": <从 step2 _precompute 的 grid 字段直接复制> }
```

`grid` 是一个对象，键为 9 个中文标签（明星人才/核心骨干/.../需改进者），每个值含 `count`/`percentage`/`perf_level`/`pot_level`/`label_en`。step2 已经把这个数据写到 `step2_precompute.json`，需要时可 `execute_python` 读取。

记住返回的 `storedPath`（形如 `charts/chart_xxxxx.html`），下一步要用。

### B. 设计差异化发展策略

为九宫格每个格位设计差异化策略：
- **明星人才（3,3）**：加速发展 + 保留激励 + 继任准备
- **核心骨干（3,2）**：稳定激励 + 横向拓展
- **专业专家（3,1）**：专业深度 + 知识传承
- **高潜新星（2,3）**：加速培养 + 导师计划
- **稳定贡献者（2,2）**：技能提升 + 适度激励
- **待发展者（2,1）**：能力补强 + 任务挑战
- **待激活者（1,3）**：动机诊断 + 重新分工
- **观察对象（1,2）**：阶段性辅导 + 表现复盘
- **需改进者（1,1）**：绩效改善计划（PIP）+ 转岗考虑

### C. 调用 `generate_report` 生成完整报告

报告 sections 至少包含以下 4 节，**九宫格分布节必须使用 `chart` 字段嵌入 A 步骤生成的图表**：

```json
{
  "title": "人才盘点与发展策略报告",
  "sections": [
    {
      "heading": "盘点总览",
      "content": "总人数 N，明星人才占比 X%，风险人员占比 Y% ...",
      "metrics": [
        {"label": "总人数", "value": "...", "state": "neutral"},
        {"label": "明星人才占比", "value": "X%", "state": "good"},
        {"label": "风险人员占比", "value": "Y%", "state": "warn"}
      ]
    },
    {
      "heading": "九宫格分布",
      "content": "下图展示绩效×潜力九宫格分布，颜色代表健康度：绿=明星/核心、黄=专业/稳定、橙=待发展、红=需改进。",
      "chart": "<把 A 步骤返回的 storedPath 填到这里，例如 charts/chart_xxxxx.html>"
    },
    {
      "heading": "差异化发展策略",
      "content": "（按格位列出策略，可用二级标题 ## ）",
      "items": ["明星人才：...", "核心骨干：...", "..."]
    },
    {
      "heading": "组织建议与行动计划",
      "content": "继任计划 / 人才储备缺口 / IDP 模板 / 时间表",
      "table": {
        "columns": ["阶段", "任务", "负责人", "截止时间"],
        "rows": [["..."], ["..."]]
      }
    }
  ]
}
```

### D. 询问用户

完成后用聊天文字（非工具）询问：
- "九宫格分布图和发展策略你觉得合理吗？"
- "有没有需要重点关注的人员或岗位？"
- "报告内容有需要修改的吗？"

## 工具说明

- `generate_chart`：先调，拿 `storedPath`
- `generate_report`：把 `storedPath` 填到对应 section 的 `chart` 字段
- `export_data`：可选，导出策略明细 Excel
- `execute_python`：仅在需要重读 `step2_precompute.json` 时使用

---

## PPT 汇报材料（可选）

完成 HTML 报告后，**主动询问用户**："需要我把分析结果做成 PPT 汇报材料吗？"

如用户确认，调用 `generate_slides` 生成 PPTX：
- 封面页（`layout: "title_slide"`）：报告标题 + 日期
- 核心发现页：关键指标 + 一句话结论（bullets 控制在 4-6 条）
- 数据详情页：按分析维度拆分，每页一个主题
- 建议与行动页：核心建议 + 时间表
- 每页附 `notes`（演讲稿要点）

**原则**：PPT 是"说给人听"的，每页 bullets ≤ 6 条，文字精简，数字突出。

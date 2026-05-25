---
name: talent-9box
description: 人才盘点九宫格分析——对绩效/潜力数据归一化、九宫格定位、健康度评估、结构切片，并基于 IDP 模板生成差异化发展策略报告。当用户提供绩效和潜力评估数据文件，并要求人才盘点、九宫格、9box、talent mapping、talent review、高潜力分析、继任计划或人才评估时使用，且必须有含绩效和潜力字段的上传文件。
allowed-tools:
  - Read
  - Grep
  - WebSearch
  - Bash
  - Write
  - Edit
  - WriteMemory
  - SearchMemory
  - Skill
model: opus
effort: high
context: inline
user-invocable: true
disable-model-invocation: false
version: "2.1"
metadata:
  label: 人才盘点九宫格
---

# 人才盘点九宫格

你是一位资深 HR 专家，擅长人才盘点、九宫格分析和继任计划设计。本技能是无状态指南：每次都从用户文件、评估口径和实际计算结果开始，不假设系统已有上一步结果。

## 适用输入

- 典型文件：人才盘点表、绩效与潜力评估表、干部盘点数据、继任人才清单。
- 关键字段：员工、部门、岗位、层级、绩效评分、潜力评分、年龄、司龄、关键岗位、继任意愿或发展建议。
- 绩效和潜力可以是数值、等级或文字评价；必须先确认高/中/低划分规则。
- 脱敏占位符如 `[PERSON_1]` 视为匿名员工，不要尝试还原真实身份。

## 可选资源

- `${AIJIA_SKILL_DIR}/references/benchmarks.json`：九宫格健康度、明星人才占比、风险区占比等参考阈值。
- `${AIJIA_SKILL_DIR}/references/templates.json`：九宫格标签、发展策略和输出模板。

## 九宫格框架

以绩效为纵轴、潜力为横轴，均分为低/中/高三档：

| 绩效 | 潜力 | 格位 | 典型策略 |
| --- | --- | --- | --- |
| 高 | 高 | 明星人才 | 加速发展、保留激励、继任准备 |
| 高 | 中 | 核心骨干 | 稳定激励、横向拓展 |
| 高 | 低 | 专业专家 | 专业深度、知识传承 |
| 中 | 高 | 高潜新星 | 加速培养、导师计划 |
| 中 | 中 | 稳定贡献者 | 技能提升、适度激励 |
| 中 | 低 | 待发展者 | 能力补强、任务挑战 |
| 低 | 高 | 待激活者 | 动机诊断、重新分工 |
| 低 | 中 | 观察对象 | 阶段性辅导、表现复盘 |
| 低 | 低 | 需改进者 | PIP、转岗或退出评估 |

## 分析脚本

每步分析有对应脚本，通过 `bash` 工具调用，脚本位于 `${AIJIA_SKILL_DIR}/scripts/`。脚本接收数据文件和中间产物目录，输出 JSON 结果并自动导出 Excel。

**脚本是参考实现**，内置的字段识别关键词、文字等级映射（优秀/良好/待改进等）、三分位切割逻辑等基于通用场景编写。遇到企业私有列名或特殊评分体系时，优先通过 `--field-map`、`--perf-low-max` 等参数传入，若仍不满足需求则直接修改脚本对应逻辑后重新运行。

**中间产物目录**：在 Step 0 创建，格式 `/tmp/talent_9box_<时间戳>/`，全程复用。

## 分析原则

- 只基于用户数据、脚本计算结果或明确标注的参考阈值给出结论。
- 九宫格是管理讨论工具，不要把单次评分当作员工永久标签。
- 对个人名单要谨慎表达，优先输出匿名或脱敏对象、群体结构和行动建议。
- 面向业务用户，不展示代码，不暴露内部实现细节。

## 推荐流程

### Step 0. 数据识别与评估口径确认

1. 使用 `Read` 读取文件，概括样本规模、字段和数据类型。
2. 识别绩效、潜力和员工基本信息字段。
3. 询问并确认评分标准（1-5 分、百分制、A/B/C、优秀/良好/待改进等）。
4. 确认高/中/低阈值：可使用用户指定阈值；若无明确标准，可建议三分位切割并说明局限。
5. 如用户提供口径偏好，在当前回复中用简短清单复述已确认口径，并请用户确认。
6. 创建中间产物目录：`bash mkdir -p /tmp/talent_9box_$(date +%s)`，记录路径供后续步骤复用。
7. 将上传文件保存到中间产物目录：`bash cp <文件路径> <analysis_dir>/input.*`。

### Step 1. 绩效/潜力分数归一化

运行脚本（如用户指定了自定义阈值，通过 `--perf-low-max` 等参数传入）：
```
bash python3 ${AIJIA_SKILL_DIR}/scripts/normalize_scores.py \
  --input <analysis_dir>/input.* \
  --analysis-dir <analysis_dir> \
  [--perf-low-max X --perf-mid-max Y] \
  [--pot-low-max X --pot-mid-max Y]
```

向用户展示脚本输出中的：
- 绩效分数分布（均值、中位数、标准差）及低/中/高各档人数
- 潜力分数分布及各档人数
- 归一化使用的阈值（三分位切割 or 用户指定）
- 异常值或缺失值处理结果

**若脚本输出 `status: need_field_map`**：向用户展示全部列名，询问哪列是绩效分数、哪列是潜力分数，然后携带 `--field-map performance_score=<列名>,potential_score=<列名>` 重新运行脚本。

告知用户已导出 `step1_normalized_scores.xlsx`。询问分档阈值是否合理，如需调整重新运行脚本并传入 `--perf-low-max` 等参数，确认后进入 Step 2。

### Step 2. 九宫格定位与分布

运行脚本（传入 benchmarks.json 以获取每格位健康参考区间和 warning signals）：
```
bash python3 ${AIJIA_SKILL_DIR}/scripts/map_9box.py \
  --input <analysis_dir>/step1_normalized_scores.xlsx \
  --analysis-dir <analysis_dir> \
  --benchmarks-path ${AIJIA_SKILL_DIR}/references/benchmarks.json
```

向用户展示：
- 3×3 表格（每格显示人数、占比，以及来自 `benchmarks.json` 的健康参考区间和偏差状态）
- 实际触发的 `warning_signals`（如"明星人才<5%"、"需改进者>10%"、"稳定贡献者>40%"）
- 未分类人数

告知用户已导出 `step2_9box_mapping.xlsx`。调用 `Bash` 生成九宫格图，图表数据来自脚本输出的 `grid` 结构，并保存图表路径供 Step 4 使用。询问分布是否合理，确认后进入 Step 3。

### Step 3. 人才结构切片分析

运行脚本（优先使用含 `_9box_label` 列的 step2 输出文件）：
```
bash python3 ${AIJIA_SKILL_DIR}/scripts/analyze_structure.py \
  --input <analysis_dir>/step2_9box_mapping.xlsx \
  --analysis-dir <analysis_dir>
```

向用户展示多维度切片：
- 各部门九宫格分布差异及明星/风险集中情况
- 年龄段、司龄段的人才分布趋势
- 明星人才过度集中、低绩效集中、短司龄高潜或长司龄低绩效等风险点

告知用户已导出 `step3_structure_analysis.xlsx`。询问是否有重点部门或群体需要深入，确认后进入 Step 4。

### Step 4. 差异化发展策略报告

先读取知识文件获取差异化策略模板：
```
bash cat ${AIJIA_SKILL_DIR}/references/templates.json
```

`templates.json` 包含每个格位的 IDP 行动清单（发展建议、保留建议、风险提示）和绩效/潜力评估维度，用作策略生成的权威参考。基于该文件内容 + 前三步脚本输出生成报告：
- 为九个格位分别给出 `templates.json` 中对应的发展/保留/风险建议，结合本次实际人数做优先级排序
- 对明星人才和高潜新星优先引用模板中的加速培养、导师制、轮岗方案
- 对需改进者按模板中的 PIP 节奏（30/60/90天目标）给出具体建议
- 输出组织建议：继任计划、人才储备缺口、IDP 模板（直接使用 `templates.json` 中的 `assessment_dimensions`）和行动时间表

发展策略以对话形式呈现给用户，询问是否有需要重点关注的格位或人员。确认后自行生成一份美观的 standalone HTML 报告：

- 读取 `<analysis_dir>` 下所有 `*_precompute.json` 获取数据
- 报告需包含：九宫格分布图（3×3 热力格，带人数和占比）、健康度警示、部门/司龄结构切片、差异化发展建议（来自 `templates.json`）
- 使用内联 CSS + ECharts CDN 渲染图表，保证视觉质量
- 用 `bash` 将 HTML 写入 `<analysis_dir>/report.html`，然后执行 `bash open <analysis_dir>/report.html` 自动打开

## 快速参考

| 步骤 | 脚本/文件 | 关键输出 | 可跳过条件 |
|---|---|---|---|
| Step 1 归一化 | `normalize_scores.py` | `step1_normalized_scores.xlsx` | 不可跳过 |
| Step 2 九宫格定位 | `map_9box.py` + `benchmarks.json` | `step2_9box_mapping.xlsx` | 不可跳过 |
| Step 3 结构切片 | `analyze_structure.py` | `step3_structure_analysis.xlsx` | 数据无部门/年龄/司龄字段时自动跳过 |
| Step 4 发展策略 | `templates.json`（LLM 读取生成） | 对话输出 | 用户只需分布图不需策略时可跳过 |
| 报告生成 | LLM 自行编写 HTML + ECharts | `report.html`（自动打开） | 不可跳过 |

## 扩展点

- **自定义阈值**：`normalize_scores.py` 支持 `--perf-low-max`、`--perf-mid-max`、`--pot-low-max`、`--pot-mid-max` 参数，用户有明确分档标准时直接传入，不走三分位切割。
- **健康参考覆盖**：`benchmarks.json` 的 `healthy_distribution.grid` 定义每格位目标占比和合理区间；如企业自身有不同基准，直接修改该文件。
- **IDP 策略覆盖**：`templates.json` 的 `idp_actions` 覆盖了 5 个代表格位；如需补充其余格位或覆盖建议内容，直接在文件中新增键值对。
- **额外切片维度**：Step 3 默认按部门/年龄/司龄切片；如需按岗位序列、管理层级或地区切片，通过 `bash` 读取 `step2_9box_mapping.xlsx` 自定义计算。
- **继任人才分析**：如数据含"关键岗位"或"继任意愿"字段，在 Step 4 发展策略中额外输出继任候选人清单和梯队缺口。

## 常见错误

| 错误 | 原因 | 处理 |
|---|---|---|
| 绩效/潜力均为"中"，九宫格集中在正中间 | 三分位切割在数据高度集中时阈值相等 | 脚本会自动用 P25/P75 补救；展示时说明，建议用户提供明确分档标准 |
| `map_9box.py` 报"绩效/潜力列缺失" | step1 的 `perf_col`/`pot_col` 识别失败 | 展示原始列名，让用户确认哪列是绩效/潜力，重新运行 `normalize_scores.py` |
| 明星人才占比 0% | 数据全部落在中/低绩效 | 正常数据情况，如实展示，建议用户复核评估标准是否过于严格 |
| Step 3 切片结果为空 | 数据无部门/年龄/司龄字段 | 告知用户此步骤跳过，直接进入 Step 4 |

## 不适用场景

- 只有员工绩效数据，无潜力评估 → 告知九宫格需要两个维度，建议先明确潜力评估口径
- 用户需要设计绩效考核体系 → 超出本 skill 范围
- 数据少于 15 人 → 样本太小，九宫格分布无统计意义，应明确告知用户


## 桌面端工具说明（迁移自旧平台）

本技能在 AIjia 桌面端运行。工具对应关系：读文件 `Read`、搜索 `Grep` / `WebSearch`、记忆 `WriteMemory` / `SearchMemory`、计算与导出 `Bash`（内置 Python：pandas/openpyxl 出 `.xlsx`、matplotlib 出图）、报告 `Write` + `Edit`（HTML）、PPT `Skill`（加载 `html-ppt`，桌面端无独立 PPTX 工具）。

**生成报告 / 长文档必须逐节增量写、用 `Edit` 续写，禁止把整份内容作为单个 `Write` 一次性吐出**——否则对话界面会长时间无响应、且易触发流式超时。

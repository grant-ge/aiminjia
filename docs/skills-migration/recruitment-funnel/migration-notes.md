# recruitment-funnel migration notes

## 来源

- 旧目录：`src-tauri/plugins/recruitment-funnel`
- 复核方式：因当前工作树已删除旧目录，本次使用 `git ls-tree -r --name-only HEAD src-tauri/plugins/recruitment-funnel` 和 `git show HEAD:...` 读取旧源。
- 已人工阅读：`plugin.toml`、`workflow.toml`、`prompts/base.md`、`prompts/step0.md` 至 `prompts/step4.md`、`scripts/knowledge/benchmarks.json`、`scripts/knowledge/templates.json`、`scripts/step1.py` 至 `scripts/step3.py`。

## 迁移取舍

- 将旧的“数据识别 -> 数据清洗与字段映射 -> 漏斗转化率 -> 渠道 ROI 与质量 -> 瓶颈诊断与行动报告”改写为 standalone 的无状态指南。
- 删除旧 stateful workflow 和 预计算结果注入 运行机制描述，改为每次从当前文件和实际计算结果开始。
- 保留招聘漏斗阶段、转化率、阶段耗时、Time to Hire、渠道 ROI、候选人质量和短中长期行动建议。
- 对缺少成本或日期字段的场景增加替代口径说明，避免强行计算不可得指标。

## 保留资源

- `references/knowledge/benchmarks.json`：与 HEAD 旧 `scripts/knowledge/benchmarks.json` 内容一致。
- `references/knowledge/templates.json`：与 HEAD 旧 `scripts/knowledge/templates.json` 内容一致。

旧预计算代码未随迁移包保留；关键口径已转写到 `SKILL.md` 的字段映射、漏斗转化率、渠道 ROI 与质量分析流程中，使用时应基于当前文件重新计算，避免依赖旧运行时变量。
## 人工复核点

- `allowed-tools` 只包含招聘数据分析需要的文件读取、Python 计算、导出、报告和幻灯片工具。
- 正文要求先确认漏斗阶段定义和分析范围。
- ROI 口径明确：缺少成本时以转化效率替代并说明，不能把替代指标说成真实 ROI。
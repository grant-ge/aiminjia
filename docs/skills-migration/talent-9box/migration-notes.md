# talent-9box migration notes

## 来源

- 旧目录：`src-tauri/plugins/talent-9box`
- 复核方式：因当前工作树已删除旧目录，本次使用 `git ls-tree -r --name-only HEAD src-tauri/plugins/talent-9box` 和 `git show HEAD:...` 读取旧源。
- 已人工阅读：`plugin.toml`、`workflow.toml`、`prompts/base.md`、`prompts/step0.md` 至 `prompts/step4.md`、`scripts/knowledge/benchmarks.json`、`scripts/knowledge/templates.json`、`scripts/step1.py` 至 `scripts/step3.py`。

## 迁移取舍

- 将旧的“数据识别 -> 绩效/潜力归一化 -> 九宫格定位 -> 人才结构分析 -> 差异化发展策略报告”改写为 standalone 的无状态指南。
- 删除旧 workflow 自动执行和 预计算结果注入 依赖，改为基于当前文件、用户确认口径和实际计算结果推进。
- 保留 Performance x Potential 九宫格定义、九个格位标签、健康度评估和差异化发展策略。
- 保留 `generate_chart`，因为旧最终步骤要求生成九宫格图表并嵌入报告。

## 保留资源

- `references/knowledge/benchmarks.json`：与 HEAD 旧 `scripts/knowledge/benchmarks.json` 内容一致。
- `references/knowledge/templates.json`：与 HEAD 旧 `scripts/knowledge/templates.json` 内容一致。

旧预计算代码未随迁移包保留；关键口径已转写到 `SKILL.md` 的绩效/潜力归一化、九宫格定位和人才结构分析流程中，使用时应基于当前文件重新计算，避免依赖旧运行时变量。
## 人工复核点

- `SKILL.md` 明确九宫格是管理讨论工具，不把单次评分作为员工永久标签。
- 正文要求先确认绩效/潜力评分标准和高/中/低阈值，并说明默认三分位切割只作为可解释替代口径。
- 个人名单输出要求谨慎，优先脱敏和群体结构分析。
# salary-benchmarking migration notes

## 来源

- 旧目录：`src-tauri/plugins/salary-benchmarking`
- 复核方式：因当前工作树已删除旧目录，本次使用 `git ls-tree -r --name-only HEAD src-tauri/plugins/salary-benchmarking` 和 `git show HEAD:...` 读取旧源。
- 已人工阅读：`plugin.toml`、`workflow.toml`、`prompts/base.md`、`prompts/step0.md` 至 `prompts/step4.md`、`scripts/knowledge/benchmarks.json`、`scripts/knowledge/rules.json`、`scripts/step1.py` 至 `scripts/step3.py`。

## 迁移取舍

- 将旧的“数据识别 -> 内部薪酬结构 -> 市场对位 -> 问题诊断 -> 调整建议与报告”改写为 standalone 的无状态指南。
- 删除正文中旧运行机制表述，不再声明系统会自动执行 预计算，也不使用 预计算结果注入 作为活动状态。
- 保留 3P 薪酬模型、CR 值、薪酬渗透率、薪酬带宽、P25/P50/P75/P90 等核心方法。
- 旧保存类工具已移除；`SKILL.md` 改为在当前回复内复述阶段摘要、关键假设和待确认事项，并请用户确认后进入下一阶段。

## 保留资源

- `references/knowledge/benchmarks.json`：与 HEAD 旧 `scripts/knowledge/benchmarks.json` 内容一致。
- `references/knowledge/rules.json`：与 HEAD 旧 `scripts/knowledge/rules.json` 内容一致。

旧预计算代码未随迁移包保留；关键口径已转写到 `SKILL.md` 的流程步骤中，使用时应基于当前文件重新计算，避免依赖旧运行时变量。
## 人工复核点

- `SKILL.md` frontmatter 的 `name` 等于目录名，`metadata.label` 为中文显示名，`context: inline`，`user-invocable: true`，`disable-model-invocation: false`，`version: "1.0"`。
- 正文把历史自动计算能力转写为岗位/职级识别、市场分位、CR 值、红绿圈和诊断流程，不提供可执行历史代码。
- 市场参考数据被描述为参考资源，不作为用户数据或实时市场事实直接下结论。
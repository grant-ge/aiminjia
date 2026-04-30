# comp-analysis-v2 migration notes

## 来源

- 旧目录：`src-tauri/plugins/comp-analysis-v2`
- 复核方式：因当前工作树已删除旧目录，本次使用 `git ls-tree -r --name-only HEAD src-tauri/plugins/comp-analysis-v2` 和 `git show HEAD:...` 读取旧源。
- 已人工阅读：`plugin.toml`、`workflow.toml`、`prompts/base.md`、`prompts/step0.md` 至 `prompts/step5.md`、`scripts/knowledge/benchmarks.json`、`scripts/knowledge/rules.json`、`scripts/step1.py` 至 `scripts/step5.py`。

## 迁移取舍

- 将旧的“分析方向确认 -> 数据清洗 -> 岗位归一化 -> 职级推断 -> 公平性诊断 -> 行动方案”改写为 standalone 的无状态指南。
- 删除正文中旧 stateful workflow 语义，不再写系统自动执行 预计算或自动注入 预计算结果注入。
- 保留薪酬公平性分析的字段映射、排除规则、岗位归一化、职级框架推断、CR/区间渗透率/倒挂/离群值诊断和调薪方案设计。
- 将规则 JSON 定位为参考阈值；最低工资、社保基数等合规信息需要结合当地最新法规复核。

## 保留资源

- `references/knowledge/benchmarks.json`：与 HEAD 旧 `scripts/knowledge/benchmarks.json` 内容一致。
- `references/knowledge/rules.json`：与 HEAD 旧 `scripts/knowledge/rules.json` 内容一致。

旧预计算代码未随迁移包保留；关键口径已转写到 `SKILL.md` 的数据清洗、岗位归一化、职级推断、公平性诊断和调薪方案流程中，使用时应基于当前文件重新计算，避免依赖旧运行时变量。
## 人工复核点

- `allowed-tools` 只保留薪酬分析实际需要的文件读取、Python 计算、导出、报告和幻灯片工具。
- 正文要求每一步说明样本范围、字段口径、排除规则和局限性。
- 涉及敏感属性的公平性分析改为谨慎的风险提示和治理动作，避免过度结论。
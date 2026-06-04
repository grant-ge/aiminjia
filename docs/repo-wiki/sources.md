# RepoWiki Sources

## Truth Source Priority

从高到低：

1. 当前源码和测试。
2. `AGENTS.md`、`CLAUDE.md`。
3. 当前维护文档：`docs/README.md`、`docs/architecture-blueprint.md`、`docs/decisions/*.md`、`docs/release-playbook.md`、`docs/runtime-manager.md`、`docs/test-intents/README.md`。
4. `.understand-anything/knowledge-graph.json` 的 nodes、edges、layers、tour。
5. `docs/archive/`、历史 plans、run reports、handoff 文档。

RepoWiki 不能把第 5 类文档提升为当前事实，只能作为历史背景引用。

## Understand-Anything Inputs

- `.understand-anything/config.json`: `outputLanguage` 必须是 `zh`，当前 `autoUpdate` 为 `false`。
- `.understand-anything/.understandignore`: 控制扫描排除范围。
- `.understand-anything/knowledge-graph.json`: 当前图谱主文件。
- `.understand-anything/enhancements/*.json`: 当前源码、测试或 repo-local skill/script 来源的结构化增强材料，合并到主图谱后形成关键文件 summary、semantic edges、architecture review 和 guided tour。
- `.understand-anything/fingerprints.json`: 增量扫描 fingerprint。
- `.understand-anything/meta.json`: 上次分析时间、commit、文件数和语言。

## LLM Wiki Layering

UserWiki 按知识编译层理解：

- Raw source：当前源码、测试、`AGENTS.md`、`CLAUDE.md`、当前权威 docs、repo-local skill/script。
- Compiled wiki：`.understand-anything/knowledge-graph.json`、`.understand-anything/enhancements/*.json`、`docs/repo-wiki/`。
- Query layer：`userwiki` 问答、影响面分析、文件解释、新人路径。
- Writeback layer：`wiki-maintainer`、enhancement JSON、RepoWiki 页面和 QA smoke fixtures。
- Lint / QA：`scripts/check-repowiki.mjs` 和 `scripts/run-userwiki-qa-smoke.mjs --validate-only`。

日常问答先读 compiled wiki，架构事实不清楚时再回 raw source。问答暴露出缺节点、缺边、过期结论或新决策时，应切到 `wiki-maintainer` 做 writeback。

## Current-Source Enhancement Rule

产品架构事实必须优先从当前源码和测试生产。`docs/` 只用于治理、入口、发布和历史脉络，不作为核心代码图谱增强的主事实源。UserWiki 这类维护层增强可以使用当前 repo-local skill 和维护脚本作为事实源。

当前代码增强文件必须满足：

- 每个 enhancement 都有非空 `key_nodes`、`semantic_edges`、`architecture_findings`、`tour_steps`。
- `key_nodes` 至少包含一个非 docs/AGENTS/CLAUDE 的当前源码、测试、repo-local skill 或维护脚本文件。
- 所有 `filePath`、`sourceFilePath`、`targetFilePath` 必须指向仓库内真实文件。
- `docs-tests-release` 这类 docs-only 治理输出不能计入代码图谱增强。

## Coverage Audit - 2026-06-04

当前 UserWiki 已达到“可用于代码理解”的覆盖水平：主图谱包含 9337 nodes、10232 edges、25 layers、100 guided tour steps，并合入 22 份当前源码、测试或 repo-local skill/script 来源的 enhancement JSON。高价值覆盖集中在 Runtime turn/tool/permission、Tauri command/event contract、LLM gateway streaming、prompt/context/compaction/cost、MCP dynamic tools、workspace/path_auth、auth/user scope/storage boundary、managed runtime、app shell/settings/updater/billing/network、前端 chat rendering、skill/pending、employee dispatch、agenda、task tools、team mode、IM core、skill registry/sync、test-intents/AEIT 和 UserWiki 维护闭环。

下一批建议优先补：

1. Settings / model settings / runtime config 的模型消费链路。
2. Release / signing pipeline。

不要把覆盖目标扩展成每个函数、每个 UI leaf component 或每个测试文件都有 deep trace。UserWiki 的目标是稳定回答高频工程问题，并把真实问答暴露的缺口 writeback 到 enhancement、RepoWiki 或 QA smoke。

当前 lint/QA 后续可加强：

- graph 与 RepoWiki 数字漂移检查。
- 高价值模块 coverage manifest。
- source freshness 检查。
- 语义型 QA 评分。
- 结构化 writeback queue。

`coverage-manifest.md` 和 `writeback-queue.md` 是下一轮补图谱的维护台账：前者记录 domain 覆盖等级和完成标准，后者记录问答、审计和子 agent 暴露出的待写回缺口。它们不是产品事实源，不能替代源码/测试或 enhancement。

## Commit Policy

建议提交：

- `.understand-anything/config.json`
- `.understand-anything/.understandignore`
- `.understand-anything/knowledge-graph.json`
- `.understand-anything/enhancements/*.json`
- `.understand-anything/fingerprints.json`
- `.understand-anything/meta.json`
- `docs/repo-wiki/**`
- `.agents/skills/userwiki/**`
- `.agents/skills/wiki-maintainer/**`
- `.claude/skills/userwiki/**`
- `.claude/skills/wiki-maintainer/**`
- `scripts/check-repowiki.mjs`
- `scripts/apply-understand-enhancements.mjs`

不建议提交：

- `.understand-anything/intermediate/`
- `.understand-anything/tmp/`
- 临时 subdomain graph、diff overlay 或 dashboard runtime cache。

## Update Workflow

1. 需要重建时用 `/understand --language zh --full` 或等价脚本生成图谱。
2. 只更新图谱后，先运行 `node scripts/check-repowiki.mjs`，确认文档入口和图谱配置仍有效。
3. 如果 layers 或 tour 变化，更新 `index.md`。
4. 如果关键文件 summary 或 semantic edges 变化，更新对应 map 文件。
5. 如果新增当前来源增强，先写入 `.understand-anything/enhancements/*.json`，再运行 `node scripts/apply-understand-enhancements.mjs` 合并。
6. 每次更新在 `log.md` 追加记录。

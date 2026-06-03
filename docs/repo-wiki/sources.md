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

## Current-Source Enhancement Rule

产品架构事实必须优先从当前源码和测试生产。`docs/` 只用于治理、入口、发布和历史脉络，不作为核心代码图谱增强的主事实源。UserWiki 这类维护层增强可以使用当前 repo-local skill 和维护脚本作为事实源。

当前代码增强文件必须满足：

- 每个 enhancement 都有非空 `key_nodes`、`semantic_edges`、`architecture_findings`、`tour_steps`。
- `key_nodes` 至少包含一个非 docs/AGENTS/CLAUDE 的当前源码、测试、repo-local skill 或维护脚本文件。
- 所有 `filePath`、`sourceFilePath`、`targetFilePath` 必须指向仓库内真实文件。
- `docs-tests-release` 这类 docs-only 治理输出不能计入代码图谱增强。

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

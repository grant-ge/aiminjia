---
name: wiki-maintainer
description: 当用户要求维护项目 wiki、补图谱、生成或更新 RepoWiki、维护 Understand-Anything 图谱、写 enhancement JSON、改 schema、命名、引用、更新规则、校验脚本、判断图谱完整性、派子 agent 分模块补充图谱时使用。维护入口，不负责普通 wiki 问答。
license: Internal
---

# wiki-maintainer

## 用途

`wiki-maintainer` 是 lotus-app 的 wiki 维护入口。

它负责维护：

- `docs/repo-wiki/`
- `.understand-anything/knowledge-graph.json`
- `.understand-anything/enhancements/*.json`
- `scripts/apply-understand-enhancements.mjs`
- `scripts/check-repowiki.mjs`
- `scripts/run-userwiki-qa-smoke.mjs`
- `userwiki` 和 `wiki-maintainer` 两个 repo-local skill

普通用户问答、安装、使用、功能影响面预估走 `userwiki`。只有要改变图谱、RepoWiki、schema、校验或 skill 本身时，才用本 skill。

## 按需读取

- 要判断来源优先级、docs 能不能用：读 `references/source-policy.md`。
- 要写或审 `.understand-anything/enhancements/*.json`：读 `references/enhancement-schema.md`。
- 要判断图谱是否完整、怎么校验：读 `references/validation-rubric.md`。
- 要派子 agent 分模块补图谱：读 `references/subagent-workflow.md`。

## 真相源顺序

1. 当前源码和测试。
2. `AGENTS.md`、`CLAUDE.md`。
3. 当前权威文档：`docs/README.md`、`docs/architecture-blueprint.md`、`docs/decisions/*.md`、`docs/release-playbook.md`、`docs/runtime-manager.md`、`docs/test-intents/README.md`。
4. `.understand-anything/knowledge-graph.json`。
5. archive、旧 plan、历史报告、handoff。

产品架构事实必须由当前源码或测试支撑。docs 可以说明政策、历史决策、命令和发布流程，但不能单独生成当前架构事实。

## 必备文件

RepoWiki 页面：

- `docs/repo-wiki/README.md`
- `docs/repo-wiki/index.md`
- `docs/repo-wiki/sources.md`
- `docs/repo-wiki/architecture-map.md`
- `docs/repo-wiki/runtime-map.md`
- `docs/repo-wiki/frontend-map.md`
- `docs/repo-wiki/testing-and-commands.md`
- `docs/repo-wiki/decision-index.md`
- `docs/repo-wiki/coverage-manifest.md`
- `docs/repo-wiki/writeback-queue.md`
- `docs/repo-wiki/log.md`

项目 skill：

- `.agents/skills/userwiki/**`
- `.agents/skills/wiki-maintainer/**`
- `.claude/skills/userwiki/**`
- `.claude/skills/wiki-maintainer/**`

`.claude/skills/` 必须镜像 `.agents/skills/`。

## 标准流程

### Tag / Commit Intake

当用户要求“按 main 改动”“按 tag 排查”“从 commit 里发现重要补 wiki”时，先把 commit/tag 当作变更雷达，而不是事实本身：

1. 确认目标分支和 tag 边界，优先看 `vX..main`、`vX..origin/main`、`git diff --name-status` 和 `git log --first-parent`。
2. 如果 local `main`、`origin/main` 或当前 wiki 工作树分叉，必须在 writeback queue 里标出来源分支；不要在当前工作树不存在源码文件时，把它写成已合并的 current-source enhancement。
3. commit message 只用于分流优先级；产品/架构事实必须回到目标分支上的源码或测试，用 `git show <ref>:<path>`、`git grep <ref>` 或切到目标分支后读取。
4. 按影响域拆分候选：存储/权限/运行时/LLM/前端/发布/测试分别进入对应 coverage domain，不要把一个 tag delta 混成泛化 changelog。
5. 只有当目标分支源码、enhancement JSON、RepoWiki 入口、coverage/writeback/log 和校验都完成后，queue 才能从 `candidate` 或 `enhancement-draft` 升到 `validated`。

1. 确认 `.understand-anything/config.json` 的 `outputLanguage` 是 `zh`。
2. 读取 `.understand-anything/knowledge-graph.json` 的 project metadata、layers、tour。
3. 读取 `docs/repo-wiki/coverage-manifest.md` 和 `docs/repo-wiki/writeback-queue.md`，先确认覆盖缺口和关闭标准。
4. 如果是架构或模块增强，按模块派子 agent；不要从旧 docs 推断架构。
5. 写当前来源 enhancement JSON 到 `.understand-anything/enhancements/`。
   - 产品架构：必须来自源码/测试。
   - wiki 工具链：可以来自 repo-local skill/script。
6. 运行 `node scripts/apply-understand-enhancements.mjs`。
7. 更新受影响的 RepoWiki 页面、`coverage-manifest.md`、`writeback-queue.md`，并追加 `docs/repo-wiki/log.md`。
8. 运行校验：

```bash
node scripts/check-repowiki.mjs
node scripts/run-userwiki-qa-smoke.mjs --validate-only
node --input-type=module -e "import fs from 'node:fs'; import { validateGraph } from '/Users/a20250311/github/Understand-Anything/understand-anything-plugin/packages/core/dist/schema.js'; const graph=JSON.parse(fs.readFileSync('.understand-anything/knowledge-graph.json','utf8')); const result=validateGraph(graph); const bad=result.issues.filter(i=>i.level==='fatal'||i.level==='dropped'); console.log(JSON.stringify({success:result.success,issues:result.issues.length,bad:bad.length,nodes:graph.nodes.length,edges:graph.edges.length,layers:graph.layers?.length??0,tour:graph.tour?.length??0}, null, 2)); process.exit(bad.length ? 1 : 0);"
```

如果用户问当前工作区影响面，也可以生成或刷新 `.understand-anything/diff-overlay.json`。

## 当前图谱完整性的定义

对本项目来说，“图谱完整”默认指“可用于代码理解”：

- `.understand-anything/knowledge-graph.json` 存在并通过 schema validation。
- 输出语言是中文。
- 核心代码文件、RepoWiki 页面和 repo-local wiki skills 有图谱节点。
- 高价值模块有当前来源 enhancement。
- 高价值模块覆盖等级和待写回缺口记录在 `coverage-manifest.md` 与 `writeback-queue.md`。
- enhancement JSON 有非空 `key_nodes`、`semantic_edges`、`architecture_findings`、`tour_steps`。
- `scripts/apply-understand-enhancements.mjs` 可幂等运行。
- `node scripts/check-repowiki.mjs` 通过。
- `node scripts/run-userwiki-qa-smoke.mjs --validate-only` 通过。

不要把“完整”主动扩展成审计系统、完整测试覆盖证明或每个函数都有 trace，除非用户明确要求。

## 命名规则

- enhancement 文件用 kebab-case：`.understand-anything/enhancements/<domain>-<chain>.json`。
- enhancement 的 `module` 用稳定 kebab-case。
- RepoWiki 页面用 kebab-case。
- domain map 用 `*-map.md`。
- index 页面用名词：`sources.md`、`decision-index.md`、`testing-and-commands.md`。
- JSON 和 docs 里使用 repo-relative path。

## 红线

- 把 dashboard UI 状态当真相源。
- `outputLanguage` 是 `zh` 时写英文图谱内容。
- 从 archive 或旧 docs 推断当前架构。
- 用户要求子 agent 时，主线程绕过子 agent 去读窄范围代码。
- 把 docs-only 输出放进 `.understand-anything/enhancements/`。
- 更新 map 页面但不更新 `docs/repo-wiki/log.md`。
- 关闭 UserWiki 缺口但不更新 `coverage-manifest.md` 或 `writeback-queue.md`。
- 只改 `.agents` 不同步 `.claude`。

# 校验标准

## 当前图谱完整性

lotus-app 图谱满足下面条件时，可以说“完整到可用于代码理解”：

1. `.understand-anything/config.json` 的 `outputLanguage` 是 `zh`。
2. `.understand-anything/knowledge-graph.json` 存在。
3. Understand-Anything core `validateGraph` 没有 fatal/dropped 问题。
4. graph 有非空 `nodes`、`edges`、`layers`、`tour`。
5. 高价值模块有代码/测试来源 enhancement；wiki tooling enhancement 可以使用 repo-local skill/script 证据。
6. 每个 enhancement 都有非空 `key_nodes`、`semantic_edges`、`architecture_findings`、`tour_steps`。
7. `node scripts/apply-understand-enhancements.mjs` 可重复运行，不重复增加边或 tour。
8. `node scripts/check-repowiki.mjs` 通过。
9. `coverage-manifest.md` 与 `writeback-queue.md` 记录高价值 domain 覆盖等级、待写回缺口和关闭标准。

## 完整性的边界

“完整”不自动等于：

- 所有风险都是一等图谱节点
- 已证明完整测试覆盖
- 每个函数都有 deep trace
- 每个 untracked 文件都有图谱节点
- dashboard 视觉布局符合用户预期

用户问到这些更严格含义时，要先说明边界，再做针对性验证。

## 标准命令

RepoWiki 和 skill 校验：

```bash
node scripts/check-repowiki.mjs
```

合并图谱增强：

```bash
node scripts/apply-understand-enhancements.mjs
```

Understand-Anything schema 校验：

```bash
node --input-type=module -e "import fs from 'node:fs'; import { validateGraph } from '/Users/a20250311/github/Understand-Anything/understand-anything-plugin/packages/core/dist/schema.js'; const graph=JSON.parse(fs.readFileSync('.understand-anything/knowledge-graph.json','utf8')); const result=validateGraph(graph); const bad=result.issues.filter(i=>i.level==='fatal'||i.level==='dropped'); console.log(JSON.stringify({success:result.success,issues:result.issues.length,bad:bad.length,nodes:graph.nodes.length,edges:graph.edges.length,layers:graph.layers?.length??0,tour:graph.tour?.length??0}, null, 2)); process.exit(bad.length ? 1 : 0);"
```

## 效果验证

效果验证选 2-3 条真实链路：

- Runtime permission chain
- LLM gateway streaming chain
- MCP dynamic tool registration chain
- Frontend chat rendering chain
- Storage/path auth/file preview chain
- UserWiki skill system
- Coverage manifest / writeback queue maintenance loop

一条链路通过，需要图谱能回答：

- 关键文件
- 主要语义边
- 相关 guided tour
- 相关测试锚点，如果存在
- 已知缺口或 architecture findings

如果链路只在 prose 里存在，没有节点、边或 tour，标记为不完整。

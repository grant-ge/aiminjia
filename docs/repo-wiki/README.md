# RepoWiki

本目录是 AIjia / lotus-app 的 graph-derived RepoWiki。它从
`.understand-anything/knowledge-graph.json`、当前权威文档和源码入口整理而来，用于快速理解项目结构、维护导航和团队交接。

RepoWiki 只做导航、索引、摘要和链接，不替代原始文档。架构判断仍以 `CLAUDE.md`、`AGENTS.md`、`docs/architecture-blueprint.md`、`docs/decisions/*.md`、`docs/release-playbook.md` 和源码为准。

## 当前图谱状态

- 图谱文件：`.understand-anything/knowledge-graph.json`
- 语言：中文，来自 `.understand-anything/config.json` 的 `outputLanguage: "zh"`
- 当前模式：全库 deterministic scan + 多轮子 agent LLM 增强
- 覆盖范围：React/TypeScript 前端、Tauri/Rust Runtime、LLM 网关、工具系统、MCP、workspace/path_auth、managed runtime dependencies、数字员工、技能/pending、存储、测试、脚本和当前文档入口

## 页面

- `index.md`: 项目概览、layer 概览和 guided tour。
- `sources.md`: 真相源优先级、图谱输入和更新规则。
- `architecture-map.md`: 全局架构地图。
- `runtime-map.md`: Rust Runtime、LLM、工具、MCP、managed runtime、storage/path_auth 和 employee runtime。
- `frontend-map.md`: 前端启动、事件、状态、聊天渲染、技能/pending/员工 UI。
- `testing-and-commands.md`: 常用验证命令和测试分层。
- `decision-index.md`: 当前决策文档索引。
- `log.md`: RepoWiki 更新日志。

## 更新规则

1. 先更新或重建 `.understand-anything/knowledge-graph.json`。
2. 按 `index.md` 的 layer/tour 更新对应页面。
3. 新增页面必须在本 `README.md` 和 `index.md` 中挂入口。
4. 不能把 archive、run report 或历史 plan 提升为当前真相源。
5. 更新后运行：

```bash
node scripts/check-repowiki.mjs
node --input-type=module -e "import fs from 'node:fs'; import { validateGraph } from '/Users/a20250311/github/Understand-Anything/understand-anything-plugin/packages/core/dist/schema.js'; const graph=JSON.parse(fs.readFileSync('.understand-anything/knowledge-graph.json','utf8')); const result=validateGraph(graph); const bad=result.issues.filter(i=>i.level==='fatal'||i.level==='dropped'); console.log(JSON.stringify({issues:result.issues.length,bad:bad.length}, null, 2)); process.exit(bad.length ? 1 : 0);"
```

## UserWiki Skills

日常问答和使用入口已固化到 `.agents/skills/userwiki/SKILL.md`，并镜像到 `.claude/skills/userwiki/SKILL.md`。

图谱、RepoWiki、enhancement schema 和校验维护规则已固化到 `.agents/skills/wiki-maintainer/SKILL.md`，并镜像到 `.claude/skills/wiki-maintainer/SKILL.md`。

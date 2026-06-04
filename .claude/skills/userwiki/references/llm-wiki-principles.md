# LLM Wiki 原则

## 核心概念

UserWiki 应被理解为 LLM 维护的项目知识中间层，而不是一次性摘要或普通 RAG。

普通 RAG 更像查询时临时检索 raw chunks；LLM Wiki 更像持续把 raw source 编译成可读、可链接、可校验的 wiki。它的价值来自复用和回写：每次有效探索都应该让中间层变得更好。

## 本仓库映射

| LLM Wiki 层 | lotus-app 中的对应物 |
|---|---|
| Raw source | `src/`、`src-tauri/`、测试、`AGENTS.md`、`CLAUDE.md`、当前权威 docs、repo-local skill/script |
| Compiled wiki | `.understand-anything/knowledge-graph.json`、`.understand-anything/enhancements/*.json`、`docs/repo-wiki/` |
| Query layer | `userwiki` 问答、影响面分析、文件解释、新人路径 |
| Writeback layer | `wiki-maintainer`、enhancement JSON、RepoWiki 页面、QA smoke fixtures |
| Lint / QA | `scripts/check-repowiki.mjs`、`scripts/run-userwiki-qa-smoke.mjs --validate-only`、真实问答案例 |

## 回答口径

日常问答时遵循这个顺序：

1. 先读 compiled wiki：RepoWiki、knowledge graph、enhancement、guided tour。
2. 如果架构事实不清楚，再读 raw source：当前源码和测试优先。
3. 回答必须落到模块、文件、上下游、测试、文档和不确定点。
4. 如果发现 compiled wiki 缺覆盖、过期或互相矛盾，不要假装完整；说明缺口并切到 `wiki-maintainer`。

## 完整性口径

UserWiki 的完整性不是“每个文件都有节点”。更有用的口径是：它能否回答高频工程问题。

典型问题包括：

- 我要改一个功能，会影响哪些模块？
- 这条 runtime / frontend / connector 链路怎么走？
- 哪些结论有源码或测试证据？
- 哪些图谱边、tour 或 RepoWiki 页面还缺？
- 新发现应该写回哪里？

## 子 agent 分工

广范围补全时，子 agent 应像 compiler pass：

- 小范围探索：用较轻模型，只读窄模块，输出关键文件、语义边、发现和缺口。
- 中等写入：用更稳模型，把探索结果落为 enhancement JSON。
- 最终整合：用强模型做跨模块冲突判断、RepoWiki 更新和验收。

主线程负责拆范围、审稿、合并和跑校验；不要用一个大 agent 通读全仓后直接下结论。

## 转维护条件

这些情况说明已经不是普通 userwiki 问答，应转 `wiki-maintainer`：

- 需要新增或修改 `.understand-anything/enhancements/*.json`。
- 需要更新 `docs/repo-wiki/**`。
- 问答暴露了缺节点、缺语义边、缺 guided tour 或过期结论。
- 需要新增 QA smoke case 或改校验脚本。
- 需要派子 agent 做模块级知识编译。

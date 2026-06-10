# 维护路由

`userwiki` 负责回答怎么用 wiki 和日常 wiki 问答。`wiki-maintainer` 负责改 wiki 系统本身。

## 留在 userwiki

这些问题留在 `userwiki`：

- 怎么安装或初始化 wiki
- 怎么使用 dashboard/chat/diff/explain/onboard
- 预估新增功能影响面
- 解释文件或模块
- 总结当前 diff 影响面
- 推荐新人阅读路径
- 判断图谱是否完整到“可用于代码理解”

## 转到 wiki-maintainer

这些问题切到 `wiki-maintainer`：

- 创建或更新 `.understand-anything/enhancements/*.json`
- 派子 agent 补图谱
- 修改 `docs/repo-wiki/**`
- 修改 `scripts/check-repowiki.mjs`
- 修改 `scripts/apply-understand-enhancements.mjs`
- 校验或修复图谱完整性
- 修改来源优先级、schema、命名、引用、更新规则或检查规则

## 转交话术

```text
这个问题已经不是普通 wiki 问答，而是图谱/RepoWiki 维护。我会切到 wiki-maintainer，按当前源码、测试或 repo-local skill/script 来源补充并跑校验。
```

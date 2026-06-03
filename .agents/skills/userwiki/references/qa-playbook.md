# 问答手册

真实问答验收样例见 `references/qa-examples.md`。需要跑 CLI smoke 时，使用 `scripts/run-userwiki-qa-smoke.mjs`。

## 问：我想增加一个功能，会影响哪些点？

适用问法：

- "wiki 我想增加一个功能，会影响哪些点？"
- "我要改 Runtime 权限，会影响哪些模块？"
- "新增一个设置项要看哪些地方？"

回答结构：

1. 可能涉及的模块/layer
2. 关键文件
3. 上游入口
4. 下游影响
5. 相关测试
6. 需要同步更新的文档/RepoWiki
7. 图谱不确定点
8. 建议下一步

方法：

1. 用关键词搜索图谱节点。
2. 跟 1-hop 语义边。
3. 看 layer 和 guided tour。
4. 如果是重要架构判断，用当前源码或测试确认。
5. 如果图谱没有命中，说明当前图谱未覆盖，并转 `wiki-maintainer`。

## 问：这个文件是干什么的？

适用问法：

- "这个文件是干什么的？"
- "wiki explain 这个文件"

回答结构：

1. 文件职责
2. 所属 layer/module
3. 重要入边
4. 重要出边
5. 相关测试
6. 什么时候应该改它
7. 不确定点

语义等价于 `/understand-explain <path>`，但不要主动打开浏览器。

## 问：当前改动影响哪些模块？

适用问法：

- "当前改动影响哪些模块？"
- "这次提交要注意什么？"

回答结构：

1. changed files 对应的图谱节点
2. 1-hop 受影响组件
3. 受影响 layer
4. 风险或注意点
5. 建议运行的测试
6. 需要更新的 docs/wiki
7. 图谱缺口

方法：

1. 跑 `git diff --name-only`，必要时加入 untracked files。
2. 在 `.understand-anything/knowledge-graph.json` 里匹配 `filePath`。
3. 找 source 或 target 包含 changed node 的边。
4. 只有用户需要 dashboard overlay 时，才写 `.understand-anything/diff-overlay.json`。

## 问：当前图谱完整了吗？

先说明完整性的口径：

- 如果是“用于代码理解”，看当前图谱、enhancement、RepoWiki 和校验是否通过。
- 不要主动扩展成审计、测试覆盖证明或函数级全链路 trace，除非用户明确问。

检查：

- `outputLanguage` 是 `zh`
- 图谱 schema validation 通过
- nodes / edges / layers / tour 非空
- enhancement files 已存在并合入
- `node scripts/check-repowiki.mjs` 通过

## 问：新人应该先看什么？

优先从这些地方回答：

- `docs/repo-wiki/index.md`
- graph guided tour
- `docs/repo-wiki/architecture-map.md`
- `docs/repo-wiki/runtime-map.md`
- `docs/repo-wiki/frontend-map.md`

给短路径，不要堆一长串资料。

## 什么时候转 wiki-maintainer

这些情况转 `wiki-maintainer`：

- 当前源码文件没有图谱节点
- 需要写 enhancement JSON
- 需要更新 RepoWiki 页面
- 需要改 schema 或校验脚本
- 用户要求重新生成、补充或修复 wiki

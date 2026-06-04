---
name: userwiki
description: 当用户用中文询问项目 wiki、UserWiki、仓库知识库、功能影响面、这个文件是干什么的、当前改动影响哪些模块、新人该先看什么、图谱完整了吗、如何安装或使用 Understand-Anything 时使用。日常 wiki 问答、安装、使用、影响面分析都走本 skill。
license: Internal
---

# userwiki

## 用途

`userwiki` 是 lotus-app 项目 wiki 的用户入口。

它负责帮助用户用自然语言询问项目：

- 功能会影响哪些模块
- 文件或模块是干什么的
- 当前 git diff 影响面是什么
- 新人应该按什么顺序读项目
- 当前图谱是否完整
- Understand-Anything / RepoWiki 怎么安装、初始化和使用

底层信息来自：

- `.understand-anything/knowledge-graph.json`
- `docs/repo-wiki/`
- Understand-Anything 命令
- 必要时的当前源码和测试

如果用户要生成、修复、补充、校验图谱或 RepoWiki，切到 `wiki-maintainer`。

## 按需读取

- 安装和初始化：读 `references/install.md`。
- 使用命令：读 `references/usage.md`。
- 理解 UserWiki / LLM Wiki 的工作心智：读 `references/llm-wiki-principles.md`。
- 日常问答、功能影响面、文件解释、diff 影响、新人路径：读 `references/qa-playbook.md`。
- 测试问答效果、做真实问答验收：读 `references/qa-examples.md`，CLI 用例在 `references/qa-smoke-cases.json`。
- 需要判断是否转维护流程：读 `references/maintenance-routing.md`。
- 图谱缺失、英文输出、过期、dashboard token 等问题：读 `references/troubleshooting.md`。

## 中文触发语

这些问题都应该命中 `userwiki`：

- "wiki 我想增加一个功能，会影响哪些点？"
- "项目 wiki 怎么用？"
- "userwiki 当前改动影响哪些模块？"
- "这个文件是干什么的？"
- "新人应该先看什么？"
- "当前图谱完整了吗？"
- "Runtime 权限链路怎么走？"
- "怎么安装 Understand-Anything？"
- "怎么打开 dashboard / 怎么问图谱？"

## 回答规则

1. 先用图谱和 RepoWiki 做导航。
2. 架构事实不清楚时，再读当前源码和测试确认。
3. 用户问当前改动时，用 graph-based diff impact 分析。
4. 把 UserWiki 理解为 LLM 维护的知识中间层：先读 wiki/enhancement 复用已有理解，必要时回到 raw source 校验，新缺口要沉淀为维护任务。
5. 如果图谱缺节点、缺边或明显过期，要明确说明，并转 `wiki-maintainer`。
6. 不要主动操作浏览器；除非用户明确要求 dashboard 或浏览器验证。
7. 回答要落到模块、文件、测试、文档、疑点和下一步。

## 路由

| 用户意图 | 处理 |
|---|---|
| 安装、初始化、怎么用 wiki | 本 skill |
| 功能影响面预估 | 本 skill，读 `qa-playbook.md` |
| 当前 diff 影响面 | 本 skill，读 `qa-playbook.md` |
| 文件或模块解释 | 本 skill，读 `qa-playbook.md` |
| LLM Wiki / UserWiki 方法论解释 | 本 skill，读 `llm-wiki-principles.md` |
| 生成、补充、修复图谱 | 切到 `wiki-maintainer` |
| enhancement JSON、schema、校验脚本 | 切到 `wiki-maintainer` |

## 功能影响面回答格式

用户问“我要加一个功能，会影响哪些点？”时，必须按这个结构回答：

1. 可能涉及的模块/layer
2. 关键文件
3. 上游入口
4. 下游影响
5. 相关测试
6. 需要同步更新的文档/RepoWiki
7. 图谱不确定点
8. 建议下一步

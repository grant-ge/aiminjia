# 来源策略

## 真相源顺序

维护 RepoWiki 或图谱增强时，按这个顺序取证：

1. 当前源码和测试。
2. `AGENTS.md`、`CLAUDE.md`。
3. 当前权威文档：
   - `docs/README.md`
   - `docs/architecture-blueprint.md`
   - `docs/decisions/*.md`
   - `docs/release-playbook.md`
   - `docs/runtime-manager.md`
   - `docs/test-intents/README.md`
4. `.understand-anything/knowledge-graph.json`。
5. archive、旧 plan、历史报告、handoff。

## docs 可以做什么

docs 可以支持：

- 命令说明
- 发布流程
- 历史决策
- 政策和约束
- 当前文档入口

docs 不能单独生成当前架构事实。只要是在说 runtime、frontend、storage、tools、LLM、MCP、permissions 或 state flow 当前怎么工作，就必须有当前源码或测试证据。

## enhancement 的来源规则

`.understand-anything/enhancements/*.json` 必须来自当前仓库文件。

允许作为证据：

- `src/`、`src-tauri/src/`、`scripts/` 下的源码或脚本
- `src-tauri/tests/`、`tests/`、`docs/test-intents/` 下的测试或测试规范
- 直接影响运行、构建、权限、工具链的当前 config
- `.agents/skills/` 和 `.claude/skills/` 下的 repo-local skills，但只能用于 wiki/tooling 工作流增强

不能作为唯一证据：

- `docs/archive/**`
- 旧设计 plan
- dashboard 截图
- 历史对话摘要
- docs-only 治理页面

产品架构声明必须来自当前源码或测试。repo-local skill 只能支撑 wiki/tooling 工作流声明。

## RepoWiki 规则

RepoWiki 可以总结 docs 和图谱元数据，但必须保留原始权威链接。不要把整份原始文档复制进 RepoWiki。

RepoWiki 页面新增或改变重要架构判断时，要写清楚对应源码、测试或权威文档路径。

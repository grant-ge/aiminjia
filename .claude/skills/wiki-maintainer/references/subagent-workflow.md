# 子 agent 工作流

用户要求广范围仓库分析、补图谱，或明确说“多使用子 agent”时，使用子 agent。

## 主线程职责

主线程负责：

- 拆范围
- 约束 enhancement schema
- 合并输出
- 运行校验命令
- 汇总结论

用户要求子 agent 分析时，主线程不能偷偷绕过子 agent 去读窄范围业务代码并替代它们的结论。

## 推荐拆分

每个独立模块一个子 agent：

- Runtime turn/session/tool permission
- LLM gateway/provider/streaming
- MCP dynamic tool registration
- Managed runtime supply chain
- Storage/workspace/path auth/file preview
- Frontend chat state/rendering
- Frontend employee settings/file preview
- Frontend skill/pending queue
- UserWiki skill system

## 子 agent prompt 必须包含

- 当前 repo 路径
- 模块名
- 只读或写入范围
- 产品架构必须代码/测试优先，不能用旧 docs 推断
- 如果是 wiki tooling，可以读当前 repo-local skill/script
- 如果写 enhancement JSON，要给输出文件路径
- 必须遵守 `references/enhancement-schema.md`
- 最终要列：读了哪些文件、关键节点、语义边、发现、缺口

## 模型选择

- `gpt-5.3-codex-spark`：边界清楚的模块提取、schema 填写、快速图谱验证。
- 更强默认模型：最终集成、schema 变更、跨模块判断、冲突处理。

## 收尾

子 agent 完成后：

1. 检查输出 schema。
2. 规范化 repo-relative paths。
3. 跑 `node scripts/apply-understand-enhancements.mjs`。
4. 跑 `node scripts/check-repowiki.mjs`。
5. 跑 Understand-Anything schema validation。
6. 关闭不再需要的子 agent。

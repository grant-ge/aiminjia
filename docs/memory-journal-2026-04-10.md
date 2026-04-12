# 对话记忆日志：lotus-app 后端架构改造

## 时间
2026-04-10 11:04 CST

## 主题
将 lotus-app 后端从 Tauri command-centric 架构重构为 runtime-first 架构，对标 claude-code-best 的分层模型。

## 决策

### 改造策略
- 渐进重构，业务不停机，直接替换，不做灰度
- 分 5 期（Phase 0-4），每期有明确 kill list / rollback / golden trace

### 身份模型
- 第 1 期引入 SessionId / RunId / AgentId / ToolCallId
- SessionId 暂时复用 conversation_id 字符串值，但类型系统升级为新类型
- RunId 每次 send_message 新生成
- RunController / RuntimeEvent / 新 store 禁止使用裸 conversation_id

### 事件协议
- 前端兼容基线：streaming:delta / streaming:done / tool:executing / tool:completed / message:updated / agent:idle
- Runtime 内部使用结构化 RuntimeEventKind，通过 TauriEventAdapter 映射到 legacy 事件
- 前两期前端事件协议 100% 兼容

### 工具系统
- ToolDispatcher 成���唯一入口
- PluginContext 和 ToolPlugin 均标记 #[deprecated]
- 新工具走 ToolExecutionContext + CapabilityContext
- 旧工具通过 LegacyToolAdapter 桥接

### Python session
- 按 RunId 隔离，不再按 conversation_id 共享
- 跨 run 依赖 artifact / snapshot / precompute cache / loaded manifest 恢复

### SubAgent
- 分 A/B/C 三段演进
- A: child run + cancel + 受限工具集
- B: background + message bridge（已接到真实 sub_agent.rs 路径）
- C: resume / worktree / team 最小模型

### Store
- 领域化拆分：session / settings / persona / file_record / conversation / audit / memory / run / task / tool_call / agent_invocation
- 所有 commands 通过 RuntimeRepositoryFacade 访问，不再直接摸 AppStorage

### Skill 归属
- Skill 是 QueryEngine 的策略插件，不是 TaskRuntime 的 supervisor

## 排除
- 不照搬 claude-code-best 的 CLI 外壳 / tmux backend / MCP 多实例
- 不做灰度 / 双写 / 旁路接入
- 不在前两期改前端事件协议
- 不一步追平 claude-code-best 的全部生态

## 遗留
- transport/tauri_commands/ 下 8 个非 chat adapter 已写好并正常编译，但 generate_handler! 仍指向 commands::*，最后一步切换还没做
- QueryEngine 在真实 send_message 主循环中还没完全接管，chat_runtime_impl.rs 仍是主路径
- PluginContext 虽已 deprecated，但 23 个 builtin tool 仍走旧 trait + allow(deprecated)
- background run 的 event_bus 在生产 PluginContext 构造处仍设为 None，需要 chat 路径在有 bus 时注入

## 产出

### 文档（19 files）
- docs/architecture-blueprint.md — 总蓝图
- docs/phase-0-baseline-audit.md — 第 0 期设计
- docs/phase-1-session-runtime.md — 第 1 期设计
- docs/phase-2-tool-permission-store.md — 第 2 期设计
- docs/phase-3-task-agent.md — 第 3 期设计
- docs/phase-4-store-transport-subagent-c.md — 第 4 期设计
- docs/README-architecture-plan.md — 文档索引
- docs/architecture-audit/*.md — 4 份审计文档
- docs/superpowers/plans/*.md — 总计划 + 5 份分期实施计划 + 索引

### 新模块（88 files）
- src-tauri/src/runtime/ — 身份、状态、事件、运行时、store、tools、task、agent
- src-tauri/src/runtime_audit/ — legacy trace capture
- src-tauri/src/transport/ — TauriEventAdapter、RuntimeHost、8 个 command adapter
- src-tauri/src/plugin/builtin/tools/echo_runtime.rs — 首个 RuntimeTool 样板
- src-tauri/tests/ — 27 个集成测试文件

### 重构现有模块（114 files）
- commands/*.rs → thin adapter + RuntimeRepositoryFacade
- lib.rs → wire facade
- llm/gateway.rs → provider adapter
- llm/sub_agent.rs → AgentRuntime + background bridge
- plugin/context.rs → deprecated
- plugin/tool_trait.rs → deprecated
- python/session.rs → RunId scope
- storage/file_store/mod.rs → RuntimeRepositoryFacade + domain store impls

## 涉及文档
- /Users/a20250311/github/claude-code-best — 对标参考项目（Bun + TS monorepo）
- claude-code-best 关键文件：src/query.ts, src/QueryEngine.ts, src/tools.ts, src/utils/permissions/permissions.ts, src/state/store.ts, src/utils/tasks.ts, src/tools/AgentTool/

## Git 信息
- 仓库：/Users/a20250311/IdeaProjects/lotus-app
- 分支：pzc
- 最新 commits：
  - 9e18984 refactor: migrate existing modules to runtime-first architecture
  - 98dba1d feat: add runtime-first backend architecture (Phase 0-4)
  - 8528c6d docs: add backend architecture blueprint, audit docs, and migration plans

## 下一次恢复提示

```
我在 lotus-app (分支 pzc) 上做后端架构改造，对标 claude-code-best。
Phase 0-4 的核心实现已提交（9e18984），500+ 测试通过。

还需要继续的：
1. 把 generate_handler! 从 commands::* 切到 transport::tauri_commands::*
2. 让 QueryEngine 真正接管 chat_runtime_impl.rs 的主循环
3. 逐步把 builtin tools 从旧 ToolPlugin trait 迁移到 RuntimeTool
4. 在生产 PluginContext 构造处注入 event_bus

参考：docs/architecture-blueprint.md 和 docs/superpowers/plans/README.md
```

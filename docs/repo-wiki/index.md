# RepoWiki Index

## Project Overview

AIjia / lotus-app 是一个 Tauri 2 桌面端应用，前端使用 React、TypeScript、Vite、Zustand、Tailwind CSS，后端使用 Rust、Tokio、Serde，并接入 RuntimeTool、MCP、LLM gateway 和 managed runtime dependencies。

当前 Understand-Anything 图谱覆盖：

- 9361 个节点
- 10353 条边
- 25 个 architecture layers
- 119 个 guided tour steps
- 415 个 LLM-enhanced 节点
- 132 个代码/维护架构评审概念节点
- 26 份当前源码/测试/skill 来源 enhancement JSON

注：其中 `.understand-anything/enhancements/llm-visible-reply-language-anchor.json` 带 `source_ref=origin/main@c4bcc8b7`，覆盖 AIjia v2 provider 语言锚定实现和中文回复回归 intents。

## Architecture Layers

| Layer | 说明 | 规模 |
|---|---:|---:|
| 文档与工程约束 | AGENTS、CLAUDE、architecture blueprint、decisions、release 和 test-intents 的权威入口 | 105 |
| 项目配置 | package、TypeScript、Vite、ESLint、workspace 等工程配置 | 25 |
| CI/CD 工作流 | 质量检查、Windows unsigned staging 构建和 finalize 闸门 | 3 |
| 脚本与发布工具 | release、bundled runtime、CI 上传、签名和辅助脚本 | 34 |
| React 前端应用入口 | main/App 和全局 side-effect hook | 14 |
| 前端组件层 | 聊天、设置、shell、skill、team 等 UI 组件 | 279 |
| 前端业务功能模块 | 聊天、数字员工、技能中心、设置等业务页面 | 84 |
| 前端 Zustand 状态层 | chat、streaming、pending、skill、runtime、file preview 和 UI route 状态 | 40 |
| 前端 Hooks 与事件订阅 | useStreaming、useChat、useTurnRenderModel 等事件与 action hook | 20 |
| 前端 API/工具封装 | `src/lib/tauri.ts` 和前端工具函数 | 25 |
| 前端国际化 | i18n 初始化与语言资源 | 3 |
| 前端样式与主题 | 全局样式、主题 token、字体缩放和 skin 工具 | 6 |
| 前端测试 | 前端测试 setup 和样式/token 回归 | 3 |
| Tauri Host 配置与入口 | Tauri 配置、capabilities、commands、prompts、resources | 136 |
| Tauri IPC 与事件适配 | command/event adapter 到 runtime 的边界层 | 21 |
| Rust Runtime / 会话编排 | Session/Query/Chat Turn、模型调用、工具回合和事件发出 | 106 |
| Rust Runtime / 工具系统 | RuntimeTool、catalog、permission、dispatcher 和 legacy adapter | 38 |
| Rust Runtime / Agent 与任务 | sub-agent、数字员工、任务、inbox、template snapshot | 36 |
| Rust Runtime / 状态存储 | session/message/tool/file/permission/workspace/employee stores | 14 |
| Rust Runtime / MCP 集成 | MCP config、connection、manager、runtime tool | 5 |
| LLM 网关与模型适配 | gateway、router、providers、streaming 和 legacy executor | 31 |
| 本地存储与文件系统能力 | workspace-first、file_store、path_auth 和本地路径安全 | 37 |
| 遗留插件桥接 | 旧插件系统与 RuntimeTool 的过渡桥 | 18 |
| Rust 集成测试 | runtime、transport、tools、storage 和 review_ 架构护栏 | 302 |
| 代码架构评审 | 当前源码、目标分支源码、测试和 repo-local skill 来源的增强材料生成的 architecture review 概念节点 | 132 |

## Guided Tour

1. 项目约束与当前真相源：`AGENTS.md`、`CLAUDE.md`、`docs/README.md`、architecture blueprint 和 decisions。
2. 前端启动与事件入口：`src/main.tsx`、`src/App.tsx`、`useStreaming`、`usePendingEventListener`、`src/lib/tauri.ts`。
3. 聊天状态与渲染语义：`useChat`、`chatStore`、`sessionStore`、`streamingStore`、`useTurnRenderModel`、`MessageList`。
4. 数字员工派活到聊天：`HireWizard`、`EmployeeDrawer`、`triggerPrechecks`、`seedDispatchConversation`、`ChatPage`。
5. Tauri Transport 进入 Runtime：chat command、runtime host、`SessionRuntime`、`TauriEventAdapter` 和 command/event contract。
6. Turn Loop 与工具执行：`RuntimeChatTurnDriver`、`ToolRoundDriver`、`QueryEngine`、`ToolDispatcher`。
7. MCP 动态工具链：`McpServerManager`、`McpConnection`、`McpRuntimeTool`、`ToolRegistry`。
8. Bundled Runtime 供应链：`ensure-bundled-runtime`、`runtime-sources`、prepare scripts、resolver、manager。
9. 发布闸门：`release.py`、Windows CI unsigned staging、本地签名、`finalize-release.yml`。
10. 测试与兼容护栏：CI、`review_*.rs`、`send_message_runtime_path_test`、test-intents / AEIT / `aijia` CLI。
11. LLM 网关与流式协议：gateway、router、provider、streaming、event bus、event adapter。
12. Workspace 与文件安全边界：CurrentUserStorage、UserScope、WorkspaceManager、FileManager、file_store、authorized workspace store 和 path_auth。
13. Managed Runtime 供应链：ensure/prepare、resolver chain、bundled resolver、manager、RuntimePanel。
14. 技能、Pending 与员工派活：SkillCenter、skillStore、pendingStore、employee runner/store/template。
15. 代码增强 tour：以代码/测试为事实源的 guided tour steps 覆盖 app shell/settings/updater/billing/network、auth/user-scope/storage boundary、Tauri command/event contract、前端 chat、employee/settings/file preview、skill/pending、LLM gateway、AIjia v2 visible reply language anchor 与中文回复回归 intents、prompt/context/compaction/cost、managed runtime、MCP、runtime permission、storage/path_auth、employee dispatch、agenda scheduler、task tools、Agent foreground auto-background、shell auto-background、team mode、IM core、skill registry 和 test-intents/AEIT。
16. UserWiki skill tour：以 repo-local skill/script 为事实源的 guided tour steps 覆盖 userwiki 问答入口、wiki-maintainer 维护入口、校验脚本和 LLM Wiki 知识中间层原则。

## Current-Source Enhancements

本轮图谱增强主体来自当前源码、目标分支源码、测试和 repo-local skill/script，不从 docs 推断架构事实。已合并的 enhancement 文件：

- `.understand-anything/enhancements/frontend-chat-state-rendering.json`
- `.understand-anything/enhancements/app-shell-settings-updater-billing.json`
- `.understand-anything/enhancements/billing-subscription-account-network.json`
- `.understand-anything/enhancements/context-budget-truncation-matrix.json`
- `.understand-anything/enhancements/frontend-employee-settings-file-preview.json`
- `.understand-anything/enhancements/frontend-skill-pending.json`
- `.understand-anything/enhancements/im-channel-core-manager.json`
- `.understand-anything/enhancements/llm-gateway-provider-streaming.json`
- `.understand-anything/enhancements/llm-visible-reply-language-anchor.json`
- `.understand-anything/enhancements/managed-runtime-supply-chain.json`
- `.understand-anything/enhancements/prompt-context-compaction-cost.json`
- `.understand-anything/enhancements/runtime-agenda-scheduler.json`
- `.understand-anything/enhancements/runtime-agent-foreground-auto-background.json`
- `.understand-anything/enhancements/runtime-employee-dispatch.json`
- `.understand-anything/enhancements/runtime-shell-auto-background.json`
- `.understand-anything/enhancements/runtime-task-tools.json`
- `.understand-anything/enhancements/runtime-team-mode-subagent.json`
- `.understand-anything/enhancements/rust-mcp-dynamic-tools.json`
- `.understand-anything/enhancements/rust-runtime-turn-tool-permission.json`
- `.understand-anything/enhancements/skill-management-registry-sync.json`
- `.understand-anything/enhancements/storage-workspace-pathauth.json`
- `.understand-anything/enhancements/tauri-command-event-contracts.json`
- `.understand-anything/enhancements/test-intents-aijia-cli.json`
- `.understand-anything/enhancements/user-scope-auth-storage-boundary.json`
- `.understand-anything/enhancements/userwiki-llm-wiki-principles.json`
- `.understand-anything/enhancements/userwiki-skill-system.json`

## LLM Wiki Working Model

UserWiki 按 LLM Wiki 心智维护：当前源码、测试、权威文档和 repo-local skill/script 是 raw source；`.understand-anything/knowledge-graph.json`、enhancement JSON 和 RepoWiki 是 compiled wiki；日常问答先复用 compiled wiki，事实不清楚时回 raw source 校验；问答暴露的缺口通过 `wiki-maintainer` writeback 到 enhancement、RepoWiki 或 QA smoke；`check-repowiki` 和 `run-userwiki-qa-smoke` 是 lint/QA 层。

## Coverage And Writeback

- `coverage-manifest.md`: 记录高价值 domain 的 strong、partial、queued、deferred 覆盖等级，以及升级到 strong 的完成标准。
- `writeback-queue.md`: 记录真实问答、覆盖审计和子 agent 探索暴露出的待写回缺口；队列项必须通过 enhancement、RepoWiki 更新和校验后才关闭。

## How To Use

- 新人入门：先读本页，再按 guided tour 读 `architecture-map.md`、`runtime-map.md`、`frontend-map.md`。
- 排查 runtime/tool/LLM 问题：先读 `runtime-map.md`，再跳到对应源码。
- 排查 UI/chat/streaming 问题：先读 `frontend-map.md`。
- 做发布或测试判断：读 `testing-and-commands.md` 和 `decision-index.md`。
- 判断 wiki 是否补够：读 `coverage-manifest.md` 和 `writeback-queue.md`。
- 日常 wiki 问答：使用 `userwiki`。
- 图谱、RepoWiki 或 enhancement 维护：使用 `wiki-maintainer`。

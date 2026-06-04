# RepoWiki Coverage Manifest

本文件记录 UserWiki 当前覆盖到什么程度，以及下一轮补图谱应优先补哪里。

Coverage manifest 不是产品事实源。它只用于维护 compiled wiki：把当前 `.understand-anything/enhancements/*.json`、RepoWiki 页面、QA smoke 和真实问答暴露出的缺口放到同一张台账里，避免用“感觉已经补够了”作为完成标准。

## Coverage Levels

| Level | 含义 | 可回答程度 |
|---|---|---|
| strong | 有当前源码/测试或 repo-local skill/script 来源 enhancement，RepoWiki 有入口，并通过校验 | 可以回答关键文件、上下游、测试锚点和已知风险 |
| partial | 有图谱或文档入口，但缺少完整 current-source enhancement 或缺少跨层边 | 可以导航，但回答影响面时必须回 raw source 校验 |
| queued | 已识别为高价值缺口，进入 writeback queue | 不应声称完整，先派子 agent 探索 |
| deferred | 有价值但不是当前高频工程问题 | 记录即可，不进入本轮补图谱主线 |

## High-Value Coverage

| Domain | Level | Evidence / Artifacts | Next Writeback |
|---|---|---|---|
| UserWiki method and maintenance loop | strong | `.understand-anything/enhancements/userwiki-skill-system.json`, `.understand-anything/enhancements/userwiki-llm-wiki-principles.json`, `.agents/skills/userwiki/`, `.agents/skills/wiki-maintainer/`, `scripts/check-repowiki.mjs`, `scripts/run-userwiki-qa-smoke.mjs`, `docs/repo-wiki/sources.md` | 真实问答暴露新缺口时补 QA smoke 或 writeback queue |
| Runtime turn / tool / permission | strong | `.understand-anything/enhancements/rust-runtime-turn-tool-permission.json`, `docs/repo-wiki/runtime-map.md` | 仅在权限策略或 tool schema 变化时补 |
| LLM gateway / provider / streaming | strong | `.understand-anything/enhancements/llm-gateway-provider-streaming.json`, `docs/repo-wiki/runtime-map.md` | prompt/context/cost 链路需要单独补 |
| MCP dynamic tools | strong | `.understand-anything/enhancements/rust-mcp-dynamic-tools.json`, `docs/repo-wiki/runtime-map.md` | 新 MCP 配置或动态工具注册变化时补 |
| Storage / workspace / path auth / file preview | strong | `.understand-anything/enhancements/storage-workspace-pathauth.json`, `docs/repo-wiki/runtime-map.md` | 账户作用域进入存储路径时补 account boundary |
| Managed runtime supply chain | strong | `.understand-anything/enhancements/managed-runtime-supply-chain.json`, `docs/repo-wiki/runtime-map.md` | 发布链路补签名/打包时再关联 |
| Frontend chat state and rendering | strong | `.understand-anything/enhancements/frontend-chat-state-rendering.json`, `docs/repo-wiki/frontend-map.md` | prompt/context 变化影响消息语义时补 |
| Frontend employee / settings / file preview | strong | `.understand-anything/enhancements/frontend-employee-settings-file-preview.json`, `docs/repo-wiki/frontend-map.md` | settings 与模型消费链路需补跨层边 |
| Skill / pending / registry / sync | strong | `.understand-anything/enhancements/frontend-skill-pending.json`, `.understand-anything/enhancements/skill-management-registry-sync.json`, `docs/repo-wiki/frontend-map.md` | 技能安装/同步协议变化时补 |
| Employee dispatch / agenda / task tools / team mode / IM | strong | `.understand-anything/enhancements/runtime-employee-dispatch.json`, `.understand-anything/enhancements/runtime-agenda-scheduler.json`, `.understand-anything/enhancements/runtime-task-tools.json`, `.understand-anything/enhancements/runtime-team-mode-subagent.json`, `.understand-anything/enhancements/im-channel-core-manager.json`, `docs/repo-wiki/runtime-map.md` | 数字员工跨会话策略变化时补 |
| App shell / settings / updater / billing / network | strong | `.understand-anything/enhancements/app-shell-settings-updater-billing.json`, `.understand-anything/enhancements/billing-subscription-account-network.json`, `docs/repo-wiki/frontend-map.md`, `docs/repo-wiki/runtime-map.md` | entitlement/checkout/recharge 未见本地闭环，保留为已知 gap |
| Auth / user scope / account / billing boundary | strong | `.understand-anything/enhancements/user-scope-auth-storage-boundary.json`, `.understand-anything/enhancements/billing-subscription-account-network.json`, `docs/repo-wiki/runtime-map.md`, `docs/repo-wiki/frontend-map.md` | 服务端 enterprise billing 行为、IM channel 深层回收和工具层最终 path_auth 调用点继续保留为已知 gap |
| Prompt / context / compaction / cost accounting | strong | `.understand-anything/enhancements/prompt-context-compaction-cost.json`, `.understand-anything/enhancements/context-budget-truncation-matrix.json`, `docs/repo-wiki/runtime-map.md`, `docs/repo-wiki/frontend-map.md` | 上下文预算矩阵已补；QueryEngine 预算阈值更正为普通 chat 主链未接入，cache token summary 写入继续保留为已知 gap |
| Tauri command / event contract surface | strong | `.understand-anything/enhancements/tauri-command-event-contracts.json`, `docs/repo-wiki/runtime-map.md`, `docs/repo-wiki/frontend-map.md` | `network:status` 缺标准 listener helper，command layer 迁移仍 mixed |
| test-intents / AEIT / `aijia` CLI | strong | `.understand-anything/enhancements/test-intents-aijia-cli.json`, `docs/repo-wiki/testing-and-commands.md`, `.agents/skills/usertest-intents/SKILL.md`, `.agents/skills/test-intents-cli-author/SKILL.md` | 保留 task 数量 13/14 和外部 tauri-pilot CLI 漂移为已知 gap |
| Release / signing pipeline | deferred | `docs/repo-wiki/testing-and-commands.md`, `docs/repo-wiki/decision-index.md`, `docs/release-playbook.md` | P3 后续补，避免本轮过宽 |

## Completion Rule

一个 domain 从 `queued` 或 `partial` 升到 `strong`，必须同时满足：

- 有当前源码、测试或 repo-local skill/script 证据。
- 有 enhancement JSON，且 `key_nodes`、`semantic_edges`、`architecture_findings`、`tour_steps` 非空。
- RepoWiki 有入口，必要时更新 runtime/frontend/testing 页面。
- 相关缺口从 `writeback-queue.md` 标记为 merged 或 validated。
- `node scripts/apply-understand-enhancements.mjs`、`node scripts/check-repowiki.mjs`、`node scripts/run-userwiki-qa-smoke.mjs --validate-only` 通过。

## Non-Goals

- 不要求每个函数都有 deep trace。
- 不要求每个 UI leaf component 都有独立节点。
- 不把 archive、旧 plan 或 dashboard UI 状态提升为当前事实源。
- 不把 coverage manifest 当作测试覆盖率报告。

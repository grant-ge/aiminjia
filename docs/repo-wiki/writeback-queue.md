# RepoWiki Writeback Queue

本文件记录 UserWiki 问答、覆盖审计和子 agent 探索暴露出的待写回缺口。

Writeback queue 的目标是让“还没补完”有明确状态，而不是在主线程里靠记忆维护。队列项只有在 enhancement、RepoWiki 和校验都完成后，才算关闭。

## States

| State | 含义 |
|---|---|
| candidate | 候选缺口，尚未确认是否进入本轮 |
| agent-exploring | 已派子 agent 只读探索，等待证据 |
| enhancement-draft | 已有 enhancement 草案，等待主线程合并和校验 |
| merged | enhancement 已合并到 knowledge graph，RepoWiki 已更新 |
| validated | merged 后校验通过，且 coverage manifest 已同步 |
| deferred | 已确认有价值，但不是本轮高优先级 |

## Active Queue

| ID | Domain | Priority | State | Agent / Model | Expected Artifact | Close Criteria |
|---|---|---:|---|---|---|---|
| WB-2026-06-04-001 | Auth / user scope / account / billing boundary | P1 | validated | Fermat + James / gpt-5.3-codex-spark + gpt-5.4 | `.understand-anything/enhancements/user-scope-auth-storage-boundary.json`, `.understand-anything/enhancements/billing-subscription-account-network.json` | 两个 enhancement 已合并，runtime/frontend 入口已更新，coverage 已升级 |
| WB-2026-06-04-002 | Prompt / context / compaction / cost accounting | P1 | validated | Halley + Noether + Aristotle + Averroes + Bohr / gpt-5.4 + gpt-5.3-codex-spark + gpt-5.4-mini | `.understand-anything/enhancements/prompt-context-compaction-cost.json`, `.understand-anything/enhancements/context-budget-truncation-matrix.json` | prompt/context enhancement 与上下文预算矩阵已合并，runtime/frontend 入口已更新，coverage 已升级 |
| WB-2026-06-04-003 | Tauri command / event contract surface | P2 | validated | McClintock / gpt-5.3-codex-spark | `.understand-anything/enhancements/tauri-command-event-contracts.json` | enhancement 已合并，跨前后端契约入口已更新，coverage 已升级 |
| WB-2026-06-04-004 | test-intents / AEIT / `aijia` CLI | P2 | validated | Kant + Descartes / gpt-5.3-codex-spark | `.understand-anything/enhancements/test-intents-aijia-cli.json` | enhancement 已合并，`testing-and-commands.md` 有入口，coverage 已升级 |
| WB-2026-06-04-005 | Release / signing pipeline | P3 | deferred | 未派发 | `.understand-anything/enhancements/release-signing-pipeline.json` | P1/P2 完成后再补，避免本轮过宽 |
| WB-2026-06-04-006 | Storage / workspace / path auth / file preview | P1 | candidate | 未派发 / tag-intake | `.understand-anything/enhancements/storage-app-data-contract.json` | 基于目标 main 源码补 app data root contract enhancement，更新 runtime/source/coverage/log 并通过校验 |
| WB-2026-06-04-007 | Managed runtime supply chain | P1 | candidate | 未派发 / tag-intake | `.understand-anything/enhancements/managed-runtime-cache-reinstall.json` | 基于目标 main 源码补 runtime cache reinstall / bundled fallback 行为，更新 runtime-map/coverage/log 并通过校验 |
| WB-2026-06-10-001 | Skill enablement / marketplace sync / runtime catalog | P1 | candidate | Dalton + Noether + Hypatia + Herschel / current session | `.understand-anything/enhancements/skill-enablement-registry-catalog.json` | 实现完成并验证后，补 skill enablement、marketplace sync、runtime catalog、AEIT CLI coverage，更新 RepoWiki maps/coverage/log 并通过校验 |

## Active Queue Details

### WB-2026-06-04-001

- Trigger: coverage audit after UserWiki LLM Wiki method update.
- Engineering question: 修改账户、登录态、用户作用域、计费入口或订阅展示时，会影响哪些前端入口、Tauri/Rust 边界、存储和网络调用。
- Current boundary: 本轮只接受当前源码/测试证据，不从旧 docs 推断账户或计费事实。
- Execution note: 原单一 agent 因上下文过宽失败，已拆成 user scope/auth/storage 与 billing/subscription/account network 两个更窄探索。

### WB-2026-06-04-002

- Trigger: coverage audit found LLM gateway streaming 已覆盖，但 prompt/context/compaction/cost accounting 尚未单独成链。
- Engineering question: 修改 prompt 构造、上下文裁剪、压缩或费用统计时，会影响 runtime、gateway、前端消息语义和测试锚点。
- Current boundary: 不把 provider streaming enhancement 直接等同于完整 prompt/cost coverage。
- Follow-up trigger: 用户用 UserWiki 排查“长对话里模型忘记前文”时暴露上下文预算硬编码表、生效性标注、旧 compact boundary 排障和 QueryEngine budget gap 口径不足。
- Execution note: Noether 校验普通 chat 主路径，Aristotle 校验工具/记忆/附件预算，Averroes 校验生效/半生效/未接入分类；Bohr 校验写回位置。

### WB-2026-06-04-003

- Trigger: coverage audit found Tauri IPC 和事件适配层有图谱节点，但缺少面向 contract surface 的跨前后端 writeback。
- Engineering question: 修改 Tauri command、invoke 参数、event payload 或 listener 时，前端、Rust handler、runtime adapter 和测试应同步检查哪里。
- Current boundary: 不扩大到全部 runtime turn loop，只追踪 IPC/event contract。

### WB-2026-06-04-004

- Trigger: coverage audit found test-intents 在文档和 skill 中有入口，但缺少脚本/CLI/test 来源 enhancement。
- Engineering question: 新增或修改 `aijia` CLI intent 子命令、AEIT task 或 intent test spec 时，应同步哪些脚本、规则和验证命令。
- Current boundary: docs/test-intents 可作入口说明，核心事实必须回到当前脚本、测试或 CLI。
- Execution note: 原 test-intents agent 因上下文过宽失败，已拆成 aijia CLI/package scripts 与 intent specs/rules 两个更窄探索。

### WB-2026-06-04-005

- Trigger: coverage audit identified release/signing as important but lower-frequency than runtime/account/prompt/test-intents。
- Engineering question: 修改发布、签名、updater 或 staging pipeline 时，如何定位 release playbook、CI workflow、脚本和验证闸门。
- Current boundary: deferred until P1/P2 queue items are closed or user explicitly asks for release wiki supplementation.

### WB-2026-06-04-006

- Trigger: user指出可以从 main/tag/commit 里发现重要 wiki 补充点；按 `v0.5.33..origin/main` 排查后发现 app data governance 进入源码契约。
- Engineering question: 修改 app data 根目录、legacy root 迁移、workspace artifacts 或用户级存储时，哪些 root entry 是 stable/transitional/workspace artifact/temporary/deprecated/review-only，哪些直接 root join 必须登记进 contract。
- Current boundary: 当前 wiki 工作树不在 `origin/main`，`src-tauri/src/storage/app_data_contract.rs` 只在目标分支对象中确认；补 enhancement 前应切到或合并目标 main，避免把当前工作树不存在的文件写成 validated current-source fact。
- Evidence from tag intake: `git show origin/main:src-tauri/src/storage/app_data_contract.rs`、`git grep origin/main app_data_contract -- src-tauri/src src-tauri/tests`。

### WB-2026-06-04-007

- Trigger: user指出可以从 main/tag/commit 里发现重要 wiki 补充点；按 `v0.5.33..main` 排查后发现 runtime cache reinstall / bundled fallback 行为超出现有 managed runtime wiki 颗粒度。
- Engineering question: 运行时依赖缺失、缓存损坏、用户安装过的 runtime package 被误覆盖、manifest 下载失败或 bundled fallback 触发时，`RuntimeManager` 如何决定保留现有 cache、从 bundled runtime bootstrap、还是执行 reinstall。
- Current boundary: 当前 wiki 工作树不在 local `main`；补 enhancement 前应在目标 main 上读取源码和测试，确认 `current_cache_result_if_available`、`install_from_bundled_fallback`、`ensure_managed`、`reinstall_managed` 与 runtime Tauri commands 的真实链路。
- Evidence from tag intake: `git grep main current_cache_result_if_available -- src-tauri/src/runtime/dependencies src-tauri/tests`、`git show main:src-tauri/tests/runtime_dependencies_manager_test.rs`。

### WB-2026-06-10-001

- Trigger: 技能中心改造讨论发现“市场/内置/已安装”不是纯前端状态，而是牵动 `skillsConfig.json`、登录用户作用域、marketplace 安装、官方技能更新、runtime catalog 注入、`Skill` 工具执行和 AEIT CLI 的跨层契约。
- Engineering question: 修改技能启用/关闭、市场添加、内置默认安装或官方技能更新时，哪些前端入口、Tauri IPC、Rust registry、上下文注入、runtime tool 和意图测试必须同步检查。
- Current boundary: candidate only。本轮先补设计计划和意图测试；不能在实现前把具体源码行为写成 validated wiki fact。
- Evidence from userwiki cross-check: `SkillCenterPage`、`SkillDetailPage`、`skillStore`、`App.tsx`、`SkillPopover`、`ChatBottomArea`、`HomeTaskComposerCard`、`WelcomeScreen`、`RichComposer`、`CurrentUserStorage`、`UserScopedPaths`、`skill_management.rs`、`sync_command.rs`、`global_sync.rs`、`get_skill_catalog()`、`LoadSkillRuntimeTool`。

## Intake Rule

新增队列项必须写清楚：

- 触发来源：用户问题、QA smoke 失败、覆盖审计或子 agent 发现。
- 需要回答的工程问题。
- 预期 artifact。
- close criteria。
- 为什么现在做或为什么 deferred。

## Close Rule

队列项关闭前必须确认：

- 子 agent 输出的证据路径存在。
- enhancement schema 合法。
- `node scripts/apply-understand-enhancements.mjs` 幂等。
- `node scripts/check-repowiki.mjs` 通过。
- `node scripts/run-userwiki-qa-smoke.mjs --validate-only` 通过。
- `coverage-manifest.md` 的对应 domain 已同步。

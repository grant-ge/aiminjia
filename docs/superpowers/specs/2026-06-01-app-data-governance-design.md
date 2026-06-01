# App Data Governance Design

> Date: 2026-06-01
> Worktree: `/Users/gezhigang/work-codeup/aijia/code/.worktrees/app-data-governance`
> Branch: `feat/app-data-governance`

## Goal

治理 AIjia 桌面端本地应用数据目录，收敛 `~/.renlijia/` 根级条目数量，清理可重生成文件，迁移历史用户产物，并保证老版本用户升级无感、可回滚、不丢数据。

本设计只覆盖应用运行数据，不清理代码仓库里的 `target/`、`node_modules/`、`.worktrees/` 等开发产物。

## Current Findings

当前机器 `~/.renlijia/` 的只读审计结果：

- 根级条目：40 个。
- 总文件数：约 6944 个。
- 二级目录数：约 226 个。
- 最大占用：
  - `logs/`: 约 119M，其中 `gate.log` 约 80M。
  - `playwright-profile/`: 约 124M。
  - `analysis/`: 约 57M。
  - `charts/`: 约 56M。
  - `users/`: 约 78M。
  - `generated/`: 约 15M。
  - `defaultFolder/`: 约 18M。

当前根级目录混合了五类东西：规范白名单、老业务数据、用户可见产物、运行缓存、临时文件。这是“目录和文件显得太多”的根本原因。

详细目录级审计见 `docs/superpowers/specs/2026-06-01-app-data-layout-audit.md`。该报告结合产品入口、现有代码写盘点和老用户升级要求，逐项标记了每个 root 条目的归宿。

## Scope Update: Audit First

2026-06-01 收敛决定：先做目录审计报告增强和代码契约强校验第一版，不改启动治理逻辑，不迁移真实数据。原因是 root 目录混乱背后有多轮产品和架构迭代，必须先明确白名单、产品归属、代码 owner 和退出条件，再进入自动治理。

## Target Root Layout

长期目标是把 `~/.renlijia/` 根级收敛到白名单：

```text
~/.renlijia/
├── global/
├── crypto/
├── users/
├── skills/
├── employee-templates-cache/
├── expert-team-templates-cache/
├── logs/
├── runtimes/
├── tmp/
├── defaultFolder/
├── device_id
├── data_version
├── .migrated
└── .archived-legacy-*
```

根级新增任何条目都必须同时满足：

1. 不能自然归入用户级 `users/{scope}/`。
2. 不能自然归入临时根 `tmp/`。
3. 不能自然归入用户 workspace。
4. 已更新存储规范和 legacy cleanup 排除/归档策略。

## Classification

### Keep In Root

这些条目保留根级：

- `global/`
- `crypto/`
- `users/`
- `skills/`
- `employee-templates-cache/`
- `expert-team-templates-cache/`
- `logs/`
- `runtimes/`
- `tmp/`
- `defaultFolder/`
- `device_id`
- `data_version`
- `.migrated`
- `.archived-legacy-*`

### Migrate User Artifacts To Workspace

这些根级条目是用户可见产物或历史 workspace 误写入，不能删除：

- `analysis/`
- `charts/`
- `generated/`
- `exports/`
- `reports/`
- `uploads/`

目标路径：

```text
~/.renlijia/defaultFolder/legacy-root-import-YYYYMMDD/
├── analysis/
├── charts/
├── generated/
├── exports/
├── reports/
└── uploads/
```

迁移方式：

- 优先 `rename`，保留 metadata，速度快。
- 跨设备或权限失败时使用 `copy + best-effort remove`。
- 写入 `global/state.json::migrations.rootArtifactImport` 门闸。
- 生成 manifest，记录源路径、目标路径、迁移时间、失败项。
- `generated/` 的历史 root 引用按产品决策忽略，不要求额外 fallback map。

### TTL Clean Temporary Data

这些目录只保存可重生成中间文件：

- `temp/`
- `tmpImage/`
- `tmp/clipboard/`
- `tmp/dingtalk_downloads/`
- `tmp/feishu_downloads/`
- `tmp/wecom_downloads/`
- `tmp/wechat_downloads/`
- `tmp/telegram_downloads/`
- `tmp/whatsapp_downloads/`

治理策略：

- 默认 TTL 为 7 天。
- 启动期 best-effort 清理。
- 删除失败只记录 warning，不影响启动。
- 后续可增加容量 LRU 上限，本次先不引入复杂策略。

### Bound Logs

当前 `logs/` 仍由启动期 logger 和诊断上传使用，按产品决策保持 app-global，不 user-scoped。治理策略：

- `renlijia.log` 继续由 Tauri log plugin 管。
- `gate.log` 超过阈值时归档为 `gate.log.YYYYMMDDHHMMSS`，只保留最近 3 个归档。
- `metrics*.jsonl` 保留最近 7 天或最近 20 个文件，取更保守者。
- 不能删除当前正在写入的 `renlijia.log`、`metrics.jsonl`、`gate.log`。

### Review / Later PR Items

这些条目和登录、权限、浏览器会话、历史对话有关，本 PR 只审计和文档标记，不自动迁移：

- `permissions.json`
- `permissions.json.bak`
- `agent_invocations.json`
- `tasks/`
- `personas/`
- `conversations/`
- `index.json`
- `shared/`
- `site-profiles/`
- `screenshots/`
- `subagent_transcripts/`
- `interrupted_turns/`
- `api-data/`
- `audit/`

这些后续要逐项做：新路径写入、新路径读取、旧路径 fallback、一次性迁移、归档。

2026-06-01 产品决策补充：

- `playwright-profile/` 应该用户隔离；当前 root 目录只作为老版本升级过渡，后续单独迁到 `users/{scope}/playwright-profile/`。
- `expert-team-templates/` 是废弃的旧专家团模板目录，后续作为归档候选。

## Upgrade Safety

老用户升级必须满足：

1. 启动不被治理失败阻塞。
2. 已登录状态不被重置。
3. 历史会话仍能打开。
4. 历史生成文件链接仍能打开。
5. 浏览器自动化 profile 不被本次迁移破坏。
6. 权限、任务、子代理状态不被本次迁移破坏。
7. 重复启动不会重复迁移或重复归档。

因此本 PR 只自动处理低风险项：日志、临时文件、根级用户产物。其余只输出审计摘要。

## Architecture

先新增 `storage::app_data_contract`，负责把 root 条目归属变成代码里的单一事实源：

- 定义 `StableRoot`、`TransitionalRoot`、`WorkspaceArtifact`、`Temporary`、`DeprecatedArchiveCandidate`、`ReviewOnly`。
- 声明每个已知 root 条目的 owner、目标路径和老用户升级策略。
- 在单元测试里强校验非 storage gateway 的直接 root join 必须已登记。
- 生产启动面对未知老目录只进入 `ReviewOnly` 审计，不阻塞、不删除。

后续再新增 `storage::app_data_governance`，负责三件事：

- 扫描 `AiJiaHome` 根级条目并分类。
- 执行低风险治理：日志限制、临时文件 TTL、用户产物迁移。
- 写入治理报告和 migration gate。

启动接入点在 `lib.rs::setup` 中，放在现有迁移之后、业务 runtime 初始化之前。治理函数必须是 best-effort：返回详细报告，调用方只写日志，不 panic。

## Data Flow

```text
app setup
  -> existing migration
  -> app_data_governance::run_startup_governance(home)
       -> load global/state.json
       -> scan root entries
       -> cleanup temporary dirs
       -> bound logs
       -> migrate root artifacts once
       -> write report into global/state.json
  -> continue normal startup
```

## Testing

测试必须先写失败用例，再实现：

- `app_data_contract::tests::classifies_root_entries_by_contract`
- `app_data_contract::tests::runtime_audit_keeps_old_user_unknown_entries_non_blocking`
- `app_data_contract::tests::stable_root_whitelist_excludes_transitional_profile`
- `app_data_contract::tests::direct_root_joins_outside_storage_gateway_must_be_contract_entries`
- `classifies_root_entries_without_touching_unknowns`
- `cleans_temp_files_older_than_ttl_and_keeps_recent_files`
- `bounds_metrics_logs_without_deleting_active_files`
- `migrates_root_artifacts_to_legacy_import_once`
- `migration_is_idempotent`
- `migration_failure_preserves_source_and_does_not_error_startup`

优先用 Rust 单元测试和临时目录，不写真实 `~/.renlijia/`。

## Out Of Scope

- 不删除真实用户数据。
- 不清理代码仓库构建产物。
- 今天不迁移 `playwright-profile/`；后续单独做用户隔离迁移。
- 不迁移权限、任务、对话主数据。
- 不改变 workspace 默认路径策略。

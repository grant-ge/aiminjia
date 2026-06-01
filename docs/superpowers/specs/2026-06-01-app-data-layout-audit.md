# App Data Layout Audit

> Date: 2026-06-01
> Scope: desktop app runtime data under `~/.renlijia/`
> Branch: `feat/app-data-governance`
> Mode: audit only, no runtime behavior change

## Executive Summary

`~/.renlijia/` 目录显得乱，不是单一清理脚本能解决的问题。它是三次架构演进叠加后的结果：

1. 早期版本把会话、配置、权限、文件产物、浏览器 profile 都写到 root-flat 数据根。
2. 后续引入 `users/{scope}/`，但为了老用户升级可用，保留了大量 root fallback。
3. 产品能力扩展后，数字员工、专家团、技能、IM 渠道、浏览器自动化、运行时依赖各自引入新缓存和临时数据。

治理方向应该是白名单控制：root 只放装机级、登录前、跨用户共享、临时根和默认 workspace。其它目录必须归属到用户域、workspace、tmp、cache 或迁移归档。

今天只做审计报告增强，不改代码、不搬数据。

## Product Surface

当前桌面端主要产品面由 `src/App.tsx` 路由确认：

- 首页 / 工作台：`HomePage`
- 聊天：`ChatPage`
- IM 频道：`ChannelPage`
- 数字员工：`EmployeesPage`
- 专家团：`ExpertTeamsPage`
- 日程：`SchedulesPage`
- 技能中心和详情：`SkillCenterPage` / `SkillDetailPage`
- 收件箱：`InboxPage`
- 设置、权限弹窗、更新弹窗、网络状态等全局能力

这些产品面对应的数据域：

| 产品域 | 数据应该归属 | 说明 |
|---|---|---|
| 登录 / 租户 / 设备 | `global/`, `crypto/`, `device_id` | 登录前必须可读，跨用户共享或装机级 |
| 聊天会话 | `users/{scope}/conversations/` | 用户私有，不能 root-flat |
| 工具权限 / MCP / 子代理记录 | `users/{scope}/permissions.json`, `mcp_servers.json`, `agent_invocations.json` | 用户私有，未登录态 no-op 或只读 global fallback |
| 数字员工 / 日程 / 收件箱 | `users/{scope}/employees/`, `schedules/`, `agenda/` | 用户私有业务对象 |
| 数字员工模板 / 专家团模板 | `employee-templates-cache/`, `expert-team-templates-cache/` | 跨用户只读版本缓存，可 root 白名单 |
| 技能 | `skills/` + `users/{scope}/skills/` | `skills/` 是服务端同步的 global skill；用户上传走 user scope |
| 用户生成文件 | workspace，默认 `defaultFolder/` | 用户可见产物，不应散落 root |
| IM 附件临时下载 | `tmp/*_downloads/` | 可重生成，统一 TTL |
| 浏览器自动化 profile | 当前 root `playwright-profile/`，目标 user scope | 当前仍被代码读写，不能本轮迁 |
| 诊断日志 | 当前 root `logs/` | 短期保留 root，需容量治理 |

## Root Whitelist

建议 root 白名单分两级。

### Stable Whitelist

这些目录/文件可以长期保留在 `~/.renlijia/` root：

| Entry | Reason |
|---|---|
| `global/` | 登录前全局元数据、auth、updater |
| `crypto/` | 装机级主密钥 |
| `users/` | 用户私有数据根 |
| `skills/` | 服务端同步的 global skill 包 |
| `employee-templates-cache/` | 数字员工模板跨用户内容寻址缓存 |
| `expert-team-templates-cache/` | 专家团模板跨用户内容寻址缓存 |
| `tmp/` | 可重生成临时数据根 |
| `defaultFolder/` | 默认 workspace，用户可见文件归宿 |
| `device_id` | 旧装机级设备 ID，未来可进 `global/device.json` |
| `data_version` | 旧数据兼容门闸，未来可进 `global/state.json` |
| `.migrated` | 旧 app_data 迁移门闸 |
| `.archived-legacy-*` | 可回滚迁移归档，30 天 GC |

### Transitional Whitelist

这些 root 条目当前还需要短期保留，但必须有后续退出计划：

| Entry | Current Reason | Exit Criteria |
|---|---|---|
| `logs/` | `lib.rs` 和诊断上传仍读 root `logs/renlijia.log` | 日志容量治理完成；如要 user scope，需要 diagnostics 同步迁 |
| `playwright-profile/` | 浏览器自动化启动、重试擦除、SingletonLock 均直接读写 root | PlaywrightBrowser 注入 `UserScopedPaths` 并保留 root fallback 一个版本 |
| `screenshots/` | Playwright screenshot 仍写 root；已有 user scope API 但调用未迁完 | 浏览器截图改写 user scope 或 conversation 附件 |
| `site-profiles/` | 已有 user scope 迁移，但 root fallback 和 legacy cleanup 仍存在 | 确认所有 site profile 写入走 user scope |
| `subagent_transcripts/` | 部分路径已有 user scope fallback，root 仍作为未登录/legacy 兜底 | 子代理输出在未登录态 no-op，不再 root fallback |
| `api-data/`, `audit/`, `shared/`, `conversations/`, `index.json` | 老 root-flat 会话体系的兼容残留 | `legacyRootArchived` 覆盖完成并验证用户级数据完整 |

## Current Root Entries

当前机器实际 root 条目 40 个：

```text
.DS_Store
.archived-legacy-20260525T060758Z
.diag-watermark.json
.migrated
agent_invocations.json
analysis
api-data
audit
charts
conversations
crypto
data_version
defaultFolder
device_id
employee-templates-cache
expert-team-templates
expert-team-templates-cache
exports
generated
global
index.json
interrupted_turns
logs
permissions.json
permissions.json.bak
personas
playwright-profile
reports
screenshots
shared
site-profiles
skills
state.json
subagent_transcripts
tasks
temp
tmp
tmpImage
uploads
users
```

## Root Entry Decision Matrix

| Entry | Product / Code Owner | Current Evidence | Decision | Rationale |
|---|---|---|---|---|
| `.DS_Store` | macOS Finder | Local system artifact | Cleanup safe | Not app data |
| `.archived-legacy-*` | `migration_root_cleanup` | 30d archive retention | Keep root | Recovery window for old data |
| `.diag-watermark.json` | diagnostics | `commands/diagnostics.rs` tracks upload offset | Keep transitional | Login-independent diagnostic state; later move to `global/diagnostics/` |
| `.migrated` | legacy app_data migration | storage conventions whitelist | Keep root | Existing migration gate |
| `agent_invocations.json` | agent runtime | `AiJiaHome::agent_invocations_path()` root fallback | Review only | User private; already has `user_agent_invocations_path` |
| `analysis/` | workspace artifacts | `WorkspaceManager` defines `analysis` subdir | Migrate to workspace | User visible files; root copy is historical workspacePath drift |
| `api-data/` | legacy user data | `migration_user_scope::LEGACY_ITEMS` includes it | Archive after claim | User private legacy root |
| `audit/` | legacy user data | `migration_user_scope::LEGACY_ITEMS` includes it | Archive after claim | User private legacy root |
| `charts/` | workspace artifacts | `WorkspaceManager` defines `charts`; file lookup searches it | Migrate to workspace | User visible generated charts |
| `conversations/` | chat | `migration_user_scope` copies to user scope; `migration_root_cleanup` archives | Archive after claim | User private legacy root |
| `crypto/` | auth / secure storage | `AiJiaHome::crypto_dir()` | Keep root |装机级主密钥 |
| `data_version` | storage compatibility | storage conventions whitelist | Keep root transitional | Future move to `global/state.json` |
| `defaultFolder/` | workspace | `AiJiaHome::default_folder()` | Keep root | Default user workspace |
| `device_id` | auth device identity | `auth/device_id.rs` persists root file | Keep root transitional | Future move to `global/device.json` |
| `employee-templates-cache/` | digital employees | `employee_template_store` content-addressed cache | Keep root | Cross-user immutable resource cache |
| `expert-team-templates/` | expert teams | Present on disk, not in current whitelist | Review only | Looks like old pre-cache layout; need code search before migration |
| `expert-team-templates-cache/` | expert teams | decision doc says server sync writes here | Keep root | Cross-user immutable resource cache |
| `exports/` | workspace artifacts | `WorkspaceManager` defines `exports` | Migrate to workspace | User visible exported files |
| `generated/` | workspace / file records | `file_store` conversation generated dirs exist; root copy is legacy | Migrate cautiously | Root files may be referenced by old messages; need fallback map |
| `global/` | auth / config / updater | `AiJiaHome::global_dir()` | Keep root | Login-pre user-independent metadata |
| `index.json` | chat legacy index | `migration_user_scope` copies; `migration_root_cleanup` archives | Archive after claim | User private legacy root |
| `interrupted_turns/` | old runtime recovery | Mentioned as technical debt in storage conventions | Review only | Need confirm no active code reads it |
| `logs/` | diagnostics / app log | `lib.rs` creates root logs; diagnostics reads `logs/renlijia.log` | Keep transitional + bound size | Do not break diagnostic upload |
| `permissions.json` | permissions | root fallback in `lib.rs` | Review only | User private; move only after no root fallback needed |
| `permissions.json.bak` | permissions backup | Local backup | Review only | Need preserve until permissions migration verified |
| `personas/` | chat persona | Root intentionally excluded from cleanup | Review only | Product still has personas; need inspect read/write before moving |
| `playwright-profile/` | browser automation | `PlaywrightBrowser` launch/shutdown uses root | Keep transitional | High-risk: cookies/session/profile |
| `reports/` | workspace artifacts | `WorkspaceManager` defines `reports` | Migrate to workspace | User visible reports |
| `screenshots/` | browser automation / legacy user data | Playwright writes root screenshot; user scope path exists | Review only | Do not move until screenshot writer changes |
| `shared/` | memory/cache | `migration_user_scope` copies; cleanup archives after claim | Archive after claim | User private legacy root |
| `site-profiles/` | browser automation | root + user paths exist | Review only | Browser/site profile migration incomplete |
| `skills/` | skills | global skill sync path | Keep root | Product requires global managed skills |
| `state.json` | old global state | storage conventions marks as technical debt | Keep transitional | Moving may break old migration checks |
| `subagent_transcripts/` | agent runtime | root fallback and user path both exist | Review only | User private; migrate after output writer root fallback removed |
| `tasks/` | old task runtime | Present on disk | Review only | Need map to current `runtime/task` and pending queue before moving |
| `temp/` | legacy temp | `lib.rs` cleanup still targets root `temp` | TTL cleanup then retire | Legacy temporary data |
| `tmp/` | temp root | `AiJiaHome::tmp_dir()` | Keep root | Current temp root |
| `tmpImage/` | clipboard legacy | `save_clipboard_image_to_tmp` writes root `tmpImage` | Move writer to `tmp/clipboard`, TTL old dir | Existing new clipboard path already exists |
| `uploads/` | workspace artifacts | `WorkspaceManager` defines `uploads` | Migrate to workspace | User visible uploaded copies |
| `users/` | user data root | `AiJiaHome::users_dir()` | Keep root | Target user-scoped storage |

## Code Findings

### Evidence Index

Key code references used for this audit:

- Product surface: `src/App.tsx:15-23`, `src/App.tsx:56-78`.
- Root and user path APIs: `src-tauri/src/storage/aijia_home.rs:48-228`.
- User-scope directory creation: `src-tauri/src/storage/aijia_home.rs:309-329`.
- Root legacy directory creation: `src-tauri/src/storage/aijia_home.rs:332-343`.
- Legacy clipboard path: `src-tauri/src/commands/file.rs:402-432`.
- Current clipboard staging path: `src-tauri/src/commands/file.rs:435-472`.
- Workspace file lookup domains: `src-tauri/src/commands/file.rs:820-829`.
- Playwright screenshots/profile: `src-tauri/src/connector/playwright_browser.rs:437-452`, `src-tauri/src/connector/playwright_browser.rs:502-518`, `src-tauri/src/connector/playwright_browser.rs:563-594`.
- User-scope migration inputs: `src-tauri/src/storage/migration_user_scope.rs:10-24`.
- Legacy root archive allowlist: `src-tauri/src/storage/migration_root_cleanup.rs:48-71`.

### Root Directories Still Created At Startup

`AiJiaHome::ensure_dirs()` still creates root legacy directories:

- `subagent_transcripts/`
- `api-data/`
- `screenshots/`
- `site-profiles/`

This keeps root noisy even on newer builds. The likely historical reason is pre-login fallback and gradual user-scope migration. Future cleanup should split startup directory creation into:

- `ensure_root_whitelist_dirs()`
- `ensure_legacy_fallback_dirs_if_needed()`
- `ensure_user_dirs(scope)`

Do not remove those root creates until call sites no longer use root fallbacks.

### Clipboard Has Two Temp Paths

There are two clipboard image paths:

- legacy: `~/.renlijia/tmpImage/`
- current intended path: `~/.renlijia/tmp/clipboard/`

`save_clipboard_image_to_tmp()` still writes `tmpImage/`, while `save_clipboard_image_to_tmp_clipboard_impl()` writes the newer `tmp/clipboard/` path. Product-wise, clipboard paste is throwaway attachment staging, so target should be `tmp/clipboard/` with TTL. Root `tmpImage/` should become read-only legacy cleanup input.

### Browser Automation Is Root-Coupled

`PlaywrightBrowser` directly uses:

- `~/.renlijia/playwright-profile/`
- `~/.renlijia/screenshots/`

This is high-risk to move because it contains browser login/session state and Chromium lock/crash recovery behavior. Product-wise, browser automation is a user-private capability, but migration must be a separate PR with:

1. user-scoped profile path injection,
2. root fallback,
3. one-shot move only when no browser process is alive,
4. corrupted-profile retry semantics preserved.

### Workspace Artifact Paths Are Product-Level, Not Cache

`WorkspaceManager` defines:

```text
uploads/
exports/
charts/
analysis/
reports/
scripts/
temp/
```

`commands/file.rs::find_file_in_workspace` searches this same set. That confirms root `analysis/charts/exports/reports/uploads` are not app-private caches; they are user-facing workspace outputs that landed in root because default workspace and data root were historically conflated. They should be migrated to `defaultFolder/legacy-root-import-*`, not deleted.

### IM Downloads Are Correctly Under `tmp/`

IM channel downloads currently resolve to `~/.renlijia/tmp/{platform}_downloads/` through `AiJiaHome::tmp_*_downloads_dir()`. This is aligned with the target domain. The gap is GC: each subdir needs uniform TTL/size cleanup.

### Permissions / MCP / Agent State Have Root Fallbacks

`lib.rs` still uses root fallbacks for:

- permissions
- MCP config
- agent invocations
- subagent transcripts

These are user-private by product semantics, but current fallback protects pre-auth and old-user startup. They should be moved only after the runtime has clear unauthenticated behavior: no-op for user-private writes, read-only legacy fallback for one version, then archive.

## Proposed Iteration Sequence

### PR 0: Audit Only

Current scope.

- Document whitelist.
- Document every current root entry.
- Document product owner and code owner.
- Decide whether each entry is keep, migrate, archive, TTL, or review-only.
- No runtime behavior change.

### PR 1: Low-Risk Hygiene

- Stop writing new clipboard images to `tmpImage/`; write only to `tmp/clipboard/`.
- Add TTL cleanup for `temp/`, `tmpImage/`, and `tmp/*_downloads/`.
- Bound `logs/` file count/size without changing log path.
- Add startup audit summary log, but no user data migration.

### PR 2: Workspace Artifact Import

- Migrate root `analysis/charts/generated/exports/reports/uploads` into `defaultFolder/legacy-root-import-*`.
- Preserve manifest for path fallback.
- Add file-open fallback from old root relative paths to import location.
- Do not remove current workspace paths.

### PR 3: Root Legacy Archive Expansion

- Extend `migration_root_cleanup` only for items already proven copied into user scope.
- Re-check `api-data/audit/shared/conversations/index.json/site-profiles/screenshots/subagent_transcripts`.
- Keep archive-before-delete behavior.

### PR 4: Browser Automation User Scope

- Move Playwright profile and screenshots behind a path resolver.
- Preserve root fallback.
- Migrate only when Chromium is not running.
- Keep corrupted-profile wipe semantics.

### PR 5: Permission / Task / Agent State User Scope

- Remove root write fallback for permissions, MCP config, agent invocations, tasks.
- Ensure unauthenticated writes no-op.
- Archive old root copies after successful user-scope read.

## Product Rules Going Forward

1. Any user-specific business state defaults to `users/{scope}/`.
2. Any user-visible file output defaults to workspace, not app root.
3. Any cross-user immutable resource cache must be content-addressed and whitelisted.
4. Any temporary file must live under `tmp/` and have TTL.
5. Any root fallback must include an exit condition in docs.
6. Any new root entry requires updating storage conventions and this audit class table.

## Immediate Recommendation

Do not implement migration today. First get agreement on the matrix above, especially:

- Whether `expert-team-templates/` is an obsolete pre-cache layout or still needed.
- Whether `generated/` root files are referenced by old message records.
- Whether root `logs/` should stay global or eventually become user-scoped.
- Whether root `playwright-profile/` should be per-user or intentionally shared on one machine.

Once those four decisions are settled, PR 1 can safely proceed without touching sensitive user data.

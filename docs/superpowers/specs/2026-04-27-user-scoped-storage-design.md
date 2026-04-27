# lotus-app 用户数据隔离设计方案

> 日期：2026-04-27
> 状态：review 完成，待进入 implementation plan
> 仓库：lotus-app

---

## 1. 背景与目标

当前 lotus-app 的所有业务数据（聊天、会话索引、memory、定时任务、权限、MCP、浏览器 profile 等）都落在同一个 `~/.renlijia` 根目录下。后端已具备用户认证能力，但本地存储没有按用户拆目录。切换账号后，上一个账号的会话列表、配置、memory 等仍然可见，核心数据会混在一起。

**本方案解决的核心问题：同一台电脑切换租户/账号时，聊天记录等业务数据不串。**

设计原则：

- 以服务端认证的 `tenant.id` + `user.id` 作为本地 scope 来源。
- 本地路径按 `users/t_{tenantId}__u_{userId}` 隔离用户业务数据。
- 只改 `AppStorage` 的 `base_dir`，不在每条消息中写 `userId`，不改 JSONL schema。
- auth bootstrap 与用户业务存储分离。
- 切换账号 = 登出 + 重新登录，不做运行时中途切换的异常兜底。
- 不做多账号 token vault（切账号要重新输密码），不做租户级元数据缓存。

---

## 2. 已确认现状

### 2.1 启动时可知道当前用户

Rust 后端 auth state 已包含用户和租户信息：

- `auth/state.rs`：`UserInfo { id, name, username }`、`TenantInfo { id, name, ... }`、`CloudAuth`、`CloudAuthInfo`
- `auth/mod.rs`：`AuthManager::restore()` 启动时恢复 `cloud_auth`，`get_auth_info()` 返回当前用户/租户

结论：

- 已登录且 auth 未过期时，启动阶段可拿到 `tenant.id` + `user.id`。
- 未登录或 auth 过期时进入未登录态，无法计算 user-scoped 路径。
- `session_key` 是云端 API 凭据，不是用户身份，不能用于目录命名。

### 2.2 当前业务存储是单根目录

`AiJiaHome::from_home()` 固定返回 `~/.renlijia`，所有路径（`mcp_servers.json`、`permissions.json`、`conversations/` 等）都是它的子级。

启动时 `AppStorage::new(aijia_home.root())` 直接绑定到根目录，`AuthManager::new(db.clone(), ...)` 依赖同一个 `AppStorage` 实例——`cloud_auth` 和业务数据混在同一个 `config.json` 里。

**已知耦合点：** `workspacePath` 在 `lib.rs` 中 **早于** `AuthManager::restore()` 被读取（约 line 106），用来配置 `FileManager` 和日志目录。这意味着切账号后 workspace、Python 沙箱、日志路径仍指向旧账号配置。

### 2.3 聊天记录按 conversation 隔离，但没有按 user 隔离

当前聊天路径结构：

```
{base_dir}/index.json
{base_dir}/conversations/{conversationId}/conv.json
{base_dir}/conversations/{conversationId}/messages.jsonl
{base_dir}/shared/memory | cognitive | cache
```

不同 conversation 已物理分开，但不同用户共享同一个 `base_dir`，`index.json` 和 `conversations/` 会混在一起。

**最小正确改法：改变 `AppStorage` 的 `base_dir` 指向 user scope 目录，不重写每条消息路径。**

---

## 3. claude-code-best 对标结论

claude-code-best 是单用户 CLI 模型，默认不按账号分目录。

**可借鉴：**

- 所有路径从集中 path resolver 派生，不散落硬编码拼接。
- auth bootstrap 与 session/business data 分层，不互相嵌套。
- MCP、permission、settings 有 scope 概念（user/project/local/policy 分层合并）。
- runtime 等大型可复用资产与用户数据分开。

**不照搬：**

- claude-code-best 没有按 `accountUuid` 分目录，它的 `CLAUDE_CONFIG_DIR` 是当前账号覆盖式。
- lotus-app 是桌面多账号产品，需要在 global/project/session 分层之外加 `UserScope`。
- lotus-app 的用户身份来自服务端租户和用户，不使用本机随机 id。

---

## 4. 目录结构

scope key 采用单段目录名 `t_{tenantId}__u_{userId}`，避免 `tenant=1,user=23` 与 `tenant=12,user=3` 的歧义。

```text
~/.renlijia/
  global/
    config.json                 # 不含用户内容的启动配置
    state.json                  # 迁移标记，复用 migrations map 模式
    auth/
      cloud_auth                # 加密保存当前登录态
      active_account.json       # 最近激活的 scopeKey，辅助排错
    downloads/
      manifest.json
      blobs/

  crypto/                       # 全局 master.key（见 §6.7 说明）
  skills/                       # 内置 skills（全局共享）
  runtimes/                     # 系统 cache（Node/Python/Playwright runtime）

  users/
    t_{tenantId}__u_{userId}/
      scope.json                # tenantId/userId/name/username/createdAt/lastSeenAt
      config.json               # workspacePath、UI 偏好、模型偏好
      index.json                # 聊天索引
      conversations/
        {conversationId}/
          conv.json
          messages.jsonl
          uploads/
          generated/
          notes/
          file_index.json
      shared/
        memory/
        cognitive/
        cache/
      schedules/
      permissions.json          # user 层（workspace 层仍在 {workspace}/.aijia/ 共享）
      mcp_servers.json
      skills/                    # 用户安装 / 企业定制 skills
      agent_invocations.json
      subagent_transcripts/
      project_memories/
      downloads/
        manifest.json
        blobs/
      playwright-profile/
      api-data/
      screenshots/
      site-profiles/
      audit/
      logs/
```

### 4.1 global 保留内容

global 下只放启动前必须可访问、或可跨用户安全复用的数据：

- `global/auth/cloud_auth`：恢复当前登录态所需。
- `global/state.json`：迁移标记，复用现有 `state.json.migrations` 模式。
- `crypto/`：全局 master.key（见 §6.7）。
- `runtimes/`：程序依赖，跨用户复用。
- 内置 skills、系统资源、只读模板。
- 用户安装 / 企业定制 skills 不放 global，放 `users/{scope}/skills/`。
- 公共下载 blob（按 sha256 命名）。

### 4.2 user 目录内容

包含用户输入、账号行为、用户授权、访问痕迹的数据都放 user 目录：

- 聊天索引、会话、消息、附件、生成文件
- memory / cognitive / cache
- 定时任务 schedules
- 用户层 permissions（workspace 层 `.aijia/permissions.json` 跟着目录走，是共享的）
- 用户层 MCP 配置
- 用户安装 / 企业定制 skills
- agent invocation、subagent transcript
- project memory
- Playwright profile、api-data、screenshots、site-profiles
- 下载 manifest 和用户私有 blob
- audit/logs（含用户内容的部分）

### 4.3 新用户首次登录后的目录树

新用户首次登录时，如果 root 下没有 legacy 数据，不触发迁移、不写迁移标记。后续所有新数据直接写入当前用户 scope。

```text
~/.renlijia/
  global/
    config.json
    state.json                    # 可不存在；没有 legacy 迁移时不强制创建
    auth/
      cloud_auth                  # 当前登录态
      active_account.json         # { scopeKey: "t_1__u_2", tenantId: 1, userId: 2 }
    downloads/
      manifest.json               # 可为空或不存在
      blobs/

  crypto/
    master.key
  skills/
  runtimes/

  users/
    t_1__u_2/
      scope.json                  # 当前用户/租户元数据
      config.json                 # workspacePath、模型偏好、UI 偏好
      index.json                  # 新用户初始为空列表
      conversations/              # 新建会话后才出现子目录
      shared/
        memory/
        cognitive/
        cache/
      audit/
      mcp_servers.json            # 用户 MCP 配置
      skills/                     # 用户安装 / 企业定制 skills
      schedules/                  # 用户定时任务
      permissions.json            # 用户层权限（workspace 层仍在 {workspace}/.aijia/）
      agent_invocations.json
      subagent_transcripts/
      project_memories/
      playwright-profile/
      api-data/
      screenshots/
      site-profiles/
      downloads/
        manifest.json
        blobs/
      logs/
```

### 4.4 legacy 用户迁移后的目录树

如果 root 下已有 legacy 数据，用户登录后同步迁移到当前 scope。legacy 原始数据保留不删，新写入只走 `users/{scope}/`。

```text
~/.renlijia/
  # legacy 原始数据：保留，不再写入
  config.json
  index.json
  state.json                      # 旧迁移标记，保留
  .migrated
  conversations/
  shared/
  audit/
  mcp_servers.json
  permissions.json
  schedules/
  playwright-profile/
  screenshots/
  ...

  global/
    config.json
    state.json                    # migrations.legacyRootClaim.claimedBy = "t_1__u_2"
    auth/
      cloud_auth
      active_account.json
    downloads/
      manifest.json
      blobs/

  crypto/
    master.key
  skills/
  runtimes/

  users/
    t_1__u_2/
      scope.json
      config.json                 # 从 legacy config.json 拆出 workspacePath、模型偏好等
      index.json                  # 从 legacy index.json 迁入
      conversations/              # 从 legacy conversations/ 迁入
      shared/                     # 从 legacy shared/ 迁入
      audit/                      # 从 legacy audit/ 迁入
      mcp_servers.json            # 从 legacy mcp_servers.json 迁入
      skills/                     # 从 legacy skills/ 中迁入用户安装 / 企业定制部分
      schedules/                  # 从 legacy schedules/ 迁入
      permissions.json            # 从 legacy permissions.json 迁入
      agent_invocations.json      # 从 legacy agent_invocations.json 迁入
      subagent_transcripts/       # 从 legacy subagent_transcripts/ 迁入
      project_memories/           # 从 legacy project_memories/ 迁入
      playwright-profile/         # 从 legacy playwright-profile/ 迁入
      api-data/                   # 从 legacy api-data/ 迁入
      screenshots/                # 从 legacy screenshots/ 迁入
      site-profiles/              # 从 legacy site-profiles/ 迁入
      downloads/
      logs/
```

---

## 5. 数据归属清单

| 数据项 | 当前位置 | 目标归属 | 说明 |
|---|---|---|---|
| `cloud_auth` | `config.json` key | `global/auth/cloud_auth` | 启动前恢复身份，不能放 user 目录 |
| 当前激活账号 | 无 | `global/auth/active_account.json` | 记录最近激活 scope |
| 用户 scope 元数据 | 无 | `users/{scope}/scope.json` | tenantId/userId/name 等 |
| 全局启动配置 | `config.json` | `global/config.json` | 不含用户内容 |
| 用户配置 | `config.json` 部分 key | `users/{scope}/config.json` | workspacePath、UI 偏好、模型偏好 |
| 聊天索引 | `index.json` | `users/{scope}/index.json` | 防止会话列表混用 |
| 聊天消息 | `conversations/{id}` | `users/{scope}/conversations/{id}` | 不改消息格式 |
| 上传/生成文件 | conversation 子目录 | 跟随 conversation | |
| memory/cognitive | `shared/` | `users/{scope}/shared/` | |
| schedules | `schedules/` | `users/{scope}/schedules/` | |
| permissions（user 层） | `permissions.json` | `users/{scope}/permissions.json` | |
| permissions（workspace 层） | `{workspace}/.aijia/permissions.json` | **不变，按目录共享** | 多个用户指向同一 workspace 时共享 |
| MCP 配置 | `mcp_servers.json` | `users/{scope}/mcp_servers.json` | |
| agent 记录 | `agent_invocations.json` | `users/{scope}/agent_invocations.json` | |
| subagent transcript | `subagent_transcripts/` | `users/{scope}/subagent_transcripts/` | |
| project memory | root 下 | `users/{scope}/project_memories/` | |
| Playwright profile | `playwright-profile/` | `users/{scope}/playwright-profile/` | cookie/session 必须隔离 |
| screenshots 等 | root 下 | `users/{scope}/...` | |
| runtime/bin | cache | global | 跨用户复用 |
| skills | `skills/` | 内置保留 global；用户安装/企业定制迁 `users/{scope}/skills/` | 需区分内置和用户安装（`_drafts/` 和市场安装的一定是用户的） |
| 下载资源 | 分散 | user manifest + global/user blob | |

---

## 6. 架构改动设计

### 6.1 新增 UserScope

```rust
pub struct UserScope {
    pub tenant_id: i64,
    pub user_id: i64,
}

impl UserScope {
    pub fn key(&self) -> String {
        format!("t_{}__u_{}", self.tenant_id, self.user_id)
    }
}
```

来源规则：

- **已登录**：从 `AuthManager::get_auth_info()` 的 `tenant.id` + `user.id` 派生。
- **未登录**：不创建 user-scoped `AppStorage`，业务命令返回未登录错误，前端停在登录页。
- **切换账号**：登出 → 重新登录 → 重新计算 scope → 重新绑定 storage。

### 6.2 扩展 AiJiaHome 为分层 path helper

`AiJiaHome` 从单一 root helper 扩展为：

- `root()` → `~/.renlijia`
- `global_dir()` → `~/.renlijia/global`
- `global_config_path()`
- `auth_dir()` / `cloud_auth_path()`
- `users_dir()`
- `user_dir(scope: &UserScope)` → `~/.renlijia/users/{scope.key()}`
- `user_config_path(scope)` / `user_conversations_dir(scope)` / `user_schedules_dir(scope)` / ...
- `runtimes_dir()` 继续使用系统 cache
- `skills_dir()` 内置 skills 保留 global；`user_skills_dir(scope)` 指向 user scope

设计原则：

- 业务模块不直接拼 `users/t_x__u_y`，所有路径只通过 resolver 取得。
- scope key 由 i64 生成，不接受任意字符串，防止路径穿越。

### 6.3 拆分 GlobalStorage 与 CurrentUserStorage

当前 `AppStorage` 继续作为文件存储实现，但实例语义拆开：

- **GlobalStorage**：负责启动配置、`cloud_auth`、active account。可复用 `AppStorage` 的 config 能力或新建 `GlobalConfigStore`。
- **CurrentUserStorage**：内部持有当前 `UserScope` + `Arc<AppStorage>`，`base_dir` 指向 `users/{scope}`。提供 `get()` / `require_logged_in()` / `reload_for_scope()`。

  **注意**：Tauri `app.manage()` 注册的 `Arc<T>` 不能替换实例。因此 `CurrentUserStorage` 内部需要使用 `RwLock<Option<Inner>>` 模式——切 scope 时替换 inner，外层 `Arc` 不变。依赖 `CurrentUserStorage` 的 `RuntimeRepositoryFacade`、`FileManager`、`PermissionStore` 等同理，切账号时通过 inner 替换而不是重新 manage。

使用规则：

- 聊天、消息、文件、memory、schedule 等 → `CurrentUserStorage`
- auth 命令 → `GlobalStorage`
- runtime/bin → global/root helper

### 6.4 启动顺序调整

```text
setup()
  ├─ AiJiaHome::from_home()
  ├─ ensure global/root dirs
  ├─ SecureStorage::new(crypto/)
  ├─ GlobalStorage::new(global_dir)
  ├─ bootstrap_cloud_auth_if_needed(root, global_dir)        // 先从 legacy config.json 复制 cloud_auth 到 global/auth/，见 §8.8
  ├─ AuthManager::new(global_storage, secure_storage)         // 从 global/auth/ 读取
  ├─ auth_manager.restore()
  ├─ derive UserScope (if logged in)
  ├─ CurrentUserStorage::new(aijia_home, optional scope)
  │    └─ if scope: AppStorage::new(user_dir(scope))
  ├─ migrate_legacy_to_user_scope_if_needed(aijia_home, scope)  // 同步阻塞，全局一次性 claim，见 §8
  ├─ migrate_legacy_config_if_needed(aijia_home, scope)          // config.json 其余 key 拆分，见 §8.7
  ├─ FileManager (从 user scope 的 config 读 workspacePath，不再提前读全局)
  ├─ create runtime services with CurrentUserStorage resolver
  └─ spawn user-scoped background services (only when logged in)
```

关键点：

- `AuthManager` 不再依赖业务 `AppStorage`。
- **`workspacePath` 必须在 user scope 确定后才能读取**（当前在 auth restore 之前就读了，导致切账号后 workspace、FileManager、日志目录、Python 沙箱全部错位）。
- 未登录时不初始化聊天 `AppStorage`，避免继续写入根目录。
- `session_runtime.rs` 和 `chat_runtime_impl.rs` 中直接调用 `AiJiaHome::from_home()` 的地方必须改为使用注入的 managed state。

### 6.5 切换账号时的状态重载

切换账号的前提是用户已停止当前会话，走正常登出 → 重新登录流程。不需要处理运行中途切账号的异常兜底。

```text
前端 logout()
  ├─ chatStore 清空 conversations / messages / streaming state
  ├─ settingsStore / schedulesStore / mcpStore 清空缓存
  ├─ brandingStore.reset()
  └─ 跳转登录页

前端 login() 成功
  ├─ 后端 derive 新 UserScope
  ├─ CurrentUserStorage 切换 base_dir 到新 user_dir
  ├─ FileManager 用新 scope 的 workspacePath 重建
  ├─ 前端 loadConversations() 拉新 scope 下的会话列表
  └─ schedule_runner / MCP / PermissionStore 按新 scope 重新加载
```

### 6.6 聊天链路最小改造

改动目标：

- 保持 `AppStorage` 内部 `conversations/{id}/messages.jsonl` 格式不变。
- 将 `AppStorage` base dir 从 root 改成 user dir。
- `get_conversations()` 只返回当前 user dir 下的 `index.json`。
- `get_messages()` / `create_conversation()` / `delete/archive/rename/export/upload_gc` 同理。

不做：

- 不把 `tenantId/userId` 加到每条 `StoredMessage`。
- 不让前端传 `userId` 决定数据归属。
- 不依赖前端过滤会话。

### 6.7 master.key 说明

当前 `~/.renlijia/crypto/master.key` 是机器级单一密钥，加密 `cloud_auth` 和所有 API key。

**当前保持全局 master.key 不变。** 同一台电脑通常是同一个人使用，全局密钥的安全性足够。如果未来有更强的安全隔离需求（如合规场景要求不同租户密钥不能互解），可通过 `HKDF(master_key, scope_key)` 派生子密钥，当前不做。

### 6.8 用户态服务全部绑定 user scope

- **schedules**：`ScheduleStore` root 指向 `user_dir/schedules`，runner 绑定当前 scope
- **permissions**：user layer 改为 `users/{scope}/permissions.json`；workspace layer 仍在 `{workspace}/.aijia/permissions.json`（跟着目录走，多用户共享同一 workspace 时共享该配置）
- **MCP**：user config 改为 `users/{scope}/mcp_servers.json`
- **skills**：内置 skills 保留 global；用户安装 / 企业定制 skills 改为 `users/{scope}/skills/`
- **agent/subagent**：invocation store 和 transcript dir 改为 user-scoped
- **project memory**：移到 `users/{scope}/project_memories/`
- **browser**：Playwright profile、api-data、screenshots、site-profiles 移到 user-scoped
- **logs/audit**：含用户内容的日志移到 user-scoped；纯启动诊断留 global

---

## 7. 下载资源设计

采用 global/user 双层模型：

```text
~/.renlijia/global/downloads/
  blobs/sha256-<hash>
  manifest.json

~/.renlijia/users/{scope}/downloads/
  manifest.json
  blobs/sha256-<hash>
```

规则：

- 公共 runtime、公开模板放 global blob，按 sha256 去重。
- 用户下载记录、来源 URL、授权状态放 user manifest。
- 私有资源放 user blob。
- manifest 引用 blob 时记录 `blobScope: "global" | "user"` 和 `sha256`。
- 删除用户只删 user manifest + user blob；global blob 通过引用计数或 GC 清理。

---

## 8. 旧数据迁移策略

### 8.1 触发时机

迁移在**启动时自动触发**，嵌入 §6.4 的启动顺序中：

```text
setup()
  ├─ ...
  ├─ auth_manager.restore()
  ├─ derive UserScope
  ├─ CurrentUserStorage::new(aijia_home, scope)
  ├─ migrate_legacy_to_user_scope_if_needed(aijia_home, scope)  ← 迁移入口（同步阻塞）
  ├─ ... (后续服务初始化)
```

判断逻辑：

```text
root 下有 legacy index.json 或 conversations/ 吗？
  ├─ 没有 → 跳过，不写任何标记（新用户 / legacy 已清理）
  └─ 有 → global/state.json 中 migrations.legacyRootClaim 是否存在？
       ├─ 不存在 → 当前 scope claim legacy root → 执行迁移 → 写 legacyRootClaim.claimedBy = scope_key
       ├─ claimedBy == 当前 scope → 已迁移过，跳过
       └─ claimedBy != 当前 scope → legacy root 已被其他账号认领，禁止自动迁移，只允许手动导入/认领
```

- **新用户**（root 下没有 legacy 数据）：不触发迁移，不写任何标记。每次启动只多一次文件存在检查，开销可忽略。
- **首个升级用户**：claim legacy root，自动迁移 legacy 数据。
- **后续其他账号**：如果 legacy root 已被其他 scope claim，不再自动迁移，避免把 A 的历史数据复制给 B。
- **未登录**：没有 scope，不触发。

### 8.2 同步还是异步

**采用同步阻塞迁移。** 原因：

- 聊天索引和 conversations 必须在前端 `loadConversations()` 之前就位，否则用户会看到空列表或半迁移状态。
- 普通用户的 legacy 数据量不大（几十到几百个会话），copy 开销在秒级，启动多等 1-2 秒可以接受。
- 异步迁移需要处理"迁移中来了新请求写入 legacy 目录"的竞争，增加不必要的复杂度。

如果未来数据量大到影响启动体验（数千个会话、大量附件），可以改为分批迁移 + 前端 loading 提示，当前不做。

### 8.3 迁移记录与防重复

**复用现有 `state.json.migrations` 模式，但改为全局一次性 claim。** 当前代码中 `storage/migration.rs` 已使用 `state.json` 来标记迁移。user-scope 迁移采用 `legacyRootClaim` 一次性标记，确保 legacy 数据只被一个 scope 认领。

迁移标记写在 `global/state.json`：

```json
{
  "migrations": {
    "legacyRootClaim": {
      "claimedBy": "t_1__u_2",
      "claimedAt": "2026-04-28T10:00:00Z"
    }
  }
}
```

- legacy root 只会被第一个登录的 scope claim。
- 后续其他 scope 登录时，如果 `legacyRootClaim.claimedBy` 存在且不等于当前 scope，**不自动迁移**。
- 如果需要让其他 scope 也获得 legacy 数据，通过应用内的手动"导入/认领"入口操作。
- 新用户（root 下没有 legacy 数据）→ 不写标记，不走迁移。
- 中途崩溃（`legacyRootClaim` 不存在）→ 下次启动重试，逐条检查目标已存在则跳过。

### 8.4 迁移执行流程

```text
migrate_legacy_to_user_scope_if_needed(aijia_home, scope):
  1. 检查 ~/.renlijia/index.json 和 ~/.renlijia/conversations/ 是否存在
     - 都不存在 → return（新用户，无 legacy 数据，不写标记）
  2. 读取 global/state.json
     - migrations.legacyRootClaim.claimedBy 存在且 == scope_key → return（已迁移过）
     - migrations.legacyRootClaim.claimedBy 存在且 != scope_key → return（已被其他账号认领，禁止自动迁移）
  3. ensure users/{scope}/ 目录存在
  4. 如果 legacy index.json 存在，读取 conversation 列表
  5. 对每个 conversation:
     a. users/{scope}/conversations/{id}/ 已存在 → 跳过
     b. 复制 conversations/{id}/ 整个目录到 users/{scope}/conversations/{id}/
     c. 加入 users/{scope}/index.json
  6. 如果 legacy conversations/ 存在但 index.json 不存在，扫描 conversations/ 子目录补建 index
  7. 复制 shared/ → users/{scope}/shared/（目标不存在时）
  8. 复制 audit/ → users/{scope}/audit/（目标不存在时）
  9. 逐个复制以下文件/目录到 users/{scope}/（目标已存在则跳过）：
     - mcp_servers.json
     - permissions.json
     - agent_invocations.json
     - subagent_transcripts/
     - schedules/
     - project_memories/
     - playwright-profile/
     - api-data/
     - screenshots/
     - site-profiles/
     - skills/ 中的用户安装 / 企业定制部分（`_drafts/` 和非内置 skills）
  10. 写入 global/state.json: migrations.legacyRootClaim = { claimedBy: scope_key, claimedAt: now }
  11. legacy 原始文件保留不删除
```

### 8.5 迁移原则

- **copy-then-mark**，不删除 legacy 数据。
- 目标已有同名 conversation 时不覆盖。
- 通过 `global/state.json` 中的 `legacyRootClaim` 全局一次性标记保证 legacy 数据只被一个 scope 认领。
- 无法确定 legacy 数据归属当前用户时不自动迁移，提供手动认领入口。

### 8.6 迁移范围

一次性全部迁移，不分阶段：

- `index.json`
- `conversations/`
- `shared/`（memory/cognitive/cache）
- `audit/`
- `mcp_servers.json`
- `permissions.json`
- `agent_invocations.json`
- `subagent_transcripts/`
- `schedules/`
- `project_memories/`
- `playwright-profile/`
- `api-data/`
- `screenshots/`
- `site-profiles/`
- `skills/` 中的用户安装 / 企业定制部分
- `config.json` 拆分（见 §8.7）

### 8.7 global config 拆分

除了聊天数据迁移，启动时还需要将 `config.json` 中的配置拆分到 global 和 user 两层：

```text
拆分时机：首次以新目录结构启动时（global/config.json 不存在）

~/.renlijia/config.json (legacy, 保留不删)
  ├─ cloud_auth          → global/auth/cloud_auth
  ├─ workspacePath       → users/{scope}/config.json
  ├─ primaryModel 等     → users/{scope}/config.json
  └─ 其他启动配置        → global/config.json
```

后续新写入的配置直接写到对应层级，不再写 legacy `config.json`。

### 8.8 pre-auth cloud_auth bootstrap

老版本的登录态在 legacy `~/.renlijia/config.json` 的 `cloud_auth` key 中。新版 `AuthManager::restore()` 会从 `global/auth/cloud_auth` 读取。如果不先迁移 `cloud_auth`，第一次新版启动会恢复不到登录态，也就无法 derive `UserScope`，后续用户数据迁移无法发生。

因此启动顺序中必须在 `AuthManager::restore()` 之前执行：

```text
bootstrap_cloud_auth_if_needed(root, global_dir):
  1. 如果 global/auth/cloud_auth 已存在 → return
  2. 读取 legacy ~/.renlijia/config.json
  3. 如果存在 cloud_auth key → 原样写入 global/auth/cloud_auth
  4. legacy config.json 保留不删
```

注意：`cloud_auth` 的值已经是加密后的字符串（或旧版 plaintext fallback），bootstrap 不解密、不解析，只做原样复制。后续由 `AuthManager::restore()` 统一解密/解析。

---

## 9. 前端状态要求

- `login()` 成功后触发重新加载 conversations、schedules、settings。
- `logout()` / auth expired 时清空：conversations、activeConversationId、messages、streaming state、busy conversations、schedules/MCP/permissions/settings UI 缓存。
- 前端不向后端传 `userId` 决定数据归属。
- UI 展示当前 `tenant.name / user.name`。

---

## 10. 实施范围

不分阶段，一次性完成所有用户数据隔离。

### 目标

- 新增 `UserScope`
- 扩展 `AiJiaHome` path helper
- 拆出 global auth storage
- 新增 `CurrentUserStorage`（`RwLock<Option<Inner>>` 模式）
- `AppStorage` base dir 切到 `users/{scope}`
- `workspacePath` 从 user scope config 读取
- legacy config.json 拆分（cloud_auth → global，workspacePath/模型偏好 → user scope）
- 一次性迁移所有 legacy 数据到 `users/{scope}/`
- 所有用户态服务（schedules/permissions/MCP/agent/subagent/project memory/browser/screenshots）切到 user scope
- schedule runner 绑定 user scope，登出后停止
- MCP manager 绑定 user scope，登出后清空
- permissions 支持 workspace layer（共享）+ user layer（隔离）合并
- 登录/登出/启动恢复时正确绑定 storage
- global/user 双层 downloads manifest

### 验收

- 用户 A 登录创建的会话只在 `users/t_A__u_A/index.json`
- 用户 B 登录后看不到用户 A 的会话、MCP 配置、定时任务、权限、browser cookie
- 未登录时不写新的根目录数据
- 原有 legacy 数据迁移到当前 scope，legacy 原件保留
- 切账号后所有用户态服务按新 scope 重新加载

---

## 11. 测试与验证策略

### 11.1 Rust 后端单测

- `UserScope::key()` 格式稳定（`t_1__u_2` 不变）
- `AiJiaHome::user_dir()` 不接受任意路径片段
- `CurrentUserStorage` 未登录时拒绝聊天写入
- 两个不同 scope 创建同名 conversation id 时物理路径不同
- legacy migration 不覆盖已有 user conversation
- legacy migration 多次执行幂等（不重复拷贝已迁移数据）

```bash
cargo check
cargo test storage::aijia_home
```

### 11.2 前端

- `authStore` 登录/登出/expired 后清理 chatStore
- `loadConversations()` 在账号切换后重新加载
- Schedules/MCP/settings 页面切账号后不保留旧缓存

```bash
pnpm test -- authStore
pnpm test -- useChat
```

### 11.3 核心场景验收

```text
1. 清理或备份 ~/.renlijia 测试目录
2. 登录用户 A → 创建会话 A1、发送消息
3. 确认 ~/.renlijia/users/t_A__u_A/conversations 存在
4. 确认 users/t_A__u_A/config.json 包含 workspacePath
5. 登出 → 登录用户 B
6. 确认用户 B 会话列表为空
7. 创建会话 B1
8. 确认 A/B 的 index.json 与 conversations 分离
9. 登出 → 登录用户 A → 确认 A1 仍可加载、B1 不可见
10. 有 legacy 根目录数据时登录，确认迁移触发且 legacy 原件保留
```

---

## 12. 风险与处理

| 风险 | 处理 |
|---|---|
| 启动时 auth 未恢复 | 未登录态不初始化业务 storage，聊天命令返回未登录 |
| 旧数据误归属 | 只在明确当前用户时迁移，保留 legacy 原件 |
| 服务持有旧 `Arc<AppStorage>` | CurrentUserStorage 用 resolver 模式，切账号时 reload |
| workspacePath 切账号后错位 | 从 user scope config 延迟读取 |
| MCP/permissions 串账号 | user layer 放 user dir，workspace layer 按目录共享 |
| browser cookie 串账号 | Playwright profile 必须 user-scoped |
| 路径穿越 | scope key 只能由 i64 生成，不接受前端传入字符串 |

---

## 13. 明确不做

- 不把前端传入的 `userId` 作为本地数据归属依据。
- 不改消息 JSONL schema（不加 userId/tenantId 字段）。
- 不把 `session_key`、conversation id、runtime session id 当作用户身份。
- 不做多账号 token vault，切账号需要重新登录。
- 不做租户级元数据缓存层（不需要 `tenants/t_{tenantId}/` 中间目录）。
- 不做运行中途切账号的异常兜底（切账号前提是已停止当前会话）。
- 不做 master.key 按 scope 派生（全局共享足够）。
- 不把 runtime/bin 等公共资源按用户重复下载。
- 不把服务端权限逻辑搬到本地目录系统。
- 不在未登录状态下向 root 写聊天业务数据。
- 不自动删除 legacy 数据（只 copy-then-mark）。

---

## 14. 下一步

进入 implementation plan，一个计划覆盖全部改动：

- UserScope + AiJiaHome path helper + GlobalStorage + CurrentUserStorage
- 所有用户态数据一次性迁移到 user scope
- config.json 拆分
- 所有用户态服务（schedules/MCP/permissions/agent/browser 等）绑定 user scope
- downloads 双层 manifest
- 前端账号切换状态清理

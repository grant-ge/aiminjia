# 数字员工 & 专家团 — 平台/租户上传 + 桌面端加载（服务化设计）

**Date**: 2026-05-25
**Status**: 设计方案（待评审，未实施）
**Scope**: lotus 服务端（ops-portal / tenant-portal / api-gateway）+ 桌面端 Rust loader + 前端 catalog 加载
**参考**:
- 技能三类来源（本设计的“母版”）→ `docs/superpowers/specs/2026-05-19-skill-sources-tenant-public-user.md`
- 数字员工模板服务化（平台层已落地）→ `~/lotus/docs/superpowers/specs/2026-05-10-employee-templates-as-a-service.md`、`docs/decisions/employee-system-decisions.md`
- 专家团轻量入口（当前硬编码实现）→ `~/lotus/docs/desktop/superpowers/specs/2026-05-15-expert-teams-design.md`
- 存储规范 → `~/lotus/docs/desktop/storage-conventions.md`

---

## 0. 一句话目标

> 把**技能**已经跑通的「平台上传 + 租户上传 → 桌面端登录后同步加载 + 内置 bootstrap 兜底」这套来源分层模型，**复刻到数字员工和专家团**上：数字员工补齐缺失的**租户层**，专家团从前端硬编码升级为**完整的平台/租户服务化资源**。

---

## 1. 参考模型回顾：技能怎么做到的（母版）

技能有三类来源、两级上传、一个桌面加载器，这是本设计要对齐的形态：

| 来源 | 上传方 | 服务端 | 落盘（桌面） | source 标签 |
|---|---|---|---|---|
| **平台公共**（OPS/Platform） | 平台运营 | ops-portal 发布 `scope=public` → OSS | `~/.renlijia/skills/<id>`（L0 共享） | `Global` |
| **租户私有**（Tenant） | 租户管理员 | tenant-portal `POST /skill-packages`（multipart, `scope=tenant`, 绑 `tenant_id`）→ OSS | `~/.renlijia/skills/<id>`（同上目录） | `Tenant` |
| **本地用户**（User） | 终端用户 | 无（本机导入 / Skill-Smith） | `~/.renlijia/users/{scope}/skills/<id>`（L1） | `User` |

桌面加载关键点（要复用的“套路”）：
1. **平台层**走 `plugin/skill/global_sync.rs`：下载 manifest → artifact → 解包到全局目录，安装态记在 `global/state.json::globalSkills.installed{plugin_id→version}`。
2. **租户层**走 `commands/skill_management.rs::list_marketplace_skills / install_marketplace_skill`，请求**鉴权网关** `ai-tenant.renlijia.com/v1/skill-packages`（一次返回 tenant + public 两类，按登录态 `tenant_id` 过滤）。
3. **重名优先级** User > Tenant > Global，`loader.rs` HashMap 先到先得，冲突打 `log::warn!`。
4. **包安全**：`storage/skill_package.rs` + `shared/pkg/skillpkg`，zip-slip / 体积 / 文件数防护。
5. **版本契约**：`shared/pkg/version` 统一 `MAJOR.MINOR`，三个 publish handler 共用。

> 数字员工和专家团的产物是**纯配置 + prompt 文本**（无 scripts/references 这类多文件），所以**不套用技能的 zip 包格式**，改用**单个版本化 JSON 快照**（content-addressed + sha256）——这正是数字员工模板已经在用的形态（`TemplateSnapshot`），专家团也对齐它。

---

## 2. 现状盘点：两个目标各缺什么

### 2.1 数字员工（platform 层已有，缺 tenant 层）

| 能力 | 状态 | 位置 |
|---|---|---|
| 版本化 JSON 快照模型 | ✅ | `runtime/employee/template_store.rs::TemplateSnapshot` |
| 平台发布 → OSS | ✅ | `ops-portal/.../employee_template.go::Publish` |
| 桌面拉平台 catalog | ✅ | `template_store::fetch_catalog/ensure_cached`，命令 `employee_template_refresh/_catalog` |
| 平台 cache（跨用户） | ✅ | `~/.renlijia/employee-templates-cache/{tid}/{ver}.json`（L0） |
| 内置 bootstrap 兜底 | ✅ | `templates_bootstrap.json`（`include_str!`） |
| 数据模型含租户维度 | ✅（**未通路**） | `employee_template.go::TenantScope`（default `"global"`） |
| **租户上传入口** | ❌ | tenant-portal 无 employee_template handler |
| **租户分发端点** | ❌ | `PublicCatalog` 硬编码 `tenant_scope="global"`，网关无 `/v1/employee-templates` |
| **桌面租户层 loader** | ❌ | `template_store` 只拉公共 catalog |

→ **数字员工只需“补租户层”**：服务端开租户上传 + 鉴权分发端点；桌面端给 `template_store` 加一个 tenant fetch tier + L1 cache。

### 2.2 专家团（全硬编码，三层全缺）

| 能力 | 状态 | 位置 |
|---|---|---|
| 团队定义 | ❌ 前端常量 | `src/features/expert-teams/teams.ts::EXPERT_TEAMS`（8 个团，experts 烤死在团里） |
| 导演 prompt | ❌ 前端代码 | `buildDirectorPrompt.ts`（按 `facilitationStyle` 分支） |
| 会话归属 | ✅ | `conv.json::source.expertTeamId`（`expertTeamRegistry.ts` 懒读） |
| 服务端模型 / 发布 / 分发 | ❌ | 无 |
| 桌面同步 / cache / bootstrap | ❌ | 无 |

→ **专家团要“从零服务化”**：新表 + ops/tenant 上传 + 分发端点 + 桌面 `expert_team_store.rs` + 前端 catalog 加载 + bootstrap 兜底。**发布单元 = 整团**（含 experts personas + facilitationStyle + 文案 + 可选 prompt 覆盖），已与用户确认。

---

## 3. 统一设计原则（三者一张表）

| 维度 | 技能（母版） | 数字员工（本次补租户） | 专家团（本次新建） |
|---|---|---|---|
| 发布单元 | skill 包（zip + SKILL.md） | template 快照（JSON） | **team 快照（JSON）** |
| content-addressed | 否（按 plugin_id+version） | sha256 | sha256 |
| 平台上传 | ops-portal `scope=public` | ops-portal `tenant_scope=global` ✅ | **ops-portal `tenant_scope=global`（新）** |
| 租户上传 | tenant-portal `scope=tenant` | **tenant-portal（新）** | **tenant-portal（新）** |
| 鉴权分发端点 | gateway `/v1/skill-packages` | **gateway `/v1/employee-templates`（新）** | **gateway `/v1/expert-teams`（新）** |
| 公共分发端点 | OSS 直读 | `/api/public/employee-templates`（有） | **`/api/public/expert-teams`（新）** |
| 平台 cache（L0） | `~/.renlijia/skills/` | `~/.renlijia/employee-templates-cache/` ✅ | **`~/.renlijia/expert-teams-cache/`（新）** |
| 租户 cache（L1） | （与平台同目录，技术债） | **`users/{scope}/employee-templates-cache/`（新）** | **`users/{scope}/expert-teams-cache/`（新）** |
| bootstrap | 无 | `templates_bootstrap.json` ✅ | **`expert_teams_bootstrap.json`（新）** |
| 桌面 source 标签 | `Global/Tenant/User` | `bootstrap/ops/tenant` | `bootstrap/ops/tenant` |
| 重名优先级 | User>Tenant>Global | Tenant>Platform>bootstrap | Tenant>Platform>bootstrap |

> **关键收敛决策（相对技能母版的改进）**：租户私有产物落 **L1 用户私有目录**（`users/{scope}/...`），不进 L0 共享 cache。
> 理由：技能当前把 tenant 技能塞进共享 `~/.renlijia/skills/`，在多租户共用机器上有隐私串号风险（storage-conventions §11 已列为技术债）。数字员工 / 专家团是新通路，直接按规范 §0「用户私有 → L1」落地，避免重蹈覆辙。平台层产物是不可变只读、跨租户共享，留 L0 content-addressed cache。

---

## 4. 数字员工 — 补齐租户层

### 4.1 数据模型（服务端）

`employee_templates` 表已有 `tenant_scope`（default `"global"`）。约定：
- `tenant_scope = "global"` → 平台公共模板（ops 发布）。
- `tenant_scope = "t_<tenantId>"` → 某租户私有模板。
- 复用现有 `status`(draft/published/deprecated) + `version`(MAJOR.MINOR，走 `shared/pkg/version`)。

无需改表结构（已具备维度），只需打通**上传**与**分发**两条通路。

### 4.2 服务端改动

**A. 租户上传（tenant-portal，镜像 `skill_package.go::Publish`）**
- 新 handler `tenant-portal/.../employee_template.go`：
  - `POST /employee-templates`（JSON，非 multipart——模板是结构化字段不是文件）：校验 + 写库 `tenant_scope = "t_"+ctxTenantID`，`status=draft`。
  - `POST /employee-templates/:id/publish`：渲染 `snapshotForDesktop`（camelCase）→ 上传 OSS `tenant/employee-templates/{tenantId}/{template_id}/{version}.json` → 回填 `package_url` + `sha256` + `status=published`。
  - `GET /employee-templates`：返回**本租户 published + global published**（`WHERE (tenant_scope=? AND status='published') OR (tenant_scope='global' AND status='published')`），对齐 skill List 的双返回语义。
  - `POST /employee-templates/:id/download`：签名 URL（复用 `ossurl` / `rewritePublicHost`，bucket 私有）。
- ops-portal handler 不动（平台层已可发 `tenant_scope=global`）。

**B. 鉴权分发（api-gateway）**
- 新路由 `GET /v1/employee-templates`（透传 tenant-portal List，鉴权注入 `tenant_id`）+ `POST /v1/employee-templates/:id/download`。与 `/v1/skill-packages` 同形。

### 4.3 桌面端改动（`runtime/employee/template_store.rs`）

新增 **tenant fetch tier**，与现有 public tier 并列：

```text
catalog = merge(
    bootstrap_templates(),                       // include_str!，最低优先级
    fetch_public_catalog(),                      // ai-ops.renlijia.com（现有）→ L0 cache
    fetch_tenant_catalog(auth, tenant_id),       // 网关 /v1/employee-templates（新）→ L1 cache
)
// 同 template_id：tenant > public > bootstrap；同 scope 内取高版本
```

- 新方法 `fetch_tenant_catalog` / `ensure_cached_tenant`：走鉴权 reqwest（带登录 token），下载 snapshot 校验 sha256，写 **L1** `users/{scope}/employee-templates-cache/{tid}/{ver}.json`。
- `UserScopedPaths` 新增 `employee_templates_cache_dir()`（L1）；`AiJiaHome::employee_templates_cache_dir()`（L0）保留。
- `merge_catalog` 扩成三源；`TemplateRef.source` 取值扩为 `"bootstrap" | "ops:<url>" | "tenant:<url>"`。
- 新命令 `employee_template_refresh_tenant`（鉴权，未登录 no-op，对齐 storage §未登录降级）；`employee_template_catalog` 改为合并三源。
- 雇佣冷冻快照逻辑（`ensure_instance_snapshot` / `stamp_snapshot_for_record`）不变——它只认 `template_id` 反查，先 bootstrap 再扫两级 cache 即可命中租户模板。

### 4.4 前端改动

`HireWizard` 已从后端 catalog 渲染（PR4）。只需：
- `employeeTemplateCatalog()` 现在含租户模板（透明）。
- catalog 项加来源标签：`source: 'platform' | 'org' | 'builtin'`，模板网格右上角小 chip（复用技能中心的 i18n key 风格 `sourcePlatform/sourceTenant`）。

---

## 5. 专家团 — 整团服务化

### 5.1 发布单元 schema（published JSON 快照）

整团为发布单元。on-disk / OSS JSON（camelCase，对齐 `TemplateSnapshot` 风格）：

```jsonc
{
  "teamId": "marketing",
  "version": "1.0.0",
  "name": "市场营销策划团",
  "emoji": "📣",
  "tagline": "发布会 / 营销活动 / 市场策略",
  "experts": [
    { "name": "品牌负责人", "agentName": "brand-lead", "persona": "关注定位、调性…", "emoji": "🎨" }
  ],
  "examples": ["策划一场新品发布会", "618 大促营销节奏怎么排"],
  "composerPlaceholder": "告诉他们你想策划什么活动…",
  "facilitationStyle": "rounds",        // rounds | debate | open，决定导演 prompt 分支
  "promptTemplate": null                 // 可选：非空时覆盖内置 buildDirectorPrompt 模板
                                         // 占位符 {teamName} {roster} {topic}
}
```

设计取舍：
- **v1 沿用 `facilitationStyle` 三选一**，导演 prompt 骨架仍在前端 `buildDirectorPrompt.ts`（与员工 dispatch prompt 骨架留在代码一致）。
- **`promptTemplate` 作为前向兼容的可选覆盖**：租户上传不落三种范式时，可带自定义模板字符串（带 `{roster}/{topic}` 占位符）。v1 实现读取它但 bootstrap/平台团都留空，零回归。

### 5.2 服务端（新 `expert_teams` 表 + handlers）

新表 `expert_teams`，列镜像 `employee_templates`：
`team_id, version, tenant_scope(global|t_<id>), status(draft/published/deprecated), name, emoji, tagline, experts(json), examples(json), composer_placeholder, facilitation_style, prompt_template, package_url, sha256, created_at, updated_at`。

- **ops-portal**：`expert_team.go` 镜像 `employee_template.go`（CRUD + Publish→OSS + `PublicCatalog`/`PublicManifest`，`tenant_scope=global`）。OSS `ops/expert-teams/{team_id}/{version}.json`。
- **tenant-portal**：`expert_team.go` 镜像 §4.2.A（租户上传 `scope=t_<tenantId>`，List 返回本租户+global，Download 签名 URL）。OSS `tenant/expert-teams/{tenantId}/{team_id}/{version}.json`。
- **api-gateway**：`GET /v1/expert-teams` + `POST /v1/expert-teams/:id/download`，鉴权注入 `tenant_id`。
- 三个 publish handler 全部接 `shared/pkg/version` 两位版本校验（与既有 3 个保持一致，共 6 个）。
- 复用 `snapshotForDesktop` 风格 view-model 保证 camelCase 与桌面 serde 对齐（员工模板踩过的坑，直接规避）。

### 5.3 桌面端（新 `runtime/expert_team/` 模块，镜像 employee template_store）

- `expert_team/team_store.rs`：`TeamSnapshot`（§5.1 字段）/ `TeamRef` / `bootstrap_teams()`（`include_str!("expert_teams_bootstrap.json")`，即当前 8 个团导出的 JSON）/ `fetch_public_catalog` / `fetch_tenant_catalog` / `ensure_cached*` / `merge_catalog`。
- cache 路径：
  - 平台 L0：`~/.renlijia/expert-teams-cache/{team_id}/{ver}.json`（**新增 root 级条目**）。
  - 租户 L1：`users/{scope}/expert-teams-cache/{team_id}/{ver}.json`。
- 合并优先级：tenant > platform > bootstrap；同 scope 取高版本。
- 命令：`expert_team_catalog`（合并三源，返回给前端）+ `expert_team_refresh_tenant`（鉴权 / 未登录 no-op）。平台层可在 startup 顺带 refresh（对齐 `employee_template_refresh` 现状）。
- **专家团无运行态冷冻快照需求**（不像员工实例要冻结模板）——会话只在 `conv.json::source.expertTeamId` 记 id，prompt 是发送时即时渲染。故无需 per-instance snapshot 目录。

### 5.4 前端（`teams.ts` 去硬编码）

- 现 `EXPERT_TEAMS` 常量降级为 `BUILTIN_TEAMS`（last-resort fallback，等价 `BUILTIN_TEMPLATES` 角色）。
- 新增 `expertTeamCatalog()` IPC + `loadExpertTeams()`：进入专家团页 / 启动时 `expert_team_refresh_tenant()`（fire-and-forget）→ `expert_team_catalog()` 取合并列表 → 映射成前端 `ExpertTeam`；任何失败回退 `BUILTIN_TEAMS`（保证离线 / 服务挂掉不挂）。
- `expertTeamRegistry.ts::VALID_IDS` 从静态常量改为**动态集合**（来自已加载 catalog），id 类型 `ExpertTeamId` 从 union 字面量放宽为 `string`（租户团 id 不在闭合枚举里）。
- `buildDirectorPrompt(team, topic)`：若 `team.promptTemplate` 非空走模板替换，否则按 `facilitationStyle` 走现有三分支。
- catalog 项带 `source: 'platform'|'org'|'builtin'`，`ExpertTeamCard` 右上角 chip。

---

## 6. 存储规范对齐（必须同步改的硬约束）

新增 1 个 root 级条目 + 2 个 L1 子目录，按 storage-conventions 规则**必须同步更新**：

1. **`~/.renlijia/expert-teams-cache/`**（root，L0 content-addressed）：
   - 更新 storage-conventions **§5 白名单**新增一行（owner = `expert_team::team_store`）。
   - 更新 `storage/migration_root_cleanup::ARCHIVE_ITEMS` 排除规则（否则下一轮 cleanup 误归档）。
2. **`users/{scope}/employee-templates-cache/`**、**`users/{scope}/expert-teams-cache/`**（L1）：在 `UserScopedPaths` 加入口方法 + storage-conventions §2 目录树补条目。
3. 缓存均为可重生成、不可变只读 → 可考虑挂 LRU/TTL（非必须，content-addressed 无脏数据）。

---

## 7. 来源优先级与重名规则（三者统一）

- **数字员工 / 专家团**：`Tenant > Platform > bootstrap`（租户定制覆盖平台同 id；都没有才用内置）。同 scope 内同 id 取高版本（沿用 `merge_catalog` 语义）。
- 与技能 `User > Tenant > Global` 形成统一心智：**越靠近用户/租户、越具体的来源优先级越高**。
- 冲突时 `log::warn!`（kept/dropped 的 source+version+path），对齐 skill loader 排错习惯。

---

## 8. 迁移路径

- **数字员工**：纯增量，无破坏。老桌面只拉 public catalog；新桌面登录后多拉一条租户 catalog。bootstrap / 实例冷冻快照行为不变。
- **专家团**：
  1. 把当前 `teams.ts` 的 8 个团导出为 `expert_teams_bootstrap.json`（一次性脚本），编进 binary。
  2. 前端 `EXPERT_TEAMS` → catalog 加载 + `BUILTIN_TEAMS` fallback，**首版 catalog 内容 = bootstrap = 现状**，UX 零差异（对齐员工 PR4 的 `snapshotToTemplate` “builtin verbatim” 策略）。
  3. 平台运营把 8 个团灌库发布到 `ops/expert-teams/`（生产化），桌面端从 cache 命中，bootstrap 退居兜底。
  4. `ExpertTeamId` 由闭合 union 放宽为 string——检查所有按 id switch 的地方（`buildDirectorPrompt` 的 style 分支不受影响，因为它读 `facilitationStyle` 不读 id）。

---

## 9. PR 拆解（建议顺序，每个独立可验收）

**数字员工租户层**
- E1（服务端）：tenant-portal `employee_template` handler + gateway `/v1/employee-templates`（+ Go 单测）。
- E2（桌面）：`template_store` tenant fetch tier + L1 cache + `employee_template_refresh_tenant` + merge 三源（+ Rust 单测，HTTP 路径 mock 或 `--ignored` live）。
- E3（前端）：HireWizard catalog 来源 chip + i18n。

**专家团服务化**
- T1（桌面，无网络）：`expert_team/team_store.rs` + `expert_teams_bootstrap.json` + `expert_team_catalog` 命令（合并先只含 bootstrap）+ 前端 `teams.ts` 去硬编码改 catalog 加载（fallback BUILTIN_TEAMS）。**此 PR 后 UX 与现状等价**，是最安全的第一步。
- T2（服务端）：`expert_teams` 表 + ops-portal + tenant-portal handler + gateway `/v1/expert-teams`（+ Go 单测）。
- T3（桌面）：`team_store` 接平台 + 租户 fetch tier + L0/L1 cache + `expert_team_refresh_tenant` + merge 三源。
- T4（前端）：`ExpertTeamId` 放宽 string + `VALID_IDS` 动态化 + `promptTemplate` 覆盖支持 + 来源 chip + i18n。
- T5（运营/数据）：8 个内置团灌库发布到生产 OSS + live 集成测试（镜像 `employee_template_lifecycle_test.rs`）。

**收尾**
- D1：storage-conventions §2/§5 + `ARCHIVE_ITEMS` + CLAUDE.md 存储结构链接同步（在引入 root 级 `expert-teams-cache/` 的 PR 内完成，不单独留尾）。

---

## 10. 显式 Non-Goals（v1 不做）

- 专家团的**用户自建/本地导入**层（技能有 User 层，专家团 v1 只做 Platform + Tenant；预留 string id + source 字段，将来加 User 不破坏）。
- 专家团 experts 拆成可复用「专家库」（已与用户确认：**整团为发布单元**，experts 内联）。
- 租户上传产物在桌面端按 scope 拆物理子目录做运行时强隔离（cache 已按 L0/L1 分层，足够 v1）。
- 技能既有 tenant 技能落 L0 的技术债顺带迁 L1（本设计只保证**新通路**正确，不回头改技能）。
- 专家团 prompt 模板的可视化编辑器 / 校验器（tenant-portal 上传 v1 用 JSON 表单 + 后端软校验）。

---

## 11. 验收标准

**数字员工租户层**
1. 租户管理员在 tenant-portal 创建并发布一个员工模板（`tenant_scope=t_X`）。
2. 该租户用户桌面端登录 → `employee_template_refresh_tenant` 日志出现 `N tenant templates cached`；`employee_template_catalog` 返回含该模板。
3. HireWizard 模板网格出现该模板，带 “企业” 来源 chip；雇佣后实例 `template/template.json` 快照正确冻结。
4. **隔离**：另一租户用户登录看不到它（cache 落各自 L1）。

**专家团服务化**
1. T1 合入后：专家团页与现状像素级等价（catalog = bootstrap），离线可用。
2. 平台发布一个新团 → 桌面端登录后出现，bootstrap 退兜底。
3. 租户上传一个团（含自定义 `promptTemplate`）→ 仅该租户桌面端可见，进入会话发送议题，导演 prompt 用其自定义模板，LLM 正常 `TeamCreate` 拉起子代理。
4. 同 id 冲突时 tenant 团覆盖 platform 团，日志有 warn。

---

## 12. 风险与注意

- **camelCase / snake_case drift**：员工模板生产化踩过（Go snake_case vs Rust camelCase 静默全 default）。专家团服务端必须用 `snapshotForDesktop` 同款 camelCase view-model，桌面 serde 加 `#[serde(default)]`。
- **OSS 签名 URL host 重写**：bucket 私有 + VPC 内网 endpoint，签名 URL 要 `rewritePublicHost` 到公网 host（员工模板 PR 已有现成函数，直接复用）。
- **未登录降级**：所有 tenant fetch 命令未登录必须 no-op（storage §未登录降级），不得回退 root，不得报错卡 UI。
- **`ExpertTeamId` 放宽为 string**：需全仓 grep 现有 8 个字面量 id 的 switch/比较点，确认放宽后无遗漏（重点 `expertTeamRegistry`、`expertAvatar`、`teamLogo`）。
- **bootstrap 与生产首版一致性**：专家团灌库的 v1.0.0 必须与 `expert_teams_bootstrap.json` 内容一致，避免“在线版和离线版细微差异”（员工 PR4 用 “builtin verbatim” 规避，专家团同策略）。

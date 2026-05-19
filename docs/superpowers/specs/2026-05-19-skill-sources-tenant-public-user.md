# 桌面端技能三类来源 + 重名处理

**Date**: 2026-05-19
**Scope**: skill loading 路径、global_sync URL、UI 来源标签

## 问题

桌面端对话能用到哪些技能?

| 类型 | 落盘位置 | 同步代码状态 | 调用状态 |
|---|---|---|---|
| 本地用户上传 | `~/.renlijia/users/{scope}/skills/<id>` | 已支持(UI 导入) | ✅ |
| 平台公共技能(OPS) | `~/.renlijia/skills/<id>` | ✅ 走 gateway `/v1/skill-packages` | ✅ |
| 租户私有技能(tenant admin push) | `~/.renlijia/skills/<id>` | ❌ **被 URL `?scope=public` 过滤掉** | ❌ |

lotus gateway `handler.SkillPackageEmployeeHandler.List` 一次返回 tenant + public 两类(`WHERE (scope='tenant' AND tenant_id=? AND status='published') OR (scope='public' AND status='published')`),桌面端额外拼 `?scope=public` 让租户私有部分**永远拿不到**——租户管理员发布的技能在桌面端永远不会出现。

## 重名规则

`loader.rs::load_one_root` 用 HashMap 去重,**先到先得**。`lib.rs` 拼装顺序:`[user_skills_dir, global_skills_dir]`,所以本地用户技能优先于服务端推送同名技能。

Pre-fix 行为:HashMap 静默丢弃后到者 → 用户看不到"我的本地版本覆盖了管理员推送的新版"。Post-fix:`log::warn!` 打印 `(kept_source, kept_path) vs (dropped_source, dropped_path)`,便于排查"为什么我看到的还是旧版"。

Install 路径 (`install_one_prepared_skill`) 总是写到 `global_skills_dir/<id>`,不会污染 user 目录。所以 server-push 同名技能不会覆盖本地副本,只是被 loader 忽略。

## 改动

### P0:同步租户私有技能

- `src-tauri/src/plugin/skill/global_sync.rs:331` 删 `?scope=public`,只留 `page+size`。Gateway 自动按 tenant_id 同时返回两类。
- `src-tauri/src/commands/skill_management.rs:580` 同步删除(技能中心 UI 浏览市场的同一端点)。

### P1:SkillSource 加 Tenant + loader 显式 source

- `types.rs::SkillSource` 加 `Tenant` 变体。文档注释说明三类优先级:User > Tenant > Global。
- `loader.rs` 新增 `load_skill_roots_tagged(&[(PathBuf, SkillSource)])`,接收显式 (root, source) 对。旧 `load_skill_roots(&[PathBuf])` 改为向 tagged 版本转发(保持 idx 0→User 的兼容)。
- `loader.rs` 同 id 二次出现时打 `log::warn!`(带 kept/dropped path+source)。
- `lib.rs` setup 改用 `load_skill_roots_tagged` 显式标注 source。
- `commands/skill_management.rs` 序列化 SkillSource 时加 `Tenant => "tenant"` 分支。
- `global_sync.rs::SkillPackageItem` 加 `scope: String` 字段(`#[serde(default)]` 兼容老 server),sync 完成后日志按 tenant/public 统计。

注:Tenant 技能与 Global 技能当前装在同一目录(`~/.renlijia/skills/`),loader 当前都打 Global 标签。后续如需运行时区分,需要 install 时按 scope 拆 `~/.renlijia/skills/{tenant,public}/` 子目录 + lib.rs 拼接 3 个 roots。本次留接口、不改物理布局。

### P2:UI 区分三类

- `SkillCenterPage.getSkillMeta` 按 source 字符串 switch 出 3 类标签 + 兼容 `'builtin'` 测试 fixture:
  - `user` / `builtin` → "自定义" / "Custom"
  - `tenant` → "企业推送" / "Org"
  - `global` → "平台" / "Platform"
- i18n keys 加 `skillCenter.sourceUser` / `sourceTenant` / `sourcePlatform`(zh+en)。

## 验证

- `cargo build --lib` ✅ 0 errors
- `pnpm exec tsc -b` ✅
- vitest skill-center suite:8 pass / 4 fail——4 个 fail 是 pre-existing(`git stash` 后同样 4 fail),与本提交无关
- 部署后回归:管理员在 lotus tenant-portal 上传一个 scope=tenant 技能,桌面端登录后调用 `sync_builtin_skills` → 日志应出现 `fetched N skills (X tenant + Y public)`,X > 0;UI 上对应 skill 显示"企业推送"标签。

## 后续

- 若需要 user 看到"管理员有新版"提示(我的本地覆盖了),要把 collision warn 升级为 store + 前端弹通知。当前 P2 不做。
- Install 路径按 scope 分 tenant/public 子目录,需要时再做。

# 技能中心启用状态改造设计

日期：2026-06-10

## 背景

当前技能中心把已加载到本地 registry 的技能全部视为可用技能。企业下发、平台默认和本地导入技能数量变多后，会带来两个问题：

- 聊天输入框、技能选择器和详情页会展示过多技能。
- Rust 运行时会把全部 registry skill 写入模型 skill catalog，挤占动态上下文。

本次改造目标是把“安装/存在”和“后续对话是否生效”拆开。用户可以保留技能，但关闭它，使它不再出现在后续对话入口，也不再注入模型上下文。

## 产品原则

技能中心保留当前页面风格和卡片墙，不做大改造。一级结构调整为：

- 市场
- 内置
- 已安装

市场只负责发现和添加，不承担开关管理。内置和已安装负责启用/关闭管理。

“对话可用”不作为主导航或市场卡片上的显性概念，只作为数据层 enabled 状态。界面上用用户更容易理解的动作表达：添加、使用、关闭、开启并使用。

## 页面设计

### 市场

市场展示可添加技能，沿用现有 `SkillCard` 卡片视觉。

市场卡片只展示安装关系：

- 未添加：展示 `+`。
- 已添加：展示 `已添加`。

市场卡片不展示：

- 开关。
- `已关闭`。
- `去对话` 按钮。
- 当前启用/关闭状态。

用户点击未添加技能的 `+` 后：

1. 安装技能。
2. 默认开启该技能。
3. 进入后续对话使用流程，由添加流程跳转或预填技能，而不是在卡片上额外放按钮。
4. 技能同时进入“已安装”。

如果技能已经添加但在“已安装”中被关闭，市场仍只展示 `已添加`。用户要管理开关，需要进入详情页、已安装页或内置页。

### 内置

内置展示系统能力。这里可以展示启用/关闭开关。

“内置”不是企业/平台远端技能的全量列表，而是一组产品必须自带的基础技能 allowlist。第一版建议包含：

- `skill-creator`：UI 可展示为 `create-skill`，用于引导用户创建新技能。
- `dingtalk-workspace`：UI 可展示为 `dws` 或“玩转钉钉”，用于把用户口语里的钉钉需求映射到 DWS 能力；注意它不是 `src-tauri/resources/dws` 二进制本身，二进制仍走现有 bundled resource / connector 逻辑。

实际落地时以远端包或 SKILL.md 的 `name` / `plugin_id` 为准。如果正式发布包 id 不是上面的字符串，需要在 allowlist 中使用真实 id，UI 再做展示名映射。

登录后当前用户 scope 已经激活时，后端执行 `ensure_required_builtin_skills`：

1. 读取必需内置技能 allowlist。
2. 对每个 id 检查本地 `~/.renlijia/skills/<id>/SKILL.md` 是否存在。
3. 不存在或版本需要更新时，只安装 allowlist 内的官方包。
4. 安装或更新后刷新 `SkillRegistry`。
5. 不改写用户的 disabled override。

因此新用户第一次初始化时，这些必需内置技能会自动安装并默认开启；如果用户后来手动关闭，后续登录、同步、更新都不能偷偷重新打开。

关闭内置技能后：

- 仍保留在内置列表。
- 不出现在聊天输入框技能选择器。
- 不出现在技能详情页的直接使用入口。
- 不进入模型 skill catalog。
- `Skill` tool 不能触发它。

### 已安装

已安装展示本地已经存在的技能，包括本地导入和用户已经添加的企业/平台技能。

已安装页可以展示开关。关闭后技能仍保留在已安装列表，用户可重新开启。

本地导入技能继续保留现有更多菜单动作，例如导出、删除。企业/平台技能保留更新相关动作。

### 技能详情

点击任意技能进入详情页。详情页沿用当前 `SkillDetailPage` 的结构：标题、描述、来源/命令/版本/更新时间、使用说明、主操作按钮。

详情页根据状态切换主动作：

| 状态 | 主动作 | 辅助动作 | 说明 |
| --- | --- | --- | --- |
| 未添加 | 添加并使用 | 无 | 不展示开关。添加后默认开启，并进入对话使用流程。 |
| 已添加且开启 | 使用 | 关闭 | 点击使用回到聊天输入框并插入技能 chip。点击关闭后刷新可用技能列表。 |
| 已添加但关闭 | 开启并使用 | 保持关闭 | 明确提示关闭后不会出现在聊天输入框，也不会注入模型上下文。 |

详情页可以展示启用状态，因为用户已经进入具体技能语境；市场卡片不展示启用状态。

## 数据模型

新增用户本地持久化的启用状态。推荐模型：

- 默认开启。
- 本地只记录 disabled skill ids。

需要支持 user-scoped 存储，避免不同登录用户互相影响。

建议持久化结构：

```json
{
  "disabledSkillIds": ["biz-plan", "weekly-report"]
}
```

如果后续需要企业策略覆盖，可扩展为：

```json
{
  "overrides": {
    "biz-plan": { "enabled": false, "updatedAt": "2026-06-10T00:00:00Z" }
  }
}
```

本次只需要用户本地关闭能力，不引入企业强制开关策略。

## 前端改造

### `SkillInfo`

在 `src/lib/tauri.ts` 的 `SkillInfo` 增加：

- `enabled: boolean`
- 可选 `installed?: boolean`，用于市场视图识别已添加状态。

如果第一阶段市场数据与已安装数据来自同一个来源，可以先通过已安装列表判断 `installed`，但 UI 语义要保持清晰。

### `skillStore`

`src/stores/skillStore.ts` 保留全量技能列表，同时提供启用技能选择器：

- `skills`: 全量本地已知技能。
- `enabledSkills`: 只包含 enabled 技能。
- `setSkillEnabled(skillId, enabled)`: 写入本地状态后 reload。

聊天输入框、SkillPopover、WelcomeScreen、HomeTaskComposerCard 等对话入口不能再直接消费全量 `skills`，必须消费 enabled skills。

### 技能中心

`src/features/skill-center/SkillCenterPage.tsx` 继续复用：

- `PageTopBar`
- 搜索框
- 更新技能下拉
- 导入技能下拉
- 现有 `SkillCard`

内容区增加 `市场 / 内置 / 已安装` 一级 Tab。

市场视图：

- 卡片展示 `+` 或 `已添加`。
- 不展示开关状态。

内置/已安装视图：

- 展示开关。
- 切换开关后调用 `setSkillEnabled`。

### 技能详情

`src/features/skill-detail/SkillDetailPage.tsx` 根据状态切换按钮：

- 未添加：`添加并使用`
- 已启用：`使用`、`关闭`
- 已关闭：`开启并使用`、`保持关闭`

已关闭状态下禁止直接 `setPendingSkill`，必须先开启。

## 后端详细设计

后端要解决两个不同问题：

1. 企业/平台技能不应该因为“登录后同步”就全部进入本地可用集合。
2. 已存在本地的技能可以被用户关闭，并且关闭后不能再进入对话入口、模型 catalog 或 `Skill` tool。

这两个问题不能只靠前端隐藏解决。前端只负责展示和发起动作，真正的可用集合必须由 Rust 端统一裁剪。

### 本地启用状态存储

新增 `src-tauri/src/plugin/skill/enablement.rs`，集中管理技能启用状态。建议结构：

```rust
pub struct SkillEnablementStore {
    current_user: Arc<CurrentUserStorage>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillEnablementState {
    #[serde(default)]
    pub disabled_skill_ids: BTreeSet<String>,
}
```

存储路径：

- `~/.renlijia/users/{scope}/skillsConfig.json`。

当前产品没有登录态无法使用主应用，所以技能状态不需要全局兜底文件。`set_skill_enabled`、市场安装后的状态清理、聊天 catalog 读取都应基于当前 `CurrentUserStorage` 的用户 scope；如果没有用户 scope，管理类写操作直接返回未登录错误。

`UserScopedPaths` 增加 `skills_config_path()`。

写入必须使用 `src-tauri/src/storage/fs_atomic.rs::write_atomic`，不能用裸 `fs::write`。读取失败或文件不存在时视为全开启，并记录 warn；不能因为状态文件损坏导致聊天不可用。

状态模型采用“默认开启，只记录 disabled ids”。原因是当前大多数用户已有大量技能，如果迁移时默认关闭会改变存量行为；同时关闭列表更小，便于回滚和排查。

### IPC 命令

新增命令：

```rust
#[tauri::command]
pub async fn set_skill_enabled(
    app: AppHandle,
    skill_id: String,
    enabled: bool,
) -> Result<SkillInfo, String>
```

行为：

- 先检查 `skill_id` 是否存在于当前 `SkillRegistry`，不存在则尝试 `refresh_skill_registry` 后再查一次。
- 仍不存在返回 unknown，不写状态文件。
- `enabled = true`：从 `disabled_skill_ids` 删除该 id。
- `enabled = false`：加入 `disabled_skill_ids`。
- 写入成功后发事件 `skill:enablement-changed`，payload 至少包含 `{ skillId, enabled }`。
- 同时也可以复用现有 `skill:registry-refreshed` 让旧监听链路 reload，但推荐新增更语义化事件，前端监听两个事件都触发 `useSkillStore.reload()`。
- 返回合并了最新 `enabled` 字段的 `SkillInfo`，方便详情页和列表页乐观更新失败时回滚。

`list_skills` 和 `get_plugin_info` 也要读取 `SkillEnablementStore`，返回全量技能，并给每个 `SkillInfo` 合并 `enabled` 字段。这里必须是全量列表，不能过滤 disabled，否则“已安装/内置”管理页会看不到被关闭的技能。

### 市场与安装拆分

当前代码已经有 `list_marketplace_skills` 和 `install_marketplace_skill`，但登录后的 `sync_builtin_skills` 会走 `global_sync::sync_skill_packages_from_server`，把服务端列表批量安装到本地。这是上下文膨胀的根因。

改造后语义调整为：

- `list_marketplace_skills`：只拉企业/平台可添加目录，不安装、不刷新 registry。
- `install_marketplace_skill`：用户点击市场卡片 `+` 时才下载安装。安装成功后默认 enabled，即从 disabled 列表移除该 id，然后 refresh registry 并发刷新事件。
- `sync_builtin_skills`：不再在登录后安装所有远端包。改为“确保必需内置技能 + 刷新市场目录缓存 + 更新已安装技能的新版本”。它只能自动安装 allowlist 内的必需内置技能；其他远端新发布技能不会自动装进 `~/.renlijia/skills` 或用户 skills 目录。
- `AuthGate` 登录后可以继续调用同步命令，但这个命令不能增加非 allowlist 的已安装技能数量；只允许安装/更新必需内置技能、刷新目录缓存、更新已安装版本、清理远端已下架且本地由该同步链路安装的包。
- Skill Center 的“更新技能”操作文案和行为要拆清：`刷新市场` 只更新可添加列表，`更新已安装` 才检查本地已有技能的新版本。

如果第一阶段不做持久化市场缓存，也可以让市场页实时调用 `list_marketplace_skills`。但无论是否缓存，都不能把远端列表直接落到 registry。

### Registry 与 catalog

`SkillRegistry` 继续只表示“磁盘上已安装且解析成功的技能”，不混入用户启用状态。这样 registry 仍然可以服务管理页、导出、删除和更新检查。

需要新增 enabled 专用方法或辅助函数：

```rust
impl SkillRegistry {
    pub fn enabled_skill_ids(&self, state: &SkillEnablementState) -> Vec<String>;
    pub fn get_enabled(&self, id: &str, state: &SkillEnablementState) -> Option<&DiskSkill>;
    pub fn format_enabled_catalog(
        &self,
        state: &SkillEnablementState,
        context_window_tokens: usize,
    ) -> String;
}
```

`skill_ids()`、`get()`、`format_full_catalog()` 保持全量语义，避免管理路径误用被过滤后的集合。聊天路径必须显式改为 `format_enabled_catalog(...)`，这样代码审查时能一眼看出这是“模型可见集合”。

`catalog_delta_for_agent` 如果后续恢复使用，也必须有 enabled 版本；否则不要把它接回主聊天路径。

### Chat catalog 注入链路

`src-tauri/src/transport/tauri_commands/chat.rs` 中的 `get_skill_catalog` 不能再直接调用 `reg.format_full_catalog(200_000)`，需要：

1. 读取当前用户的 `SkillEnablementState`。
2. 锁 registry。
3. 调用 `format_enabled_catalog(&state, 200_000)`。

`src-tauri/src/runtime/chat/context_builder.rs` 不需要知道 disabled 细节，只接收过滤后的 catalog。这样 context builder 仍保持纯拼装职责。

### `Skill` runtime tool

`src-tauri/src/runtime/tools/builtin/load_skill.rs` 需要持有或能访问 `SkillEnablementStore`。

`definition()`：

- 读取启用状态。
- `可用 skill_id` 只列 enabled ids。
- 如果 enabled ids 为空，显示无可用技能。

`execute()`：

- 先解析 `skill_id`。
- 读取启用状态；如果 id 在 disabled 集合中，直接返回 unavailable，不做 refresh，也不加载 body。
- 如果 registry miss，可以按现有逻辑 throttle refresh 一次。
- refresh 后再次检查：存在但 disabled 仍返回 unavailable；不存在返回 unknown。

这样可以防止模型通过手写 `Skill({"skill_id":"disabled-id"})` 绕过前端选择器。

### 安装、卸载、更新的状态规则

- 市场点击 `+` 安装：安装成功后默认开启，删除 disabled override。
- 必需内置技能初始化安装：默认开启，但仅在用户没有 disabled override 时生效；如果用户曾关闭该 id，安装/更新后仍保持关闭。
- 本地导入：默认开启；如果是 force 覆盖同 id，也删除 disabled override，因为用户明确重新导入。
- 更新已安装技能：保留原 enabled/disabled 状态，不因为版本更新自动开启。
- 卸载技能：从 disabled 列表移除该 id，避免残留状态污染未来同名新技能。
- 同 id 用户技能 shadow 企业/平台技能：enabled 状态按 skill id 生效，作用于当前有效技能。这个行为简单且符合用户认知：“我关的是这个名字的技能”。

### 事件刷新与小家内存

关闭或开启技能后，必须同时刷新三处：

- 前端 `useSkillStore.reload()`：刷新技能中心、详情页、聊天输入框技能选择器。
- 后端 model catalog：下一次 `get_skill_catalog` 读取最新 enablement state，不依赖旧缓存。
- `Skill` tool definition：下一次工具定义生成读取最新 enablement state。

推荐事件：

- `skill:enablement-changed`：启用状态变化。
- `skill:registry-refreshed`：磁盘技能集合变化。

`AuthGate` 统一监听两个事件，触发 `useSkillStore.reload()`。这样“关闭后小家内存中的可使用技能列表刷新”不是 UI 层假象，而是前后端入口都切到同一个 enabled 集合。

## 关键行为

关闭一个技能后，必须同时满足：

- 技能仍在已安装/内置管理页可见。
- 市场页只显示已添加，不显示已关闭。
- 聊天输入框技能选择器不显示它。
- 技能详情页不允许直接使用，只能开启并使用。
- 模型动态上下文不包含它。
- `Skill` tool 不能加载它。
- 状态重启后仍然保留。

## 非目标

本次不做：

- 企业管理员强制启用/禁用策略。
- 完整技能广场运营系统，例如评分、排行榜、复杂推荐和下载量排序。
- 技能权限模型重构。
- 技能文件格式改造。

## 测试计划

前端：

- `skillStore`：reload 后保留 enabled 字段，`enabledSkills` 正确过滤。
- 技能中心：市场卡片不显示开关和已关闭；已安装/内置显示开关。
- 技能详情：三种状态按钮正确切换。
- ChatBottomArea / SkillPopover：只展示 enabled 技能。

后端：

- `list_skills` 返回全量技能和 enabled 状态。
- disabled 配置持久化到 user-scoped `skillsConfig.json` 本地文件。
- `format_full_catalog` 仍保持全量语义；新增 `format_enabled_catalog` 且不包含 disabled 技能。
- `Skill` tool definition 不包含 disabled id。
- `Skill` tool execute disabled id 返回 unavailable。
- 登录同步只自动安装 allowlist 内的必需内置技能，例如 `skill-creator` / `dingtalk-workspace`；其他服务端新增技能只进入市场，不自动安装。
- 必需内置技能默认开启，但用户关闭后同步不能重新开启。
- `sync_builtin_skills` 不再自动安装非 allowlist 服务端新增技能，只更新已安装技能或市场目录缓存。
- `install_marketplace_skill` 安装成功后默认 enabled，并刷新 registry 与前端 store。
- refresh/install/sync/toggle 后 registry、enabled 集合与前端 store 都刷新。

建议验证命令：

```powershell
pnpm exec vitest run src/stores/skillStore.test.ts src/features/skill-center/SkillCenterPage.integration.test.tsx src/features/skill-detail/SkillDetailPage.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx
cd src-tauri; cargo test --test skill_md_catalog_test -- --nocapture
cd src-tauri; cargo test --test load_skill_skill_md_test -- --nocapture
```

实际实现时按改动范围补充更聚焦的测试。

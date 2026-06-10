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

如果市场数据与已安装数据暂时来自同一个来源，可以先通过已安装列表判断 `installed`，但 UI 语义要保持清晰。

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

## 后端改造

### list skills

`src-tauri/src/commands/skill_management.rs` 的 `SkillInfo` 返回 `enabled`。

`list_skills_from_registry` 需要读取用户本地 disabled 配置，并把 enabled 合并到返回值。

### registry/catalog

当前 `SkillRegistry::format_full_catalog(200_000)` 会格式化全部 registry skill。改造后必须只格式化 enabled 技能。

推荐做法：

- `SkillRegistry` 保留全量技能。
- 增加 enabled filter，或在 registry replace/load 后附加 enabled 状态。
- 保留全量枚举能力，供 `list_skills` 和管理页使用。
- 新增 enabled 专用枚举/格式化能力，供模型 catalog 和 `Skill` tool 描述使用。
- `format_full_catalog` 的聊天路径必须只输出 enabled 技能；如果保留原方法名，需要确认所有管理页调用都不依赖它展示全量。
- `list_skills` 走全量接口并附带 enabled，确保关闭技能仍能在已安装/内置管理页可见。

关闭技能后要刷新小家内存中的可使用技能列表，至少触发：

- 前端 `useSkillStore.reload()`。
- 后端 skill registry/catalog 可用集合刷新。
- `skill:registry-refreshed` 或新增更明确事件。

### Skill tool

`src-tauri/src/runtime/tools/builtin/load_skill.rs` 的 tool definition 和 execute 都要尊重 enabled 状态。

要求：

- Tool description 中的可用 skill ids 只列 enabled。
- 执行 disabled skill 时返回 unavailable，不自动 refresh 后绕过。
- 错误信息区分 unknown 和 disabled/unavailable，便于测试和排查。

### 聊天上下文

`src-tauri/src/transport/tauri_commands/chat.rs` 中获取 skill catalog 的链路必须只拿 enabled 技能。

`src-tauri/src/runtime/chat/context_builder.rs` 不需要知道 disabled 细节，只接收已过滤后的 skill catalog。

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
- 技能评分、下载量、热门排序。
- 独立远程技能广场完整市场系统。
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
- disabled 配置持久化到 user-scoped 本地文件。
- `format_full_catalog` 不包含 disabled 技能。
- `Skill` tool definition 不包含 disabled id。
- `Skill` tool execute disabled id 返回 unavailable。
- refresh/install/sync/toggle 后 registry 与前端 store 都刷新。

建议验证命令：

```powershell
pnpm exec vitest run src/stores/skillStore.test.ts src/features/skill-center/SkillCenterPage.integration.test.tsx src/features/skill-detail/SkillDetailPage.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx
cd src-tauri; cargo test --test skill_md_catalog_test -- --nocapture
cd src-tauri; cargo test --test load_skill_skill_md_test -- --nocapture
```

实际实现时按改动范围补充更聚焦的测试。

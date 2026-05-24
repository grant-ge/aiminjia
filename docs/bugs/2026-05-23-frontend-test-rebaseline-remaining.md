# 前端单测重对齐 —— 已解决(2026-05-23 起,2026-05-24 收尾)

## 背景

`pnpm test` 曾有一批既有失败单测,根因是 oayzz 一轮 UI 大改(IM 多平台频道矩阵、技能页、产物预览面板、设置页等)后**测试腐烂**:组件已随 v0.5.29 上线、是事实上的预期行为,但测试断言停留在旧设计。

## 结果

**既有失败 43 → 0**。全量:`160 files passed / 1002 passed / 10 skipped`。

排查准则:组件已上线即事实预期,默认以组件为准重对齐断言;但凡看着像**真实回归**(测试对、组件错)的就**修组件**,不弱化测试。

## 顺带发现并修复的 3 个真实 bug

1. **`ExecutionTraceCard` 可折叠头部缺 `aria-expanded`** —— 补上(ToolGroupCard 折叠 a11y 测试守护的就是它)。
2. **侧栏 4 个 body tab(项目/员工/专家/频道)图标按钮无可访问名称** —— 未激活时纯图标,补 `aria-label`(无障碍 + 测试可定位)。
3. **`messageList.cannotPreview/cannotOpen/cannotReveal` 三个 i18n key 缺失** —— 文件预览/打开/定位失败时 toast 标题显示生肉 key(如 `messageList.cannotOpen`),补齐 zh-CN + en-US。

## 主要测试重对齐(组件为准)

- 测试基建:新增 `src/test/setup-tauri.ts` 全局 stub `window.__TAURI_INTERNALS__`,消除组件 mount 时直接调 `@tauri-apps/api` 的同步崩溃。
- store/mock 补全:`useUiStore`(getState/subscribe/sidebarTab/consumePendingSkill,AppSidebar 改为响应式 mock)、`@/lib/tauri`(getLastBrand/saveLastBrand)、`chatStore`(busyConversations / getState.conversations 供 hasExpertTeam)、turn 的 `peerBanners`。
- 文案/标签/尺寸/class:ChatTopBar、AuthGate(登录表单改账号+密码)、FilePreviewPane、SettingsModal、ArchivedPanel、TenantHeader、AboutPanel、HomeMascotHero、ScheduleTemplateCard、GeneratedFileCard 等。
- 交互改版:SkillCenter 导入改下拉菜单(pointerDown 开菜单 + 点「导入技能目录」)、AiBubble transcript 改展开式、SkillDetail「试试」模块移除后重写为「使用」按钮、AppSidebar 多平台频道矩阵(频道会话标签「钉钉私聊」)、专家团 tab→「专家」。
- 逻辑:useTurnRenderModel 用 `toMatchObject` 容纳新增透传字段。
- 清理:删除 2 个过时重复测试文件(`layout/Sidebar.test`、`sidebar/AppSidebar.test`)。

## 唯一待办(功能恢复时)

`src/components/chat/RightPanel.test.tsx` 有 **10 个 `it.skip`**:RightPanel 改版后 `SHOW_TASK_MONITOR=false`,任务监控/产物侧栏返回 null(代码路径保留)。这些测产物列表的用例已 skip 并注明 —— **当 `SHOW_TASK_MONITOR` 翻回 `true` 时,去掉 `it.skip` 即自动恢复**。

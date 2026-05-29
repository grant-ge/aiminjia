# Frontend Visual Realignment To Design Pen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 `design.pen` 重做 lotus-app 前端视觉，实现“先组件、后页面”的高还原度界面重构。

**Architecture:** 保留现有前端状态与路由语义，先在组合组件层建立 `AppSidebar / PageTopBar / 首页任务卡 / 技能卡 / 设置外壳 / 聊天场景组件` 等设计稿对应物，再让页面层仅负责拼装这些组件。所有视觉判断以 `design.pen` 为准，页面不得继续直接堆 shadcn 原子组件来定义主视觉结构。

**Tech Stack:** React 19 / TypeScript / Zustand / Vitest / Testing Library / Tailwind CSS v4 / shadcn/ui / Tauri frontend shell

---

## 涉及文件总览

### 新增文件

- `src/components/shell/PageTopBar.tsx` — 页面统一顶栏，覆盖 Title / Breadcrumb / Compact 变体
- `src/components/shell/PageSectionShell.tsx` — 页面内容区统一宽度、边距、垂直节奏
- `src/components/home/HomeTaskComposerCard.tsx` — 首页主任务输入卡
- `src/components/home/HomeSuggestionList.tsx` — 首页推荐能力/建议列表
- `src/components/skills/SkillHeroHeader.tsx` — 技能中心/详情页顶部头区
- `src/components/skills/SkillCategoryTabs.tsx` — 技能分类切换条
- `src/components/skills/SkillWorkflowPreviewCard.tsx` — 技能详情工作流卡
- `src/components/skills/SkillActionBar.tsx` — 技能详情 CTA 区
- `src/components/schedules/ScheduleTemplateCard.tsx` — 定时任务模板卡
- `src/components/schedules/ScheduleListShell.tsx` — 定时任务列表外壳
- `src/components/chat-scene/ChatComposerCompact.tsx` — 聊天页紧凑输入区壳
- `src/components/chat-scene/ToolGroupBar.tsx` — 工具过程折叠摘要 bar
- `src/components/chat-scene/ToolGroupPanel.tsx` — 工具过程展开容器
- `src/components/chat-scene/ChatEmptyState.tsx` — 聊天空态/欢迎区

### 修改文件

- `src/components/sidebar/AppSidebar.tsx` — 侧栏整体结构与间距、选中态、底部设置入口对齐设计稿
- `src/components/sidebar/TenantHeader.tsx` — 租户头部与品牌区对齐设计稿
- `src/components/sidebar/SidebarNav.tsx` — 顶部主导航项对齐设计稿
- `src/components/sidebar/ConversationTree.tsx` — 项目/会话树样式对齐设计稿
- `src/components/skill-center/SkillCard.tsx` — 技能卡视觉重做
- `src/components/settings/SettingsModal.tsx` — 设置弹窗外壳与内容区结构重做
- `src/components/layout/InputBar.tsx` — 用 `ChatComposerCompact` 承载输入区视觉
- `src/components/chat/WelcomeScreen.tsx` — 改为 `ChatEmptyState` 风格
- `src/components/rich-content/GeneratedFileCard.tsx` — 视觉风格与设计稿对齐
- `src/features/home/HomePage.tsx` — 改为拼装首页组合组件
- `src/features/skill-center/SkillCenterPage.tsx` — 改为拼装技能中心组合组件
- `src/features/skill-detail/SkillDetailPage.tsx` — 改为拼装技能详情组合组件
- `src/features/schedules/SchedulesPage.tsx` — 改为拼装定时任务组合组件
- `src/features/chat/ChatPage.tsx` — 接入 `PageTopBar` / `ChatComposerCompact` / 聊天空态结构
- `src/components/auth/LoginPage.tsx` — 登录页视觉对齐设计稿
- `src/components/layout/Sidebar.test.tsx` — 跟随侧栏结构更新断言
- `src/features/skill-center/SkillCenterPage.integration.test.tsx` — 跟随技能中心结构更新
- `src/components/layout/InputBar.agent-selector.test.tsx` — 跟随输入区结构更新
- `src/components/settings/McpServerList.tsx` / `src/components/settings/McpTab.tsx` / `src/components/settings/SkillMarketplace.tsx` / `src/components/settings/SkillsTab.tsx` — 如果被设置弹窗外壳重构影响，则仅做最小适配

### 参考文件（不直接修改）

- `design.pen` — 唯一视觉真源
- `docs/superpowers/specs/2026-04-23-frontend-visual-realignment-to-design-pen.md` — 本轮设计规格

---

## Task 1：建立 Shell 级组合组件骨架

**Files:**
- Create: `src/components/shell/PageTopBar.tsx`
- Create: `src/components/shell/PageSectionShell.tsx`
- Modify: `src/components/sidebar/AppSidebar.tsx`
- Modify: `src/components/sidebar/TenantHeader.tsx`
- Modify: `src/components/sidebar/SidebarNav.tsx`
- Modify: `src/components/sidebar/ConversationTree.tsx`
- Test: `src/components/layout/Sidebar.test.tsx`

- [ ] **Step 1: 写侧栏结构失败测试，锁定设计稿最小结构**

在 `src/components/layout/Sidebar.test.tsx` 增加或改成以下断言意图：

```tsx
it('renders design-pen style sidebar shell', () => {
  render(<Sidebar onOpenSettings={vi.fn()} />)

  expect(screen.getByText('新任务')).toBeInTheDocument()
  expect(screen.getByText('技能中心')).toBeInTheDocument()
  expect(screen.getByText('定时任务')).toBeInTheDocument()
  expect(screen.getByLabelText('搜索对话')).toBeInTheDocument()
  expect(screen.getByText('设置')).toBeInTheDocument()
})
```

- [ ] **Step 2: 运行侧栏测试确认当前结构不足或即将重构**

Run: `pnpm exec vitest run src/components/layout/Sidebar.test.tsx`
Expected: 若失败，失败点应落在设计稿结构断言；若已通过，也作为基线锁定。

- [ ] **Step 3: 新建 `PageSectionShell.tsx`，统一页面内容容器节奏**

创建：
```tsx
import type { PropsWithChildren, ReactNode } from 'react'

interface PageSectionShellProps extends PropsWithChildren {
  header?: ReactNode
  className?: string
}

export function PageSectionShell({ header, className = '', children }: PageSectionShellProps) {
  return (
    <div className="flex h-full flex-col overflow-hidden bg-background">
      {header}
      <div className="min-h-0 flex-1 overflow-auto">
        <div className={`mx-auto flex w-full max-w-[1040px] flex-col gap-6 px-8 py-8 ${className}`.trim()}>
          {children}
        </div>
      </div>
    </div>
  )
}
```

- [ ] **Step 4: 新建 `PageTopBar.tsx`，提供 Title / Breadcrumb / Compact 变体**

创建：
```tsx
import type { ReactNode } from 'react'

interface PageTopBarProps {
  title?: ReactNode
  subtitle?: ReactNode
  leading?: ReactNode
  trailing?: ReactNode
  compact?: boolean
}

export function PageTopBar({ title, subtitle, leading, trailing, compact = false }: PageTopBarProps) {
  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b border-border bg-background px-6">
      <div className="min-w-0 flex items-center gap-3">
        {leading}
        <div className="min-w-0">
          {title ? <div className={`truncate ${compact ? 'text-sm font-medium' : 'text-base font-semibold'}`}>{title}</div> : null}
          {subtitle ? <div className="truncate text-xs text-muted-foreground">{subtitle}</div> : null}
        </div>
      </div>
      {trailing ? <div className="flex items-center gap-2">{trailing}</div> : null}
    </header>
  )
}
```

- [ ] **Step 5: 重构 `AppSidebar.tsx`，只保留设计稿里的侧栏职责**

目标：
- 248px 左右固定宽度
- 顶部品牌头
- 主导航
- 搜索框
- 会话树
- 底部设置入口

实现时优先复用 `TenantHeader` / `SidebarNav` / `ConversationTree`，但把间距和容器层次按设计稿重排。

- [ ] **Step 6: 重构 `TenantHeader.tsx` / `SidebarNav.tsx` / `ConversationTree.tsx` 以贴近设计稿**

要求：
- 品牌区更紧凑，信息层级更弱化
- nav item 有明确 active/default 区分
- conversation item 更轻、更紧凑，当前活跃项更突出
- 不再用默认按钮视觉伪装成导航

- [ ] **Step 7: 运行侧栏测试并做类型检查**

Run: `pnpm exec vitest run src/components/layout/Sidebar.test.tsx && pnpm exec tsc -p tsconfig.app.json --noEmit`
Expected: 全部通过

- [ ] **Step 8: Commit**

```bash
git add src/components/shell/PageTopBar.tsx src/components/shell/PageSectionShell.tsx src/components/sidebar/AppSidebar.tsx src/components/sidebar/TenantHeader.tsx src/components/sidebar/SidebarNav.tsx src/components/sidebar/ConversationTree.tsx src/components/layout/Sidebar.test.tsx
git commit -m "refactor(frontend): rebuild shell sidebar and page top bar"
```

---

## Task 2：重做首页组合组件

**Files:**
- Create: `src/components/home/HomeTaskComposerCard.tsx`
- Create: `src/components/home/HomeSuggestionList.tsx`
- Modify: `src/features/home/HomePage.tsx`
- Test: `src/features/home/HomePage.tsx`（如无现成测试，可通过页面级 smoke + TS 验证）

- [ ] **Step 1: 为首页主结构写最小 smoke 测试或断言入口**

若当前无专门测试，可新增 `src/features/home/HomePage.test.tsx`：
```tsx
it('renders design-pen inspired home task composer', () => {
  render(<HomePage />)
  expect(screen.getByText('创建你的下一条任务')).toBeInTheDocument()
  expect(screen.getByPlaceholderText('描述目标、补充信息，或输入 / 选择你要调用的技能来开始。')).toBeInTheDocument()
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/features/home/HomePage.test.tsx`
Expected: FAIL，文案或结构不存在

- [ ] **Step 3: 创建 `HomeTaskComposerCard.tsx`**

要求：
- 中央大任务卡
- 顶部品牌/标题区
- 大 textarea
- 下方能力标签与快捷入口
- CTA 区明显但不刺眼

- [ ] **Step 4: 创建 `HomeSuggestionList.tsx`**

要求：
- 三条推荐能力或建议项
- 图标 + 标题 + 辅助说明
- 与设计稿节奏接近

- [ ] **Step 5: 改写 `HomePage.tsx`，只拼装 `AppSidebar + PageTopBar + PageSectionShell + HomeTaskComposerCard + HomeSuggestionList`**

页面不再直接管理视觉细节。

- [ ] **Step 6: 跑首页测试与类型检查**

Run: `pnpm exec vitest run src/features/home/HomePage.test.tsx && pnpm exec tsc -p tsconfig.app.json --noEmit`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/components/home/HomeTaskComposerCard.tsx src/components/home/HomeSuggestionList.tsx src/features/home/HomePage.tsx src/features/home/HomePage.test.tsx
git commit -m "refactor(frontend): rebuild home page from design pen"
```

---

## Task 3：重做技能中心与技能详情组件

**Files:**
- Create: `src/components/skills/SkillHeroHeader.tsx`
- Create: `src/components/skills/SkillCategoryTabs.tsx`
- Create: `src/components/skills/SkillWorkflowPreviewCard.tsx`
- Create: `src/components/skills/SkillActionBar.tsx`
- Modify: `src/components/skill-center/SkillCard.tsx`
- Modify: `src/features/skill-center/SkillCenterPage.tsx`
- Modify: `src/features/skill-detail/SkillDetailPage.tsx`
- Modify: `src/features/skill-center/SkillCenterPage.integration.test.tsx`

- [ ] **Step 1: 先更新技能中心集成测试断言到新页面骨架**

测试仍保留原行为目标：切分类、点卡片进详情。

- [ ] **Step 2: 运行测试确认失败或作为基线**

Run: `pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx`
Expected: 若失败，应落在结构断言不匹配；若通过，继续作为基线。

- [ ] **Step 3: 创建 `SkillHeroHeader.tsx` 与 `SkillCategoryTabs.tsx`**

目的：统一技能页顶部标题、副标题、操作区与分类标签条视觉。

- [ ] **Step 4: 重构 `SkillCard.tsx`，按设计稿重做信息密度与 CTA 布局**

要求：
- 卡片层次更清晰
- 图标/标题/说明/动作的 spacing 与边框节奏统一
- 不再只是默认 card + buttons

- [ ] **Step 5: 创建 `SkillWorkflowPreviewCard.tsx` 与 `SkillActionBar.tsx`**

供技能详情页复用。

- [ ] **Step 6: 改写 `SkillCenterPage.tsx` 与 `SkillDetailPage.tsx` 为纯拼装页**

要求：
- 顶部统一走 `PageTopBar` / `PageSectionShell`
- 技能中心使用 `SkillHeroHeader + SkillCategoryTabs + SkillCard grid`
- 技能详情使用 `SkillHeroHeader + SkillWorkflowPreviewCard + SkillActionBar`

- [ ] **Step 7: 运行技能中心测试与类型检查**

Run: `pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx && pnpm exec tsc -p tsconfig.app.json --noEmit`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src/components/skills src/components/skill-center/SkillCard.tsx src/features/skill-center/SkillCenterPage.tsx src/features/skill-detail/SkillDetailPage.tsx src/features/skill-center/SkillCenterPage.integration.test.tsx
git commit -m "refactor(frontend): rebuild skills pages from design pen"
```

---

## Task 4：重做定时任务页与设置弹窗外壳

**Files:**
- Create: `src/components/schedules/ScheduleTemplateCard.tsx`
- Create: `src/components/schedules/ScheduleListShell.tsx`
- Modify: `src/features/schedules/SchedulesPage.tsx`
- Modify: `src/components/settings/SettingsModal.tsx`
- Modify: `src/components/settings/SkillMarketplace.tsx`
- Modify: `src/components/settings/SkillsTab.tsx`
- Modify: `src/components/settings/McpTab.tsx`
- Modify: `src/components/settings/McpServerList.tsx`

- [ ] **Step 1: 先为定时任务页新增最小 smoke 测试**

创建 `src/features/schedules/SchedulesPage.test.tsx`：
```tsx
it('renders schedule templates and list shell', () => {
  render(<SchedulesPage />)
  expect(screen.getByText('定时任务')).toBeInTheDocument()
  expect(screen.getByText('任务列表')).toBeInTheDocument()
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/features/schedules/SchedulesPage.test.tsx`
Expected: FAIL 或部分断言不满足

- [ ] **Step 3: 创建 `ScheduleTemplateCard.tsx` 与 `ScheduleListShell.tsx`**

目标：把当前定时任务页从普通 cards 升级为设计稿对应的模板区 + 列表区。

- [ ] **Step 4: 重写 `SchedulesPage.tsx` 为组合组件拼装**

要求：页面本身不再定义卡片细节。

- [ ] **Step 5: 重构 `SettingsModal.tsx` 为真正的设计稿分栏壳**

要求：
- 左侧 tab 导航
- 右侧内容区统一 panel 容器
- account/general/about/usage 都挂在统一视觉壳内
- 现有具体 panel 内容先尽量复用，但容器视觉要对齐设计稿

- [ ] **Step 6: 适配 `SkillsTab` / `SkillMarketplace` / `McpTab` / `McpServerList` 到新设置壳**

仅做最小必要适配，避免破坏已有功能。

- [ ] **Step 7: 运行设置与定时任务相关测试、类型检查**

Run: `pnpm exec vitest run src/features/schedules/SchedulesPage.test.tsx src/components/settings/McpTab.test.tsx src/components/settings/McpServerList.test.tsx && pnpm exec tsc -p tsconfig.app.json --noEmit`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src/components/schedules src/features/schedules/SchedulesPage.tsx src/features/schedules/SchedulesPage.test.tsx src/components/settings/SettingsModal.tsx src/components/settings/SkillMarketplace.tsx src/components/settings/SkillsTab.tsx src/components/settings/McpTab.tsx src/components/settings/McpServerList.tsx
git commit -m "refactor(frontend): rebuild schedules page and settings shell"
```

---

## Task 5：重做聊天场景组件与聊天页面壳

**Files:**
- Create: `src/components/chat-scene/ChatComposerCompact.tsx`
- Create: `src/components/chat-scene/ToolGroupBar.tsx`
- Create: `src/components/chat-scene/ToolGroupPanel.tsx`
- Create: `src/components/chat-scene/ChatEmptyState.tsx`
- Modify: `src/components/layout/InputBar.tsx`
- Modify: `src/components/chat/WelcomeScreen.tsx`
- Modify: `src/components/rich-content/GeneratedFileCard.tsx`
- Modify: `src/features/chat/ChatPage.tsx`
- Modify: `src/components/layout/InputBar.agent-selector.test.tsx`
- Modify: `src/components/rich-content/GeneratedFileCard.test.tsx`

- [ ] **Step 1: 先调整输入区与文件卡相关测试为新结构目标**

保留行为目标：
- SkillPopover 可触达
- 输入区能工作
- 文件卡动作仍能触发

- [ ] **Step 2: 运行相关测试确认失败或作为基线**

Run: `pnpm exec vitest run src/components/layout/InputBar.agent-selector.test.tsx src/components/rich-content/GeneratedFileCard.test.tsx`
Expected: 结构断言可能失败，但行为目标保持明确。

- [ ] **Step 3: 创建 `ChatComposerCompact.tsx`**

目标：承载设计稿中的紧凑输入区结构，包括：
- 文本输入
- 技能触发入口
- 底部轻量工具行/附件行
- 更贴近设计稿的外壳与边距

- [ ] **Step 4: 创建 `ChatEmptyState.tsx`，替换 `WelcomeScreen` 的骨架感**

要求：
- 统一空态图标、标题、说明、建议卡
- 与首页视觉同体系

- [ ] **Step 5: 创建 `ToolGroupBar.tsx` / `ToolGroupPanel.tsx` 作为后续 ToolGroup UI 承载层**

即便当前消息结构暂不完全切过去，也先把设计稿对应的可复用视觉容器建好，避免继续在消息组件里散写样式。

- [ ] **Step 6: 重构 `GeneratedFileCard.tsx` 与 `InputBar.tsx`**

要求：
- `GeneratedFileCard` 更贴近设计稿卡片层次
- `InputBar` 视觉由 `ChatComposerCompact` 接管

- [ ] **Step 7: 改写 `ChatPage.tsx`，统一接入 `PageTopBar` / `ChatEmptyState` / 新输入区壳**

页面主要负责：
- 顶栏
- 消息区容器
- 空态或消息列表
- 底部输入区

- [ ] **Step 8: 运行聊天相关测试与类型检查**

Run: `pnpm exec vitest run src/components/layout/InputBar.agent-selector.test.tsx src/components/rich-content/GeneratedFileCard.test.tsx src/components/chat/MessageItem.test.tsx && pnpm exec tsc -p tsconfig.app.json --noEmit`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add src/components/chat-scene src/components/layout/InputBar.tsx src/components/chat/WelcomeScreen.tsx src/components/rich-content/GeneratedFileCard.tsx src/features/chat/ChatPage.tsx src/components/layout/InputBar.agent-selector.test.tsx src/components/rich-content/GeneratedFileCard.test.tsx
git commit -m "refactor(frontend): rebuild chat scene from design pen"
```

---

## Task 6：对齐登录页并做最终全量验收

**Files:**
- Modify: `src/components/auth/LoginPage.tsx`
- Modify: `src/features/auth/AuthGate.integration.test.tsx`
- Verify: 现有前端关键测试与全量构建

- [ ] **Step 1: 先补登录页结构断言**

在 `src/features/auth/AuthGate.integration.test.tsx` 中确保未登录态仍能找到登录页核心元素，并允许视觉结构变化但不破坏交互语义。

- [ ] **Step 2: 运行认证测试确认基线**

Run: `pnpm exec vitest run src/features/auth/AuthGate.integration.test.tsx`
Expected: PASS 或在结构更新后有明确失败点

- [ ] **Step 3: 重构 `LoginPage.tsx` 对齐设计稿**

要求：
- 居中单卡片布局
- 标题、副标题、输入区、按钮区视觉与主系统统一
- 不再是通用 shadcn form 示例页

- [ ] **Step 4: 运行认证测试、lint、类型检查、全量测试**

Run: `pnpm exec vitest run src/features/auth/AuthGate.integration.test.tsx && pnpm lint && pnpm exec tsc -b --pretty false && pnpm test`
Expected: 全部通过

- [ ] **Step 5: 手工视觉验收（必须打开应用）**

Run: `pnpm tauri:dev`
验收点：
- 首页、技能中心、技能详情、定时任务、设置、聊天、登录页主视觉均明显向 `design.pen` 对齐
- 页面不再呈现“默认 shadcn 骨架页”观感
- 侧栏、顶栏、卡片、输入区是一套统一设计语言

- [ ] **Step 6: Commit**

```bash
git add src/components/auth/LoginPage.tsx src/features/auth/AuthGate.integration.test.tsx
git commit -m "refactor(frontend): align login and finish visual realignment"
```

---

## 自检

- spec 覆盖检查：已覆盖 Sidebar / PageTopBar / 首页 / 技能中心 / 技能详情 / 定时任务 / 设置 / 聊天 / 登录页；没有把后端 runtime 任务混入。
- placeholder 检查：无 TBD / TODO / “类似 Task N” 之类空洞指令。
- 类型一致性检查：新增组合组件名称在文件总览与各任务中保持一致；页面层均以“拼装组合组件”为唯一职责。


# Frontend Design System And Skill-First Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 lotus-app 前端切换为 shadcn token + Skill-First 信息架构，落地全屏登录 Gate、租户单色皮肤、技能中心骨架，并删除 Persona / AgentSelector / useCloud 相关前端概念。

**Architecture:** 先收敛 token 与 branding 派生入口，再建立基于 route atom 的 AppShell/AuthGate 骨架，随后新增首页/技能中心/技能详情/定时任务页面并迁移聊天页与设置弹窗，最后清理 persona/useCloud 残留与做 DoD 验收。整体保持前端单向依赖：`styles -> stores -> ui primitives -> feature pages -> App shell`，后端 IPC 仅做兼容保留，不在本计划里改 Rust 语义。

**Tech Stack:** React 19 / TypeScript / Zustand / Vitest / Testing Library / Tailwind CSS v4 / shadcn/ui (Radix UI + class-variance-authority + lucide-react + sonner) / Tauri IPC wrappers

---

## 实施前约束

- 仅在当前分支 `pzc` 执行，不创建 worktree，不切分支。
- 实施时必须先用 `superpowers:test-driven-development`，执行完成前必须用 `superpowers:verification-before-completion`。
- 所有页面颜色必须走 token class；禁止 inline color style、禁止 JS hover、禁止新增 persona/agent/useCloud UI 分支。
- 若对标 `claude-code-best` 后发现当前 spec 的前端边界不合理，先回改本计划，再继续实现。

## 涉及文件总览

### 新增文件

- `docs/superpowers/plans/2026-04-22-frontend-design-system-and-skill-first-shell-design.md` — 本计划。
- `src/styles/skin.ts` — 单一租户色派生算法与 CSS 变量 key 列表。
- `src/styles/skin.test.ts` — 纯函数边界测试。
- `src/stores/uiStore.ts` — route / settings modal / 侧栏导航状态。
- `src/stores/skillStore.ts` — 技能中心读取、分类过滤、安装/卸载/上传占位 action。
- `src/stores/skillStore.test.ts` — 技能分类与 reload 行为测试。
- `src/data/skill-categories.ts` — 10 个固定分类与推荐分类辅助方法。
- `src/components/ui/*` — shadcn 基础组件（Button/Input/Card/Dialog/Tabs/Sidebar/Dropdown/Tooltip/Alert/AlertDialog/Badge/Separator/ScrollArea/Skeleton/Sheet/Textarea/Popover）。
- `src/components/auth/AuthGate.tsx` — 登录 Gate 与启动恢复兜底。
- `src/components/auth/FullscreenLoader.tsx` — 启动恢复全屏 loading。
- `src/components/auth/LoginPage.tsx` — 全屏登录页。
- `src/components/sidebar/AppSidebar.tsx` — 新主侧栏容器。
- `src/components/sidebar/SidebarNav.tsx` — 顶部四入口导航。
- `src/components/sidebar/ConversationTree.tsx` — 项目/会话列表区。
- `src/components/sidebar/TenantHeader.tsx` — 品牌头部与账号下拉。
- `src/components/skill-center/SkillCard.tsx` — 技能卡。
- `src/components/chat/SkillPopover.tsx` — 聊天输入区技能快捷弹层。
- `src/features/home/HomePage.tsx` — 新任务首页。
- `src/features/skill-center/SkillCenterPage.tsx` — 技能中心页。
- `src/features/skill-detail/SkillDetailPage.tsx` — 技能详情页。
- `src/features/skill-center/SkillCenterPage.integration.test.tsx` — 技能中心交互测试。
- `src/features/skill-center/SkillMarketModal.tsx` — 技能市场骨架。
- `src/features/skill-center/SkillUploadModal.tsx` — 技能上传骨架。
- `src/features/schedules/SchedulesPage.tsx` — 定时任务骨架页。
- `src/features/auth/AuthGate.integration.test.tsx` — 登录 Gate 集成测试。
- `src/features/chat/ChatPage.tsx` — 承接现有 ChatArea / TopBar / InputBar 的对话页容器。
- `src/lib/utils.ts` — shadcn 常用 `cn()` 帮助函数（若仓库尚无同类文件）。

### 修改文件

- `package.json` — 增加 shadcn 基础依赖与可能的 `dlx`/generator 依赖。
- `src/styles/globals.css` — 重写为 shadcn 原生命名 token + `@theme inline`。
- `src/lib/themeUtils.ts` — 导出 `mix` 并保留现有色彩工具。
- `src/stores/brandingStore.ts` — 收敛为单 `accentColor` 派生入口。
- `src/stores/brandingStore.test.ts` — 改写断言，覆盖派生变量数量与 reset。
- `src/stores/authStore.ts` — 增加 `redirectFrom` / `isAuthPending` / restore/login/logout helpers。
- `src/stores/settingsStore.ts` — 删除 `useCloud` 语义暴露，保留 cloud model 选择。
- `src/stores/settingsStore.test.ts` — 更新为无 `useCloud` 断言。
- `src/types/settings.ts` — 删除 `useCloud`，保留 `cloudModel` / `cloudModelType`。
- `src/lib/tauri.ts` — 补齐 auth/skill 相关前端类型、保留 persona IPC 但标注 deprecated。
- `src/App.tsx` — 重建顶层渲染树为 `AuthGate + AppShell + RouteSwitch + SettingsModal`。
- `src/main.tsx` — 继续挂载 App；若 Sonner provider 需要顶层容器则在此注入。
- `src/components/layout/InputBar.tsx` — 删除 `AgentSelector`，接入 `SkillPopover`。
- `src/components/layout/Sidebar.tsx` — 替换为兼容导出或直接迁移到新 `AppSidebar`。
- `src/components/layout/Sidebar.test.tsx` — 跟随新侧栏结构改测试。
- `src/components/chat/WelcomeScreen.tsx` — 删除 persona 文案与 linked category 过滤。
- `src/components/settings/SettingsModal.tsx` — 删除 persona/models/search 的未登录分支，改为 account/general/about/usage。
- `src/components/settings/LoginSection.tsx` — 改为仅显示账号信息/退出入口，移除云端-本地 toggle。
- `src/components/settings/PersonaTab.tsx` — 删除。
- `src/components/onboarding/PersonaSelector.tsx` — 删除。
- `src/stores/personaStore.ts` — 删除。
- `src/components/chat/AgentSelector.tsx` — 删除。
- `src/i18n/zh-CN.json` — 删除 persona/useCloud 文案，新增登录 gate、技能中心、定时任务、设置新文案。
- `src/i18n/en-US.json` — 同上。
- `src/hooks/useChat.ts` — 支持从技能中心 CTA 创建对话后跳到 chat route。
- `src/hooks/useWorkspaceAuthorization.ts` / `src/hooks/useAuthorizedWorkspace.ts` — 如依赖 activeConversationId，则适配 route-first 结构。

### 删除文件

- `src/stores/personaStore.ts`
- `src/components/chat/AgentSelector.tsx`
- `src/components/onboarding/PersonaSelector.tsx`
- `src/components/settings/PersonaTab.tsx`

---

## Task 1：收敛 token 系统与租户皮肤派生

**Files:**
- Create: `src/styles/skin.ts`
- Create: `src/styles/skin.test.ts`
- Modify: `src/lib/themeUtils.ts`
- Modify: `src/styles/globals.css`
- Test: `src/styles/skin.test.ts`

- [ ] **Step 1: 先写 `skin.ts` 的失败测试**

创建 `src/styles/skin.test.ts`：
```ts
import { describe, expect, it } from 'vitest'

import { DERIVED_SKIN_KEYS, deriveSkin } from '@/styles/skin'


describe('deriveSkin', () => {
  it('从 accentColor 派生 shadcn token', () => {
    const skin = deriveSkin('#DBAA22')

    expect(skin['--primary']).toBe('#DBAA22')
    expect(skin['--ring']).toBe('#DBAA22')
    expect(skin['--sidebar']).toMatch(/^#/i)
    expect(skin['--sidebar-accent']).toMatch(/^#/i)
    expect(skin['--primary-foreground']).toBe('#1A1A1A')
    expect(Object.keys(skin)).toEqual(DERIVED_SKIN_KEYS)
  })

  it('深色主色时返回白色前景', () => {
    const skin = deriveSkin('#1A2E22')

    expect(skin['--primary-foreground']).toBe('#FFFFFF')
    expect(skin['--sidebar-primary-foreground']).toBe('#FFFFFF')
  })

  it('非法输入时回退默认金色', () => {
    const skin = deriveSkin('bad-color')

    expect(skin['--primary']).toBe('#DBAA22')
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/styles/skin.test.ts`
Expected: FAIL，报 `Cannot find module '@/styles/skin'` 或 `deriveSkin is not exported`

- [ ] **Step 3: 实现 `skin.ts` 与补齐 `themeUtils.ts`**

创建 `src/styles/skin.ts`：
```ts
import { darken, isDarkColor, mix } from '@/lib/themeUtils'

export const DEFAULT_ACCENT_COLOR = '#DBAA22'

export const DERIVED_SKIN_KEYS = [
  '--primary',
  '--primary-foreground',
  '--ring',
  '--sidebar-primary',
  '--sidebar-primary-foreground',
  '--sidebar',
  '--sidebar-accent',
] as const

function normalizeAccentColor(input?: string): string {
  return /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(input ?? '')
    ? (input as string)
    : DEFAULT_ACCENT_COLOR
}

export function deriveSkin(accentColor?: string): Record<(typeof DERIVED_SKIN_KEYS)[number], string> {
  const accent = normalizeAccentColor(accentColor)
  const foreground = isDarkColor(accent) ? '#FFFFFF' : '#1A1A1A'
  const sidebar = mix(accent, '#FFFFFF', 0.93)

  return {
    '--primary': accent,
    '--primary-foreground': foreground,
    '--ring': accent,
    '--sidebar-primary': accent,
    '--sidebar-primary-foreground': foreground,
    '--sidebar': sidebar,
    '--sidebar-accent': darken(sidebar, 0.08),
  }
}
```

修改 `src/lib/themeUtils.ts`，将现有 `mixColors()` 改为可复用导出：
```ts
export function mix(hex: string, other: string, weightOfOther: number): string {
  const [r1, g1, b1] = hexToRgb(hex)
  const [r2, g2, b2] = hexToRgb(other)
  const weightOfBase = 1 - weightOfOther

  return rgbToHex(
    r1 * weightOfBase + r2 * weightOfOther,
    g1 * weightOfBase + g2 * weightOfOther,
    b1 * weightOfBase + b2 * weightOfOther,
  )
}
```

- [ ] **Step 4: 重写 `globals.css` 的 `:root` 与 `@theme inline`，并保留旧 token 兼容别名层**

将 `src/styles/globals.css` 的 token 区改成（注意：默认 `accentColor = #DBAA22` 时，foreground 要和 `deriveSkin()` 保持一致，因此这里使用 `#1A1A1A` 而不是旧稿里的 `#FFFFFF`）。另外，由于仓库里还有大量旧变量消费方尚未在 Task 1 一并迁移，所以这一阶段必须保留一层旧 token alias，至少把现有高频旧变量继续映射到新 token，避免 UI 默认态回退：
- `--color-bg-main: var(--background)`
- `--color-bg-sidebar: var(--sidebar)`
- `--color-bg-card: var(--card)`
- `--color-text-primary: var(--foreground)`
- `--color-text-secondary: var(--muted-foreground)`
- `--color-border: var(--border)`
- `--color-border-light: var(--sidebar-border)`
- `--color-accent: var(--primary)`
- `--color-text-on-accent: var(--primary-foreground)`

这层 alias 是过渡措施，只为保证 Task 1 落地后现有界面不崩；真正删除旧变量要等后续页面/组件迁移完成。

此外，Task 1 不能只补 9 个示例变量，而是要以当前前端实际消费为准，先扫描 `src/` 中仍被使用的 `--color-*` 旧变量，把其中在默认态会影响界面结构/可读性的高频变量一并补齐到 alias 层；至少要覆盖本轮 code review 已确认的这些变量：
- `--color-text-muted`
- `--color-border-subtle`
- `--color-primary-subtle`
- `--color-bg-elevated`

若扫描后发现还有同类高频旧变量被 Layout / Chat / Settings / Markdown 等核心界面直接消费，也应一并补齐，而不是留到后续任务才修。Task 1 的验证也要增加一条兼容性检查：用 `rg` 搜出旧变量消费清单，确认 `globals.css` 中已存在对应 alias，至少不能再出现 code review 已点名的缺口。

```css
@import "tailwindcss";

:root {
  --primary: #DBAA22;
  --primary-foreground: #1A1A1A;
  --ring: var(--primary);
  --sidebar-primary: var(--primary);
  --sidebar-primary-foreground: #1A1A1A;

  --background: #FAFAFA;
  --foreground: #0a0a0a;
  --muted: #f5f5f5;
  --muted-foreground: #737373;
  --card: #fafafa;
  --card-foreground: #0a0a0a;
  --popover: #fafafa;
  --popover-foreground: #0a0a0a;
  --secondary: #f5f5f5;
  --secondary-foreground: #0a0a0a;
  --accent: #f5f5f5;
  --accent-foreground: #0a0a0a;
  --border: #e5e5e5;
  --input: #e5e5e5;
  --sidebar: #FBF6E6;
  --sidebar-accent: #E9DEB2;
  --sidebar-accent-foreground: #0a0a0a;
  --sidebar-foreground: #0a0a0a;
  --sidebar-border: #E1DAC6;
  --sidebar-ring: #71717a;
  --destructive: #e7000b;
  --destructive-foreground: #FFFFFF;
  --font-sans: -apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", Inter, "Segoe UI", system-ui, sans-serif;
  --font-mono: "SF Mono", "Fira Code", "JetBrains Mono", Menlo, monospace;
  --radius-sm: 4px;
  --radius: 6px;
  --radius-md: 8px;
  --radius-lg: 12px;
  --radius-xl: 16px;
}

@theme inline {
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-muted: var(--muted);
  --color-muted-foreground: var(--muted-foreground);
  --color-card: var(--card);
  --color-card-foreground: var(--card-foreground);
  --color-popover: var(--popover);
  --color-popover-foreground: var(--popover-foreground);
  --color-secondary: var(--secondary);
  --color-secondary-foreground: var(--secondary-foreground);
  --color-accent: var(--accent);
  --color-accent-foreground: var(--accent-foreground);
  --color-border: var(--border);
  --color-input: var(--input);
  --color-ring: var(--ring);
  --color-sidebar: var(--sidebar);
  --color-sidebar-accent: var(--sidebar-accent);
  --color-sidebar-accent-foreground: var(--sidebar-accent-foreground);
  --color-sidebar-foreground: var(--sidebar-foreground);
  --color-sidebar-border: var(--sidebar-border);
  --color-sidebar-primary: var(--sidebar-primary);
  --color-sidebar-primary-foreground: var(--sidebar-primary-foreground);
  --color-sidebar-ring: var(--sidebar-ring);
  --color-destructive: var(--destructive);
  --color-destructive-foreground: var(--destructive-foreground);
  --font-sans: var(--font-sans);
  --font-mono: var(--font-mono);
  --radius-sm: var(--radius-sm);
  --radius: var(--radius);
  --radius-md: var(--radius-md);
  --radius-lg: var(--radius-lg);
  --radius-xl: var(--radius-xl);
}
```

- [ ] **Step 5: 跑测试确认通过**

Run: `pnpm exec vitest run src/styles/skin.test.ts`
Expected: PASS，3 个用例全部通过

- [ ] **Step 6: Commit**

```bash
git add src/styles/skin.ts src/styles/skin.test.ts src/lib/themeUtils.ts src/styles/globals.css
git commit -m "feat(frontend): add shadcn skin derivation tokens"
```

---

## Task 2：重写 brandingStore，收敛到单 accentColor 输入

**Files:**
- Modify: `src/stores/brandingStore.ts`
- Modify: `src/stores/brandingStore.test.ts`
- Test: `src/stores/brandingStore.test.ts`

- [ ] **Step 1: 先写/改失败测试，锁定新行为**

将 `src/stores/brandingStore.test.ts` 核心断言改成：
```ts
import { beforeEach, describe, expect, it } from 'vitest'

import { DEFAULTS, useBrandingStore } from '@/stores/brandingStore'
import { DERIVED_SKIN_KEYS } from '@/styles/skin'


describe('brandingStore', () => {
  beforeEach(() => {
    useBrandingStore.getState().reset()
    document.documentElement.removeAttribute('style')
  })

  it('applyBranding 仅使用 accentColor 派生 token', () => {
    useBrandingStore.getState().applyBranding({
      productName: '租户 A',
      accentColor: '#960505',
      primaryColor: '#123456',
      bgColor: '#eeeeee',
      sidebarBgColor: '#cccccc',
    })

    expect(document.documentElement.style.getPropertyValue('--primary')).toBe('#960505')
    expect(document.documentElement.style.getPropertyValue('--sidebar')).not.toBe('')
    expect(useBrandingStore.getState().accentColor).toBe('#960505')
    expect(useBrandingStore.getState().isCustom).toBe(true)
  })

  it('reset 会移除全部派生变量并回退默认值', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#1A2E22' })
    useBrandingStore.getState().reset()

    for (const key of DERIVED_SKIN_KEYS) {
      expect(document.documentElement.style.getPropertyValue(key)).toBe('')
    }
    expect(useBrandingStore.getState().productName).toBe(DEFAULTS.productName)
    expect(useBrandingStore.getState().accentColor).toBe(DEFAULTS.accentColor)
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/stores/brandingStore.test.ts`
Expected: FAIL，现有 store 仍会写 `--color-*` 变量或保留 `primaryColor/bgColor`

- [ ] **Step 3: 按 spec 重写 `brandingStore.ts`**

将 `src/stores/brandingStore.ts` 里的状态与动作改为：
```ts
import i18n from '@/i18n'
import { create } from 'zustand'

import { DEFAULT_ACCENT_COLOR, DERIVED_SKIN_KEYS, deriveSkin } from '@/styles/skin'

export const DEFAULTS = {
  productName: 'AI小家',
  productNameEn: 'AIjia',
  logoUrl: '/app-icon.png',
  accentColor: DEFAULT_ACCENT_COLOR,
  fontFamily: '',
}

interface TenantBranding {
  productName?: string
  logoUrl?: string
  accentColor?: string
  fontFamily?: string
  primaryColor?: string
  bgColor?: string
  sidebarBgColor?: string
}

interface BrandingState {
  productName: string
  productNameEn: string
  logoUrl: string
  accentColor: string
  fontFamily: string
  isCustom: boolean
  applyBranding(tenant: TenantBranding | null): void
  reset(): void
}

function setWindowTitle(title: string) {
  document.title = `${title} — ${i18n.t('welcome.defaultSubtitle')}`
}

function setDerivedVariables(accentColor?: string) {
  const derived = deriveSkin(accentColor)
  for (const [key, value] of Object.entries(derived)) {
    document.documentElement.style.setProperty(key, value)
  }
}

function clearDerivedVariables() {
  for (const key of DERIVED_SKIN_KEYS) {
    document.documentElement.style.removeProperty(key)
  }
}

export const useBrandingStore = create<BrandingState>((set) => ({
  ...DEFAULTS,
  isCustom: false,
  applyBranding(tenant) {
    if (!tenant) {
      setWindowTitle(DEFAULTS.productName)
      set({ ...DEFAULTS, isCustom: false })
      clearDerivedVariables()
      return
    }

    const accentColor = tenant.accentColor?.trim() || DEFAULTS.accentColor
    const productName = tenant.productName?.trim() || DEFAULTS.productName
    const logoUrl = tenant.logoUrl?.trim() || DEFAULTS.logoUrl
    const fontFamily = tenant.fontFamily?.trim() || DEFAULTS.fontFamily

    setDerivedVariables(accentColor)
    if (fontFamily) {
      document.documentElement.style.setProperty('--font-sans', fontFamily)
    } else {
      document.documentElement.style.removeProperty('--font-sans')
    }

    setWindowTitle(productName)
    set({ productName, productNameEn: DEFAULTS.productNameEn, logoUrl, accentColor, fontFamily, isCustom: !!tenant.accentColor })
  },
  reset() {
    clearDerivedVariables()
    document.documentElement.style.removeProperty('--font-sans')
    setWindowTitle(DEFAULTS.productName)
    set({ ...DEFAULTS, isCustom: false })
  },
}))
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm exec vitest run src/stores/brandingStore.test.ts`
Expected: PASS，覆盖 apply/reset 与变量数量断言

- [ ] **Step 5: Commit**

```bash
git add src/stores/brandingStore.ts src/stores/brandingStore.test.ts
git commit -m "refactor(frontend): simplify branding store to accent-only skin"
```

---

## Task 3：引入 shadcn 基础设施与核心 UI 原语

**Files:**
- Modify: `package.json`
- Create: `src/lib/utils.ts`
- Create: `src/components/ui/*`
- Test: `src/components/layout/Sidebar.test.tsx`

- [ ] **Step 1: 先补一个最小 smoke 测试，锁定新 Button/Card 可渲染**

在 `src/components/layout/Sidebar.test.tsx` 顶部先加入最小 smoke case：
```tsx
it('renders sidebar navigation skeleton', () => {
  render(<Sidebar onOpenSettings={vi.fn()} />)

  expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: '技能中心' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: '定时任务' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: '设置' })).toBeInTheDocument()
})
```

- [ ] **Step 2: 安装 shadcn 依赖并生成首批组件**

Run:
```bash
pnpm add @radix-ui/react-alert-dialog @radix-ui/react-dialog @radix-ui/react-dropdown-menu @radix-ui/react-popover @radix-ui/react-scroll-area @radix-ui/react-separator @radix-ui/react-slot @radix-ui/react-tabs class-variance-authority clsx lucide-react sonner tailwind-merge
```

Run:
```bash
pnpm dlx shadcn@latest add button card dialog input textarea tabs dropdown-menu badge separator scroll-area alert alert-dialog tooltip sheet skeleton popover
```

Expected: `src/components/ui/` 下生成对应组件文件，`package.json` 新依赖可安装成功

- [ ] **Step 3: 落地 `cn()` 帮助函数**

创建 `src/lib/utils.ts`：
```ts
import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
```

- [ ] **Step 4: 对齐 Button/Card 默认视觉**

如果生成后的 `src/components/ui/button.tsx` 和 `src/components/ui/card.tsx` 与设计稿不一致，至少改成：
```tsx
const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[var(--radius)] text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
  {
    variants: {
      variant: {
        default: 'bg-primary text-primary-foreground hover:brightness-110 active:brightness-95',
        secondary: 'bg-secondary text-secondary-foreground hover:bg-muted active:bg-muted/80',
        ghost: 'text-foreground hover:bg-accent hover:text-accent-foreground active:bg-accent/80',
        destructive: 'bg-destructive text-destructive-foreground hover:brightness-110 active:brightness-95',
      },
    },
  },
)
```

- [ ] **Step 5: 运行基础测试**

Run: `pnpm exec vitest run src/components/layout/Sidebar.test.tsx`
Expected: 此时仍可能 FAIL，但失败原因应从“缺少组件依赖”收敛为“Sidebar 结构未改完”

- [ ] **Step 6: Commit**

```bash
git add package.json pnpm-lock.yaml src/lib/utils.ts src/components/ui
git commit -m "feat(frontend): add shadcn ui foundation"
```

---

## Task 4：建立 route atom / authStore / AuthGate 顶层骨架

**Files:**
- Create: `src/stores/uiStore.ts`
- Modify: `src/stores/authStore.ts`
- Create: `src/components/auth/AuthGate.tsx`
- Create: `src/components/auth/FullscreenLoader.tsx`
- Create: `src/components/auth/LoginPage.tsx`
- Create: `src/features/auth/AuthGate.integration.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/types/settings.ts`
- Modify: `src/stores/settingsStore.ts`
- Modify: `src/components/settings/LoginSection.tsx`
- Modify: `src/components/settings/SettingsModal.tsx`
- Test: `src/features/auth/AuthGate.integration.test.tsx`

> Task 4 边界修正：
> 1. `App.tsx` 在这一阶段需要切到 `AuthGate + route shell`，但 `AppSidebar` 与 feature pages 的正式实现属于后续 Task 5+；因此 Task 4 允许引入最小可编译占位层（例如 `AppSidebar` 先桥接旧侧栏、`HomePage/SkillCenterPage/...` 先返回骨架容器），后续任务再替换成正式实现。
> 2. `SettingsModal` 的无状态 store 化也属于后续页面迁移的一部分；Task 4 不要求把它改成无 props 组件，只要求去掉 `useCloud` 依赖并能由 `uiStore.settingsModal` 驱动现有 `open/onClose` props。
> 3. 删除 `useCloud` 后，`SettingsModal` 的保存/切换 provider 逻辑也必须同步删掉对应字段，否则 Task 4 无法通过 TypeScript 编译。

- [ ] **Step 1: 先写 AuthGate 集成失败测试**

创建 `src/features/auth/AuthGate.integration.test.tsx`：
```tsx
import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { AuthGate } from '@/components/auth/AuthGate'
import { useAuthStore } from '@/stores/authStore'
import { useUiStore } from '@/stores/uiStore'

vi.mock('@/lib/tauri', () => ({
  cloudLogin: vi.fn().mockResolvedValue({
    loggedIn: true,
    user: { id: 1, name: 'Test', username: 'test' },
    tenant: { id: 2, name: 'Tenant', balance: '0', accentColor: '#DBAA22' },
    models: [{ id: 'glm', name: 'GLM', modelType: 'cloud' }],
  }),
}))

describe('AuthGate', () => {
  beforeEach(() => {
    useAuthStore.setState({
      isLoggedIn: false,
      user: null,
      tenant: null,
      cloudModels: [],
      selectedCloudModel: null,
      redirectFrom: { kind: 'skill-center' },
      isAuthPending: false,
    })
    useUiStore.setState({ route: { kind: 'home' }, settingsModal: null })
  })

  it('未登录时渲染 LoginPage', () => {
    render(<AuthGate><div>APP SHELL</div></AuthGate>)
    expect(screen.getByRole('button', { name: '登录' })).toBeInTheDocument()
    expect(screen.queryByText('APP SHELL')).not.toBeInTheDocument()
  })

  it('登录成功后恢复 redirectFrom', async () => {
    render(<AuthGate><div>APP SHELL</div></AuthGate>)

    fireEvent.change(screen.getByLabelText('账号'), { target: { value: 'demo' } })
    fireEvent.change(screen.getByLabelText('密码'), { target: { value: '123456' } })
    fireEvent.click(screen.getByRole('button', { name: '登录' }))

    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-center' })
    })
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/features/auth/AuthGate.integration.test.tsx`
Expected: FAIL，缺少 `uiStore/AuthGate/LoginPage`

- [ ] **Step 3: 创建 `uiStore.ts` 定义 route 与 settings modal 状态**

创建 `src/stores/uiStore.ts`：
```ts
import { create } from 'zustand'

export type Route =
  | { kind: 'home' }
  | { kind: 'skill-center' }
  | { kind: 'skill-detail'; skillId: string }
  | { kind: 'schedules' }
  | { kind: 'chat'; conversationId: string }

export type SettingsModalState = null | 'account' | 'general' | 'about' | 'usage'

interface UiState {
  route: Route
  settingsModal: SettingsModalState
  setRoute(route: Route): void
  openSettings(tab: Exclude<SettingsModalState, null>): void
  closeSettings(): void
}

export const useUiStore = create<UiState>((set) => ({
  route: { kind: 'home' },
  settingsModal: null,
  setRoute: (route) => set({ route }),
  openSettings: (settingsModal) => set({ settingsModal }),
  closeSettings: () => set({ settingsModal: null }),
}))
```

- [ ] **Step 4: 扩展 `authStore.ts` 并删除 `useCloud` 状态耦合**

将 `src/stores/authStore.ts` 改成：
```ts
import { create } from 'zustand'

import { cloudLogin, getCloudAuth, getCloudModels } from '@/lib/tauri'
import type { CloudAuthInfo, CloudModel } from '@/lib/tauri'
import type { Route } from '@/stores/uiStore'

interface AuthState {
  isLoggedIn: boolean
  user: { id: number; name: string; username: string } | null
  tenant: CloudAuthInfo['tenant']
  cloudModels: CloudModel[]
  selectedCloudModel: string | null
  redirectFrom: Route | null
  isAuthPending: boolean
  setAuth(info: CloudAuthInfo): void
  setCloudModels(models: CloudModel[]): void
  setSelectedCloudModel(model: string | null): void
  setRedirectFrom(route: Route | null): void
  restoreFromStorage(): Promise<void>
  login(username: string, password: string): Promise<void>
  logout(): Promise<void>
  clearAndRedirect(route?: Route): void
}
```

其中 `restoreFromStorage()` 至少调用：
```ts
const info = await getCloudAuth()
if (!info.loggedIn) {
  set({ isLoggedIn: false, user: null, tenant: null, cloudModels: [], selectedCloudModel: null, isAuthPending: false })
  return
}
const models = await getCloudModels()
set({ ...mappedState, isAuthPending: false, cloudModels: models, selectedCloudModel: models[0]?.id ?? null })
```

- [ ] **Step 5: 实现 `LoginPage` / `AuthGate` / `FullscreenLoader`**

创建 `src/components/auth/LoginPage.tsx`：
```tsx
import { FormEvent, useState } from 'react'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuthStore } from '@/stores/authStore'

export function LoginPage() {
  const login = useAuthStore((state) => state.login)
  const isAuthPending = useAuthStore((state) => state.isAuthPending)
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    try {
      setError('')
      await login(username, password)
    } catch (err) {
      setPassword('')
      setError(err instanceof Error ? err.message : '登录失败，请重试')
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-6">
      <Card className="w-full max-w-[400px] border-border bg-card shadow-sm">
        <CardHeader>
          <CardTitle className="text-center text-xl">登录</CardTitle>
        </CardHeader>
        <CardContent>
          <form className="space-y-4" onSubmit={handleSubmit}>
            <div className="space-y-2">
              <Label htmlFor="username">账号</Label>
              <Input id="username" value={username} onChange={(e) => setUsername(e.target.value)} />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">密码</Label>
              <Input id="password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
            </div>
            {error ? <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert> : null}
            <Button className="w-full" disabled={isAuthPending} type="submit">登录</Button>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}
```

创建 `src/components/auth/AuthGate.tsx`：
```tsx
import { PropsWithChildren, useEffect } from 'react'

import { useAuthStore } from '@/stores/authStore'
import { useUiStore } from '@/stores/uiStore'
import { FullscreenLoader } from './FullscreenLoader'
import { LoginPage } from './LoginPage'

export function AuthGate({ children }: PropsWithChildren) {
  const isLoggedIn = useAuthStore((state) => state.isLoggedIn)
  const isAuthPending = useAuthStore((state) => state.isAuthPending)
  const redirectFrom = useAuthStore((state) => state.redirectFrom)
  const restoreFromStorage = useAuthStore((state) => state.restoreFromStorage)
  const setRoute = useUiStore((state) => state.setRoute)

  useEffect(() => {
    void restoreFromStorage()
  }, [restoreFromStorage])

  useEffect(() => {
    if (isLoggedIn && redirectFrom) {
      setRoute(redirectFrom)
      useAuthStore.getState().setRedirectFrom(null)
    }
  }, [isLoggedIn, redirectFrom, setRoute])

  if (isAuthPending) {
    return <FullscreenLoader />
  }

  if (!isLoggedIn) {
    return <LoginPage />
  }

  return <>{children}</>
}
```

- [ ] **Step 6: 重建 `App.tsx` 顶层骨架**

将 `src/App.tsx` 先收敛为 route shell；若 `AppSidebar` / feature pages 正式实现尚未开始，可先使用最小占位组件或桥接包装器，但最终结构必须是：
```tsx
import { AuthGate } from '@/components/auth/AuthGate'
import { AppSidebar } from '@/components/sidebar/AppSidebar'
import { SettingsModal } from '@/components/settings/SettingsModal'
import { ChatPage } from '@/features/chat/ChatPage'
import { HomePage } from '@/features/home/HomePage'
import { SchedulesPage } from '@/features/schedules/SchedulesPage'
import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import { SkillDetailPage } from '@/features/skill-detail/SkillDetailPage'
import { useUiStore } from '@/stores/uiStore'

function RouteSwitch() {
  const route = useUiStore((state) => state.route)

  switch (route.kind) {
    case 'home':
      return <HomePage />
    case 'skill-center':
      return <SkillCenterPage />
    case 'skill-detail':
      return <SkillDetailPage skillId={route.skillId} />
    case 'schedules':
      return <SchedulesPage />
    case 'chat':
      return <ChatPage conversationId={route.conversationId} />
  }
}

export default function App() {
  return (
    <AuthGate>
      <div className="flex h-screen w-screen bg-background text-foreground">
        <AppSidebar />
        <main className="min-w-0 flex-1 overflow-hidden">
          <RouteSwitch />
        </main>
        <SettingsModal open={settingsModal !== null} onClose={closeSettings} />
      </div>
    </AuthGate>
  )
}
```

- [ ] **Step 7: 更新 settings 类型，删除 `useCloud`**

在 `src/types/settings.ts` 删除：
```ts
useCloud: boolean
```

并把默认值从：
```ts
useCloud: false,
```
删掉；`src/stores/settingsStore.ts` 也删除所有 `useCloud` setter/引用。

- [ ] **Step 7.5: 最小改造 `SettingsModal` / `LoginSection` 去掉 `useCloud` 耦合**

要求：
- `src/components/settings/LoginSection.tsx` 改为只依赖 `authStore` 的 `login/logout/isAuthPending` 与账号展示，不再读写 `settings.useCloud`。
- `src/components/settings/SettingsModal.tsx` 中所有 `updateSettings(... useCloud ...)`、`settings.useCloud`、未登录时隐藏/显示 models/search 的逻辑都要改成不依赖 `useCloud`；Task 4 允许先保留旧 tab 结构，但不能再引用已删除字段。
- `App.tsx` 通过 `uiStore.settingsModal` 驱动现有 `SettingsModal open/onClose`，避免在 Task 4 提前做完整设置页重构。

- [ ] **Step 8: 运行 AuthGate 测试**

Run: `pnpm exec vitest run src/features/auth/AuthGate.integration.test.tsx`
Expected: PASS，验证未登录显示登录页、登录成功恢复 route

- [ ] **Step 9: Commit**

```bash
git add src/stores/uiStore.ts src/stores/authStore.ts src/components/auth src/features/auth/AuthGate.integration.test.tsx src/App.tsx src/types/settings.ts src/stores/settingsStore.ts src/components/settings/LoginSection.tsx
git commit -m "feat(frontend): add auth gate and route shell"
```

---

## Task 5：实现 Skill-First 侧栏并替换旧 Sidebar

**Files:**
- Create: `src/components/sidebar/AppSidebar.tsx`
- Create: `src/components/sidebar/SidebarNav.tsx`
- Create: `src/components/sidebar/ConversationTree.tsx`
- Create: `src/components/sidebar/TenantHeader.tsx`
- Modify: `src/components/layout/Sidebar.tsx`
- Modify: `src/components/layout/Sidebar.test.tsx`
- Modify: `src/hooks/useChat.ts`
- Test: `src/components/layout/Sidebar.test.tsx`

- [ ] **Step 1: 先改失败测试到新结构**

将 `src/components/layout/Sidebar.test.tsx` 的主断言改成：
```tsx
it('renders skill-first navigation and conversation list', () => {
  render(<Sidebar onOpenSettings={vi.fn()} />)

  expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: '技能中心' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: '定时任务' })).toBeInTheDocument()
  expect(screen.getByText('任务')).toBeInTheDocument()
  expect(screen.getByText('Python 分析')).toBeInTheDocument()
})
```

并删除 persona 相关 mock，新增 `uiStore` mock：
```ts
vi.mock('@/stores/uiStore', () => ({
  useUiStore: (selector: (state: { route: { kind: string }; settingsModal: null; setRoute: (route: unknown) => void; openSettings: () => void }) => unknown) =>
    selector({
      route: { kind: 'home' },
      settingsModal: null,
      setRoute: vi.fn(),
      openSettings: vi.fn(),
    }),
}))
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/components/layout/Sidebar.test.tsx`
Expected: FAIL，旧侧栏仍显示 persona 切换器

- [ ] **Step 3: 落地主侧栏组件**

创建 `src/components/sidebar/AppSidebar.tsx`：
```tsx
import { Settings, Sparkles, Puzzle, Timer } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useChat } from '@/hooks/useChat'
import { useUiStore } from '@/stores/uiStore'

import { ConversationTree } from './ConversationTree'
import { TenantHeader } from './TenantHeader'

const NAV_ITEMS = [
  { kind: 'home' as const, label: '新任务', icon: Sparkles },
  { kind: 'skill-center' as const, label: '技能中心', icon: Puzzle },
  { kind: 'schedules' as const, label: '定时任务', icon: Timer },
]

export function AppSidebar() {
  const route = useUiStore((state) => state.route)
  const setRoute = useUiStore((state) => state.setRoute)
  const openSettings = useUiStore((state) => state.openSettings)
  const { createNewConversation } = useChat()

  return (
    <aside className="flex h-full w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground">
      <TenantHeader />
      <div className="space-y-1 px-3 py-3">
        {NAV_ITEMS.map(({ kind, label, icon: Icon }) => (
          <Button
            key={kind}
            className="w-full justify-start"
            variant={route.kind === kind ? 'secondary' : 'ghost'}
            onClick={() => setRoute({ kind })}
          >
            <Icon className="size-4" />
            {label}
          </Button>
        ))}
      </div>
      <div className="px-4 pb-2 pt-3 text-xs font-medium text-muted-foreground">任务</div>
      <div className="px-3 pb-2">
        <Button className="w-full justify-start" variant="outline" onClick={() => void createNewConversation()}>
          + 新对话
        </Button>
      </div>
      <ScrollArea className="min-h-0 flex-1 px-3">
        <ConversationTree />
      </ScrollArea>
      <div className="p-3">
        <Button className="w-full justify-start" variant="ghost" onClick={() => openSettings('account')}>
          <Settings className="size-4" />
          设置
        </Button>
      </div>
    </aside>
  )
}
```

- [ ] **Step 4: 让旧 `Sidebar.tsx` 变成兼容导出**

将 `src/components/layout/Sidebar.tsx` 收敛成：
```tsx
import { AppSidebar } from '@/components/sidebar/AppSidebar'

interface SidebarProps {
  onOpenSettings?: () => void
}

export function Sidebar(_: SidebarProps) {
  return <AppSidebar />
}
```

- [ ] **Step 5: 在 `useChat.ts` 里让创建会话自动切 route**

在 `createNewConversation()` 成功后追加：
```ts
import { useUiStore } from '@/stores/uiStore'

useUiStore.getState().setRoute({ kind: 'chat', conversationId: backendId })
```

同时在 `switchConversation(id)` 成功后：
```ts
useUiStore.getState().setRoute({ kind: 'chat', conversationId: id })
```

- [ ] **Step 6: 跑侧栏测试确认通过**

Run: `pnpm exec vitest run src/components/layout/Sidebar.test.tsx`
Expected: PASS，persona UI 不再出现，四个入口存在

- [ ] **Step 7: Commit**

```bash
git add src/components/sidebar src/components/layout/Sidebar.tsx src/components/layout/Sidebar.test.tsx src/hooks/useChat.ts
git commit -m "feat(frontend): replace sidebar with skill-first navigation"
```

---

## Task 6：实现技能分类数据与 skillStore

**Files:**
- Create: `src/data/skill-categories.ts`
- Create: `src/stores/skillStore.ts`
- Create: `src/stores/skillStore.test.ts`
- Modify: `src/lib/tauri.ts`
- Test: `src/stores/skillStore.test.ts`

- [ ] **Step 1: 先写失败测试**

创建 `src/stores/skillStore.test.ts`：
```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useSkillStore } from '@/stores/skillStore'

vi.mock('@/lib/tauri', () => ({
  listSkills: vi.fn().mockResolvedValue([
    { id: 'write-plan', displayName: '写计划', description: 'desc', source: 'builtin', hasWorkflow: true, icon: 'file-text', category: 'dev', triggerText: '', shortDescription: 'short', displayNameEn: 'Plan', shortDescriptionEn: 'short' },
    { id: 'shop-report', displayName: '店铺日报', description: 'desc', source: 'user', hasWorkflow: false, icon: 'store', category: 'ops', triggerText: '', shortDescription: 'short', displayNameEn: 'Ops', shortDescriptionEn: 'short' },
  ]),
}))

describe('skillStore', () => {
  beforeEach(() => {
    useSkillStore.setState({ skills: [], recommendedIds: ['write-plan'], isLoading: false })
  })

  it('reload 后可按分类过滤', async () => {
    await useSkillStore.getState().reload()

    expect(useSkillStore.getState().listByCategory('dev')).toHaveLength(1)
    expect(useSkillStore.getState().listByCategory('recommended')).toHaveLength(1)
    expect(useSkillStore.getState().getById('shop-report')?.displayName).toBe('店铺日报')
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/stores/skillStore.test.ts`
Expected: FAIL，缺少 `skillStore`

- [ ] **Step 3: 创建分类常量与 store**

创建 `src/data/skill-categories.ts`：
```ts
export type SkillCategoryId =
  | 'recommended'
  | 'general'
  | 'ecommerce'
  | 'finance'
  | 'design'
  | 'dev'
  | 'legal'
  | 'media'
  | 'health'
  | 'ops'
  | 'content'

export interface SkillCategory {
  id: Exclude<SkillCategoryId, 'recommended'>
  name: string
  icon: string
}

export const SKILL_CATEGORIES: SkillCategory[] = [
  { id: 'general', name: '通用工具', icon: 'wrench' },
  { id: 'ecommerce', name: '电商', icon: 'shopping-cart' },
  { id: 'finance', name: '门店与财务', icon: 'store' },
  { id: 'design', name: '设计与制造', icon: 'pencil-ruler' },
  { id: 'dev', name: '开发', icon: 'code' },
  { id: 'legal', name: '律所', icon: 'scale' },
  { id: 'media', name: '媒介', icon: 'megaphone' },
  { id: 'health', name: '健康与学习', icon: 'heart-pulse' },
  { id: 'ops', name: '运营', icon: 'trending-up' },
  { id: 'content', name: '内容创作', icon: 'feather' },
]
```

创建 `src/stores/skillStore.ts`：
```ts
import { create } from 'zustand'

import { listSkills, type SkillInfo } from '@/lib/tauri'
import type { SkillCategoryId } from '@/data/skill-categories'

const RECOMMENDED_SKILL_IDS = ['writing-plans', 'skill-smith', 'table-analysis', 'ppt-builder', 'research-brief']

interface SkillState {
  skills: SkillInfo[]
  recommendedIds: string[]
  isLoading: boolean
  listByCategory(id: SkillCategoryId): SkillInfo[]
  getById(id: string): SkillInfo | null
  reload(): Promise<void>
  install(id: string): Promise<void>
  uninstall(id: string): Promise<void>
  upload(file: File): Promise<void>
}

export const useSkillStore = create<SkillState>((set, get) => ({
  skills: [],
  recommendedIds: RECOMMENDED_SKILL_IDS,
  isLoading: false,
  listByCategory(id) {
    const { skills, recommendedIds } = get()
    if (id === 'recommended') {
      return skills.filter((skill) => recommendedIds.includes(skill.id))
    }
    return skills.filter((skill) => (skill.category || 'general') === id)
  },
  getById(id) {
    return get().skills.find((skill) => skill.id === id) ?? null
  },
  async reload() {
    set({ isLoading: true })
    try {
      const skills = await listSkills()
      set({ skills, isLoading: false })
    } catch (error) {
      set({ isLoading: false })
      throw error
    }
  },
  async install() {
    throw new Error('技能市场即将开放')
  },
  async uninstall() {
    throw new Error('卸载功能即将开放')
  },
  async upload() {
    throw new Error('上传功能即将开放')
  },
}))
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm exec vitest run src/stores/skillStore.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/data/skill-categories.ts src/stores/skillStore.ts src/stores/skillStore.test.ts
git commit -m "feat(frontend): add skill categories and skill store"
```

---

## Task 7：实现首页、技能中心、技能详情、定时任务页面骨架

**Files:**
- Create: `src/features/home/HomePage.tsx`
- Create: `src/components/skill-center/SkillCard.tsx`
- Create: `src/features/skill-center/SkillCenterPage.tsx`
- Create: `src/features/skill-center/SkillMarketModal.tsx`
- Create: `src/features/skill-center/SkillUploadModal.tsx`
- Create: `src/features/skill-detail/SkillDetailPage.tsx`
- Create: `src/features/schedules/SchedulesPage.tsx`
- Create: `src/features/skill-center/SkillCenterPage.integration.test.tsx`
- Modify: `src/hooks/useChat.ts`
- Test: `src/features/skill-center/SkillCenterPage.integration.test.tsx`

- [ ] **Step 1: 先写技能中心失败测试**

创建 `src/features/skill-center/SkillCenterPage.integration.test.tsx`：
```tsx
import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const createNewConversationFromSkill = vi.fn()

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    createConversationFromSkill: createNewConversationFromSkill,
  }),
}))

describe('SkillCenterPage', () => {
  beforeEach(() => {
    useSkillStore.setState({
      skills: [
        { id: 'writing-plans', displayName: '写计划', description: 'desc', source: 'builtin', hasWorkflow: true, icon: 'file-text', category: 'dev', triggerText: '', shortDescription: '短描述', displayNameEn: 'Plan', shortDescriptionEn: 'short' },
      ],
      recommendedIds: ['writing-plans'],
      isLoading: false,
    })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null })
  })

  it('切换分类并点击卡片进入详情', async () => {
    render(<SkillCenterPage />)

    fireEvent.click(screen.getByRole('tab', { name: '开发' }))
    fireEvent.click(screen.getByText('写计划'))

    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'writing-plans' })
    })
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx`
Expected: FAIL，页面组件尚不存在

- [ ] **Step 3: 实现页面骨架**

创建 `src/features/home/HomePage.tsx`：
```tsx
import { Sparkles } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Textarea } from '@/components/ui/textarea'

const QUICK_PROMPTS = ['写一份周报', '分析销售数据', '帮我拆解执行计划']

export function HomePage() {
  return (
    <div className="flex h-full flex-col gap-6 overflow-auto px-8 py-8">
      <Card className="border-border bg-card">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-2xl"><Sparkles className="size-5" />新任务</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <Textarea className="min-h-40 resize-none" placeholder="描述你现在要完成的任务..." />
          <div className="flex flex-wrap gap-2">
            {QUICK_PROMPTS.map((prompt) => (
              <Button key={prompt} variant="secondary">{prompt}</Button>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
```

创建 `src/components/skill-center/SkillCard.tsx`：
```tsx
import { ArrowRight, BadgeCheck } from 'lucide-react'

import type { SkillInfo } from '@/lib/tauri'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'

export function SkillCard({ skill, onOpen, onUse }: { skill: SkillInfo; onOpen(): void; onUse(): void }) {
  return (
    <Card className="flex h-full flex-col border-border bg-card transition-colors hover:border-primary/40">
      <CardHeader className="space-y-3">
        <div className="flex items-start justify-between gap-3">
          <CardTitle className="text-base">{skill.displayName}</CardTitle>
          <Badge variant={skill.source === 'builtin' ? 'secondary' : 'outline'}>
            {skill.source === 'builtin' ? '内置' : '已安装'}
          </Badge>
        </div>
        <p className="text-sm text-muted-foreground">{skill.shortDescription || skill.description}</p>
      </CardHeader>
      <CardContent className="flex-1">
        {skill.hasWorkflow ? <div className="flex items-center gap-2 text-sm text-primary"><BadgeCheck className="size-4" />支持工作流</div> : null}
      </CardContent>
      <CardFooter className="flex gap-2">
        <Button className="flex-1" variant="secondary" onClick={onOpen}>查看详情</Button>
        <Button className="flex-1" onClick={onUse}>开始使用<ArrowRight className="size-4" /></Button>
      </CardFooter>
    </Card>
  )
}
```

创建 `src/features/skill-center/SkillCenterPage.tsx`：
```tsx
import { useMemo, useState } from 'react'

import { Plus, Store } from 'lucide-react'

import { SkillCard } from '@/components/skill-center/SkillCard'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { SKILL_CATEGORIES, type SkillCategoryId } from '@/data/skill-categories'
import { useChat } from '@/hooks/useChat'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'
import { SkillMarketModal } from './SkillMarketModal'
import { SkillUploadModal } from './SkillUploadModal'

export function SkillCenterPage() {
  const [category, setCategory] = useState<SkillCategoryId>('recommended')
  const [marketOpen, setMarketOpen] = useState(false)
  const [uploadOpen, setUploadOpen] = useState(false)
  const listByCategory = useSkillStore((state) => state.listByCategory)
  const setRoute = useUiStore((state) => state.setRoute)
  const { createConversationFromSkill } = useChat()

  const categories = useMemo(() => [{ id: 'recommended' as const, name: '为你推荐' }, ...SKILL_CATEGORIES], [])
  const skills = listByCategory(category)

  return (
    <div className="flex h-full flex-col gap-6 overflow-auto px-8 py-8">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">技能中心</h1>
          <p className="text-sm text-muted-foreground">浏览、试用和管理你的技能。</p>
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => setMarketOpen(true)}><Store className="size-4" />技能市场</Button>
          <Button onClick={() => setUploadOpen(true)}><Plus className="size-4" />上传技能</Button>
        </div>
      </div>
      <Tabs value={category} onValueChange={(value) => setCategory(value as SkillCategoryId)}>
        <TabsList className="flex w-full justify-start overflow-auto">
          {categories.map((item) => (
            <TabsTrigger key={item.id} value={item.id}>{item.name}</TabsTrigger>
          ))}
        </TabsList>
      </Tabs>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {skills.map((skill) => (
          <SkillCard
            key={skill.id}
            skill={skill}
            onOpen={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
            onUse={() => void createConversationFromSkill(skill.id)}
          />
        ))}
      </div>
      <SkillMarketModal open={marketOpen} onOpenChange={setMarketOpen} />
      <SkillUploadModal open={uploadOpen} onOpenChange={setUploadOpen} />
    </div>
  )
}
```

- [ ] **Step 4: 在 `useChat.ts` 加一个按 skill 创建会话 helper**

追加：
```ts
const createConversationFromSkill = useCallback(async (skillId: string) => {
  const conversationId = await createNewConversation()
  useUiStore.getState().setRoute({ kind: 'chat', conversationId })
  return conversationId
}, [createNewConversation])
```

返回对象里导出 `createConversationFromSkill`。

- [ ] **Step 5: 实现 `SkillDetailPage`、`SkillMarketModal`、`SkillUploadModal`、`SchedulesPage`**

`src/features/skill-detail/SkillDetailPage.tsx` 至少包含：
```tsx
export function SkillDetailPage({ skillId }: { skillId: string }) {
  const skill = useSkillStore((state) => state.getById(skillId))
  const { createConversationFromSkill } = useChat()

  if (!skill) {
    return <div className="p-8 text-sm text-muted-foreground">技能不存在或尚未加载。</div>
  }

  return (
    <div className="flex h-full flex-col gap-6 overflow-auto px-8 py-8">
      <div className="space-y-2">
        <h1 className="text-3xl font-semibold">{skill.displayName}</h1>
        <p className="max-w-3xl text-sm text-muted-foreground">{skill.description}</p>
      </div>
      <div className="rounded-lg border border-border bg-card p-6">
        <h2 className="text-base font-medium">工作流预览</h2>
        <ol className="mt-4 list-decimal space-y-2 pl-5 text-sm text-muted-foreground">
          <li>识别任务目标与上下文</li>
          <li>生成对应执行步骤</li>
          <li>回到会话中继续完成任务</li>
        </ol>
      </div>
      <div className="flex gap-3">
        <Button onClick={() => void createConversationFromSkill(skill.id)}>开始使用</Button>
        <Button variant="secondary">上传新版本</Button>
      </div>
    </div>
  )
}
```

`src/features/schedules/SchedulesPage.tsx` 至少包含推荐模板卡片和静态表格标题；市场/上传弹层统一显示“即将开放”。

- [ ] **Step 6: 跑技能中心测试**

Run: `pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/features/home src/components/skill-center src/features/skill-center src/features/skill-detail src/features/schedules src/hooks/useChat.ts
git commit -m "feat(frontend): add skill center pages and home shell"
```

---

## Task 8：迁移聊天页，删除 AgentSelector 与 persona 过滤

**Files:**
- Create: `src/components/chat/SkillPopover.tsx`
- Create: `src/features/chat/ChatPage.tsx`
- Modify: `src/components/layout/InputBar.tsx`
- Modify: `src/components/chat/WelcomeScreen.tsx`
- Modify: `src/components/settings/SettingsModal.tsx`
- Delete: `src/components/chat/AgentSelector.tsx`
- Delete: `src/components/settings/PersonaTab.tsx`
- Delete: `src/stores/personaStore.ts`
- Delete: `src/components/onboarding/PersonaSelector.tsx`
- Modify: `src/App.tsx`
- Test: `src/components/layout/Sidebar.test.tsx`

> Task 8 边界修正：
> 删除 `personaStore` 不能只删聊天入口；当前设置页里的 `PersonaTab` 仍直接依赖该 store。如果 Task 8 要完成 persona 前端概念清理，必须同步从 `SettingsModal` 去掉 `persona` tab，并删除 `src/components/settings/PersonaTab.tsx`，否则会导致 TypeScript 编译失败。

- [ ] **Step 1: 先改 WelcomeScreen 文案断言**

如果已有聊天欢迎页测试，替换 persona 文案为：
```tsx
expect(screen.getByText(/你好！我是/)).toBeInTheDocument()
expect(screen.queryByText(/persona/i)).not.toBeInTheDocument()
```

若无现成测试，在 `src/components/layout/Sidebar.test.tsx` 增一个静态断言：
```tsx
expect(screen.queryByText('选择角色')).not.toBeInTheDocument()
```

- [ ] **Step 2: 运行现有相关测试确认失败**

Run: `pnpm exec vitest run src/components/layout/Sidebar.test.tsx`
Expected: 若旧组件还在引用 AgentSelector/persona，则 FAIL

- [ ] **Step 3: 在 InputBar 接入 SkillPopover，删 AgentSelector**

将 `src/components/layout/InputBar.tsx` 里：
```tsx
import { AgentSelector } from '@/components/chat/AgentSelector'
```
替换为：
```tsx
import { SkillPopover } from '@/components/chat/SkillPopover'
```

并把原来：
```tsx
<AgentSelector value={selectedAgent} onChange={setSelectedAgent} />
```
替换为：
```tsx
<SkillPopover />
```

`src/components/chat/SkillPopover.tsx` 最小实现：
```tsx
import { useState } from 'react'
import { ChevronRight, Puzzle } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

export function SkillPopover() {
  const [open, setOpen] = useState(false)
  const skills = useSkillStore((state) => state.skills)
  const setRoute = useUiStore((state) => state.setRoute)

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button size="sm" variant="ghost"><Puzzle className="size-4" />技能</Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 space-y-2">
        {skills.slice(0, 6).map((skill) => (
          <button
            key={skill.id}
            className="flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-accent"
            onClick={() => {
              setRoute({ kind: 'skill-detail', skillId: skill.id })
              setOpen(false)
            }}
            type="button"
          >
            <span>{skill.displayName}</span>
            <ChevronRight className="size-4 text-muted-foreground" />
          </button>
        ))}
        <Button className="w-full" variant="secondary" onClick={() => setRoute({ kind: 'skill-center' })}>去技能中心</Button>
      </PopoverContent>
    </Popover>
  )
}
```

- [ ] **Step 4: 删除 persona 相关前端依赖**

在 `src/components/chat/WelcomeScreen.tsx` 删除：
```ts
import { usePersonaStore } from '@/stores/personaStore'
```

并把 greeting 改成只依赖 productName：
```tsx
const greeting = t('welcome.defaultGreeting', { productName })
```

同时删除文件：
```bash
rm src/components/chat/AgentSelector.tsx src/components/settings/PersonaTab.tsx src/stores/personaStore.ts src/components/onboarding/PersonaSelector.tsx
```

并同步从 `src/components/settings/SettingsModal.tsx` 删除 `PersonaTab` import、`persona` tab 按钮和对应面板内容。

- [ ] **Step 5: 创建对话页容器**

创建 `src/features/chat/ChatPage.tsx`：
```tsx
import { BrowserPanel } from '@/components/browser/BrowserPanel'
import { ChatArea } from '@/components/layout/ChatArea'
import { InputBar } from '@/components/layout/InputBar'
import { TitleBar } from '@/components/layout/TitleBar'
import { TopBar } from '@/components/layout/TopBar'

export function ChatPage({ conversationId }: { conversationId: string }) {
  return (
    <div className="flex h-full min-h-0">
      <div className="flex min-w-0 flex-1 flex-col bg-background">
        <TitleBar />
        <TopBar />
        <ChatArea conversationId={conversationId} />
        <InputBar />
      </div>
      <BrowserPanel />
    </div>
  )
}
```

- [ ] **Step 6: 跑受影响测试**

Run: `pnpm exec vitest run src/components/layout/Sidebar.test.tsx src/stores/chatStore.test.ts src/hooks/useStreaming.integration.test.tsx`
Expected: PASS 或只剩与新 route 结构相关的明确失败

- [ ] **Step 7: Commit**

```bash
git add src/components/chat/SkillPopover.tsx src/features/chat/ChatPage.tsx src/components/layout/InputBar.tsx src/components/chat/WelcomeScreen.tsx src/components/settings/SettingsModal.tsx src/App.tsx
git rm src/components/chat/AgentSelector.tsx src/components/settings/PersonaTab.tsx src/stores/personaStore.ts src/components/onboarding/PersonaSelector.tsx
git commit -m "refactor(frontend): remove persona and agent selector from chat"
```

---

## Task 9：重做 SettingsModal 与退出登录流，删除未登录分支和 Persona Tab

**Files:**
- Modify: `src/components/settings/SettingsModal.tsx`
- Modify: `src/components/settings/LoginSection.tsx`
- Delete: `src/components/settings/PersonaTab.tsx`
- Modify: `src/stores/authStore.ts`
- Modify: `src/App.tsx`
- Test: `src/features/auth/AuthGate.integration.test.tsx`

- [ ] **Step 1: 先给 AuthGate 测试补一个退出登录场景**

在 `src/features/auth/AuthGate.integration.test.tsx` 追加：
```tsx
it('主动退出登录后回到登录页且不保留 redirectFrom', async () => {
  useAuthStore.setState({
    isLoggedIn: true,
    user: { id: 1, name: 'Test', username: 'test' },
    tenant: { id: 2, name: 'Tenant', balance: '0' },
    cloudModels: [],
    selectedCloudModel: null,
    redirectFrom: { kind: 'chat', conversationId: 'c1' },
    isAuthPending: false,
  })

  await useAuthStore.getState().logout()

  expect(useAuthStore.getState().isLoggedIn).toBe(false)
  expect(useAuthStore.getState().redirectFrom).toBeNull()
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/features/auth/AuthGate.integration.test.tsx`
Expected: FAIL，当前 `logout` 还未实现或不清空 redirectFrom

- [ ] **Step 3: 重构 SettingsModal tab 结构**

将 `src/components/settings/SettingsModal.tsx` 中：
```ts
type MainTab = 'account' | 'models' | 'search' | 'general' | 'persona' | 'skills' | 'mcp'
```
替换为：
```ts
type MainTab = 'account' | 'general' | 'about' | 'usage'
```

渲染入口改为读取 `useUiStore((s) => s.settingsModal)`，去掉组件 `open/onClose` props，主体结构至少为：
```tsx
const currentTab = useUiStore((state) => state.settingsModal)
const closeSettings = useUiStore((state) => state.closeSettings)

if (!currentTab) return null
```

- [ ] **Step 4: 在账号页加入退出登录确认**

在 `SettingsModal.tsx` 引入：
```tsx
<AlertDialog>
  <AlertDialogTrigger asChild>
    <Button variant="destructive">退出登录</Button>
  </AlertDialogTrigger>
  <AlertDialogContent>
    <AlertDialogHeader>
      <AlertDialogTitle>确认退出登录？</AlertDialogTitle>
      <AlertDialogDescription>退出后将返回登录页，本次主动退出不会保留当前现场。</AlertDialogDescription>
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogCancel>取消</AlertDialogCancel>
      <AlertDialogAction onClick={() => void logout()}>退出登录</AlertDialogAction>
    </AlertDialogFooter>
  </AlertDialogContent>
</AlertDialog>
```

并在 `authStore.logout()` 里实现：
```ts
async logout() {
  set({ isAuthPending: true })
  try {
    set({ isLoggedIn: false, user: null, tenant: null, cloudModels: [], selectedCloudModel: null, redirectFrom: null, isAuthPending: false })
  } catch (error) {
    set({ isAuthPending: false })
    throw error
  }
}
```

- [ ] **Step 5: 删除 PersonaTab**

执行：
```bash
git rm src/components/settings/PersonaTab.tsx
```

并清理 `SettingsModal.tsx` 中所有 persona tab/button/import。

- [ ] **Step 6: 跑测试确认通过**

Run: `pnpm exec vitest run src/features/auth/AuthGate.integration.test.tsx`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/components/settings/SettingsModal.tsx src/components/settings/LoginSection.tsx src/stores/authStore.ts src/App.tsx
git rm src/components/settings/PersonaTab.tsx
git commit -m "refactor(frontend): simplify settings modal and logout flow"
```

---

## Task 10：清理 i18n、废弃 IPC 注释、跑 DoD 验收

**Files:**
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`
- Modify: `src/lib/tauri.ts`
- Modify: `src/stores/settingsStore.test.ts`
- Test: 全量命令

- [ ] **Step 1: 先更新 settingsStore 测试，删除 `useCloud` 断言**

在 `src/stores/settingsStore.test.ts` 把旧断言：
```ts
expect(state.useCloud).toBe(false)
```
删除，新增：
```ts
expect(state.cloudModel).toBe('')
expect(state.cloudModelType).toBe('')
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/stores/settingsStore.test.ts`
Expected: FAIL，若 store/types 仍残留 `useCloud`

- [ ] **Step 3: 清理 i18n 与 tauri 注释**

在 `src/i18n/zh-CN.json` / `src/i18n/en-US.json`：
- 删除 `personas.*`、`persona.*`、云端-本地 toggle 文案。
- 新增键：
```json
{
  "auth": {
    "login": "登录",
    "username": "账号",
    "password": "密码",
    "expired": "登录已失效",
    "expiredDesc": "请重新登录后继续操作。"
  },
  "sidebar": {
    "home": "新任务",
    "skillCenter": "技能中心",
    "schedules": "定时任务",
    "settings": "设置",
    "tasks": "任务"
  },
  "skills": {
    "market": "技能市场",
    "upload": "上传技能",
    "comingSoon": "即将开放"
  }
}
```

在 `src/lib/tauri.ts` 给 persona IPC 注释加 deprecated 标记：
```ts
/** @deprecated 前端 Skill-First 改版后不再引用，仅为后端兼容保留。 */
export function listAgents(): Promise<AgentInfo[]> { ... }
```
对 `list_personas` / `get_active_persona` / `set_active_persona` 等同样处理。

- [ ] **Step 4: 跑 DoD 验收命令**

Run: `pnpm lint`
Expected: PASS

Run: `pnpm exec tsc -b --pretty false`
Expected: PASS

Run: `pnpm test`
Expected: PASS

Run: `pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts`
Expected: PASS，无既有回归

Run: `rg -n "useCloud|PersonaTab|AgentSelector|personaStore|onMouseEnter|onMouseLeave|backgroundColor:" src`
Expected: 无业务代码命中；仅允许 deprecated 注释或测试夹具中出现说明性文本

- [ ] **Step 5: 手工视觉验收**

Run: `pnpm dev`
Expected: 本地打开后满足以下检查：
- 未登录冷启动直接显示登录页
- 登录成功后进入首页，若 `redirectFrom` 已有值则回对应页
- 侧栏金色默认皮肤接近 design.pen
- `accentColor = #960505` / `#1A2E22` 时侧栏派生正常，1px 分隔线仍可见
- 技能中心有 10 个分类，市场/上传弹层显示“即将开放”
- 对话页不再显示 persona/agent 控件

- [ ] **Step 6: Commit**

```bash
git add src/i18n/zh-CN.json src/i18n/en-US.json src/lib/tauri.ts src/stores/settingsStore.test.ts
git commit -m "chore(frontend): finalize skill-first cleanup and verification"
```

---

## Spec 覆盖自检

- **Token 切换到 shadcn 命名**：Task 1、Task 3 覆盖。
- **租户皮肤收敛到单 accentColor**：Task 1、Task 2 覆盖。
- **Skill-First 信息架构与四入口侧栏**：Task 4、Task 5 覆盖。
- **登录全屏 Gate / redirectFrom / 主动退出**：Task 4、Task 9 覆盖。
- **技能中心 / 技能详情 / 市场骨架 / 上传骨架**：Task 6、Task 7 覆盖。
- **首页 / 定时任务骨架 / 对话页迁移**：Task 7、Task 8 覆盖。
- **删除 Persona / AgentSelector / useCloud**：Task 4、Task 8、Task 9、Task 10 覆盖。
- **DoD 验收与禁用硬编码颜色/JS hover**：Task 10 覆盖。

## 占位符检查

- 已避免使用 `TODO` / `TBD` / “后续实现” 作为执行步骤。
- 所有代码步骤都给了实际文件、代码片段和命令。
- “市场/上传即将开放”是产品要求本身，不是计划占位符；实现方式已在 Task 7 明确。

## 类型一致性检查

- 路由统一使用 `Route`：`home | skill-center | skill-detail | schedules | chat`。
- 设置弹窗统一使用 `SettingsModalState`：`account | general | about | usage`。
- 技能分类统一使用 `SkillCategoryId`，推荐分类固定为 `recommended`。
- 登录态恢复统一走 `authStore.restoreFromStorage()`；跳转统一经 `uiStore.setRoute()`。

## 执行顺序说明

- 本计划属于当前总序列中的前端改版计划；若其对应 `Plan-U -> Plan-AF` 主序列中的某一阶段，需要先把该阶段计划文件更新为引用本计划，再执行实现。
- 若实现中发现 spec 与 `claude-code-best` 的控制面/设置分层冲突，先更新本计划对应 Task，再继续开发，不要直接偏离计划。

Plan complete and saved to `docs/superpowers/plans/2026-04-22-frontend-design-system-and-skill-first-shell-design.md`. Two execution options:

1. Subagent-Driven (recommended) - 我按任务逐个派发新 subagent 执行，并在任务间做 review

2. Inline Execution - 我在当前会话里按这个计划连续执行，并在关键点做 checkpoint

Which approach?

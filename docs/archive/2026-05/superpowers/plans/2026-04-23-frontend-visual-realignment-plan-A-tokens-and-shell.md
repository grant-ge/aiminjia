# 前端视觉重构 · plan-A：Tokens & AppShell 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把视觉 token 收敛到 design.pen 的 Light + Neutral + Default 单一皮肤，建立 AppShell 三件套（AppSidebar / PageTopBar 4 variant / ChatTopBar / PageSectionShell）。

**Architecture:** 先导出设计稿基线 PNG 入库；再清瘦 `globals.css` + `brandingStore` + `styles/skin.ts`，删掉本轮不需要的暗色/Accent 派生逻辑，只保留运行时由租户接口下发单版的能力；最后按设计稿 node 重构侧栏与顶栏，使其作为后续 B/C/D 阶段所有页面的统一外壳。

**Tech Stack:** React 18 + TypeScript + Tailwind v4（@import "tailwindcss"）+ Zustand + Vitest + @testing-library/react + Playwright（仅用于截图）+ Pencil MCP（导出设计稿）。

**对应 spec：** `docs/superpowers/specs/2026-04-23-frontend-visual-realignment-to-design-pen.md` 第 3、4、5.1、9 章。

**前置：** 当前分支 `pzc`，工作目录 `/Users/oayzz/project/lotus/lotus-workbench/lotus-app`。

---

## 文件结构

### 新建

| 路径 | 责任 |
|---|---|
| `docs/superpowers/specs/assets/design-pen-exports/*.png` | 10 页 + 组件分区基线截图 |
| `src/components/shell/ChatTopBar.tsx` | 聊天页 TopBar（标题 / Workspace / 右侧操作） |
| `src/components/shell/PageTopBar.tsx`（重写） | 4 variant：default / title / breadcrumb / compact |
| `src/components/shell/PageSectionShell.tsx`（重写） | 容器 maxW 1032，padding 由调用方传入 |
| `src/components/sidebar/AppSidebar.tsx`（重写） | 三段式 header/content/footer + 侧栏交互聚合 |
| `src/components/sidebar/TenantHeader.tsx`（重写） | logo32 圆角 10 + 名称 + chevrons-up-down |
| `src/components/sidebar/SidebarNav.tsx`（重写） | 3 nav 项 active 用 `--sidebar-accent` 底 |
| `src/components/sidebar/SidebarSectionTitle.tsx` | "任务" 段标题 |
| `src/components/sidebar/ProjectAccordion.tsx` | 项目折叠头 + 子会话容器 slot |
| `src/components/sidebar/ConversationRow.tsx` | 会话行（active / loading / default） |
| `src/components/sidebar/SidebarFooterSettings.tsx` | 底部"设置"行 |
| `src/components/sidebar/__tests__/AppSidebar.test.tsx` | 三段式 + active 类断言 |
| `src/components/sidebar/__tests__/ProjectAccordion.test.tsx` | 折叠/展开、子项缩进 |
| `src/components/sidebar/__tests__/ConversationRow.test.tsx` | active/loading 渲染断言 |
| `src/components/shell/__tests__/PageTopBar.test.tsx` | 4 variant 渲染 |
| `src/components/shell/__tests__/ChatTopBar.test.tsx` | 标题 + workspace + 操作槽 |

### 修改

| 路径 | 修改内容 |
|---|---|
| `src/styles/globals.css` | 增补 `--brand-primary-subtle / --brand-secondary / --brand-secondary-subtle`、修正 `--sidebar / --sidebar-accent / --sidebar-border` 取值；删除 legacy alias 中本轮已无消费者的项；保留三档阴影变量 |
| `src/styles/skin.ts` | 移除 `mix/darken` 派生逻辑，改为 "accentColor 仅替换 `--primary / --primary-foreground / --ring / --sidebar-primary / --sidebar-primary-foreground`，其他保持 design.pen 静态值" |
| `src/stores/brandingStore.ts` | 同步 skin 调整；保持对 deprecated 字段（`primaryColor / bgColor / sidebarBgColor`）的容错忽略 |
| `src/stores/brandingStore.test.ts` | 跟随调整断言 |
| `src/styles/skin.test.ts` | 跟随调整断言 |
| `src/components/layout/Sidebar.test.tsx` | 适配新的 `AppSidebar` 结构（保留行为意图，更新选择器） |
| `package.json` | 增加脚本 `"export:design-pen": "node scripts/export-design-pen.mjs"` |
| `scripts/export-design-pen.mjs`（新增） | 调 Pencil MCP CLI 把 10 个 page node 导出到 `docs/.../assets/design-pen-exports/`（若 MCP 不能脚本化调用，则作为说明文档：列出 node-id 表，由人工通过 IDE 中的 pencil mcp 一次性导出） |
| `.gitignore` | 增加 `tmp/ui-capture/` |

---

## Task A-0：导出设计稿基线 PNG 入库

**Files:**
- Create: `docs/superpowers/specs/assets/design-pen-exports/{home,chat-long,chat-skill-popover,skill-center,skill-detail,schedules,settings-account,settings-about,settings-usage,login}.png`
- Create: `docs/superpowers/specs/assets/design-pen-exports/README.md`

- [ ] **Step 1：建立基线目录与索引**

```bash
mkdir -p docs/superpowers/specs/assets/design-pen-exports
```

写 `docs/superpowers/specs/assets/design-pen-exports/README.md`：

```markdown
# design.pen 基线截图

来源：`/Users/oayzz/project/lotus/lotus-workbench/lotus-app/design.pen`
导出时间：2026-04-23
导出主题：Light + Neutral + Default

| 文件名 | 设计稿 node-id | 设计尺寸 |
|---|---|---|
| home.png | 2cYHh | 1280×820 |
| chat-long.png | ju2pU | 1280×1244 |
| chat-skill-popover.png | 9qve3 | 1280×900 |
| skill-center.png | dVE8r | 1280×900 |
| skill-detail.png | cSdAy | 1280×1180 |
| schedules.png | s8Rc7 | 1280×900 |
| settings-account.png | S3D6p | 1280×900 |
| settings-about.png | 1MCFZ | 1280×900 |
| settings-usage.png | az6ZY | 1280×900 |
| login.png | epkyz | 1280×820 |

更新方法：当 design.pen 有改动时，用 pencil MCP `export_nodes` 重新导出对应 node 覆盖此目录。
```

- [ ] **Step 2：通过 Pencil MCP 导出 10 张 PNG**

调用（在与 pencil MCP 接通的会话中执行）：

```json
{
  "filePath": "/Users/oayzz/project/lotus/lotus-workbench/lotus-app/design.pen",
  "nodeIds": ["2cYHh","ju2pU","9qve3","dVE8r","cSdAy","s8Rc7","S3D6p","1MCFZ","az6ZY","epkyz"],
  "outputDir": "/Users/oayzz/project/lotus/lotus-workbench/lotus-app/docs/superpowers/specs/assets/design-pen-exports",
  "format": "png",
  "scale": 2
}
```

把生成的 `<node-id>.png` 重命名为 README 中对应文件名：

```bash
cd docs/superpowers/specs/assets/design-pen-exports
mv 2cYHh.png home.png
mv ju2pU.png chat-long.png
mv 9qve3.png chat-skill-popover.png
mv dVE8r.png skill-center.png
mv cSdAy.png skill-detail.png
mv s8Rc7.png schedules.png
mv S3D6p.png settings-account.png
mv 1MCFZ.png settings-about.png
mv az6ZY.png settings-usage.png
mv epkyz.png login.png
```

- [ ] **Step 3：验证 10 张 PNG 都存在且非空**

```bash
ls -la docs/superpowers/specs/assets/design-pen-exports/*.png | wc -l
```

Expected: `10`。每个文件 size > 10 KB。

- [ ] **Step 4：更新 .gitignore 增加运行截图目录**

修改 `.gitignore`，在末尾追加：

```gitignore
# Plan-A: visual realignment captures
tmp/ui-capture/
```

- [ ] **Step 5：commit**

```bash
git add docs/superpowers/specs/assets/design-pen-exports .gitignore
git commit -m "chore(frontend): add design.pen baseline exports for visual realignment"
```

---

## Task A-1.1：清瘦 `styles/skin.ts`，去掉派生逻辑

**Files:**
- Modify: `src/styles/skin.ts`
- Modify: `src/styles/skin.test.ts`

- [ ] **Step 1：写新的失败测试**

完全重写 `src/styles/skin.test.ts`：

```ts
import { describe, expect, it } from 'vitest'

import { DEFAULT_ACCENT_COLOR, DERIVED_SKIN_KEYS, deriveSkin } from './skin'

describe('deriveSkin', () => {
  it('returns only the 5 accent-bound CSS variables', () => {
    const result = deriveSkin(DEFAULT_ACCENT_COLOR)
    expect(Object.keys(result).sort()).toEqual([
      '--primary',
      '--primary-foreground',
      '--ring',
      '--sidebar-primary',
      '--sidebar-primary-foreground',
    ])
  })

  it('uses the given accent color for --primary / --ring / --sidebar-primary', () => {
    const result = deriveSkin('#DBAA22')
    expect(result['--primary']).toBe('#DBAA22')
    expect(result['--ring']).toBe('#DBAA22')
    expect(result['--sidebar-primary']).toBe('#DBAA22')
  })

  it('chooses white foreground for dark accent colors', () => {
    const result = deriveSkin('#000000')
    expect(result['--primary-foreground']).toBe('#FFFFFF')
    expect(result['--sidebar-primary-foreground']).toBe('#FFFFFF')
  })

  it('chooses near-black foreground for light accent colors', () => {
    const result = deriveSkin('#FFFFFF')
    expect(result['--primary-foreground']).toBe('#1A1A1A')
    expect(result['--sidebar-primary-foreground']).toBe('#1A1A1A')
  })

  it('falls back to default accent color when input is invalid', () => {
    expect(deriveSkin('not-a-color')['--primary']).toBe(DEFAULT_ACCENT_COLOR)
    expect(deriveSkin(undefined)['--primary']).toBe(DEFAULT_ACCENT_COLOR)
  })

  it('exports DERIVED_SKIN_KEYS matching the result keys', () => {
    const result = deriveSkin(DEFAULT_ACCENT_COLOR)
    expect([...DERIVED_SKIN_KEYS].sort()).toEqual(Object.keys(result).sort())
  })
})
```

- [ ] **Step 2：运行测试确认失败**

```bash
pnpm exec vitest run src/styles/skin.test.ts
```

Expected: 至少 `returns only the 5 accent-bound CSS variables` 与 `exports DERIVED_SKIN_KEYS matching the result keys` 失败（旧实现返回 7 个 key，包含 `--sidebar` 与 `--sidebar-accent`）。

- [ ] **Step 3：实现新的 skin.ts**

把 `src/styles/skin.ts` 替换为：

```ts
import { isDarkColor } from '@/lib/themeUtils'

export const DEFAULT_ACCENT_COLOR = '#DBAA22'

export const DERIVED_SKIN_KEYS = [
  '--primary',
  '--primary-foreground',
  '--ring',
  '--sidebar-primary',
  '--sidebar-primary-foreground',
] as const

function normalizeAccentColor(input?: string): string {
  return /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(input ?? '')
    ? (input as string)
    : DEFAULT_ACCENT_COLOR
}

export function deriveSkin(
  accentColor?: string,
): Record<(typeof DERIVED_SKIN_KEYS)[number], string> {
  const accent = normalizeAccentColor(accentColor)
  const foreground = isDarkColor(accent) ? '#FFFFFF' : '#1A1A1A'

  return {
    '--primary': accent,
    '--primary-foreground': foreground,
    '--ring': accent,
    '--sidebar-primary': accent,
    '--sidebar-primary-foreground': foreground,
  }
}
```

- [ ] **Step 4：运行测试确认通过**

```bash
pnpm exec vitest run src/styles/skin.test.ts
```

Expected: 全部 PASS。

- [ ] **Step 5：commit**

```bash
git add src/styles/skin.ts src/styles/skin.test.ts
git commit -m "refactor(frontend): slim skin to 5 accent-bound vars only"
```

---

## Task A-1.2：调整 `globals.css` 对齐 design.pen 实测值

**Files:**
- Modify: `src/styles/globals.css`

- [ ] **Step 1：写视觉常量断言（手工 grep 测试）**

在 `src/styles/globals.css` 末尾**临时**加一行注释 `/* PLAN-A-FENCE */`，然后写一个 sanity 测试 `src/styles/__tests__/globals-tokens.test.ts`：

```ts
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const CSS = fs.readFileSync(
  path.resolve(__dirname, '../globals.css'),
  'utf8',
)

function tokenValue(name: string): string | null {
  const m = CSS.match(new RegExp(`${name}\\s*:\\s*([^;]+);`))
  return m ? m[1].trim() : null
}

describe('design.pen token alignment', () => {
  it.each([
    ['--background', '#fafafa'],
    ['--foreground', '#0a0a0a'],
    ['--card', '#fafafa'],
    ['--border', '#e5e5e5'],
    ['--input', '#e5e5e5'],
    ['--muted', '#f5f5f5'],
    ['--muted-foreground', '#737373'],
    ['--popover', '#fafafa'],
    ['--secondary', '#f5f5f5'],
    ['--primary', '#DBAA22'],
    ['--primary-foreground', '#FFFFFF'],
    ['--brand-primary-subtle', '#FBF3DC'],
    ['--brand-secondary', '#3F3F46'],
    ['--brand-secondary-subtle', '#F3F4F6'],
    ['--ring', '#DBAA22'],
    ['--sidebar', '#F4F0E6'],
    ['--sidebar-accent', '#E1DAC6'],
    ['--sidebar-border', '#E1DAC6'],
    ['--sidebar-primary', '#DBAA22'],
    ['--sidebar-primary-foreground', '#FFFFFF'],
    ['--destructive', '#e7000b'],
  ])('token %s equals %s (design.pen)', (name, expected) => {
    const value = tokenValue(name)
    expect(value?.toLowerCase()).toBe(expected.toLowerCase())
  })
})
```

- [ ] **Step 2：运行测试确认失败**

```bash
pnpm exec vitest run src/styles/__tests__/globals-tokens.test.ts
```

Expected: 多条 FAIL（`--primary-foreground` 当前是 `#1A1A1A` 而 design.pen 用 `#FFFFFF`、`--sidebar` 当前是 `#FBF6E6`、`--sidebar-accent` 当前是 `#E9DEB2`、缺 `--brand-*` 三条等）。

- [ ] **Step 3：修改 `globals.css` 把 :root 的开头部分改成下面这一段**

将 `:root { ... }` 内 line 4-31 的 token 部分（`--primary` 到 `--destructive-foreground` 段）替换为：

```css
:root {
  /* === design.pen Light + Neutral + Default 单一皮肤 === */
  --primary: #DBAA22;
  --primary-foreground: #FFFFFF;
  --ring: var(--primary);
  --sidebar-primary: var(--primary);
  --sidebar-primary-foreground: #FFFFFF;

  --background: #fafafa;
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

  --sidebar: #F4F0E6;
  --sidebar-accent: #E1DAC6;
  --sidebar-accent-foreground: #18181b;
  --sidebar-foreground: #0a0a0a;
  --sidebar-border: #E1DAC6;
  --sidebar-ring: #71717a;

  --brand-primary-subtle: #FBF3DC;
  --brand-secondary: #3F3F46;
  --brand-secondary-subtle: #F3F4F6;

  --destructive: #e7000b;
  --destructive-foreground: #FFFFFF;
```

紧接其后保留原有的 `--font-sans / --font-mono / --radius-*` 与 legacy alias 段（不在本任务删除，避免破坏其他模块）。

- [ ] **Step 4：在 `@theme inline { ... }` 中追加品牌弱底变量**

在 `@theme inline` 块内（约 line 131-166），在 `--color-destructive-foreground: var(--destructive-foreground);` 之前插入：

```css
  --color-brand-primary-subtle: var(--brand-primary-subtle);
  --color-brand-secondary: var(--brand-secondary);
  --color-brand-secondary-subtle: var(--brand-secondary-subtle);
```

这样 Tailwind v4 能产出 `bg-brand-primary-subtle / text-brand-secondary` 等工具类。

- [ ] **Step 5：运行测试确认通过 & 整体 lint/类型**

```bash
pnpm exec vitest run src/styles/__tests__/globals-tokens.test.ts
pnpm lint
```

Expected: token 测试全部 PASS；lint 0 error（可能有 warning 不算）。

- [ ] **Step 6：commit**

```bash
git add src/styles/globals.css src/styles/__tests__/globals-tokens.test.ts
git commit -m "feat(frontend): align CSS tokens with design.pen Light+Neutral+Default"
```

---

## Task A-1.3：调整 `brandingStore` 适配新 skin

**Files:**
- Modify: `src/stores/brandingStore.ts`
- Modify: `src/stores/brandingStore.test.ts`

- [ ] **Step 1：写失败测试**

在 `src/stores/brandingStore.test.ts` 顶部追加：

```ts
import { describe, expect, it, beforeEach } from 'vitest'
import { useBrandingStore } from './brandingStore'

describe('brandingStore (plan-A token slimming)', () => {
  beforeEach(() => {
    useBrandingStore.getState().reset()
  })

  it('applyBranding writes ONLY the 5 accent-bound CSS vars to documentElement', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#DBAA22' })
    const style = document.documentElement.style
    expect(style.getPropertyValue('--primary').trim()).toBe('#DBAA22')
    expect(style.getPropertyValue('--ring').trim()).toBe('#DBAA22')
    expect(style.getPropertyValue('--sidebar-primary').trim()).toBe('#DBAA22')
    // sidebar 与 sidebar-accent 由 globals.css 提供静态值，运行时不被 branding 改写
    expect(style.getPropertyValue('--sidebar').trim()).toBe('')
    expect(style.getPropertyValue('--sidebar-accent').trim()).toBe('')
  })

  it('reset clears the 5 accent-bound vars', () => {
    useBrandingStore.getState().applyBranding({ accentColor: '#FF0000' })
    useBrandingStore.getState().reset()
    const style = document.documentElement.style
    expect(style.getPropertyValue('--primary')).toBe('')
    expect(style.getPropertyValue('--sidebar-primary')).toBe('')
  })
})
```

- [ ] **Step 2：运行测试确认失败**

```bash
pnpm exec vitest run src/stores/brandingStore.test.ts
```

Expected: 新增第一条 FAIL，因为旧 skin 还会写 `--sidebar / --sidebar-accent`。

- [ ] **Step 3：让 brandingStore 通过**

`brandingStore.ts` 本身只迭代 `Object.entries(skin)`，所以它不需要源代码层面修改 —— skin 已在 A-1.1 收敛到 5 个 key，本测试在 A-1.1 完成后会自然通过。在本任务里仅需：
1. 复核 `brandingStore.ts` 的 `reset` 函数：`DERIVED_SKIN_KEYS.forEach(removeVar)` 已自动跟随。
2. 在 `brandingStore.ts` 文件顶部加一行 JSDoc：

```ts
/**
 * Plan-A 收敛：本 store 只在运行时下发 5 个 accent-bound CSS 变量。
 * `--sidebar / --sidebar-accent / --background / --card` 等静态值由 globals.css 固定，
 * 不再由租户接口改写。deprecated 字段（primaryColor/bgColor/sidebarBgColor）保留接口容错忽略。
 */
```

- [ ] **Step 4：运行测试确认通过**

```bash
pnpm exec vitest run src/stores/brandingStore.test.ts src/styles/skin.test.ts
```

Expected: 全 PASS。

- [ ] **Step 5：commit**

```bash
git add src/stores/brandingStore.ts src/stores/brandingStore.test.ts
git commit -m "test(frontend): pin brandingStore to write only accent-bound vars"
```

---

## Task A-2.1：重写 TenantHeader 对齐 `6xhgh`

**Files:**
- Modify: `src/components/sidebar/TenantHeader.tsx`
- Create: `src/components/sidebar/__tests__/TenantHeader.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/sidebar/__tests__/TenantHeader.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { TenantHeader } from '../TenantHeader'

describe('TenantHeader', () => {
  it('renders the brand logo image and tenant name', () => {
    render(<TenantHeader name="仁励家网络科技(杭州)" logoUrl="/app-icon.png" />)
    const img = screen.getByRole('img', { name: /brand logo/i })
    expect(img).toHaveAttribute('src', '/app-icon.png')
    expect(screen.getByText('仁励家网络科技(杭州)')).toBeInTheDocument()
  })

  it('renders a chevrons-up-down indicator on the right', () => {
    const { container } = render(
      <TenantHeader name="X" logoUrl="/app-icon.png" />,
    )
    expect(container.querySelector('[data-icon="chevrons-up-down"]')).toBeInTheDocument()
  })

  it('logo box has 32x32 sizing classes', () => {
    const { container } = render(
      <TenantHeader name="X" logoUrl="/app-icon.png" />,
    )
    const logoWrap = container.querySelector('[data-testid="tenant-logo"]')
    expect(logoWrap?.className).toMatch(/h-8/)
    expect(logoWrap?.className).toMatch(/w-8/)
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/sidebar/__tests__/TenantHeader.test.tsx
```

Expected: FAIL（旧组件不接受 props，没有 chevrons-up-down，也没有 data-testid="tenant-logo"）。

- [ ] **Step 3：实现新组件**

完全替换 `src/components/sidebar/TenantHeader.tsx`：

```tsx
/**
 * @designSource design.pen#6xhgh
 * @sizing width fluid, padding 8, gap 8
 */
import { ChevronsUpDown } from 'lucide-react'

interface TenantHeaderProps {
  name: string
  logoUrl: string
  onClick?: () => void
}

export function TenantHeader({ name, logoUrl, onClick }: TenantHeaderProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center justify-between gap-2 rounded-md p-2 text-left transition-colors hover:bg-sidebar-accent/50"
    >
      <div className="flex min-w-0 items-center gap-2">
        <div
          data-testid="tenant-logo"
          className="h-8 w-8 shrink-0 overflow-hidden rounded-[10px] border border-sidebar-border"
        >
          <img
            src={logoUrl}
            alt="Brand logo"
            className="h-full w-full object-cover"
          />
        </div>
        <div className="min-w-0 truncate text-sm font-semibold text-sidebar-foreground">
          {name}
        </div>
      </div>
      <ChevronsUpDown
        data-icon="chevrons-up-down"
        className="h-4 w-4 shrink-0 text-muted-foreground"
      />
    </button>
  )
}
```

- [ ] **Step 4：运行测试确认通过**

```bash
pnpm exec vitest run src/components/sidebar/__tests__/TenantHeader.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/sidebar/TenantHeader.tsx src/components/sidebar/__tests__/TenantHeader.test.tsx
git commit -m "refactor(frontend): rebuild TenantHeader to design.pen #6xhgh"
```

---

## Task A-2.2：抽出 `SidebarSectionTitle` + 重写 `SidebarNav`

**Files:**
- Create: `src/components/sidebar/SidebarSectionTitle.tsx`
- Modify: `src/components/sidebar/SidebarNav.tsx`
- Create: `src/components/sidebar/__tests__/SidebarNav.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/sidebar/__tests__/SidebarNav.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SidebarNav } from '../SidebarNav'

describe('SidebarNav', () => {
  it('renders 3 nav items: 新任务 / 技能中心 / 定时任务', () => {
    render(<SidebarNav activeKey="home" onSelect={() => {}} />)
    expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '技能中心' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '定时任务' })).toBeInTheDocument()
  })

  it('marks the active item with sidebar-accent background class', () => {
    render(<SidebarNav activeKey="skill-center" onSelect={() => {}} />)
    const active = screen.getByRole('button', { name: '技能中心' })
    expect(active.className).toMatch(/bg-sidebar-accent/)
  })

  it('calls onSelect with the kind on click', () => {
    const onSelect = vi.fn()
    render(<SidebarNav activeKey="home" onSelect={onSelect} />)
    screen.getByRole('button', { name: '定时任务' }).click()
    expect(onSelect).toHaveBeenCalledWith('schedules')
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/sidebar/__tests__/SidebarNav.test.tsx
```

Expected: FAIL（旧组件用内置 useUiStore 而不是 props）。

- [ ] **Step 3：实现 `SidebarSectionTitle`**

创建 `src/components/sidebar/SidebarSectionTitle.tsx`：

```tsx
/**
 * @designSource design.pen#24cM4
 * @sizing padding 8, fontSize 12
 */
interface SidebarSectionTitleProps {
  label: string
}

export function SidebarSectionTitle({ label }: SidebarSectionTitleProps) {
  return (
    <div className="px-2 py-2 text-xs font-semibold tracking-wide text-muted-foreground">
      {label}
    </div>
  )
}
```

- [ ] **Step 4：替换 `SidebarNav.tsx`**

```tsx
/**
 * @designSource design.pen#47U5w (nv1/nv2/nv3)
 * @sizing each row padding [6,8], gap 2
 */
import { Blocks, Clock3, Sparkles, type LucideIcon } from 'lucide-react'

export type SidebarNavKey = 'home' | 'skill-center' | 'schedules'

interface SidebarNavProps {
  activeKey: SidebarNavKey
  onSelect: (key: SidebarNavKey) => void
}

const NAV: Array<{ key: SidebarNavKey; label: string; icon: LucideIcon }> = [
  { key: 'home', label: '新任务', icon: Sparkles },
  { key: 'skill-center', label: '技能中心', icon: Blocks },
  { key: 'schedules', label: '定时任务', icon: Clock3 },
]

export function SidebarNav({ activeKey, onSelect }: SidebarNavProps) {
  return (
    <nav className="flex flex-col gap-0.5">
      {NAV.map(({ key, label, icon: Icon }) => {
        const active = key === activeKey
        return (
          <button
            key={key}
            type="button"
            onClick={() => onSelect(key)}
            className={
              active
                ? 'flex w-full items-center gap-2 rounded-md bg-sidebar-accent px-2 py-1.5 text-left text-sm text-sidebar-foreground'
                : 'flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent/40'
            }
          >
            <Icon className="h-4 w-4 shrink-0" />
            <span className="truncate">{label}</span>
          </button>
        )
      })}
    </nav>
  )
}
```

- [ ] **Step 5：运行测试确认通过**

```bash
pnpm exec vitest run src/components/sidebar/__tests__/SidebarNav.test.tsx
```

Expected: PASS。

- [ ] **Step 6：commit**

```bash
git add src/components/sidebar/SidebarNav.tsx src/components/sidebar/SidebarSectionTitle.tsx src/components/sidebar/__tests__/SidebarNav.test.tsx
git commit -m "refactor(frontend): rebuild SidebarNav with controlled props and design.pen rhythm"
```

---

## Task A-2.3：新增 `ConversationRow` + `ProjectAccordion`

**Files:**
- Create: `src/components/sidebar/ConversationRow.tsx`
- Create: `src/components/sidebar/ProjectAccordion.tsx`
- Create: `src/components/sidebar/__tests__/ConversationRow.test.tsx`
- Create: `src/components/sidebar/__tests__/ProjectAccordion.test.tsx`

- [ ] **Step 1：写失败测试 ConversationRow**

```tsx
// src/components/sidebar/__tests__/ConversationRow.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ConversationRow } from '../ConversationRow'

describe('ConversationRow', () => {
  it('renders title with left padding 30 (indent under project)', () => {
    const { container } = render(
      <ConversationRow title="测试会话" onClick={() => {}} />,
    )
    const btn = container.querySelector('button')
    expect(btn?.className).toMatch(/pl-\[30px\]/)
  })

  it('uses sidebar-accent bg when active', () => {
    const { container } = render(
      <ConversationRow title="X" active onClick={() => {}} />,
    )
    expect(container.querySelector('button')?.className).toMatch(/bg-sidebar-accent/)
  })

  it('shows a loader icon when loading', () => {
    const { container } = render(
      <ConversationRow title="X" loading onClick={() => {}} />,
    )
    expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
  })

  it('invokes onClick on click', () => {
    const onClick = vi.fn()
    render(<ConversationRow title="X" onClick={onClick} />)
    screen.getByRole('button').click()
    expect(onClick).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2：写失败测试 ProjectAccordion**

```tsx
// src/components/sidebar/__tests__/ProjectAccordion.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ProjectAccordion } from '../ProjectAccordion'

describe('ProjectAccordion', () => {
  it('shows children only when expanded', () => {
    const { rerender } = render(
      <ProjectAccordion name="默认项目" expanded={false} onToggle={() => {}}>
        <div>子项 A</div>
      </ProjectAccordion>,
    )
    expect(screen.queryByText('子项 A')).toBeNull()

    rerender(
      <ProjectAccordion name="默认项目" expanded onToggle={() => {}}>
        <div>子项 A</div>
      </ProjectAccordion>,
    )
    expect(screen.getByText('子项 A')).toBeInTheDocument()
  })

  it('invokes onToggle when header clicked', () => {
    const onToggle = vi.fn()
    render(
      <ProjectAccordion name="默认项目" expanded onToggle={onToggle}>
        <div>x</div>
      </ProjectAccordion>,
    )
    fireEvent.click(screen.getByRole('button', { name: /默认项目/ }))
    expect(onToggle).toHaveBeenCalled()
  })

  it('shows ChevronDown icon (rotates via expanded)', () => {
    const { container } = render(
      <ProjectAccordion name="X" expanded onToggle={() => {}}>
        <div />
      </ProjectAccordion>,
    )
    expect(container.querySelector('[data-icon="chevron-down"]')).toBeInTheDocument()
  })
})
```

- [ ] **Step 3：运行确认失败**

```bash
pnpm exec vitest run src/components/sidebar/__tests__/ConversationRow.test.tsx src/components/sidebar/__tests__/ProjectAccordion.test.tsx
```

Expected: FAIL（两组件都不存在）。

- [ ] **Step 4：实现 ConversationRow**

```tsx
// src/components/sidebar/ConversationRow.tsx
/**
 * @designSource design.pen#0EZDr / HsGnf / GknhC
 * @sizing padding [6,8,6,30] (indent 30 under ProjectAccordion), fontSize 13
 */
import { Loader2 } from 'lucide-react'

interface ConversationRowProps {
  title: string
  active?: boolean
  loading?: boolean
  onClick: () => void
}

export function ConversationRow({
  title,
  active = false,
  loading = false,
  onClick,
}: ConversationRowProps) {
  const base =
    'flex w-full items-center gap-2 rounded-md py-1.5 pl-[30px] pr-2 text-left text-[13px]'
  const cls = active
    ? `${base} bg-sidebar-accent text-sidebar-foreground`
    : `${base} text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/40`

  return (
    <button type="button" onClick={onClick} className={cls}>
      {loading ? (
        <Loader2
          data-icon="loader"
          className="h-3.5 w-3.5 shrink-0 animate-spin text-sidebar-foreground"
        />
      ) : null}
      <span className="truncate">{title}</span>
    </button>
  )
}
```

- [ ] **Step 5：实现 ProjectAccordion**

```tsx
// src/components/sidebar/ProjectAccordion.tsx
/**
 * @designSource design.pen#lqhdx / L29MQ
 * @sizing header padding [6,8], gap 8
 */
import { ChevronDown } from 'lucide-react'
import type { PropsWithChildren } from 'react'

interface ProjectAccordionProps extends PropsWithChildren {
  name: string
  expanded: boolean
  onToggle: () => void
}

export function ProjectAccordion({
  name,
  expanded,
  onToggle,
  children,
}: ProjectAccordionProps) {
  return (
    <div className="flex flex-col">
      <button
        type="button"
        onClick={onToggle}
        className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm font-medium text-sidebar-foreground transition-colors hover:bg-sidebar-accent/40"
      >
        <ChevronDown
          data-icon="chevron-down"
          className={
            expanded
              ? 'h-4 w-4 shrink-0 text-muted-foreground transition-transform'
              : 'h-4 w-4 shrink-0 -rotate-90 text-muted-foreground transition-transform'
          }
        />
        <span className="truncate">{name}</span>
      </button>
      {expanded ? <div className="flex flex-col gap-0.5">{children}</div> : null}
    </div>
  )
}
```

- [ ] **Step 6：运行测试确认通过**

```bash
pnpm exec vitest run src/components/sidebar/__tests__/ConversationRow.test.tsx src/components/sidebar/__tests__/ProjectAccordion.test.tsx
```

Expected: 全 PASS。

- [ ] **Step 7：commit**

```bash
git add src/components/sidebar/ConversationRow.tsx src/components/sidebar/ProjectAccordion.tsx src/components/sidebar/__tests__/ConversationRow.test.tsx src/components/sidebar/__tests__/ProjectAccordion.test.tsx
git commit -m "feat(frontend): add ConversationRow and ProjectAccordion"
```

---

## Task A-2.4：抽出 `SidebarFooterSettings` 并重写 `ConversationTree`

**Files:**
- Create: `src/components/sidebar/SidebarFooterSettings.tsx`
- Modify: `src/components/sidebar/ConversationTree.tsx`

- [ ] **Step 1���实现 SidebarFooterSettings**

```tsx
// src/components/sidebar/SidebarFooterSettings.tsx
/**
 * @designSource design.pen#jTgSA
 * @sizing padding [6,8], gap 8
 */
import { Settings } from 'lucide-react'

interface SidebarFooterSettingsProps {
  onClick: () => void
}

export function SidebarFooterSettings({ onClick }: SidebarFooterSettingsProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/40"
    >
      <Settings className="h-4 w-4 shrink-0 text-muted-foreground" />
      <span>设置</span>
    </button>
  )
}
```

- [ ] **Step 2：重写 ConversationTree 为受控+按项目分组**

```tsx
// src/components/sidebar/ConversationTree.tsx
/**
 * @designSource design.pen#47U5w (proj1/conv1..3 + proj2/convA..B)
 *
 * 按 project 分组渲染会话；项目折叠状态由本组件内部 state 管理。
 */
import { useState } from 'react'

import { ConversationRow } from './ConversationRow'
import { ProjectAccordion } from './ProjectAccordion'

export interface ConversationTreeItem {
  id: string
  title: string
  active?: boolean
  loading?: boolean
}

export interface ConversationTreeProject {
  id: string
  name: string
  conversations: ConversationTreeItem[]
}

interface ConversationTreeProps {
  projects: ConversationTreeProject[]
  onSelectConversation: (conversationId: string) => void
}

export function ConversationTree({
  projects,
  onSelectConversation,
}: ConversationTreeProps) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})

  if (projects.length === 0) {
    return (
      <div className="px-2 py-4 text-[13px] text-muted-foreground">还没有历史任务</div>
    )
  }

  return (
    <div className="flex flex-col gap-1">
      {projects.map((p) => (
        <ProjectAccordion
          key={p.id}
          name={p.name}
          expanded={!collapsed[p.id]}
          onToggle={() => setCollapsed((s) => ({ ...s, [p.id]: !s[p.id] }))}
        >
          {p.conversations.map((c) => (
            <ConversationRow
              key={c.id}
              title={c.title}
              active={c.active}
              loading={c.loading}
              onClick={() => onSelectConversation(c.id)}
            />
          ))}
        </ProjectAccordion>
      ))}
    </div>
  )
}
```

- [ ] **Step 3：lint + 类型**

```bash
pnpm lint
pnpm exec tsc --noEmit
```

Expected: 0 error（旧的 `ConversationTree` 调用方在下一任务里改）。

- [ ] **Step 4：commit**

```bash
git add src/components/sidebar/SidebarFooterSettings.tsx src/components/sidebar/ConversationTree.tsx
git commit -m "refactor(frontend): make ConversationTree project-grouped and controlled"
```

---

## Task A-2.5：重写 `AppSidebar` 装配三段式

**Files:**
- Modify: `src/components/sidebar/AppSidebar.tsx`
- Create: `src/components/sidebar/__tests__/AppSidebar.test.tsx`
- Create: `src/components/sidebar/conversationProjects.ts`（数据装配 helper）

- [ ] **Step 1：写失败测试**

```tsx
// src/components/sidebar/__tests__/AppSidebar.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    conversations: [],
    activeConversationId: null,
    switchConversation: vi.fn(),
    createNewConversation: vi.fn(),
  }),
}))

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (sel: any) =>
    sel({
      route: { kind: 'home' },
      setRoute: vi.fn(),
      openSettings: vi.fn(),
    }),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: any) => sel({ user: null, tenant: null }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (sel: any) =>
    sel({ productName: '仁励家网络科技(杭州)', logoUrl: '/app-icon.png' }),
}))

import { AppSidebar } from '../AppSidebar'

describe('AppSidebar', () => {
  it('has sidebar background and 256 px width', () => {
    const { container } = render(<AppSidebar />)
    const aside = container.querySelector('aside')
    expect(aside?.className).toMatch(/w-\[256px\]/)
    expect(aside?.className).toMatch(/bg-sidebar/)
  })

  it('renders TenantHeader name', () => {
    render(<AppSidebar />)
    expect(screen.getByText('仁励家网络科技(杭州)')).toBeInTheDocument()
  })

  it('renders 3 nav items, the section title 任务, and footer 设置', () => {
    render(<AppSidebar />)
    expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '技能中心' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '定时任务' })).toBeInTheDocument()
    expect(screen.getByText('任务')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '设置' })).toBeInTheDocument()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/sidebar/__tests__/AppSidebar.test.tsx
```

Expected: FAIL（旧组件 `w-[248px]` 而非 `w-[256px]`，没有 "任务" 段标题，nav 用 `useUiStore` 直接耦合）。

- [ ] **Step 3：抽出数据装配 helper**

```ts
// src/components/sidebar/conversationProjects.ts
/**
 * Plan-A：把 useChat() 的 flat conversations 转成 project-grouped 结构。
 * 项目分组当前由 `conversation.projectId` 决定，未提供则归到 "默认项目"。
 */
import type { ConversationTreeProject } from './ConversationTree'

export interface RawConversation {
  id: string
  title: string
  projectId?: string | null
  projectName?: string | null
  loading?: boolean
}

export function groupConversationsByProject(
  conversations: RawConversation[],
  activeId: string | null,
): ConversationTreeProject[] {
  const map = new Map<string, ConversationTreeProject>()
  for (const c of conversations) {
    const projectId = c.projectId || 'default'
    const projectName = c.projectName || '默认项目'
    let project = map.get(projectId)
    if (!project) {
      project = { id: projectId, name: projectName, conversations: [] }
      map.set(projectId, project)
    }
    project.conversations.push({
      id: c.id,
      title: c.title,
      active: c.id === activeId,
      loading: c.loading,
    })
  }
  return [...map.values()]
}
```

- [ ] **Step 4：重写 AppSidebar**

```tsx
// src/components/sidebar/AppSidebar.tsx
/**
 * @designSource design.pen#PV1ln (Sidebar) + #EbnTy (Sidebar Content)
 * @sizing width 256, padding 8, gap 16
 */
import { useChat } from '@/hooks/useChat'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore, type Route } from '@/stores/uiStore'

import { ConversationTree } from './ConversationTree'
import { groupConversationsByProject } from './conversationProjects'
import { SidebarFooterSettings } from './SidebarFooterSettings'
import { SidebarNav, type SidebarNavKey } from './SidebarNav'
import { SidebarSectionTitle } from './SidebarSectionTitle'
import { TenantHeader } from './TenantHeader'

export function AppSidebar() {
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const tenant = useAuthStore((s) => s.tenant)
  const route = useUiStore((s) => s.route)
  const setRoute = useUiStore((s) => s.setRoute)
  const openSettings = useUiStore((s) => s.openSettings)
  const { conversations, activeConversationId, switchConversation } = useChat()

  const projects = groupConversationsByProject(
    conversations as never,
    activeConversationId,
  )

  const activeKey: SidebarNavKey =
    route.kind === 'skill-center'
      ? 'skill-center'
      : route.kind === 'schedules'
        ? 'schedules'
        : 'home'

  const tenantDisplay = tenant?.name ?? productName

  return (
    <aside className="flex h-full w-[256px] shrink-0 flex-col gap-4 overflow-hidden border-r border-sidebar-border bg-sidebar p-2 text-sidebar-foreground">
      <TenantHeader name={tenantDisplay} logoUrl={logoUrl} />

      <SidebarNav
        activeKey={activeKey}
        onSelect={(key) => setRoute({ kind: key } as Route)}
      />

      <SidebarSectionTitle label="任务" />

      <div className="min-h-0 flex-1 overflow-auto pr-1">
        <ConversationTree
          projects={projects}
          onSelectConversation={(id) => void switchConversation(id)}
        />
      </div>

      <SidebarFooterSettings onClick={() => openSettings('account')} />
    </aside>
  )
}
```

- [ ] **Step 5：运行测试确认通过 & 修旧 Sidebar.test.tsx**

`src/components/layout/Sidebar.test.tsx` 是旧测试，可能因结构改动失败。打开它，把硬选择器（`getByText('搜索对话...')` 等已不存在的元素）替换为新的：例如断言侧栏宽度、3 个 nav、设置按钮存在。本步只做最小适配，不重写测试意图。

```bash
pnpm exec vitest run src/components/sidebar src/components/layout/Sidebar.test.tsx
```

Expected: 全 PASS。

- [ ] **Step 6：commit**

```bash
git add src/components/sidebar/AppSidebar.tsx src/components/sidebar/conversationProjects.ts src/components/sidebar/__tests__/AppSidebar.test.tsx src/components/layout/Sidebar.test.tsx
git commit -m "refactor(frontend): rebuild AppSidebar to design.pen three-section layout"
```

---

## Task A-2.6：重写 `PageTopBar` 支持 4 variant

**Files:**
- Modify: `src/components/shell/PageTopBar.tsx`
- Create: `src/components/shell/__tests__/PageTopBar.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/shell/__tests__/PageTopBar.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { PageTopBar } from '../PageTopBar'

describe('PageTopBar', () => {
  it('default variant: empty bar with bottom border, h-14, px-6', () => {
    const { container } = render(<PageTopBar variant="default" />)
    const header = container.querySelector('header')
    expect(header?.className).toMatch(/h-14/)
    expect(header?.className).toMatch(/px-6/)
    expect(header?.className).toMatch(/border-b/)
  })

  it('title variant renders the title text', () => {
    render(<PageTopBar variant="title" title="技能中心" />)
    expect(screen.getByText('技能中心')).toBeInTheDocument()
  })

  it('breadcrumb variant renders provided crumbs', () => {
    render(
      <PageTopBar
        variant="breadcrumb"
        breadcrumbs={[{ label: 'A' }, { label: 'B' }]}
      />,
    )
    expect(screen.getByText('A')).toBeInTheDocument()
    expect(screen.getByText('B')).toBeInTheDocument()
  })

  it('compact variant uses smaller text class', () => {
    const { container } = render(<PageTopBar variant="compact" title="X" />)
    expect(container.querySelector('header')?.querySelector('div')?.className).toMatch(
      /text-sm/,
    )
  })

  it('renders trailing slot when provided', () => {
    render(
      <PageTopBar
        variant="title"
        title="X"
        trailing={<span>extra</span>}
      />,
    )
    expect(screen.getByText('extra')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/shell/__tests__/PageTopBar.test.tsx
```

Expected: FAIL（旧组件没有 `variant` 也没有 `breadcrumbs` 属性）。

- [ ] **Step 3：实现新 PageTopBar**

```tsx
// src/components/shell/PageTopBar.tsx
/**
 * @designSource design.pen#BixkY/aAO2u/tCYsE/WgoHO
 * @sizing height 56, padding [0,24], bottom border 1
 */
import type { ReactNode } from 'react'
import { ChevronRight } from 'lucide-react'

export type PageTopBarVariant = 'default' | 'title' | 'breadcrumb' | 'compact'

export interface BreadcrumbCrumb {
  label: string
  onClick?: () => void
  current?: boolean
}

interface PageTopBarProps {
  variant: PageTopBarVariant
  title?: ReactNode
  breadcrumbs?: BreadcrumbCrumb[]
  leading?: ReactNode
  trailing?: ReactNode
}

export function PageTopBar({
  variant,
  title,
  breadcrumbs,
  leading,
  trailing,
}: PageTopBarProps) {
  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b border-border bg-background px-6">
      <div className="flex min-w-0 items-center gap-3">
        {leading}
        {variant === 'title' || variant === 'compact' ? (
          <div
            className={
              variant === 'compact'
                ? 'truncate text-sm font-semibold text-foreground'
                : 'truncate text-base font-semibold text-foreground'
            }
          >
            {title}
          </div>
        ) : null}
        {variant === 'breadcrumb' && breadcrumbs ? (
          <ol className="flex min-w-0 items-center gap-2 text-sm text-muted-foreground">
            {breadcrumbs.map((c, i) => (
              <li key={i} className="flex items-center gap-2">
                {i > 0 ? <ChevronRight className="h-3.5 w-3.5" /> : null}
                {c.onClick ? (
                  <button
                    type="button"
                    className={c.current ? 'text-foreground' : 'hover:text-foreground'}
                    onClick={c.onClick}
                  >
                    {c.label}
                  </button>
                ) : (
                  <span className={c.current ? 'text-foreground' : ''}>{c.label}</span>
                )}
              </li>
            ))}
          </ol>
        ) : null}
      </div>
      {trailing ? <div className="flex items-center gap-2">{trailing}</div> : null}
    </header>
  )
}
```

- [ ] **Step 4：运行测试确认通过 + 修复其他调用点**

```bash
pnpm exec tsc --noEmit
```

如果出现旧调用点（`<PageTopBar title=... compact />`）的类型错误，修改这些调用点把 prop 改为 `variant="compact"` / `variant="title"`，本任务范围内只做最小修复，不替换页面结构。

```bash
pnpm exec vitest run src/components/shell/__tests__/PageTopBar.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/shell/PageTopBar.tsx src/components/shell/__tests__/PageTopBar.test.tsx $(git ls-files -m | grep -v test)
git commit -m "refactor(frontend): PageTopBar supports default/title/breadcrumb/compact variants"
```

---

## Task A-2.7：新增 `ChatTopBar`

**Files:**
- Create: `src/components/shell/ChatTopBar.tsx`
- Create: `src/components/shell/__tests__/ChatTopBar.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/shell/__tests__/ChatTopBar.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ChatTopBar } from '../ChatTopBar'

describe('ChatTopBar', () => {
  it('renders title, separator and workspace', () => {
    render(
      <ChatTopBar
        title="打开 BI 看板导出绩效分析数据并总结"
        workspace="Desktop"
      />,
    )
    expect(
      screen.getByText('打开 BI 看板导出绩效分析数据并总结'),
    ).toBeInTheDocument()
    expect(screen.getByText('Desktop')).toBeInTheDocument()
    expect(screen.getByText('/')).toBeInTheDocument()
  })

  it('fires share/more/toggleSidebar callbacks', () => {
    const onShare = vi.fn()
    const onMore = vi.fn()
    const onToggleSidebar = vi.fn()
    render(
      <ChatTopBar
        title="X"
        workspace="W"
        onShare={onShare}
        onMore={onMore}
        onToggleSidebar={onToggleSidebar}
      />,
    )
    screen.getByRole('button', { name: /分享/ }).click()
    screen.getByRole('button', { name: /更多/ }).click()
    screen.getByRole('button', { name: /折叠侧栏/ }).click()
    expect(onShare).toHaveBeenCalled()
    expect(onMore).toHaveBeenCalled()
    expect(onToggleSidebar).toHaveBeenCalled()
  })

  it('header has h-14, px-6 and bottom border', () => {
    const { container } = render(<ChatTopBar title="X" workspace="Y" />)
    const header = container.querySelector('header')
    expect(header?.className).toMatch(/h-14/)
    expect(header?.className).toMatch(/px-6/)
    expect(header?.className).toMatch(/border-b/)
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/shell/__tests__/ChatTopBar.test.tsx
```

Expected: FAIL（组件不存在）。

- [ ] **Step 3：实现 ChatTopBar**

```tsx
// src/components/shell/ChatTopBar.tsx
/**
 * @designSource design.pen#qLmzZ
 * @sizing height 56, padding [0,24], bottom border 1, left gap 12, right gap 14
 */
import { Ellipsis, PanelLeft, Share2 } from 'lucide-react'

interface ChatTopBarProps {
  title: string
  workspace?: string
  onShare?: () => void
  onMore?: () => void
  onToggleSidebar?: () => void
}

export function ChatTopBar({
  title,
  workspace,
  onShare,
  onMore,
  onToggleSidebar,
}: ChatTopBarProps) {
  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b border-border bg-background px-6">
      <div className="flex min-w-0 items-center gap-3">
        <div className="truncate text-[15px] font-semibold text-foreground">
          {title}
        </div>
        {workspace ? (
          <>
            <span className="text-[13px] text-muted-foreground">/</span>
            <span className="truncate text-[13px] text-muted-foreground">
              {workspace}
            </span>
          </>
        ) : null}
      </div>
      <div className="flex items-center gap-3.5">
        {onShare ? (
          <button
            type="button"
            aria-label="分享"
            onClick={onShare}
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <Share2 className="h-4 w-4" />
          </button>
        ) : null}
        {onMore ? (
          <button
            type="button"
            aria-label="更多"
            onClick={onMore}
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <Ellipsis className="h-4 w-4" />
          </button>
        ) : null}
        {onToggleSidebar ? (
          <button
            type="button"
            aria-label="折叠侧栏"
            onClick={onToggleSidebar}
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <PanelLeft className="h-4 w-4" />
          </button>
        ) : null}
      </div>
    </header>
  )
}
```

- [ ] **Step 4：运行测试确认通过**

```bash
pnpm exec vitest run src/components/shell/__tests__/ChatTopBar.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/shell/ChatTopBar.tsx src/components/shell/__tests__/ChatTopBar.test.tsx
git commit -m "feat(frontend): add ChatTopBar shell component"
```

---

## Task A-2.8：调整 `PageSectionShell` 让 padding 由调用方传入

**Files:**
- Modify: `src/components/shell/PageSectionShell.tsx`

- [ ] **Step 1：替换实现**

```tsx
// src/components/shell/PageSectionShell.tsx
/**
 * @designSource design.pen#PqcAk / canvas* family
 *
 * 把"max-w + padding"分离：max-w 固定 1032，padding/gap 由 padding/gap props 传入。
 * 这样 home / skills / schedules 各页可保留自己的稿子 padding 节奏，而页面层不需要写颜色/边框。
 */
import type { PropsWithChildren, ReactNode } from 'react'

interface PageSectionShellProps extends PropsWithChildren {
  topBar?: ReactNode
  /** Tailwind padding classes, e.g. "px-10 pt-8 pb-7" */
  padding?: string
  /** Tailwind gap class, e.g. "gap-4" */
  gap?: string
  /** override max width if needed (default 1032) */
  maxWidthClass?: string
}

export function PageSectionShell({
  topBar,
  padding = 'px-10 pt-8 pb-7',
  gap = 'gap-4',
  maxWidthClass = 'max-w-[1032px]',
  children,
}: PageSectionShellProps) {
  return (
    <div className="flex h-full flex-col overflow-hidden bg-background">
      {topBar}
      <div className="min-h-0 flex-1 overflow-auto">
        <div
          className={`mx-auto flex w-full ${maxWidthClass} flex-col ${gap} ${padding}`}
        >
          {children}
        </div>
      </div>
    </div>
  )
}
```

- [ ] **Step 2：lint + 类型**

```bash
pnpm lint
pnpm exec tsc --noEmit
```

Expected: 0 error（如果 HomePage 等老调用点崩，让它们继续传旧 `header` prop 时报错；下个 plan-B 会重写它们）。

如果旧调用点用了 `header={...}`，临时把 `topBar` 也接受 `header` 别名以保持 API 不破：
```ts
interface PageSectionShellProps extends PropsWithChildren {
  topBar?: ReactNode
  /** @deprecated alias of topBar, removed in plan-B */
  header?: ReactNode
  ...
}
```
并在函数体顶部 `const top = topBar ?? header`。

- [ ] **Step 3：commit**

```bash
git add src/components/shell/PageSectionShell.tsx
git commit -m "refactor(frontend): PageSectionShell exposes padding/gap as props"
```

---

## Task A-3：阶段 A 验收

**Files:** 无新增

- [ ] **Step 1：跑完整测试套件**

```bash
pnpm test
```

Expected: 全 PASS（含本阶段新增的 token/skin/sidebar/shell 测试以及现有 chat/auth 集成测试）。

- [ ] **Step 2：跑 lint + 类型**

```bash
pnpm lint
pnpm exec tsc --noEmit
```

Expected: 0 error。

- [ ] **Step 3：开发模式启动并目视确认侧栏与首页 TopBar 已呈现暖金色**

```bash
pnpm tauri:dev
```

打开应用，**目视**确认：
- 侧栏底色为 `#F4F0E6` 暖奶色（非以前的 `#FBF6E6`）；
- 当前路由对应 nav 项底色为 `#E1DAC6`；
- 侧栏宽度为 256px。

确认后退出 dev。本步只做目视，不做 PNG diff（PNG 对照在 plan-E 统一做）。

- [ ] **Step 4：阶段总结 commit（无代码）**

```bash
git commit --allow-empty -m "chore(frontend): plan-A milestone — tokens slimmed, AppShell rebuilt"
```

---

## 自审

**Spec coverage：** 本 plan 覆盖 spec 的：
- 第 3 章 Token 映射与皮肤策略（A-1.1 / A-1.2 / A-1.3）✓
- 第 5.1 章 Shell 组件清单（A-2.1 ~ A-2.8）✓
- 第 9.1 章 基线 PNG 入库（A-0）✓
- 第 4.3 章 `@designSource` JSDoc 标注（每个新组件文件已加）✓

未覆盖（在后续 plan 处理）：第 4.2 章页面层禁止清单（plan-E 中以 review checklist 形式落地，eslint 规则可后续按需加）；第 5.2-5.7 章组件清单（plan-B/C/D 处理）；第 6 章交互改造（plan-D 处理）；第 9.2 章 30 个检查点（plan-E 统一执行）。

**Placeholder scan：** 已扫；无 TBD/TODO/"add appropriate error handling"。

**Type consistency：** `SidebarNavKey` 在 SidebarNav 与 AppSidebar 同名同形；`ConversationTreeProject/ConversationTreeItem` 在 ConversationTree 导出，被 conversationProjects 直接 import；`PageTopBarVariant` 4 个值贯穿测试与组件实现。`DERIVED_SKIN_KEYS` 长度从 7 调整为 5，brandingStore 与 skin 测试均已对齐。

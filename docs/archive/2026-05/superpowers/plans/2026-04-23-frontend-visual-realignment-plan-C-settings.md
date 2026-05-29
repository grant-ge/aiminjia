# 前端视觉重构 · plan-C：Settings Modal 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把当前粗糙的 `SettingsModal` 重构为 design.pen 中"980×680/760 居中弹窗 + 左 220 menu + 右 content"的 7 项菜单设置面板，账户/关于/用量三面板补齐内容，其余 4 项做"即将上线"占位。

**Architecture:** 抽出 `SettingsShell` 作为容器（提供遮罩、modal 大小、阴影 lvl-3、左右两列 grid），抽出 `SettingsMenu` 作为受控菜单，抽出 `SettingsContentTop / SettingsContentBody` 作为右侧内容外壳。每个 tab 一个独立 panel 文件，便于扩展。`uiStore.SettingsModalState` 扩展为 7 个 key，保持 deprecated key 兼容旧调用。

**Tech Stack:** 同 plan-A/B。继续依赖 plan-A 的 token 体系，依赖 shadcn `Dialog` 作为遮罩与 a11y 焦点管理底座（视觉用 `SettingsShell` 完全覆盖）。

**对应 spec：** `docs/superpowers/specs/2026-04-23-frontend-visual-realignment-to-design-pen.md` 第 5.5、6.3、7.7 章。

**前置：** plan-A、plan-B 全部任务完成。分支 `pzc`。

---

## 文件结构

### 新建

| 路径 | 责任 |
|---|---|
| `src/components/settings/SettingsShell.tsx` | Modal 外壳：遮罩 + 980 宽 + 圆角 18 + lvl-3 阴影 + 左右两列 grid |
| `src/components/settings/SettingsMenu.tsx` | 左侧 220 宽菜单（受控 active） |
| `src/components/settings/SettingsContentTop.tsx` | 右侧 56 高 title + × 关闭 |
| `src/components/settings/SettingsContentBody.tsx` | 右侧 padding [24,32] gap 24 容器 |
| `src/components/settings/panels/AccountPanel.tsx` | 账户卡 + 退出登录 + 通知说明 |
| `src/components/settings/panels/AboutPanel.tsx` | appCard + helpSec + devSec |
| `src/components/settings/panels/UsagePanel.tsx` | planCard + quotaSec + detailSec |
| `src/components/settings/panels/PlaceholderPanel.tsx` | "即将上线" 通用占位面板（系统权限 / MCP 服务 / SSO 集成 / 快捷键 共用） |
| `src/components/settings/__tests__/SettingsShell.test.tsx` | 渲染遮罩、宽高、关闭事件 |
| `src/components/settings/__tests__/SettingsMenu.test.tsx` | 7 项菜单 + active 高亮 + 选中回调 |
| `src/components/settings/__tests__/AccountPanel.test.tsx` | 账户卡 + 退出按钮触发 |

### 修改

| 路径 | 修改内容 |
|---|---|
| `src/stores/uiStore.ts` | `SettingsModalState` 扩展为 7 keys（'account'/'usage'/'permissions'/'mcp'/'sso'/'shortcuts'/'about'）+ 兼容 `'general'` 旧值映射到 `'permissions'` |
| `src/components/settings/SettingsModal.tsx` | 完全重写：用 SettingsShell + SettingsMenu + 7 panel 路由 |
| `src/components/sidebar/SidebarFooterSettings.tsx` | onClick 默认打开 'account'（已是默认值，无需改动；plan-A 已完成） |
| `src/components/settings/McpTab.tsx` 等已有 tab 文件 | 暂保留，不在本 plan 内重构；新版 SettingsModal 仅展示占位面板，待后续 plan 接入 |

---

## Task C-1：扩展 `uiStore.SettingsModalState` 为 7 keys

**Files:**
- Modify: `src/stores/uiStore.ts`
- Modify: `src/stores/settingsStore.test.ts`（若该测试覆盖了 settingsModal）

- [ ] **Step 1：写失败测试**

新增 `src/stores/__tests__/uiStore.settingsModal.test.ts`：

```ts
import { beforeEach, describe, expect, it } from 'vitest'

import { useUiStore } from '../uiStore'

describe('uiStore.settingsModal', () => {
  beforeEach(() => {
    useUiStore.getState().closeSettings()
  })

  it('accepts all 7 plan-C keys', () => {
    const keys = [
      'account',
      'usage',
      'permissions',
      'mcp',
      'sso',
      'shortcuts',
      'about',
    ] as const
    for (const k of keys) {
      useUiStore.getState().openSettings(k)
      expect(useUiStore.getState().settingsModal).toBe(k)
    }
  })

  it('maps deprecated "general" to "permissions"', () => {
    useUiStore.getState().openSettings('general' as never)
    expect(useUiStore.getState().settingsModal).toBe('permissions')
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/stores/__tests__/uiStore.settingsModal.test.ts
```

Expected: FAIL（旧 type 只允许 4 个 key）。

- [ ] **Step 3：修改 `uiStore.ts`**

在 `src/stores/uiStore.ts` 中：

```ts
// 找到这一行：
// export type SettingsModalState = null | 'account' | 'general' | 'about' | 'usage'
// 替换为：
export type SettingsModalKey =
  | 'account'
  | 'usage'
  | 'permissions'
  | 'mcp'
  | 'sso'
  | 'shortcuts'
  | 'about'

export type SettingsModalState = null | SettingsModalKey

// 在 openSettings 内部加 deprecation 兼容：
function normalizeSettingsKey(input: string): SettingsModalKey {
  if (input === 'general') return 'permissions'
  return input as SettingsModalKey
}

// 修改 openSettings：
//   openSettings: (key) => set({ settingsModal: normalizeSettingsKey(key as string) }),
```

- [ ] **Step 4：测试通过 + tsc**

```bash
pnpm exec vitest run src/stores/__tests__/uiStore.settingsModal.test.ts
pnpm exec tsc --noEmit
```

Expected: PASS / 0 error。tsc 错若来自旧 `SettingsModal.tsx` 直接 `as const` 写 `'general'`，留待 Task C-3 一并修复。

- [ ] **Step 5：commit**

```bash
git add src/stores/uiStore.ts src/stores/__tests__/uiStore.settingsModal.test.ts
git commit -m "feat(frontend): expand SettingsModalKey to 7 panels"
```

---

## Task C-2：SettingsShell + SettingsMenu + 内容外壳

**Files:**
- Create: `src/components/settings/SettingsShell.tsx`
- Create: `src/components/settings/SettingsMenu.tsx`
- Create: `src/components/settings/SettingsContentTop.tsx`
- Create: `src/components/settings/SettingsContentBody.tsx`
- Create: `src/components/settings/__tests__/SettingsShell.test.tsx`
- Create: `src/components/settings/__tests__/SettingsMenu.test.tsx`

- [ ] **Step 1：写失败测试 SettingsShell**

```tsx
// src/components/settings/__tests__/SettingsShell.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SettingsShell } from '../SettingsShell'

describe('SettingsShell', () => {
  it('renders menu and content slots', () => {
    render(
      <SettingsShell
        open
        menu={<div>menu-slot</div>}
        content={<div>content-slot</div>}
        onClose={() => {}}
      />,
    )
    expect(screen.getByText('menu-slot')).toBeInTheDocument()
    expect(screen.getByText('content-slot')).toBeInTheDocument()
  })

  it('does not render when open=false', () => {
    render(
      <SettingsShell
        open={false}
        menu={<div>m</div>}
        content={<div>c</div>}
        onClose={() => {}}
      />,
    )
    expect(screen.queryByText('m')).toBeNull()
  })

  it('clicking the overlay invokes onClose', () => {
    const onClose = vi.fn()
    render(
      <SettingsShell
        open
        menu={<div>m</div>}
        content={<div>c</div>}
        onClose={onClose}
      />,
    )
    fireEvent.click(screen.getByTestId('settings-overlay'))
    expect(onClose).toHaveBeenCalled()
  })

  it('modal box uses width 980 with rounded-[18px]', () => {
    const { container } = render(
      <SettingsShell open menu={<div />} content={<div />} onClose={() => {}} />,
    )
    const modal = container.querySelector('[data-testid="settings-modal-box"]')
    expect(modal?.className).toMatch(/w-\[980px\]/)
    expect(modal?.className).toMatch(/rounded-\[18px\]/)
  })
})
```

- [ ] **Step 2：写失败测试 SettingsMenu**

```tsx
// src/components/settings/__tests__/SettingsMenu.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SettingsMenu, SETTINGS_MENU_ITEMS } from '../SettingsMenu'

describe('SettingsMenu', () => {
  it('renders all 7 menu items', () => {
    render(<SettingsMenu activeKey="account" onSelect={() => {}} />)
    for (const it of SETTINGS_MENU_ITEMS) {
      expect(screen.getByRole('button', { name: it.label })).toBeInTheDocument()
    }
  })

  it('marks active item with bg-card class', () => {
    render(<SettingsMenu activeKey="usage" onSelect={() => {}} />)
    const active = screen.getByRole('button', { name: '用量' })
    expect(active.className).toMatch(/bg-card/)
  })

  it('fires onSelect with key', () => {
    const onSelect = vi.fn()
    render(<SettingsMenu activeKey="account" onSelect={onSelect} />)
    fireEvent.click(screen.getByRole('button', { name: 'MCP 服务' }))
    expect(onSelect).toHaveBeenCalledWith('mcp')
  })
})
```

- [ ] **Step 3：运行确认失败**

```bash
pnpm exec vitest run src/components/settings/__tests__/SettingsShell.test.tsx src/components/settings/__tests__/SettingsMenu.test.tsx
```

Expected: FAIL。

- [ ] **Step 4：实现四个组件**

```tsx
// src/components/settings/SettingsShell.tsx
/**
 * @designSource design.pen#giMe2/kFHCj/vHMr4
 * @sizing 980 × auto, r-18, shadow lvl-3; overlay #0000004d
 */
import type { ReactNode } from 'react'

interface SettingsShellProps {
  open: boolean
  menu: ReactNode
  content: ReactNode
  onClose: () => void
  /** content height; design uses 680 for account, 760 for about/usage */
  height?: number
}

export function SettingsShell({
  open,
  menu,
  content,
  onClose,
  height = 680,
}: SettingsShellProps) {
  if (!open) return null
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div
        data-testid="settings-overlay"
        className="absolute inset-0 bg-black/30"
        onClick={onClose}
      />
      <div
        data-testid="settings-modal-box"
        className="relative z-10 grid w-[980px] grid-cols-[220px_1fr] overflow-hidden rounded-[18px] border border-border bg-card"
        style={{
          height,
          boxShadow:
            '0 20px 20px rgba(0,0,0,0.10), 0 10px 10px rgba(0,0,0,0.04)',
        }}
      >
        {menu}
        {content}
      </div>
    </div>
  )
}
```

```tsx
// src/components/settings/SettingsMenu.tsx
/**
 * @designSource design.pen#YboA7/Z9asD/r95Aa
 * @sizing 220 width, bg secondary, top-left radius 18; row r-10 padding [10,12]
 */
import type { SettingsModalKey } from '@/stores/uiStore'

export interface SettingsMenuItem {
  key: SettingsModalKey
  label: string
}

export const SETTINGS_MENU_ITEMS: SettingsMenuItem[] = [
  { key: 'account', label: '账户' },
  { key: 'usage', label: '用量' },
  { key: 'permissions', label: '系统权限' },
  { key: 'mcp', label: 'MCP 服务' },
  { key: 'sso', label: 'SSO 集成' },
  { key: 'shortcuts', label: '快捷键' },
  { key: 'about', label: '关于 AI 小家' },
]

interface SettingsMenuProps {
  activeKey: SettingsModalKey
  onSelect: (key: SettingsModalKey) => void
}

export function SettingsMenu({ activeKey, onSelect }: SettingsMenuProps) {
  return (
    <aside className="flex flex-col gap-1 rounded-l-[18px] bg-secondary px-4 py-6">
      <div className="mb-2 text-lg font-bold text-foreground">设置</div>
      {SETTINGS_MENU_ITEMS.map((it) => {
        const active = it.key === activeKey
        return (
          <button
            key={it.key}
            type="button"
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'flex items-center rounded-[10px] bg-card px-3 py-2.5 text-left text-sm font-semibold text-foreground'
                : 'flex items-center rounded-[10px] px-3 py-2.5 text-left text-sm font-medium text-muted-foreground transition-colors hover:bg-card/60'
            }
          >
            {it.label}
          </button>
        )
      })}
    </aside>
  )
}
```

```tsx
// src/components/settings/SettingsContentTop.tsx
/**
 * @designSource design.pen#5aczK/dQk75/YuBIQ
 * @sizing h 56, padding [0,28], bottom-border 1
 */
import { X } from 'lucide-react'

interface SettingsContentTopProps {
  title: string
  onClose: () => void
}

export function SettingsContentTop({ title, onClose }: SettingsContentTopProps) {
  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b border-border px-7">
      <div className="text-base font-bold text-foreground">{title}</div>
      <button
        type="button"
        aria-label="关闭"
        onClick={onClose}
        className="text-muted-foreground transition-colors hover:text-foreground"
      >
        <X className="h-4 w-4" />
      </button>
    </header>
  )
}
```

```tsx
// src/components/settings/SettingsContentBody.tsx
/**
 * @designSource design.pen#7wrps/fRV7f/0M01f
 * @sizing padding [24,32] gap 24
 */
import type { PropsWithChildren } from 'react'

export function SettingsContentBody({ children }: PropsWithChildren) {
  return (
    <div className="flex flex-1 flex-col gap-6 overflow-auto px-8 py-6">
      {children}
    </div>
  )
}
```

- [ ] **Step 5：测试通过**

```bash
pnpm exec vitest run src/components/settings/__tests__/SettingsShell.test.tsx src/components/settings/__tests__/SettingsMenu.test.tsx
```

Expected: PASS。

- [ ] **Step 6：commit**

```bash
git add src/components/settings/SettingsShell.tsx src/components/settings/SettingsMenu.tsx src/components/settings/SettingsContentTop.tsx src/components/settings/SettingsContentBody.tsx src/components/settings/__tests__/SettingsShell.test.tsx src/components/settings/__tests__/SettingsMenu.test.tsx
git commit -m "feat(frontend): add SettingsShell/Menu/Top/Body composite parts"
```

---

## Task C-3：四个 panel — Account / About / Usage / Placeholder

**Files:**
- Create: `src/components/settings/panels/AccountPanel.tsx`
- Create: `src/components/settings/panels/AboutPanel.tsx`
- Create: `src/components/settings/panels/UsagePanel.tsx`
- Create: `src/components/settings/panels/PlaceholderPanel.tsx`
- Create: `src/components/settings/__tests__/AccountPanel.test.tsx`

- [ ] **Step 1：写 AccountPanel 失败测试**

```tsx
// src/components/settings/__tests__/AccountPanel.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { AccountPanel } from '../panels/AccountPanel'

describe('AccountPanel', () => {
  it('renders user info card and notice', () => {
    render(
      <AccountPanel
        user={{ name: '姚域权', tenantName: '仁励家网络科技(杭州)有限公司', avatarUrl: '/x.png' }}
        onLogout={() => {}}
      />,
    )
    expect(screen.getByText('姚域权')).toBeInTheDocument()
    expect(screen.getByText(/仁励家网络科技/)).toBeInTheDocument()
    expect(screen.getByText(/账户信息以企业 SSO/)).toBeInTheDocument()
  })

  it('fires onLogout when 退出登录 clicked', () => {
    const onLogout = vi.fn()
    render(
      <AccountPanel
        user={{ name: 'X', tenantName: 'Y', avatarUrl: '' }}
        onLogout={onLogout}
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: '退出登录' }))
    expect(onLogout).toHaveBeenCalled()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/settings/__tests__/AccountPanel.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现四个 panel**

```tsx
// src/components/settings/panels/AccountPanel.tsx
/**
 * @designSource design.pen#IIzfj acctCard + nKzUU notice
 * @sizing acctCard r-14 padding 18; notice r-14 padding 24
 */
import { Info } from 'lucide-react'

import { Button } from '@/components/ui/button'

interface AccountPanelProps {
  user: { name: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
}

export function AccountPanel({ user, onLogout }: AccountPanelProps) {
  return (
    <>
      <div className="flex items-center gap-3.5 rounded-[14px] bg-secondary p-[18px]">
        <div className="h-12 w-12 shrink-0 overflow-hidden rounded-full bg-muted">
          {user.avatarUrl ? (
            <img src={user.avatarUrl} alt="" className="h-full w-full object-cover" />
          ) : null}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="text-sm font-bold text-foreground">{user.name}</div>
          <div className="truncate text-[13px] text-muted-foreground">{user.tenantName}</div>
        </div>
        <Button variant="outline" onClick={onLogout}>
          退出登录
        </Button>
      </div>
      <div className="flex flex-col items-center gap-1.5 rounded-[14px] bg-secondary px-6 py-6 text-center">
        <Info className="h-4 w-4 text-muted-foreground" />
        <div className="text-[13px] text-muted-foreground">
          账户信息以企业 SSO / 登录账号为准，如需更换请退出后重新登录。
        </div>
      </div>
    </>
  )
}
```

```tsx
// src/components/settings/panels/AboutPanel.tsx
/**
 * @designSource design.pen#MQLyd appCard + 7s18f helpSec + lcRrf devSec
 * @sizing appCard r-14 padding 20; helpSec/devSec gap 16
 */
import { ArrowRight } from 'lucide-react'

interface AboutPanelProps {
  appName: string
  version: string
  tenantName: string
  helpLinks: { label: string; onClick: () => void }[]
  devInfo: { label: string; value: string }[]
}

export function AboutPanel({
  appName,
  version,
  tenantName,
  helpLinks,
  devInfo,
}: AboutPanelProps) {
  return (
    <>
      <div className="flex items-center justify-between gap-4 rounded-[14px] bg-secondary p-5">
        <div className="flex flex-col gap-1">
          <div className="text-base font-bold text-foreground">{appName}</div>
          <div className="text-[13px] text-muted-foreground">v{version} · {tenantName}</div>
        </div>
      </div>
      <section className="flex flex-col gap-4">
        <div className="text-sm font-semibold text-foreground">帮助与支持</div>
        <div className="flex flex-col gap-2">
          {helpLinks.map((l) => (
            <button
              key={l.label}
              type="button"
              onClick={l.onClick}
              className="flex items-center justify-between rounded-md px-3 py-2 text-sm text-foreground transition-colors hover:bg-muted"
            >
              <span>{l.label}</span>
              <ArrowRight className="h-3.5 w-3.5 text-muted-foreground" />
            </button>
          ))}
        </div>
      </section>
      <section className="flex flex-col gap-4">
        <div className="text-sm font-semibold text-foreground">开发者信息</div>
        <dl className="grid grid-cols-2 gap-y-2 text-[13px]">
          {devInfo.map((d) => (
            <div key={d.label} className="contents">
              <dt className="text-muted-foreground">{d.label}</dt>
              <dd className="text-foreground">{d.value}</dd>
            </div>
          ))}
        </dl>
      </section>
    </>
  )
}
```

```tsx
// src/components/settings/panels/UsagePanel.tsx
/**
 * @designSource design.pen#mbeKY planCard + BtLe0 quotaSec + SAOik detailSec
 * @sizing planCard padding [16,0] bottom-border 1; quota gap 18; detail gap 16
 */
interface QuotaItem {
  label: string
  used: number
  total: number
}

interface DetailRow {
  label: string
  value: string
}

interface UsagePanelProps {
  planName: string
  planRenewLabel: string
  quota: QuotaItem[]
  detail: DetailRow[]
}

export function UsagePanel({ planName, planRenewLabel, quota, detail }: UsagePanelProps) {
  return (
    <>
      <div className="flex items-center justify-between border-b border-border py-4">
        <div className="flex flex-col gap-1">
          <div className="text-sm font-semibold text-foreground">{planName}</div>
          <div className="text-[13px] text-muted-foreground">{planRenewLabel}</div>
        </div>
      </div>
      <section className="flex flex-col gap-[18px]">
        {quota.map((q) => {
          const pct = Math.min(100, Math.round((q.used / Math.max(1, q.total)) * 100))
          return (
            <div key={q.label} className="flex flex-col gap-2">
              <div className="flex items-center justify-between text-[13px] text-foreground">
                <span>{q.label}</span>
                <span className="text-muted-foreground">
                  {q.used} / {q.total}
                </span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full bg-primary"
                  style={{ width: `${pct}%` }}
                />
              </div>
            </div>
          )
        })}
      </section>
      <section className="flex flex-col gap-4">
        <div className="text-sm font-semibold text-foreground">用量明细</div>
        <dl className="grid grid-cols-2 gap-y-2 text-[13px]">
          {detail.map((d) => (
            <div key={d.label} className="contents">
              <dt className="text-muted-foreground">{d.label}</dt>
              <dd className="text-foreground">{d.value}</dd>
            </div>
          ))}
        </dl>
      </section>
    </>
  )
}
```

```tsx
// src/components/settings/panels/PlaceholderPanel.tsx
/**
 * 通用 "即将上线" 占位面板，复用于系统权限 / MCP 服务 / SSO 集成 / 快捷键
 */
import { Sparkles } from 'lucide-react'

interface PlaceholderPanelProps {
  title: string
}

export function PlaceholderPanel({ title }: PlaceholderPanelProps) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 rounded-[14px] bg-secondary py-12 text-center">
      <Sparkles className="h-5 w-5 text-muted-foreground" />
      <div className="text-sm font-semibold text-foreground">{title} · 即将上线</div>
      <div className="max-w-[420px] text-[13px] text-muted-foreground">
        当前版本暂未提供该模块的可视化配置入口，下个迭代会按 design.pen 完整接入。
      </div>
    </div>
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/settings/__tests__/AccountPanel.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/settings/panels src/components/settings/__tests__/AccountPanel.test.tsx
git commit -m "feat(frontend): add Settings panels (Account/About/Usage/Placeholder)"
```

---

## Task C-4：重写 `SettingsModal` 拼装 + lint/tsc 修复旧调用点

**Files:**
- Modify: `src/components/settings/SettingsModal.tsx`
- Create: `src/components/settings/__tests__/SettingsModal.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/settings/__tests__/SettingsModal.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: any) =>
    sel({
      user: { name: '姚域权', username: 'yyq' },
      tenant: { name: '仁励家网络科技(杭州)有限公司' },
      logout: vi.fn().mockResolvedValue(undefined),
    }),
}))

import { useUiStore } from '@/stores/uiStore'
import { SettingsModal } from '../SettingsModal'

describe('SettingsModal', () => {
  beforeEach(() => useUiStore.getState().closeSettings())

  it('renders nothing when closed', () => {
    const { container } = render(<SettingsModal />)
    expect(container.firstChild).toBeNull()
  })

  it('renders 7 menu items + content title when account opened', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)
    expect(screen.getByRole('button', { name: '账户' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '关于 AI 小家' })).toBeInTheDocument()
    expect(screen.getByText('姚域权')).toBeInTheDocument()
  })

  it('switching menu changes the right panel', () => {
    useUiStore.getState().openSettings('account')
    render(<SettingsModal />)
    fireEvent.click(screen.getByRole('button', { name: 'MCP 服务' }))
    expect(screen.getByText(/MCP 服务 · 即将上线/)).toBeInTheDocument()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/settings/__tests__/SettingsModal.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：完全替换 `SettingsModal.tsx`**

```tsx
/**
 * @designSource design.pen#S3D6p / 1MCFZ / az6ZY
 * 由 SettingsShell + SettingsMenu + 7 panel 组成。
 */
import { useState } from 'react'

import { useAuthStore } from '@/stores/authStore'
import { useUiStore, type SettingsModalKey } from '@/stores/uiStore'
import { useBrandingStore } from '@/stores/brandingStore'

import { SettingsContentBody } from './SettingsContentBody'
import { SettingsContentTop } from './SettingsContentTop'
import { SettingsMenu, SETTINGS_MENU_ITEMS } from './SettingsMenu'
import { SettingsShell } from './SettingsShell'
import { AboutPanel } from './panels/AboutPanel'
import { AccountPanel } from './panels/AccountPanel'
import { PlaceholderPanel } from './panels/PlaceholderPanel'
import { UsagePanel } from './panels/UsagePanel'

const PANEL_HEIGHT: Partial<Record<SettingsModalKey, number>> = {
  account: 680,
  about: 760,
  usage: 760,
}

export function SettingsModal() {
  const settingsModal = useUiStore((s) => s.settingsModal)
  const closeSettings = useUiStore((s) => s.closeSettings)
  const openSettings = useUiStore((s) => s.openSettings)
  const user = useAuthStore((s) => s.user)
  const tenant = useAuthStore((s) => s.tenant)
  const logout = useAuthStore((s) => s.logout)
  const productName = useBrandingStore((s) => s.productName)
  const [pendingLogout, setPendingLogout] = useState(false)

  if (!settingsModal) return null

  const activeLabel =
    SETTINGS_MENU_ITEMS.find((m) => m.key === settingsModal)?.label || '设置'

  const onLogout = async () => {
    if (pendingLogout) return
    setPendingLogout(true)
    try {
      await logout()
      closeSettings()
    } finally {
      setPendingLogout(false)
    }
  }

  return (
    <SettingsShell
      open
      onClose={closeSettings}
      height={PANEL_HEIGHT[settingsModal] || 720}
      menu={
        <SettingsMenu
          activeKey={settingsModal}
          onSelect={(k) => openSettings(k)}
        />
      }
      content={
        <div className="flex min-w-0 flex-1 flex-col">
          <SettingsContentTop title={activeLabel} onClose={closeSettings} />
          <SettingsContentBody>
            {settingsModal === 'account' ? (
              <AccountPanel
                user={{
                  name: user?.name ?? user?.username ?? '未登录',
                  tenantName: tenant?.name ?? '',
                  avatarUrl: '',
                }}
                onLogout={() => void onLogout()}
              />
            ) : null}
            {settingsModal === 'about' ? (
              <AboutPanel
                appName={productName}
                version="0.9.30"
                tenantName="仁励家网络科技(杭州)有限公司"
                helpLinks={[
                  { label: '使用手册', onClick: () => {} },
                  { label: '反馈问题', onClick: () => {} },
                ]}
                devInfo={[
                  { label: '架构', value: 'Tauri 2.x · React' },
                  { label: '更新通道', value: '稳定版' },
                ]}
              />
            ) : null}
            {settingsModal === 'usage' ? (
              <UsagePanel
                planName="标准版"
                planRenewLabel="按企业账号自动续期"
                quota={[
                  { label: '会话次数', used: 142, total: 500 },
                  { label: '模型调用 tokens', used: 234_000, total: 1_000_000 },
                ]}
                detail={[
                  { label: '本月会话', value: '142 次' },
                  { label: '本月技能调用', value: '38 次' },
                ]}
              />
            ) : null}
            {settingsModal === 'permissions' ? (
              <PlaceholderPanel title="系统权限" />
            ) : null}
            {settingsModal === 'mcp' ? <PlaceholderPanel title="MCP 服务" /> : null}
            {settingsModal === 'sso' ? <PlaceholderPanel title="SSO 集成" /> : null}
            {settingsModal === 'shortcuts' ? (
              <PlaceholderPanel title="快捷键" />
            ) : null}
          </SettingsContentBody>
        </div>
      }
    />
  )
}
```

- [ ] **Step 4：lint/tsc + 跑测试**

```bash
pnpm exec tsc --noEmit
pnpm exec vitest run src/components/settings src/stores/__tests__/uiStore.settingsModal.test.ts
pnpm lint
```

Expected: 0 error / PASS。

如果 tsc 报旧测试 / 旧调用点用了 `'general'` 字面量，把它们改为 `'permissions'`（因为 `openSettings('general')` 会被 store 自动归一化，但 type 不再接受 `'general'`，需要改字面量；`as 'general'` 强转保留兼容性）。

- [ ] **Step 5：commit**

```bash
git add src/components/settings/SettingsModal.tsx src/components/settings/__tests__/SettingsModal.test.tsx
git commit -m "refactor(frontend): rebuild SettingsModal with shell/menu/7 panels"
```

---

## Task C-Final：阶段 C 验收

- [ ] **Step 1：跑全部测试 + lint + tsc**

```bash
pnpm test
pnpm lint
pnpm exec tsc --noEmit
```

Expected: 全 PASS / 0 error。

- [ ] **Step 2：dev 启动目视确认**

```bash
pnpm tauri:dev
```

侧栏点击"设置"，目视确认：
- 弹窗居中、980 宽、圆角 18 + 大阴影 + 半透明黑遮罩；
- 左侧 220 menu 显示 7 项，"账户" 默认激活白底，其余金卡 secondary 底；
- 右侧顶 56 高 title + × 关闭；
- 切换"用量"看到 plan 卡 + 进度条；
- 切换"关于 AI 小家"看到 appCard + 帮助 + 开发者信息；
- 切换"系统权限/MCP 服务/SSO 集成/快捷键"看到 placeholder。

- [ ] **Step 3：阶段 commit**

```bash
git commit --allow-empty -m "chore(frontend): plan-C milestone — settings modal aligned to design.pen"
```

---

## 自审

**Spec coverage：** 第 5.5 章组件清单（SettingsShell/Menu/Top/Body + Account/About/Usage + Placeholder）✓；第 6.3 章 7 个 menu key 接入 ✓；第 7.7 章 modal 三态拼装 ✓。

**Placeholder scan：** 已扫，无 TBD。

**Type consistency：** `SettingsModalKey` 在 store / Menu / Modal 三处一致；`SETTINGS_MENU_ITEMS` 类型导出确保 Menu 与 Modal 共享枚举；`PANEL_HEIGHT` partial map 落在 modal 拼装层，提供 fallback 720。

# General Settings Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the "账户" tab to "通用", expand `AccountPanel` with 通用 (language, disabled autostart, disabled prevent-sleep) and 外观 (accent color picker) sections.

**Architecture:** All changes are frontend-only. `AccountPanel` becomes `GeneralPanel` (rename component + file). The accent color picker calls `useBrandingStore.applyBranding()` with the chosen color. Language calls `useSettingsStore.setAppLanguage()`. Autostart and prevent-sleep render as disabled toggles with no wiring. Menu label and test assertions update to match.

**Tech Stack:** React/TypeScript, Zustand (`brandingStore`, `settingsStore`), Vitest + Testing Library, Tailwind CSS

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Rename + rewrite | `src/components/settings/panels/AccountPanel.tsx` → `GeneralPanel.tsx` | New panel with all sections |
| Modify | `src/components/settings/SettingsMenu.tsx` | Change label `'账户'` → `'通用'` |
| Modify | `src/components/settings/SettingsModal.tsx` | Import `GeneralPanel`, pass correct props |
| Rename + update | `src/components/settings/__tests__/AccountPanel.test.tsx` → `GeneralPanel.test.tsx` | Tests for new panel sections |
| Modify | `src/components/settings/__tests__/SettingsModal.test.tsx` | Update menu label assertion |

---

### Task 1: Rename AccountPanel → GeneralPanel and update test file name

**Files:**
- Delete: `src/components/settings/panels/AccountPanel.tsx`
- Create: `src/components/settings/panels/GeneralPanel.tsx`
- Delete: `src/components/settings/__tests__/AccountPanel.test.tsx`
- Create: `src/components/settings/__tests__/GeneralPanel.test.tsx`

- [ ] **Step 1: Write failing tests for GeneralPanel in the new test file**

Create `src/components/settings/__tests__/GeneralPanel.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'

import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { GeneralPanel } from '../panels/GeneralPanel'

const mockUser = { name: '姚域权', tenantName: '仁励家网络科技(杭州)有限公司', avatarUrl: '' }

describe('GeneralPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders user info card with name, tenant, and logout button', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('姚域权')).toBeInTheDocument()
    expect(screen.getByText(/仁励家网络科技/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '退出登录' })).toBeInTheDocument()
  })

  it('fires onLogout when logout button clicked', () => {
    const onLogout = vi.fn()
    render(<GeneralPanel user={mockUser} onLogout={onLogout} />)
    fireEvent.click(screen.getByRole('button', { name: '退出登录' }))
    expect(onLogout).toHaveBeenCalledTimes(1)
  })

  it('renders 通用 section with language selector', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('通用')).toBeInTheDocument()
    expect(screen.getByText('语言')).toBeInTheDocument()
    expect(screen.getByRole('combobox', { name: '语言' })).toBeInTheDocument()
  })

  it('renders disabled autostart and prevent-sleep toggles', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('开机自启动')).toBeInTheDocument()
    expect(screen.getByText('任务运行时阻止自动休眠')).toBeInTheDocument()
    const toggles = screen.getAllByRole('switch')
    expect(toggles).toHaveLength(2)
    toggles.forEach((t) => expect(t).toBeDisabled())
  })

  it('renders 外观 section with accent color swatches', () => {
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    expect(screen.getByText('外观')).toBeInTheDocument()
    expect(screen.getByText('强调色')).toBeInTheDocument()
    const swatches = screen.getAllByRole('radio')
    expect(swatches.length).toBeGreaterThanOrEqual(5)
  })

  it('selecting an accent color swatch calls applyBranding with new color', () => {
    const applyBranding = vi.fn()
    useBrandingStore.setState({ accentColor: '#DBAA22', applyBranding } as never)
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    // Click the indigo swatch (#4f46e5)
    fireEvent.click(screen.getByRole('radio', { name: '#4f46e5' }))
    expect(applyBranding).toHaveBeenCalledWith({ accentColor: '#4f46e5' })
  })

  it('changing language select calls setAppLanguage', () => {
    const setAppLanguage = vi.fn()
    useSettingsStore.setState({ setAppLanguage } as never)
    render(<GeneralPanel user={mockUser} onLogout={() => {}} />)
    fireEvent.change(screen.getByRole('combobox', { name: '语言' }), { target: { value: 'en-US' } })
    expect(setAppLanguage).toHaveBeenCalledWith('en-US')
  })
})
```

- [ ] **Step 2: Run test to verify it fails (GeneralPanel not found)**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec vitest run src/components/settings/__tests__/GeneralPanel.test.tsx 2>&1 | tail -20
```

Expected: FAIL — `Cannot find module '../panels/GeneralPanel'`

- [ ] **Step 3: Create GeneralPanel.tsx**

Create `src/components/settings/panels/GeneralPanel.tsx`:

```tsx
import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import type { AppLanguage } from '@/i18n'
import { Button } from '@/components/ui/button'

const ACCENT_PRESETS = [
  '#DBAA22',
  '#4f46e5',
  '#0ea5e9',
  '#10b981',
  '#f43f5e',
  '#8b5cf6',
  '#f97316',
]

interface GeneralPanelProps {
  user: { name: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
}

export function GeneralPanel({ user, onLogout }: GeneralPanelProps) {
  const accentColor = useBrandingStore((s) => s.accentColor)
  const applyBranding = useBrandingStore((s) => s.applyBranding)
  const appLanguage = useSettingsStore((s) => s.appLanguage)
  const setAppLanguage = useSettingsStore((s) => s.setAppLanguage)

  return (
    <div className="flex flex-col gap-6">
      {/* 用户信息卡 */}
      <div className="flex items-center gap-3.5 rounded-[14px] bg-secondary p-[18px]">
        <div className="h-12 w-12 shrink-0 overflow-hidden rounded-full bg-primary">
          {user.avatarUrl ? (
            <img src={user.avatarUrl} alt="" className="h-full w-full object-cover" />
          ) : (
            <span className="flex h-full w-full items-center justify-center text-lg font-semibold text-primary-foreground">
              {user.name.charAt(0).toUpperCase()}
            </span>
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="text-sm font-bold text-foreground">{user.name}</div>
          <div className="truncate text-[13px] text-muted-foreground">{user.tenantName}</div>
        </div>
        <Button variant="outline" onClick={onLogout}>
          退出登录
        </Button>
      </div>

      {/* 通用分组 */}
      <div className="flex flex-col gap-2">
        <div className="text-sm font-semibold text-foreground">通用</div>
        <div className="divide-y divide-border rounded-[14px] border border-border bg-card">
          {/* 语言 */}
          <div className="flex items-center justify-between px-4 py-3.5">
            <div className="flex flex-col gap-0.5">
              <span className="text-sm font-medium text-foreground">语言</span>
              <span className="text-xs text-muted-foreground">选择应用界面显示的语言</span>
            </div>
            <select
              aria-label="语言"
              value={appLanguage ?? 'zh-CN'}
              onChange={(e) => setAppLanguage(e.target.value as AppLanguage)}
              className="rounded-md border border-border bg-background px-3 py-1.5 text-sm text-foreground outline-none focus:ring-2 focus:ring-ring"
            >
              <option value="zh-CN">跟随系统（简体中文）</option>
              <option value="en-US">English</option>
            </select>
          </div>

          {/* 开机自启动（禁用） */}
          <div className="flex items-center justify-between px-4 py-3.5 opacity-50">
            <div className="flex flex-col gap-0.5">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-foreground">开机自启动</span>
                <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">即将支持</span>
              </div>
              <span className="text-xs text-muted-foreground">系统启动时自动运行</span>
            </div>
            <button
              role="switch"
              aria-checked={false}
              aria-label="开机自启动"
              disabled
              className="relative h-6 w-10 cursor-not-allowed rounded-full bg-muted"
            >
              <span className="absolute left-1 top-1 h-4 w-4 rounded-full bg-muted-foreground/40 shadow" />
            </button>
          </div>

          {/* 阻止休眠（禁用） */}
          <div className="flex items-center justify-between px-4 py-3.5 opacity-50">
            <div className="flex flex-col gap-0.5">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-foreground">任务运行时阻止自动休眠</span>
                <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">即将支持</span>
              </div>
              <span className="text-xs text-muted-foreground">任务处理期间阻止电脑因空闲自动进入休眠</span>
            </div>
            <button
              role="switch"
              aria-checked={false}
              aria-label="任务运行时阻止自动休眠"
              disabled
              className="relative h-6 w-10 cursor-not-allowed rounded-full bg-muted"
            >
              <span className="absolute left-1 top-1 h-4 w-4 rounded-full bg-muted-foreground/40 shadow" />
            </button>
          </div>
        </div>
      </div>

      {/* 外观分组 */}
      <div className="flex flex-col gap-2">
        <div className="text-sm font-semibold text-foreground">外观</div>
        <div className="rounded-[14px] border border-border bg-card">
          <div className="flex items-center justify-between px-4 py-3.5">
            <div className="flex flex-col gap-0.5">
              <span className="text-sm font-medium text-foreground">强调色</span>
              <span className="text-xs text-muted-foreground">选择界面的主题强调色</span>
            </div>
            <div className="flex items-center gap-2" role="radiogroup" aria-label="强调色">
              {ACCENT_PRESETS.map((color) => (
                <button
                  key={color}
                  role="radio"
                  aria-checked={accentColor === color}
                  aria-label={color}
                  onClick={() => applyBranding({ accentColor: color })}
                  className="h-6 w-6 rounded-full transition-transform hover:scale-110 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  style={{
                    background: color,
                    outline: accentColor === color ? '2px solid currentColor' : 'none',
                    outlineOffset: '2px',
                  }}
                />
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec vitest run src/components/settings/__tests__/GeneralPanel.test.tsx 2>&1 | tail -20
```

Expected: all 7 tests PASS

- [ ] **Step 5: Delete old AccountPanel files**

```bash
rm src/components/settings/panels/AccountPanel.tsx
rm src/components/settings/__tests__/AccountPanel.test.tsx
```

- [ ] **Step 6: Commit**

```bash
git add src/components/settings/panels/GeneralPanel.tsx src/components/settings/__tests__/GeneralPanel.test.tsx
git rm src/components/settings/panels/AccountPanel.tsx src/components/settings/__tests__/AccountPanel.test.tsx
git commit -m "feat(settings): add GeneralPanel with language, disabled toggles, accent color picker"
```

---

### Task 2: Update SettingsMenu — rename "账户" → "通用"

**Files:**
- Modify: `src/components/settings/SettingsMenu.tsx:14`

- [ ] **Step 1: Update the failing test in SettingsModal.test.tsx**

In `src/components/settings/__tests__/SettingsModal.test.tsx`, change the assertion that checks for button `'账户'`:

```tsx
// Line 28: change
expect(screen.getByRole('button', { name: '账户' })).toBeInTheDocument()
// to:
expect(screen.getByRole('button', { name: '通用' })).toBeInTheDocument()
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec vitest run src/components/settings/__tests__/SettingsModal.test.tsx 2>&1 | tail -20
```

Expected: FAIL — `Unable to find role="button" name="通用"`

- [ ] **Step 3: Change label in SettingsMenu.tsx**

In `src/components/settings/SettingsMenu.tsx` line 14, change:

```ts
{ key: 'account', label: '账户' },
```

to:

```ts
{ key: 'account', label: '通用' },
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec vitest run src/components/settings/__tests__/SettingsModal.test.tsx 2>&1 | tail -20
```

Expected: all 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/components/settings/SettingsMenu.tsx src/components/settings/__tests__/SettingsModal.test.tsx
git commit -m "feat(settings): rename 账户 tab to 通用"
```

---

### Task 3: Wire GeneralPanel into SettingsModal

**Files:**
- Modify: `src/components/settings/SettingsModal.tsx`

- [ ] **Step 1: Update SettingsModal.test.tsx to assert GeneralPanel content visible under 'account' key**

In `src/components/settings/__tests__/SettingsModal.test.tsx`, update the test `'renders 7 menu items + account content when account opened'`:

```tsx
it('renders menu and general panel content when account opened', () => {
  useUiStore.getState().openSettings('account')
  render(<SettingsModal />)
  expect(screen.getByRole('button', { name: '通用' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: '关于 AI 小家' })).toBeInTheDocument()
  expect(screen.getByText('姚域权')).toBeInTheDocument()
  // New sections present
  expect(screen.getByText('语言')).toBeInTheDocument()
  expect(screen.getByText('强调色')).toBeInTheDocument()
})
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec vitest run src/components/settings/__tests__/SettingsModal.test.tsx 2>&1 | tail -20
```

Expected: FAIL — `Unable to find an element with the text: 语言`

- [ ] **Step 3: Replace AccountPanel import and usage with GeneralPanel in SettingsModal.tsx**

In `src/components/settings/SettingsModal.tsx`, replace:

```tsx
import { AccountPanel } from './panels/AccountPanel'
```

with:

```tsx
import { GeneralPanel } from './panels/GeneralPanel'
```

And replace the `settingsModal === 'account'` block:

```tsx
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
```

with:

```tsx
{settingsModal === 'account' ? (
  <GeneralPanel
    user={{
      name: user?.name ?? user?.username ?? '未登录',
      tenantName: tenant?.name ?? '',
      avatarUrl: '',
    }}
    onLogout={() => void onLogout()}
  />
) : null}
```

- [ ] **Step 4: Run all settings tests to verify everything passes**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec vitest run src/components/settings/ 2>&1 | tail -20
```

Expected: all tests PASS, no errors

- [ ] **Step 5: Commit**

```bash
git add src/components/settings/SettingsModal.tsx src/components/settings/__tests__/SettingsModal.test.tsx
git commit -m "feat(settings): wire GeneralPanel into SettingsModal"
```

---

### Task 4: Smoke test in browser

**Files:** none (manual verification)

- [ ] **Step 1: Start dev server**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm dev
```

- [ ] **Step 2: Open settings → 通用 tab and verify**

Check:
1. Left menu shows **通用** (not 账户)
2. User info card renders at top with name, tenant, 退出登录 button
3. **通用** section: 语言 dropdown works (switching language changes UI), 开机自启动 and 任务运行时阻止自动休眠 show as disabled with 「即将支持」 badge
4. **外观** section: 7 color swatches render, clicking one changes the app's accent color immediately (sidebar, buttons, etc. update)

- [ ] **Step 3: Run full settings test suite one final time**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec vitest run src/components/settings/ 2>&1 | tail -10
```

Expected: all tests PASS

---

## Self-Review

**Spec coverage:**
- ✅ 账户 → 通用 rename: Task 2
- ✅ 用户信息卡保留: Task 1 (GeneralPanel top section)
- ✅ 语言下拉（接已有 setAppLanguage）: Task 1
- ✅ 开机自启动禁用 toggle: Task 1
- ✅ 阻止休眠禁用 toggle: Task 1
- ✅ 强调色预设色块（接 applyBranding）: Task 1
- ✅ 字体缩放不做: not in plan

**Placeholder scan:** No TBDs, all steps have complete code.

**Type consistency:**
- `GeneralPanel` props: `{ user: { name, tenantName, avatarUrl }, onLogout }` — consistent across Task 1 (impl), Task 3 (usage in SettingsModal)
- `applyBranding({ accentColor: color })` — matches `BrandingState.applyBranding(tenant: TenantBranding)` where `TenantBranding` accepts optional `accentColor`
- `setAppLanguage(lang: AppLanguage)` — matches `settingsStore.ts` signature

# 前端视觉重构 · plan-B：Static Pages 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Home / Skills（中心 + 详情）/ Schedules 三组"无消息流"的页面，按 design.pen 重写为组合组件 + 薄页面层。

**Architecture:** 严格走"先组件后页面"。每个页面下沉 5-9 个组合组件到 `src/components/<bucket>/`，页面文件只负责 1) 从 store 拉数据 → 2) 拼组合组件 → 3) 绑事件，单文件 ≤ 120 行。组件层独立可测，页面层不写颜色/边框/阴影。

**Tech Stack:** 同 plan-A。依赖 plan-A 已交付的 `AppSidebar / PageTopBar / PageSectionShell` 与新 token 体系。

**对应 spec：** `docs/superpowers/specs/2026-04-23-frontend-visual-realignment-to-design-pen.md` 第 5.2 / 5.3 / 5.4、第 7.1 / 7.4 / 7.5 / 7.6 章。

**前置：** plan-A 全部任务已完成且测试通过；分支 `pzc`。

---

## 文件结构

### 新建

| 路径 | 责任 |
|---|---|
| `src/components/home/HomeMascotHero.tsx` | mascot64 + 主标题 + 副标 |
| `src/components/home/HomeCategoryChipRow.tsx` | 推荐 + 5 个分类 chip 行 |
| `src/components/home/HomeStatusList.tsx` | 空 / 加载 / 成功 三态行卡片 |
| `src/components/home/HomeSkillCenterPill.tsx` | 底部 "前往技能中心" pill |
| `src/components/home/__tests__/HomeMascotHero.test.tsx` | 渲染断言 |
| `src/components/home/__tests__/HomeCategoryChipRow.test.tsx` | active chip + 点击回调 |
| `src/components/home/__tests__/HomeStatusList.test.tsx` | 三 variant icon 底色 |
| `src/components/skills/SkillHotSection.tsx` | "热门推荐" + 网格 |
| `src/components/skills/SkillOfficeSection.tsx` | "办公效率" + 分类条 + 网格 |
| `src/components/skills/SkillCategoryBar.tsx` | 分类条（受控） |
| `src/components/skills/SkillCard.tsx` | 重写以贴近稿子（取代旧 `src/components/skill-center/SkillCard.tsx`） |
| `src/components/skills/SkillDetailHero.tsx` | 88×88 heroIc + 标题 + actionBar |
| `src/components/skills/SkillMetaRow.tsx` | 来源 / 更新时间 双列 |
| `src/components/skills/SkillTryGrid.tsx` | "试试让 AI 小家这样做" 卡片网格 |
| `src/components/skills/SkillUsageBlock.tsx` | "使用说明" 文段 |
| `src/components/skills/SkillActionBar.tsx` | 详情页右上"禁用 / 使用"按钮组 |
| `src/components/skills/__tests__/*.test.tsx` | 每组件 1 条 render test |
| `src/components/schedules/ScheduleTemplateCard.tsx` | 模板卡（标题 + 描述 + cta） |
| `src/components/schedules/ScheduleListCard.tsx` | 列表外壳，含 header / table / empty 三 slot |
| `src/components/schedules/ScheduleTableHeader.tsx` | 列头行 |
| `src/components/schedules/ScheduleEmptyState.tsx` | 空态居中区 |
| `src/components/schedules/__tests__/*.test.tsx` | 每组件 1 条 render test |

### 修改

| 路径 | 修改内容 |
|---|---|
| `src/features/home/HomePage.tsx` | 重写为 PageSectionShell + 5 组件拼装 |
| `src/features/home/HomePage.test.tsx` | 跟随调整断言 |
| `src/features/skill-center/SkillCenterPage.tsx` | 重写为 PageTopBar + SkillHotSection + SkillOfficeSection |
| `src/features/skill-center/SkillCenterPage.integration.test.tsx` | 跟随调整选择器 |
| `src/features/skill-detail/SkillDetailPage.tsx` | 重写为 SkillDetailHero + SkillMetaRow + SkillTryGrid + SkillUsageBlock |
| `src/features/schedules/SchedulesPage.tsx` | 重写为 PageTopBar + ScheduleTemplateGrid + ScheduleListCard |
| `src/components/home/HomeTaskComposerCard.tsx` | 接 plan-D 之前先保留并复用现状（仅样式微调以匹配 width 820） |
| `src/components/home/HomeSuggestionList.tsx` | 删除（功能由 HomeStatusList 接管） |
| `src/components/skill-center/SkillCard.tsx` | 删除（被 `src/components/skills/SkillCard.tsx` 取代） |

---

## Task B-1.1：HomeMascotHero

**Files:**
- Create: `src/components/home/HomeMascotHero.tsx`
- Create: `src/components/home/__tests__/HomeMascotHero.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { HomeMascotHero } from '../HomeMascotHero'

describe('HomeMascotHero', () => {
  it('renders title and subtitle', () => {
    render(
      <HomeMascotHero
        mascotUrl="/app-icon.png"
        title="创建你的下一条任务"
        subtitle="用清晰的任务描述和参数，让 AI 更快给出可执行结果。"
      />,
    )
    expect(screen.getByText('创建你的下一条任务')).toBeInTheDocument()
    expect(
      screen.getByText('用清晰的任务描述和参数，让 AI 更快给出可执行结果。'),
    ).toBeInTheDocument()
  })

  it('mascot is 64x64 with full radius', () => {
    const { container } = render(
      <HomeMascotHero mascotUrl="/x.png" title="t" subtitle="s" />,
    )
    const mascot = container.querySelector('[data-testid="home-mascot"]')
    expect(mascot?.className).toMatch(/h-16/)
    expect(mascot?.className).toMatch(/w-16/)
    expect(mascot?.className).toMatch(/rounded-full/)
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/home/__tests__/HomeMascotHero.test.tsx
```

Expected: FAIL（组件不存在）。

- [ ] **Step 3：实现**

```tsx
/**
 * @designSource design.pen#PqcAk > mascot+hello+subHello
 * @sizing mascot 64×64 r-full, title 30/700, subtitle 14 muted, gap 16
 */
interface HomeMascotHeroProps {
  mascotUrl: string
  title: string
  subtitle: string
}

export function HomeMascotHero({ mascotUrl, title, subtitle }: HomeMascotHeroProps) {
  return (
    <div className="flex flex-col items-center gap-4">
      <div
        data-testid="home-mascot"
        className="h-16 w-16 overflow-hidden rounded-full"
      >
        <img src={mascotUrl} alt="" className="h-full w-full object-cover" />
      </div>
      <div className="text-[30px] font-bold leading-tight text-foreground">
        {title}
      </div>
      <div className="max-w-[760px] text-center text-sm text-muted-foreground">
        {subtitle}
      </div>
    </div>
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/home/__tests__/HomeMascotHero.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/home/HomeMascotHero.tsx src/components/home/__tests__/HomeMascotHero.test.tsx
git commit -m "feat(frontend): add HomeMascotHero"
```

---

## Task B-1.2：HomeCategoryChipRow

**Files:**
- Create: `src/components/home/HomeCategoryChipRow.tsx`
- Create: `src/components/home/__tests__/HomeCategoryChipRow.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { HomeCategoryChipRow } from '../HomeCategoryChipRow'

const ITEMS = [
  { key: 'recommend', label: '为你推荐' },
  { key: 'writing', label: '文案有意' },
  { key: 'industry', label: '行业研究' },
]

describe('HomeCategoryChipRow', () => {
  it('renders all items', () => {
    render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={() => {}} />,
    )
    expect(screen.getByRole('button', { name: /为你推荐/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /行业研究/ })).toBeInTheDocument()
  })

  it('marks the active chip with brand-primary-subtle background', () => {
    render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={() => {}} />,
    )
    const active = screen.getByRole('button', { name: /为你推荐/ })
    expect(active.className).toMatch(/bg-brand-primary-subtle/)
  })

  it('calls onSelect with key on click', () => {
    const onSelect = vi.fn()
    render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={onSelect} />,
    )
    fireEvent.click(screen.getByRole('button', { name: /行业研究/ }))
    expect(onSelect).toHaveBeenCalledWith('industry')
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/home/__tests__/HomeCategoryChipRow.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现**

```tsx
/**
 * @designSource design.pen#Mk2H9 catRow
 * @sizing wrapper padding [8,12] r-14 border 1, chip padding [8,12] r-10
 */
import { Sparkles } from 'lucide-react'

export interface HomeChipItem {
  key: string
  label: string
}

interface HomeCategoryChipRowProps {
  items: HomeChipItem[]
  activeKey: string
  onSelect: (key: string) => void
}

export function HomeCategoryChipRow({
  items,
  activeKey,
  onSelect,
}: HomeCategoryChipRowProps) {
  return (
    <div className="flex w-full items-center gap-2 rounded-[14px] border border-border bg-card px-3 py-2">
      {items.map((it) => {
        const active = it.key === activeKey
        return (
          <button
            key={it.key}
            type="button"
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'flex items-center gap-1.5 rounded-[10px] bg-brand-primary-subtle px-3 py-2 text-[13px] font-semibold text-primary'
                : 'flex items-center gap-1.5 rounded-[10px] px-3 py-2 text-[13px] font-medium text-muted-foreground transition-colors hover:bg-muted'
            }
          >
            {active ? <Sparkles className="h-3.5 w-3.5" /> : null}
            <span>{it.label}</span>
          </button>
        )
      })}
    </div>
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/home/__tests__/HomeCategoryChipRow.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/home/HomeCategoryChipRow.tsx src/components/home/__tests__/HomeCategoryChipRow.test.tsx
git commit -m "feat(frontend): add HomeCategoryChipRow"
```

---

## Task B-1.3：HomeStatusList

**Files:**
- Create: `src/components/home/HomeStatusList.tsx`
- Create: `src/components/home/__tests__/HomeStatusList.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
import { render, screen } from '@testing-library/react'
import { Inbox, Loader2, CheckCircle2 } from 'lucide-react'
import { describe, expect, it } from 'vitest'

import { HomeStatusList } from '../HomeStatusList'

describe('HomeStatusList', () => {
  it('renders 3 rows with title and desc', () => {
    render(
      <HomeStatusList
        items={[
          { key: 'a', variant: 'empty', icon: <Inbox />, title: '空状态占位', desc: '...' },
          { key: 'b', variant: 'loading', icon: <Loader2 />, title: '加载状态占位', desc: '...' },
          { key: 'c', variant: 'success', icon: <CheckCircle2 />, title: '成功状态占位', desc: '...' },
        ]}
      />,
    )
    expect(screen.getByText('空状态占位')).toBeInTheDocument()
    expect(screen.getByText('加载状态占位')).toBeInTheDocument()
    expect(screen.getByText('成功状态占位')).toBeInTheDocument()
  })

  it('iconBox for empty uses brand-primary-subtle bg', () => {
    const { container } = render(
      <HomeStatusList
        items={[{ key: 'a', variant: 'empty', icon: <Inbox />, title: 't', desc: 'd' }]}
      />,
    )
    expect(
      container.querySelector('[data-testid="status-iconbox-a"]')?.className,
    ).toMatch(/bg-brand-primary-subtle/)
  })

  it('iconBox for success uses #DCFCE7 inline style', () => {
    const { container } = render(
      <HomeStatusList
        items={[{ key: 's', variant: 'success', icon: <CheckCircle2 />, title: 't', desc: 'd' }]}
      />,
    )
    const box = container.querySelector(
      '[data-testid="status-iconbox-s"]',
    ) as HTMLElement
    expect(box?.style.backgroundColor.toLowerCase()).toBe('rgb(220, 252, 231)')
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/home/__tests__/HomeStatusList.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现**

```tsx
/**
 * @designSource design.pen#ORsy4 statusList
 * @sizing wrapper r-14 border 1 padding 8 gap 8; iconBox 34×34 r-10
 */
import type { ReactNode } from 'react'

export type HomeStatusVariant = 'empty' | 'loading' | 'success'

export interface HomeStatusItem {
  key: string
  variant: HomeStatusVariant
  icon: ReactNode
  title: string
  desc: string
}

interface HomeStatusListProps {
  items: HomeStatusItem[]
}

const VARIANT_BG: Record<HomeStatusVariant, string | undefined> = {
  empty: 'bg-brand-primary-subtle',
  loading: 'bg-brand-secondary-subtle',
  success: undefined, // applied via inline style for #DCFCE7
}

export function HomeStatusList({ items }: HomeStatusListProps) {
  return (
    <div className="flex w-full flex-col gap-2 rounded-[14px] border border-border bg-card p-2">
      {items.map((it) => {
        const bgClass = VARIANT_BG[it.variant]
        const successStyle =
          it.variant === 'success' ? { backgroundColor: '#DCFCE7' } : undefined
        return (
          <div key={it.key} className="flex items-center gap-3.5 rounded-[10px] px-4 py-3.5">
            <div
              data-testid={`status-iconbox-${it.key}`}
              style={successStyle}
              className={
                bgClass
                  ? `flex h-[34px] w-[34px] shrink-0 items-center justify-center rounded-[10px] ${bgClass}`
                  : 'flex h-[34px] w-[34px] shrink-0 items-center justify-center rounded-[10px]'
              }
            >
              {it.icon}
            </div>
            <div className="flex min-w-0 flex-col gap-1">
              <div className="text-sm font-semibold text-foreground">{it.title}</div>
              <div className="text-[13px] text-muted-foreground">{it.desc}</div>
            </div>
          </div>
        )
      })}
    </div>
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/home/__tests__/HomeStatusList.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/home/HomeStatusList.tsx src/components/home/__tests__/HomeStatusList.test.tsx
git commit -m "feat(frontend): add HomeStatusList with 3 variants"
```

---

## Task B-1.4：HomeSkillCenterPill

**Files:**
- Create: `src/components/home/HomeSkillCenterPill.tsx`

- [ ] **Step 1：实现 + 内联测试**

写文件并直接配套测试：

`src/components/home/HomeSkillCenterPill.tsx`：

```tsx
/**
 * @designSource design.pen#M2pKg
 * @sizing pill padding [10,16] r-999 bg secondary
 */
import { ArrowRight } from 'lucide-react'

interface HomeSkillCenterPillProps {
  onClick: () => void
  label?: string
}

export function HomeSkillCenterPill({
  onClick,
  label = '前往技能中心',
}: HomeSkillCenterPillProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex items-center gap-1.5 rounded-full bg-secondary px-4 py-2.5 text-[13px] font-medium text-muted-foreground transition-colors hover:text-foreground"
    >
      <span>{label}</span>
      <ArrowRight className="h-3.5 w-3.5" />
    </button>
  )
}
```

`src/components/home/__tests__/HomeSkillCenterPill.test.tsx`：

```tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { HomeSkillCenterPill } from '../HomeSkillCenterPill'

describe('HomeSkillCenterPill', () => {
  it('renders label and fires onClick', () => {
    const onClick = vi.fn()
    render(<HomeSkillCenterPill onClick={onClick} />)
    fireEvent.click(screen.getByRole('button', { name: /前往技能中心/ }))
    expect(onClick).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2：测试通过**

```bash
pnpm exec vitest run src/components/home/__tests__/HomeSkillCenterPill.test.tsx
```

Expected: PASS。

- [ ] **Step 3：commit**

```bash
git add src/components/home/HomeSkillCenterPill.tsx src/components/home/__tests__/HomeSkillCenterPill.test.tsx
git commit -m "feat(frontend): add HomeSkillCenterPill"
```

---

## Task B-1.5：重写 HomePage 拼装

**Files:**
- Modify: `src/features/home/HomePage.tsx`
- Modify: `src/features/home/HomePage.test.tsx`
- Delete: `src/components/home/HomeSuggestionList.tsx`

- [ ] **Step 1：调整 HomePage 测试为新结构**

```tsx
// src/features/home/HomePage.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (sel: any) =>
    sel({
      route: { kind: 'home' },
      setRoute: vi.fn(),
    }),
}))

vi.mock('@/stores/brandingStore', () => ({
  useBrandingStore: (sel: any) =>
    sel({ logoUrl: '/app-icon.png', productName: 'AI 小家' }),
}))

import { HomePage } from './HomePage'

describe('HomePage', () => {
  it('renders mascot title and category chips and the skill-center pill', () => {
    render(<HomePage />)
    expect(screen.getByText('创建你的下一条任务')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /为你推荐/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /前往技能中心/ })).toBeInTheDocument()
  })

  it('does not write background or shadow utility classes from page level', () => {
    const { container } = render(<HomePage />)
    const pageRoot = container.firstChild as HTMLElement
    // 页面根 wrapper 不应自带 bg-* / shadow-* 工具类（这些都应该来自 PageSectionShell / 组合组件内部）
    expect(pageRoot.className || '').not.toMatch(/\bbg-\w+/)
    expect(pageRoot.className || '').not.toMatch(/\bshadow-\w+/)
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/features/home/HomePage.test.tsx
```

Expected: FAIL（旧 HomePage 没有 mascot 标题、没有推荐 chip、没有 pill）。

- [ ] **Step 3：实现新 HomePage（≤120 行）**

```tsx
// src/features/home/HomePage.tsx
import { useState } from 'react'
import { CheckCircle2, Inbox, Loader2 } from 'lucide-react'

import { HomeCategoryChipRow } from '@/components/home/HomeCategoryChipRow'
import { HomeMascotHero } from '@/components/home/HomeMascotHero'
import { HomeSkillCenterPill } from '@/components/home/HomeSkillCenterPill'
import { HomeStatusList } from '@/components/home/HomeStatusList'
import { HomeTaskComposerCard } from '@/components/home/HomeTaskComposerCard'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore, type Route } from '@/stores/uiStore'

const CHIP_ITEMS = [
  { key: 'recommend', label: '为你推荐' },
  { key: 'writing', label: '文案有意' },
  { key: 'industry', label: '行业研究' },
  { key: 'file', label: '文件智能' },
  { key: 'commerce', label: '电商运营' },
  { key: 'ding', label: '玩转钉钉' },
]

const STATUS_ITEMS = [
  {
    key: 'empty',
    variant: 'empty' as const,
    icon: <Inbox className="h-4 w-4 text-primary" />,
    title: '空状态占位',
    desc: '暂无任务结果。你可以从上方输入任务，或选择下方推荐分类快速开始。',
  },
  {
    key: 'loading',
    variant: 'loading' as const,
    icon: <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />,
    title: '加载状态占位',
    desc: 'AI 正在分析任务上下文并生成执行计划，请稍候...',
  },
  {
    key: 'success',
    variant: 'success' as const,
    icon: <CheckCircle2 className="h-4 w-4" style={{ color: '#16A34A' }} />,
    title: '成功状态占位',
    desc: '任务已创建成功，可前往对话页继续补充指令，或进入技能中心选择能力模块。',
  },
]

export function HomePage() {
  const [activeChip, setActiveChip] = useState('recommend')
  const setRoute = useUiStore((s) => s.setRoute)
  const logoUrl = useBrandingStore((s) => s.logoUrl)

  return (
    <PageSectionShell padding="px-10 pt-8 pb-7" gap="gap-4">
      <div className="mx-auto flex w-[820px] flex-col items-center gap-4">
        <HomeMascotHero
          mascotUrl={logoUrl}
          title="创建你的下一条任务"
          subtitle="用清晰的任务描述和参数，让 AI 更快给出可执行结果。"
        />
        <div className="w-full">
          <HomeTaskComposerCard />
        </div>
        <HomeCategoryChipRow
          items={CHIP_ITEMS}
          activeKey={activeChip}
          onSelect={setActiveChip}
        />
        <HomeStatusList items={STATUS_ITEMS} />
        <HomeSkillCenterPill
          onClick={() => setRoute({ kind: 'skill-center' } as Route)}
        />
      </div>
    </PageSectionShell>
  )
}
```

- [ ] **Step 4：删除已废弃 `HomeSuggestionList`**

```bash
rm src/components/home/HomeSuggestionList.tsx
```

- [ ] **Step 5：测试通过 + lint + tsc**

```bash
pnpm exec vitest run src/features/home/HomePage.test.tsx
pnpm lint
pnpm exec tsc --noEmit
```

Expected: PASS / 0 error。

- [ ] **Step 6：commit**

```bash
git add src/features/home src/components/home
git commit -m "refactor(frontend): rebuild HomePage with mascot/chips/status-list/pill"
```

---

## Task B-2.1：Skill 原子组合 — `SkillCard` / `SkillCategoryBar`

**Files:**
- Create: `src/components/skills/SkillCard.tsx`
- Create: `src/components/skills/SkillCategoryBar.tsx`
- Create: `src/components/skills/__tests__/SkillCard.test.tsx`
- Create: `src/components/skills/__tests__/SkillCategoryBar.test.tsx`

- [ ] **Step 1：写测试**

```tsx
// src/components/skills/__tests__/SkillCard.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillCard } from '../SkillCard'

describe('SkillCard', () => {
  it('renders title, desc and fires actions', () => {
    const onUse = vi.fn()
    const onOpen = vi.fn()
    render(
      <SkillCard
        title="数据分析"
        desc="上传 Excel 或 CSV，一键生成报告"
        iconNode={<span data-testid="ic">ic</span>}
        onUse={onUse}
        onOpen={onOpen}
      />,
    )
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByTestId('ic')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /使用/ }))
    expect(onUse).toHaveBeenCalled()
  })

  it('uses border 1 + r-8 + light shadow on the card root', () => {
    const { container } = render(
      <SkillCard title="t" desc="d" iconNode={null} onUse={() => {}} onOpen={() => {}} />,
    )
    const card = container.querySelector('[data-testid="skill-card"]')
    expect(card?.className).toMatch(/border/)
    expect(card?.className).toMatch(/rounded-md|rounded-lg|rounded-\[8px\]/)
  })
})
```

```tsx
// src/components/skills/__tests__/SkillCategoryBar.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillCategoryBar } from '../SkillCategoryBar'

describe('SkillCategoryBar', () => {
  it('marks active and fires onSelect', () => {
    const onSelect = vi.fn()
    render(
      <SkillCategoryBar
        items={[
          { key: 'a', label: 'A' },
          { key: 'b', label: 'B' },
        ]}
        activeKey="a"
        onSelect={onSelect}
      />,
    )
    const a = screen.getByRole('button', { name: 'A' })
    const b = screen.getByRole('button', { name: 'B' })
    expect(a.className).toMatch(/bg-secondary|bg-card/)
    fireEvent.click(b)
    expect(onSelect).toHaveBeenCalledWith('b')
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/skills/__tests__
```

Expected: FAIL。

- [ ] **Step 3：实现**

```tsx
// src/components/skills/SkillCard.tsx
/**
 * @designSource design.pen technical card derivative (Card / Card Action)
 * @sizing r-8 border 1 padding 16
 */
import type { ReactNode } from 'react'

import { Button } from '@/components/ui/button'

interface SkillCardProps {
  title: string
  desc: string
  iconNode: ReactNode
  onUse: () => void
  onOpen: () => void
}

export function SkillCard({ title, desc, iconNode, onUse, onOpen }: SkillCardProps) {
  return (
    <div
      data-testid="skill-card"
      className="flex h-full flex-col rounded-lg border border-border bg-card p-4 shadow-sm transition-colors hover:border-primary/40"
    >
      <div className="mb-3 flex items-center gap-2">
        <div className="flex h-8 w-8 items-center justify-center rounded-md bg-brand-primary-subtle">
          {iconNode}
        </div>
        <div className="text-sm font-semibold text-foreground">{title}</div>
      </div>
      <p className="flex-1 text-[13px] text-muted-foreground">{desc}</p>
      <div className="mt-4 flex items-center gap-2">
        <Button variant="secondary" className="flex-1" onClick={onOpen}>
          详情
        </Button>
        <Button className="flex-1" onClick={onUse}>
          使用
        </Button>
      </div>
    </div>
  )
}
```

```tsx
// src/components/skills/SkillCategoryBar.tsx
/**
 * @designSource design.pen#ueSct catBar
 * @sizing row gap 8; chip padding [6,12] r-6
 */
export interface SkillCategoryItem {
  key: string
  label: string
}

interface SkillCategoryBarProps {
  items: SkillCategoryItem[]
  activeKey: string
  onSelect: (key: string) => void
}

export function SkillCategoryBar({ items, activeKey, onSelect }: SkillCategoryBarProps) {
  return (
    <div className="flex w-full flex-wrap items-center gap-2">
      {items.map((it) => {
        const active = it.key === activeKey
        return (
          <button
            key={it.key}
            type="button"
            onClick={() => onSelect(it.key)}
            className={
              active
                ? 'rounded-md bg-secondary px-3 py-1.5 text-[13px] font-semibold text-foreground shadow-sm'
                : 'rounded-md px-3 py-1.5 text-[13px] font-medium text-muted-foreground transition-colors hover:bg-muted'
            }
          >
            {it.label}
          </button>
        )
      })}
    </div>
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/skills/__tests__
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/skills/SkillCard.tsx src/components/skills/SkillCategoryBar.tsx src/components/skills/__tests__
git commit -m "feat(frontend): add SkillCard and SkillCategoryBar"
```

---

## Task B-2.2：技能中心两 section + 重写 SkillCenterPage

**Files:**
- Create: `src/components/skills/SkillHotSection.tsx`
- Create: `src/components/skills/SkillOfficeSection.tsx`
- Create: `src/components/skills/__tests__/SkillHotSection.test.tsx`
- Create: `src/components/skills/__tests__/SkillOfficeSection.test.tsx`
- Modify: `src/features/skill-center/SkillCenterPage.tsx`
- Modify: `src/features/skill-center/SkillCenterPage.integration.test.tsx`
- Delete: `src/components/skill-center/SkillCard.tsx`

- [ ] **Step 1：写两个 section 的简单 render 测试**

```tsx
// src/components/skills/__tests__/SkillHotSection.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillHotSection } from '../SkillHotSection'

describe('SkillHotSection', () => {
  it('renders title 热门推荐 and the children grid', () => {
    render(
      <SkillHotSection>
        <div>cardA</div>
        <div>cardB</div>
      </SkillHotSection>,
    )
    expect(screen.getByText('热门推荐')).toBeInTheDocument()
    expect(screen.getByText('cardA')).toBeInTheDocument()
    expect(screen.getByText('cardB')).toBeInTheDocument()
  })
})
```

```tsx
// src/components/skills/__tests__/SkillOfficeSection.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillOfficeSection } from '../SkillOfficeSection'

describe('SkillOfficeSection', () => {
  it('renders title 办公效率 and forwards categoryBar/grid slots', () => {
    render(
      <SkillOfficeSection
        categoryBar={<div data-testid="bar">bar</div>}
      >
        <div>card1</div>
      </SkillOfficeSection>,
    )
    expect(screen.getByText('办公效率')).toBeInTheDocument()
    expect(screen.getByTestId('bar')).toBeInTheDocument()
    expect(screen.getByText('card1')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/skills/__tests__/SkillHotSection.test.tsx src/components/skills/__tests__/SkillOfficeSection.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现两个 section**

```tsx
// src/components/skills/SkillHotSection.tsx
/**
 * @designSource design.pen#znwZc hotSec
 * @sizing title 15/600; grid gap 16
 */
import type { PropsWithChildren } from 'react'

export function SkillHotSection({ children }: PropsWithChildren) {
  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-[15px] font-semibold text-foreground">热门推荐</h2>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {children}
      </div>
    </section>
  )
}
```

```tsx
// src/components/skills/SkillOfficeSection.tsx
/**
 * @designSource design.pen#CoiX7 ofcSec
 * @sizing title 15/600; outer gap 14
 */
import type { PropsWithChildren, ReactNode } from 'react'

interface SkillOfficeSectionProps extends PropsWithChildren {
  categoryBar: ReactNode
}

export function SkillOfficeSection({ categoryBar, children }: SkillOfficeSectionProps) {
  return (
    <section className="flex flex-col gap-3.5">
      <h2 className="text-[15px] font-semibold text-foreground">办公效率</h2>
      {categoryBar}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {children}
      </div>
    </section>
  )
}
```

- [ ] **Step 4：重写 SkillCenterPage**

```tsx
// src/features/skill-center/SkillCenterPage.tsx
import { useMemo, useState } from 'react'
import { Sparkles } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { SkillHotSection } from '@/components/skills/SkillHotSection'
import { SkillOfficeSection } from '@/components/skills/SkillOfficeSection'
import { Button } from '@/components/ui/button'
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
  const listByCategory = useSkillStore((s) => s.listByCategory)
  const setRoute = useUiStore((s) => s.setRoute)
  const { createConversationFromSkill } = useChat()

  const officeCategories = useMemo(
    () => SKILL_CATEGORIES.map((c) => ({ key: c.id, label: c.name })),
    [],
  )
  const recommended = listByCategory('recommended')
  const officeSkills = listByCategory(category)

  return (
    <PageSectionShell
      topBar={
        <PageTopBar
          variant="default"
          trailing={
            <div className="flex items-center gap-2">
              <Button variant="secondary" onClick={() => setMarketOpen(true)}>
                技能市场
              </Button>
              <Button onClick={() => setUploadOpen(true)}>上传技能</Button>
            </div>
          }
        />
      }
      padding="px-7 pt-6 pb-8"
      gap="gap-5"
    >
      <SkillHotSection>
        {recommended.map((skill) => (
          <SkillCard
            key={skill.id}
            title={skill.displayName}
            desc={skill.shortDescription || skill.description}
            iconNode={<Sparkles className="h-4 w-4 text-primary" />}
            onOpen={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
            onUse={() => void createConversationFromSkill(skill.id)}
          />
        ))}
      </SkillHotSection>
      <SkillOfficeSection
        categoryBar={
          <SkillCategoryBar
            items={officeCategories}
            activeKey={category}
            onSelect={(key) => setCategory(key as SkillCategoryId)}
          />
        }
      >
        {officeSkills.map((skill) => (
          <SkillCard
            key={skill.id}
            title={skill.displayName}
            desc={skill.shortDescription || skill.description}
            iconNode={<Sparkles className="h-4 w-4 text-primary" />}
            onOpen={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
            onUse={() => void createConversationFromSkill(skill.id)}
          />
        ))}
      </SkillOfficeSection>
      <SkillMarketModal open={marketOpen} onOpenChange={setMarketOpen} />
      <SkillUploadModal open={uploadOpen} onOpenChange={setUploadOpen} />
    </PageSectionShell>
  )
}
```

- [ ] **Step 5：删除旧 SkillCard 文件并更新 integration test 选择器**

```bash
rm src/components/skill-center/SkillCard.tsx
rmdir src/components/skill-center 2>/dev/null || true
```

打开 `src/features/skill-center/SkillCenterPage.integration.test.tsx`，把对旧 `查看详情/开始使用` 等按钮文案的查找改为对新 `详情/使用` 文案；保留主要行为意图。

- [ ] **Step 6：测试 + lint + tsc**

```bash
pnpm exec vitest run src/components/skills src/features/skill-center
pnpm lint
pnpm exec tsc --noEmit
```

Expected: PASS / 0 error。

- [ ] **Step 7：commit**

```bash
git add src/components/skills src/features/skill-center src/components/skill-center
git commit -m "refactor(frontend): rebuild SkillCenterPage with hot+office sections"
```

---

## Task B-2.3：技能详情组件 + 重写 SkillDetailPage

**Files:**
- Create: `src/components/skills/SkillDetailHero.tsx`
- Create: `src/components/skills/SkillMetaRow.tsx`
- Create: `src/components/skills/SkillTryGrid.tsx`
- Create: `src/components/skills/SkillUsageBlock.tsx`
- Create: `src/components/skills/SkillActionBar.tsx`
- Create: `src/components/skills/__tests__/SkillDetailHero.test.tsx`
- Create: `src/components/skills/__tests__/SkillMetaRow.test.tsx`
- Modify: `src/features/skill-detail/SkillDetailPage.tsx`

- [ ] **Step 1：写两条关键测试**

```tsx
// src/components/skills/__tests__/SkillDetailHero.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillDetailHero } from '../SkillDetailHero'

describe('SkillDetailHero', () => {
  it('renders title, subtitle and slots', () => {
    render(
      <SkillDetailHero
        iconNode={<span>ic</span>}
        title="数据分析"
        subtitle="上传 Excel 或 CSV ..."
        actionBar={<span data-testid="ab">ab</span>}
      />,
    )
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByText(/上传 Excel/)).toBeInTheDocument()
    expect(screen.getByTestId('ab')).toBeInTheDocument()
  })

  it('heroIc box is 88×88 with brand-primary-subtle bg', () => {
    const { container } = render(
      <SkillDetailHero
        iconNode={null}
        title="t"
        subtitle="s"
        actionBar={null}
      />,
    )
    const box = container.querySelector('[data-testid="hero-ic"]')
    expect(box?.className).toMatch(/h-\[88px\]/)
    expect(box?.className).toMatch(/w-\[88px\]/)
    expect(box?.className).toMatch(/bg-brand-primary-subtle/)
  })
})
```

```tsx
// src/components/skills/__tests__/SkillMetaRow.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillMetaRow } from '../SkillMetaRow'

describe('SkillMetaRow', () => {
  it('renders all label/value pairs', () => {
    render(
      <SkillMetaRow
        items={[
          { label: '来源', value: 'AI 小家内置' },
          { label: '更新时间', value: '2026-04-20' },
        ]}
      />,
    )
    expect(screen.getByText('来源')).toBeInTheDocument()
    expect(screen.getByText('AI 小家内置')).toBeInTheDocument()
    expect(screen.getByText('更新时间')).toBeInTheDocument()
    expect(screen.getByText('2026-04-20')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/skills/__tests__/SkillDetailHero.test.tsx src/components/skills/__tests__/SkillMetaRow.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现五个组件**

```tsx
// src/components/skills/SkillDetailHero.tsx
/**
 * @designSource design.pen#UDRR3 hero
 * @sizing heroIc 88×88 r-22 brand-primary-subtle; gap 20
 */
import type { ReactNode } from 'react'

interface SkillDetailHeroProps {
  iconNode: ReactNode
  title: string
  subtitle: string
  actionBar: ReactNode
}

export function SkillDetailHero({ iconNode, title, subtitle, actionBar }: SkillDetailHeroProps) {
  return (
    <div className="flex w-full items-start gap-5">
      <div
        data-testid="hero-ic"
        className="flex h-[88px] w-[88px] shrink-0 items-center justify-center rounded-[22px] bg-brand-primary-subtle"
      >
        {iconNode}
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="text-[28px] font-bold leading-tight text-foreground">{title}</div>
        <div className="text-sm text-muted-foreground">{subtitle}</div>
      </div>
      <div className="shrink-0">{actionBar}</div>
    </div>
  )
}
```

```tsx
// src/components/skills/SkillMetaRow.tsx
/**
 * @designSource design.pen#DWw8D metaRow
 * @sizing gap 48; label 13 muted; value 14 foreground
 */
interface SkillMetaItem {
  label: string
  value: string
}

interface SkillMetaRowProps {
  items: SkillMetaItem[]
}

export function SkillMetaRow({ items }: SkillMetaRowProps) {
  return (
    <div className="flex w-full flex-wrap items-start gap-12">
      {items.map((it) => (
        <div key={it.label} className="flex flex-col gap-1.5">
          <div className="text-[13px] text-muted-foreground">{it.label}</div>
          <div className="text-sm text-foreground">{it.value}</div>
        </div>
      ))}
    </div>
  )
}
```

```tsx
// src/components/skills/SkillTryGrid.tsx
/**
 * @designSource design.pen#ZQLFS trySec
 * @sizing title 15/600; grid gap 16
 */
import type { PropsWithChildren } from 'react'

export function SkillTryGrid({ children }: PropsWithChildren) {
  return (
    <section className="flex w-full flex-col gap-3.5">
      <div className="text-[15px] font-semibold text-foreground">试试让 AI 小家这样做</div>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">{children}</div>
    </section>
  )
}
```

```tsx
// src/components/skills/SkillUsageBlock.tsx
/**
 * @designSource design.pen#MTvV8 useSec
 * @sizing title 15/600 + body 13 muted; gap 8
 */
interface SkillUsageBlockProps {
  text: string
}

export function SkillUsageBlock({ text }: SkillUsageBlockProps) {
  return (
    <section className="flex w-full flex-col gap-2">
      <div className="text-[15px] font-semibold text-foreground">使用说明</div>
      <p className="max-w-[880px] text-[13px] text-muted-foreground">{text}</p>
    </section>
  )
}
```

```tsx
// src/components/skills/SkillActionBar.tsx
/**
 * @designSource design.pen#C4WXv heroAct
 * @sizing gap 10; outline button + primary button
 */
import { Button } from '@/components/ui/button'

interface SkillActionBarProps {
  primaryLabel: string
  secondaryLabel: string
  onPrimary: () => void
  onSecondary: () => void
  primaryDisabled?: boolean
}

export function SkillActionBar({
  primaryLabel,
  secondaryLabel,
  onPrimary,
  onSecondary,
  primaryDisabled,
}: SkillActionBarProps) {
  return (
    <div className="flex items-center gap-2.5">
      <Button variant="outline" onClick={onSecondary}>
        {secondaryLabel}
      </Button>
      <Button onClick={onPrimary} disabled={primaryDisabled}>
        {primaryLabel}
      </Button>
    </div>
  )
}
```

- [ ] **Step 4：重写 SkillDetailPage**

```tsx
// src/features/skill-detail/SkillDetailPage.tsx
import { Sparkles } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillActionBar } from '@/components/skills/SkillActionBar'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillDetailHero } from '@/components/skills/SkillDetailHero'
import { SkillMetaRow } from '@/components/skills/SkillMetaRow'
import { SkillTryGrid } from '@/components/skills/SkillTryGrid'
import { SkillUsageBlock } from '@/components/skills/SkillUsageBlock'
import { useChat } from '@/hooks/useChat'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore, type Route } from '@/stores/uiStore'

interface SkillDetailPageProps {
  skillId: string
}

const TRY_PROMPTS = [
  '依据这份表格，分析本月经营数据，输出 KPI 达成率、趋势图和 P0/P1 行动建议。',
  '帮我分析表格数据，自动挖掘 KPI、趋势和异常，输出可视化报告。',
  '把这份多 sheet Excel 拆开分析，各模块独立出报告并关联对比。',
]

export function SkillDetailPage({ skillId }: SkillDetailPageProps) {
  const skill = useSkillStore((s) => s.getById(skillId))
  const setRoute = useUiStore((s) => s.setRoute)
  const { createConversationFromSkill } = useChat()

  if (!skill) {
    return (
      <PageSectionShell padding="px-10 pt-10 pb-8" gap="gap-4">
        <div className="text-sm text-muted-foreground">技能不存在或尚未加载。</div>
      </PageSectionShell>
    )
  }

  return (
    <PageSectionShell
      topBar={<PageTopBar variant="default" />}
      padding="px-10 pt-7 pb-8"
      gap="gap-6"
    >
      <SkillDetailHero
        iconNode={<Sparkles className="h-9 w-9 text-primary" />}
        title={skill.displayName}
        subtitle={skill.shortDescription || skill.description}
        actionBar={
          <SkillActionBar
            secondaryLabel="禁用"
            primaryLabel="使用"
            onSecondary={() => {}}
            onPrimary={() => void createConversationFromSkill(skill.id)}
          />
        }
      />
      <SkillMetaRow
        items={[
          { label: '来源', value: skill.source === 'builtin' ? 'AI 小家内置' : '已安装' },
          { label: '更新时间', value: '2026-04-20' },
        ]}
      />
      <SkillTryGrid>
        {TRY_PROMPTS.map((p, i) => (
          <SkillCard
            key={i}
            iconNode={<Sparkles className="h-4 w-4 text-primary" />}
            title={skill.displayName}
            desc={p}
            onOpen={() => setRoute({ kind: 'skill-detail', skillId: skill.id } as Route)}
            onUse={() => void createConversationFromSkill(skill.id)}
          />
        ))}
      </SkillTryGrid>
      <SkillUsageBlock text={skill.description || '上传 Excel 或 CSV 表格，一键生成可视化数据分析报告。'} />
    </PageSectionShell>
  )
}
```

- [ ] **Step 5：测试 + lint + tsc**

```bash
pnpm exec vitest run src/components/skills src/features/skill-detail
pnpm lint
pnpm exec tsc --noEmit
```

Expected: PASS / 0 error。

- [ ] **Step 6：commit**

```bash
git add src/components/skills src/features/skill-detail
git commit -m "refactor(frontend): rebuild SkillDetailPage with hero/meta/try/usage"
```

---

## Task B-3.1：Schedules 组合组件

**Files:**
- Create: `src/components/schedules/ScheduleTemplateCard.tsx`
- Create: `src/components/schedules/ScheduleListCard.tsx`
- Create: `src/components/schedules/ScheduleTableHeader.tsx`
- Create: `src/components/schedules/ScheduleEmptyState.tsx`
- Create: `src/components/schedules/__tests__/ScheduleTemplateCard.test.tsx`
- Create: `src/components/schedules/__tests__/ScheduleListCard.test.tsx`

- [ ] **Step 1：写两条关键测试**

```tsx
// src/components/schedules/__tests__/ScheduleTemplateCard.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ScheduleTemplateCard } from '../ScheduleTemplateCard'

describe('ScheduleTemplateCard', () => {
  it('renders title/desc and fires CTA', () => {
    const onCta = vi.fn()
    render(
      <ScheduleTemplateCard
        title="日报汇总"
        desc="每天 9 点把昨日数据汇总成日报。"
        cta={{ label: '使用模板', onClick: onCta }}
      />,
    )
    expect(screen.getByText('日报汇总')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '使用模板' }))
    expect(onCta).toHaveBeenCalled()
  })

  it('uses r-14 border 1 padding 18', () => {
    const { container } = render(
      <ScheduleTemplateCard title="t" desc="d" cta={{ label: 'x', onClick: () => {} }} />,
    )
    const card = container.querySelector('[data-testid="schedule-template-card"]')
    expect(card?.className).toMatch(/rounded-\[14px\]/)
    expect(card?.className).toMatch(/border/)
    expect(card?.className).toMatch(/p-\[18px\]/)
  })
})
```

```tsx
// src/components/schedules/__tests__/ScheduleListCard.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { ScheduleListCard } from '../ScheduleListCard'

describe('ScheduleListCard', () => {
  it('renders all three slots', () => {
    render(
      <ScheduleListCard
        header={<div>head</div>}
        table={<div>table</div>}
        empty={<div>empty</div>}
      />,
    )
    expect(screen.getByText('head')).toBeInTheDocument()
    expect(screen.getByText('table')).toBeInTheDocument()
    expect(screen.getByText('empty')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/schedules/__tests__
```

Expected: FAIL。

- [ ] **Step 3：实现四个组件**

```tsx
// src/components/schedules/ScheduleTemplateCard.tsx
/**
 * @designSource design.pen#YQ44C tpl1
 * @sizing r-14 border 1 padding 18 gap 10
 */
import { Button } from '@/components/ui/button'

interface ScheduleTemplateCardProps {
  title: string
  desc: string
  cta: { label: string; onClick: () => void }
}

export function ScheduleTemplateCard({ title, desc, cta }: ScheduleTemplateCardProps) {
  return (
    <div
      data-testid="schedule-template-card"
      className="flex w-full flex-col gap-2.5 rounded-[14px] border border-border bg-card p-[18px]"
    >
      <div className="text-[15px] font-semibold text-foreground">{title}</div>
      <p className="flex-1 text-[13px] text-muted-foreground">{desc}</p>
      <div>
        <Button variant="secondary" onClick={cta.onClick}>
          {cta.label}
        </Button>
      </div>
    </div>
  )
}
```

```tsx
// src/components/schedules/ScheduleListCard.tsx
/**
 * @designSource design.pen#jhWGa listCard
 * @sizing r-14 border 1 bg card; header padding [16,20]; tableHeader padding [10,20] bottom-border; empty h 280 center
 */
import type { ReactNode } from 'react'

interface ScheduleListCardProps {
  header: ReactNode
  table: ReactNode
  empty?: ReactNode
}

export function ScheduleListCard({ header, table, empty }: ScheduleListCardProps) {
  return (
    <div className="flex w-full flex-col rounded-[14px] border border-border bg-card">
      <div className="px-5 py-4">{header}</div>
      <div className="border-t border-border">{table}</div>
      {empty ? <div className="flex h-[280px] items-center justify-center">{empty}</div> : null}
    </div>
  )
}
```

```tsx
// src/components/schedules/ScheduleTableHeader.tsx
/**
 * @designSource design.pen#j4hWs tableHead
 * @sizing padding [10,20] bottom-border 1
 */
interface ScheduleTableHeaderProps {
  columns: string[]
}

export function ScheduleTableHeader({ columns }: ScheduleTableHeaderProps) {
  return (
    <div
      className="grid items-center gap-3 px-5 py-2.5 text-[13px] font-medium text-muted-foreground"
      style={{ gridTemplateColumns: `repeat(${columns.length}, minmax(0, 1fr))` }}
    >
      {columns.map((c) => (
        <span key={c}>{c}</span>
      ))}
    </div>
  )
}
```

```tsx
// src/components/schedules/ScheduleEmptyState.tsx
/**
 * @designSource design.pen#Ifs8C emptyArea
 * @sizing h 280 center; gap 14
 */
import type { ReactNode } from 'react'

interface ScheduleEmptyStateProps {
  icon?: ReactNode
  title: string
  desc?: string
  cta?: { label: string; onClick: () => void }
}

import { Button } from '@/components/ui/button'

export function ScheduleEmptyState({ icon, title, desc, cta }: ScheduleEmptyStateProps) {
  return (
    <div className="flex flex-col items-center gap-3.5">
      {icon}
      <div className="text-sm font-semibold text-foreground">{title}</div>
      {desc ? <div className="text-[13px] text-muted-foreground">{desc}</div> : null}
      {cta ? <Button onClick={cta.onClick}>{cta.label}</Button> : null}
    </div>
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/schedules/__tests__
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/schedules
git commit -m "feat(frontend): add Schedules composite components"
```

---

## Task B-3.2：重写 SchedulesPage

**Files:**
- Modify: `src/features/schedules/SchedulesPage.tsx`

- [ ] **Step 1：实现新 SchedulesPage**

```tsx
// src/features/schedules/SchedulesPage.tsx
import { CalendarClock } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { ScheduleEmptyState } from '@/components/schedules/ScheduleEmptyState'
import { ScheduleListCard } from '@/components/schedules/ScheduleListCard'
import { ScheduleTableHeader } from '@/components/schedules/ScheduleTableHeader'
import { ScheduleTemplateCard } from '@/components/schedules/ScheduleTemplateCard'

const TEMPLATES = [
  { title: '日报汇总', desc: '每天 9 点自动汇总昨日数据生成日报。' },
  { title: '门店巡检', desc: '每周一汇总各门店巡检结果并生成报表。' },
  { title: '周度复盘', desc: '每周五汇总周度 KPI 与团队复盘要点。' },
]

export function SchedulesPage() {
  return (
    <PageSectionShell
      topBar={<PageTopBar variant="title" title="定时任务" />}
      padding="px-7 pt-6 pb-8"
      gap="gap-6"
    >
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        {TEMPLATES.map((t) => (
          <ScheduleTemplateCard
            key={t.title}
            title={t.title}
            desc={t.desc}
            cta={{ label: '使用模板', onClick: () => {} }}
          />
        ))}
      </div>
      <ScheduleListCard
        header={
          <div className="flex items-center justify-between">
            <div className="text-sm font-semibold text-foreground">任务列表</div>
            <div className="text-[13px] text-muted-foreground">共 0 条</div>
          </div>
        }
        table={
          <ScheduleTableHeader columns={['任务名称', '执行频率', '状态']} />
        }
        empty={
          <ScheduleEmptyState
            icon={<CalendarClock className="h-8 w-8 text-muted-foreground" />}
            title="还没有定时任务"
            desc="选择上方模板或在对话中创建你的第一个定时任务。"
          />
        }
      />
    </PageSectionShell>
  )
}
```

- [ ] **Step 2：lint + tsc + 整体测试**

```bash
pnpm lint
pnpm exec tsc --noEmit
pnpm test
```

Expected: 0 error / 全 PASS。

- [ ] **Step 3：commit**

```bash
git add src/features/schedules
git commit -m "refactor(frontend): rebuild SchedulesPage with template grid + list card"
```

---

## Task B-Final：阶段 B 验收

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

逐个打开：首页、技能中心、技能详情、定时任务，目视确认整体节奏与设计稿一致：
- 首页：mascot 居中 + 推荐 chip 金底激活 + 三态行卡 + 底部 pill；
- 技能中心：顶栏右上"技能市场 / 上传技能" + 热门推荐 + 办公效率 categoryBar + 网格；
- 技能详情：88×88 hero icon + meta gap 48 + 试试网格 + 使用说明；
- 定时任务：顶栏 "定时任务" + 3 张模板卡 + 列表卡空态。

- [ ] **Step 3：阶段总结 commit**

```bash
git commit --allow-empty -m "chore(frontend): plan-B milestone — home/skills/schedules realigned"
```

---

## 自审

**Spec coverage:** 覆盖 spec 第 5.2 / 5.3 / 5.4、第 7.1 / 7.4 / 7.5 / 7.6 章。`HomeTaskComposerCard` 的视觉留待 plan-D 与 ChatComposerCompact 一并替换（本阶段保留旧实现以解耦）。

**Placeholder scan:** 已扫；无 TBD。

**Type consistency:** `HomeChipItem / SkillCategoryItem / SkillCategoryId / SkillNavKey` 等命名贯通；所有页面统一通过 `PageSectionShell` 的 `topBar / padding / gap / maxWidthClass` 接入，padding 由调用方在页面层显式给出（页面层只允许传 padding/gap 两类传值，不写颜色边框阴影）。

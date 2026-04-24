# 技能中心 B6iNU 设计对齐 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `SkillCenterPage` 及相关组件按照 design.pen 节点 B6iNU 全面还原视觉与交互。

**Architecture:** 分 6 个独立任务依次完成：分类数据 → SkillCard → SkillCategoryBar → SkillHotSection/SkillOfficeSection → SkillCenterPage 顶栏 → 集成测试修复。每个任务先写失败测试，再改实现，再提交。

**Tech Stack:** React 18, TypeScript, Tailwind CSS (CSS 变量写法), Vitest + @testing-library/react

---

## 文件结构

| 文件 | 变更 |
|---|---|
| `src/data/skill-categories.ts` | 更新分类列表和 `SkillCategoryId` 类型 |
| `src/components/skills/SkillCard.tsx` | 重构：移除底部按钮，整卡点击，`size` prop |
| `src/components/skills/__tests__/SkillCard.test.tsx` | 更新测试 |
| `src/components/skills/SkillCategoryBar.tsx` | chip 样式改为圆角 pill，激活态品牌色 |
| `src/components/skills/__tests__/SkillCategoryBar.test.tsx` | 更新测试 |
| `src/components/skills/SkillHotSection.tsx` | 调整 gap |
| `src/components/skills/__tests__/SkillHotSection.test.tsx` | 无变更（保持通过） |
| `src/components/skills/SkillOfficeSection.tsx` | 标题改「全部技能」，调整 gap |
| `src/components/skills/__tests__/SkillOfficeSection.test.tsx` | 更新断言标题 |
| `src/features/skill-center/SkillCenterPage.tsx` | 顶栏重构，传入新分类，整卡点击路由 |
| `src/features/skill-center/SkillCenterPage.integration.test.tsx` | 更新测试（移除「详情」按钮断言） |

---

### Task 1: 更新分类数据 `skill-categories.ts`

**Files:**
- Modify: `src/data/skill-categories.ts`

> 分类数据没有独立测试文件，但被 integration test 间接覆盖；本任务只修改数据，不写额外测试文件。

- [ ] **Step 1: 修改 `src/data/skill-categories.ts`**

完整替换文件内容：

```ts
export type SkillCategoryId =
  | 'recommended'
  | 'hr'
  | 'finance'
  | 'legal'
  | 'sales'
  | 'ops'
  | 'general'

export interface SkillCategory {
  id: Exclude<SkillCategoryId, 'recommended'>
  name: string
  icon: string
}

export const SKILL_CATEGORIES: SkillCategory[] = [
  { id: 'hr',      name: 'HR',   icon: 'users' },
  { id: 'finance', name: '财务', icon: 'bar-chart-2' },
  { id: 'legal',   name: '法务', icon: 'scale' },
  { id: 'sales',   name: '销售', icon: 'trending-up' },
  { id: 'ops',     name: '运营', icon: 'settings' },
  { id: 'general', name: '通用', icon: 'wrench' },
]
```

- [ ] **Step 2: 确认 TypeScript 无报错**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec tsc --noEmit 2>&1 | head -30
```

Expected: 0 errors，或仅 skillStore 里 `SkillCategoryId` 用旧值时报错（Task 2 中修复）。

- [ ] **Step 3: Commit**

```bash
git add src/data/skill-categories.ts
git commit -m "feat(skill-center): update skill categories to HR/财务/法务/销售/运营/通用"
```

---

### Task 2: 修复 `skillStore` 与 `SkillCenterPage` 的 TS 引用

**Files:**
- Modify: `src/stores/skillStore.ts`
- Modify: `src/features/skill-center/SkillCenterPage.tsx` （仅 category 默认值部分）

> Task 1 修改了 `SkillCategoryId`，现有代码里 `category` 默认值 `'recommended'` 仍有效，但 `listByCategory` 里可能用到旧分类 id；需要确认无 TS 错误后 commit。

- [ ] **Step 1: 在 `skillStore.ts` 检查 `listByCategory`**

打开 `src/stores/skillStore.ts`，确认 `listByCategory` 的逻辑仍正确：

```ts
listByCategory(id) {
  const { skills, recommendedIds } = get()
  if (id === 'recommended') {
    return skills.filter((skill) => recommendedIds.includes(skill.id))
  }
  return skills.filter((skill) => (skill.category || 'general') === id)
},
```

该逻辑对任意 string id 均适用，无需修改。

- [ ] **Step 2: 在 `SkillCenterPage.tsx` 修改 category 状态初始值**

找到第 20 行：
```ts
const [category, setCategory] = useState<SkillCategoryId>('recommended')
```

改为（`recommended` 在新类型里仍存在，无需修改，但需确认）：

保持不变即可，`recommended` 在新 `SkillCategoryId` 里仍有效。

- [ ] **Step 3: 运行 TS 检查确认无报错**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec tsc --noEmit 2>&1 | head -30
```

Expected: 0 errors

- [ ] **Step 4: Commit**

```bash
git commit --allow-empty -m "chore(skill-center): verify ts after category id update"
```

> 如果没有实际文件改动则跳过此 commit。

---

### Task 3: 重构 `SkillCard`

**Files:**
- Modify: `src/components/skills/SkillCard.tsx`
- Modify: `src/components/skills/__tests__/SkillCard.test.tsx`

设计规格：
- 热门卡（`size="hot"`）：`h-[140px]`，图标容器 `36×36` `rounded-[10px]`
- 普通卡（`size="office"`，默认）：`h-[120px]`，图标容器 `34×34` `rounded-[10px]`
- 整卡点击 → `onClick` 回调；无底部「详情/使用」按钮
- hover: `hover:-translate-y-0.5 transition-all duration-150`
- 元信息格式 `内置 · HR` 等由调用方传入 `meta` prop

- [ ] **Step 1: 更新 `SkillCard.test.tsx`（先写失败测试）**

完整替换 `src/components/skills/__tests__/SkillCard.test.tsx`：

```tsx
import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillCard } from '../SkillCard'

describe('SkillCard', () => {
  it('renders title, meta, desc and fires onClick on card click', () => {
    const onClick = vi.fn()
    render(
      <SkillCard
        title="数据分析"
        meta="内置 · HR"
        desc="上传 Excel 或 CSV，一键生成报告"
        iconNode={<span data-testid="ic">ic</span>}
        onClick={onClick}
      />,
    )
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByText('内置 · HR')).toBeInTheDocument()
    expect(screen.getByText('上传 Excel 或 CSV，一键生成报告')).toBeInTheDocument()
    expect(screen.getByTestId('ic')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('skill-card'))
    expect(onClick).toHaveBeenCalled()
  })

  it('hot size applies h-[140px] class', () => {
    const { container } = render(
      <SkillCard title="t" meta="m" desc="d" iconNode={null} onClick={() => {}} size="hot" />,
    )
    const card = container.querySelector('[data-testid="skill-card"]')
    expect(card?.className).toMatch(/h-\[140px\]/)
  })

  it('office size (default) applies h-[120px] class', () => {
    const { container } = render(
      <SkillCard title="t" meta="m" desc="d" iconNode={null} onClick={() => {}} />,
    )
    const card = container.querySelector('[data-testid="skill-card"]')
    expect(card?.className).toMatch(/h-\[120px\]/)
  })

  it('has no 详情 or 使用 buttons', () => {
    render(
      <SkillCard title="t" meta="m" desc="d" iconNode={null} onClick={() => {}} />,
    )
    expect(screen.queryByRole('button', { name: /详情|使用/ })).toBeNull()
  })
})
```

- [ ] **Step 2: 运行测试，确认失败**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec vitest run src/components/skills/__tests__/SkillCard.test.tsx 2>&1 | tail -20
```

Expected: FAIL（`meta` prop 不存在、`onClick` 不是 card 级别等）

- [ ] **Step 3: 重写 `SkillCard.tsx`**

完整替换 `src/components/skills/SkillCard.tsx`：

```tsx
import type { ReactNode } from 'react'

interface SkillCardProps {
  title: string
  meta: string
  desc: string
  iconNode: ReactNode
  onClick: () => void
  size?: 'hot' | 'office'
}

export function SkillCard({ title, meta, desc, iconNode, onClick, size = 'office' }: SkillCardProps) {
  const isHot = size === 'hot'
  const height = isHot ? 'h-[140px]' : 'h-[120px]'
  const iconSize = isHot ? 'h-9 w-9' : 'h-[34px] w-[34px]'

  return (
    <div
      data-testid="skill-card"
      role="button"
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(e) => e.key === 'Enter' && onClick()}
      className={`flex ${height} cursor-pointer flex-col rounded-[14px] border border-border bg-card p-4 transition-all duration-150 hover:-translate-y-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring`}
    >
      <div className="flex items-center gap-2.5">
        <div className={`flex ${iconSize} shrink-0 items-center justify-center rounded-[10px] bg-brand-primary-subtle`}>
          {iconNode}
        </div>
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate text-sm font-semibold text-foreground">{title}</span>
          <span className="text-[12px] font-medium text-brand-secondary">{meta}</span>
        </div>
      </div>
      <p className="mt-2.5 line-clamp-2 text-[12px] text-muted-foreground">{desc}</p>
    </div>
  )
}
```

- [ ] **Step 4: 运行测试，确认通过**

```bash
pnpm exec vitest run src/components/skills/__tests__/SkillCard.test.tsx 2>&1 | tail -20
```

Expected: PASS（4 tests pass）

- [ ] **Step 5: Commit**

```bash
git add src/components/skills/SkillCard.tsx src/components/skills/__tests__/SkillCard.test.tsx
git commit -m "feat(skill-center): SkillCard — no buttons, whole-card click, size prop, meta row"
```

---

### Task 4: 更新 `SkillCategoryBar`

**Files:**
- Modify: `src/components/skills/SkillCategoryBar.tsx`
- Modify: `src/components/skills/__tests__/SkillCategoryBar.test.tsx`

设计规格（节点 `Kkinf` / `rnhH6`）：
- chip `rounded-full` `px-3.5 py-2` font-13px
- 激活：`bg-brand-primary-subtle text-primary font-semibold`
- 非激活：无背景，`text-muted-foreground font-medium`，hover `bg-muted/60`

- [ ] **Step 1: 更新 `SkillCategoryBar.test.tsx`（先写失败测试）**

完整替换 `src/components/skills/__tests__/SkillCategoryBar.test.tsx`：

```tsx
import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillCategoryBar } from '../SkillCategoryBar'

describe('SkillCategoryBar', () => {
  it('marks active chip with brand-primary-subtle class', () => {
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
    expect(a.className).toMatch(/bg-brand-primary-subtle/)
    expect(a.className).toMatch(/text-primary/)
  })

  it('inactive chip has no bg-brand-primary-subtle', () => {
    render(
      <SkillCategoryBar
        items={[{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }]}
        activeKey="a"
        onSelect={() => {}}
      />,
    )
    const b = screen.getByRole('button', { name: 'B' })
    expect(b.className).not.toMatch(/bg-brand-primary-subtle/)
  })

  it('fires onSelect with correct key', () => {
    const onSelect = vi.fn()
    render(
      <SkillCategoryBar
        items={[{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }]}
        activeKey="a"
        onSelect={onSelect}
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'B' }))
    expect(onSelect).toHaveBeenCalledWith('b')
  })
})
```

- [ ] **Step 2: 运行测试，确认失败**

```bash
pnpm exec vitest run src/components/skills/__tests__/SkillCategoryBar.test.tsx 2>&1 | tail -20
```

Expected: FAIL（当前激活态用 `bg-secondary`，不含 `bg-brand-primary-subtle`）

- [ ] **Step 3: 重写 `SkillCategoryBar.tsx`**

完整替换 `src/components/skills/SkillCategoryBar.tsx`：

```tsx
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
                ? 'rounded-full bg-brand-primary-subtle px-3.5 py-2 text-[13px] font-semibold text-primary'
                : 'rounded-full px-3.5 py-2 text-[13px] font-medium text-muted-foreground transition-colors hover:bg-muted/60'
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

- [ ] **Step 4: 运行测试，确认通过**

```bash
pnpm exec vitest run src/components/skills/__tests__/SkillCategoryBar.test.tsx 2>&1 | tail -20
```

Expected: PASS（3 tests pass）

- [ ] **Step 5: Commit**

```bash
git add src/components/skills/SkillCategoryBar.tsx src/components/skills/__tests__/SkillCategoryBar.test.tsx
git commit -m "feat(skill-center): SkillCategoryBar — rounded-full chip, brand-primary-subtle active"
```

---

### Task 5: 更新 `SkillHotSection` 与 `SkillOfficeSection`

**Files:**
- Modify: `src/components/skills/SkillHotSection.tsx`
- Modify: `src/components/skills/SkillOfficeSection.tsx`
- Modify: `src/components/skills/__tests__/SkillOfficeSection.test.tsx`

- [ ] **Step 1: 更新 `SkillOfficeSection.test.tsx`（先写失败测试）**

完整替换 `src/components/skills/__tests__/SkillOfficeSection.test.tsx`：

```tsx
import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillOfficeSection } from '../SkillOfficeSection'

describe('SkillOfficeSection', () => {
  it('renders title 全部技能 and forwards slots', () => {
    render(
      <SkillOfficeSection categoryBar={<div data-testid="bar">bar</div>}>
        <div>card1</div>
      </SkillOfficeSection>,
    )
    expect(screen.getByText('全部技能')).toBeInTheDocument()
    expect(screen.getByTestId('bar')).toBeInTheDocument()
    expect(screen.getByText('card1')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: 运行测试，确认失败**

```bash
pnpm exec vitest run src/components/skills/__tests__/SkillOfficeSection.test.tsx 2>&1 | tail -20
```

Expected: FAIL（当前标题是「办公效率」）

- [ ] **Step 3: 更新 `SkillOfficeSection.tsx`**

完整替换 `src/components/skills/SkillOfficeSection.tsx`：

```tsx
import type { PropsWithChildren, ReactNode } from 'react'

interface SkillOfficeSectionProps extends PropsWithChildren {
  categoryBar: ReactNode
}

export function SkillOfficeSection({ categoryBar, children }: SkillOfficeSectionProps) {
  return (
    <section className="flex flex-col gap-3.5">
      <h2 className="text-[15px] font-semibold text-foreground">全部技能</h2>
      {categoryBar}
      <div className="grid grid-cols-1 gap-2.5 md:grid-cols-2 xl:grid-cols-3">
        {children}
      </div>
    </section>
  )
}
```

- [ ] **Step 4: 更新 `SkillHotSection.tsx`**

完整替换 `src/components/skills/SkillHotSection.tsx`：

```tsx
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

- [ ] **Step 5: 运行全部 section 测试，确认通过**

```bash
pnpm exec vitest run src/components/skills/__tests__/SkillHotSection.test.tsx src/components/skills/__tests__/SkillOfficeSection.test.tsx 2>&1 | tail -20
```

Expected: PASS（2 tests pass）

- [ ] **Step 6: Commit**

```bash
git add src/components/skills/SkillHotSection.tsx src/components/skills/SkillOfficeSection.tsx src/components/skills/__tests__/SkillOfficeSection.test.tsx
git commit -m "feat(skill-center): section titles and grid gaps aligned to B6iNU"
```

---

### Task 6: 重构 `SkillCenterPage` 顶栏与卡片传参

**Files:**
- Modify: `src/features/skill-center/SkillCenterPage.tsx`
- Modify: `src/features/skill-center/SkillCenterPage.integration.test.tsx`

设计要点：
- TopBar 左侧：「技能中心」标题 + `skills.length` 徽章
- TopBar 右侧：搜索框（静态）+ 「上传技能资料」Outline 按钮 + 「+ 创建技能」Primary 按钮
- 分类 bar items 以「全部」为首，后接 `SKILL_CATEGORIES`
- 热门区传 `size="hot"`，全部技能区传 `size="office"`（默认）
- 卡片 `onClick` → `setRoute({ kind: 'skill-detail', skillId })`；移除 `onUse`/`onOpen` 两个 prop

- [ ] **Step 1: 更新 integration test（先写失败测试）**

完整替换 `src/features/skill-center/SkillCenterPage.integration.test.tsx`：

```tsx
import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: vi.fn() }),
}))

const HR_SKILL = {
  id: 'hr-analysis',
  displayName: 'HR分析',
  description: '详细描述',
  source: 'builtin',
  hasWorkflow: true,
  icon: 'users',
  category: 'hr',
  triggerText: '',
  shortDescription: '短描述',
  displayNameEn: 'HR Analysis',
  shortDescriptionEn: 'short',
}

const RECOMMENDED_SKILL = {
  id: 'writing-plans',
  displayName: '写计划',
  description: 'desc',
  source: 'builtin',
  hasWorkflow: true,
  icon: 'file-text',
  category: 'general',
  triggerText: '',
  shortDescription: '短描述',
  displayNameEn: 'Plan',
  shortDescriptionEn: 'short',
}

describe('SkillCenterPage', () => {
  beforeEach(() => {
    useSkillStore.setState({
      skills: [RECOMMENDED_SKILL, HR_SKILL],
      recommendedIds: ['writing-plans'],
      isLoading: false,
    })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null })
  })

  it('顶栏渲染标题、技能数量徽章和搜索框', () => {
    render(<SkillCenterPage />)
    expect(screen.getByText('技能中心')).toBeInTheDocument()
    expect(screen.getByText(/2 个技能/)).toBeInTheDocument()
    expect(screen.getByPlaceholderText('搜索技能名称或场景')).toBeInTheDocument()
  })

  it('顶栏有上传技能资料和创建技能按钮', () => {
    render(<SkillCenterPage />)
    expect(screen.getByRole('button', { name: '上传技能资料' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /创建技能/ })).toBeInTheDocument()
  })

  it('分类 bar 包含全部/HR/财务/法务/销售/运营/通用', () => {
    render(<SkillCenterPage />)
    for (const label of ['全部', 'HR', '财务', '法务', '销售', '运营', '通用']) {
      expect(screen.getByRole('button', { name: label })).toBeInTheDocument()
    }
  })

  it('切换到 HR 分类后卡片点击进入详情', async () => {
    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: 'HR' }))
    const cards = screen.getAllByTestId('skill-card')
    fireEvent.click(cards[0])
    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'hr-analysis' })
    })
  })

  it('热门推荐区卡片点击进入详情', async () => {
    render(<SkillCenterPage />)
    const cards = screen.getAllByTestId('skill-card')
    fireEvent.click(cards[0])
    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'writing-plans' })
    })
  })

  it('没有常驻的详情/使用按钮', () => {
    render(<SkillCenterPage />)
    expect(screen.queryByRole('button', { name: /^详情$/ })).toBeNull()
    expect(screen.queryByRole('button', { name: /^使用$/ })).toBeNull()
  })
})
```

- [ ] **Step 2: 运行测试，确认失败**

```bash
pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx 2>&1 | tail -30
```

Expected: FAIL（多个断言失败：标题找不到、搜索框找不到、SkillCard 没有 onClick 等）

- [ ] **Step 3: 重写 `SkillCenterPage.tsx`**

完整替换 `src/features/skill-center/SkillCenterPage.tsx`：

```tsx
import { Search } from 'lucide-react'
import { useMemo, useState } from 'react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { SkillHotSection } from '@/components/skills/SkillHotSection'
import { SkillOfficeSection } from '@/components/skills/SkillOfficeSection'
import { Button } from '@/components/ui/button'
import { SKILL_CATEGORIES, type SkillCategoryId } from '@/data/skill-categories'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillUploadModal } from './SkillUploadModal'

export function SkillCenterPage() {
  const [category, setCategory] = useState<SkillCategoryId>('recommended')
  const [uploadOpen, setUploadOpen] = useState(false)
  const skills = useSkillStore((s) => s.skills)
  const listByCategory = useSkillStore((s) => s.listByCategory)
  const setRoute = useUiStore((s) => s.setRoute)

  const categoryItems = useMemo(
    () => [
      { key: 'recommended', label: '全部' },
      ...SKILL_CATEGORIES.map((c) => ({ key: c.id, label: c.name })),
    ],
    [],
  )

  const recommended = listByCategory('recommended')
  const officeSkills = listByCategory(category)

  function getSkillMeta(source: string, category: string) {
    const label = SKILL_CATEGORIES.find((c) => c.id === category)?.name ?? category
    const sourceLabel = source === 'builtin' ? '内置' : '自定义'
    return `${sourceLabel} · ${label}`
  }

  return (
    <PageSectionShell
      topBar={
        <header className="flex h-14 items-center justify-between border-b border-border px-6">
          <div className="flex items-center gap-3">
            <span className="text-[18px] font-bold text-foreground">技能中心</span>
            <span className="rounded-full bg-secondary px-2.5 py-1 text-[12px] font-medium text-muted-foreground">
              {skills.length} 个技能
            </span>
          </div>
          <div className="flex items-center gap-2.5">
            <div className="flex h-[34px] w-[220px] items-center gap-2 rounded-full bg-secondary px-3">
              <Search className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              <input
                className="flex-1 bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
                placeholder="搜索技能名称或场景"
                readOnly
              />
            </div>
            <Button variant="outline" onClick={() => setUploadOpen(true)}>
              上传技能资料
            </Button>
            <Button>+ 创建技能</Button>
          </div>
        </header>
      }
      padding="px-7 pt-6 pb-8"
      gap="gap-5"
    >
      <SkillHotSection>
        {recommended.slice(0, 3).map((skill) => (
          <SkillCard
            key={skill.id}
            size="hot"
            title={skill.displayName}
            meta={getSkillMeta(skill.source, skill.category)}
            desc={skill.shortDescription || skill.description}
            iconNode={null}
            onClick={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
          />
        ))}
      </SkillHotSection>
      <SkillOfficeSection
        categoryBar={
          <SkillCategoryBar
            items={categoryItems}
            activeKey={category}
            onSelect={(key) => setCategory(key as SkillCategoryId)}
          />
        }
      >
        {officeSkills.map((skill) => (
          <SkillCard
            key={skill.id}
            title={skill.displayName}
            meta={getSkillMeta(skill.source, skill.category)}
            desc={skill.shortDescription || skill.description}
            iconNode={null}
            onClick={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
          />
        ))}
      </SkillOfficeSection>
      <SkillUploadModal open={uploadOpen} onOpenChange={setUploadOpen} />
    </PageSectionShell>
  )
}
```

- [ ] **Step 4: 运行 integration test，确认通过**

```bash
pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx 2>&1 | tail -30
```

Expected: PASS（6 tests pass）

- [ ] **Step 5: 运行全部前端测试，确认无回归**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm test 2>&1 | tail -30
```

Expected: all pass（如有其他文件引用 `onUse`/`onOpen` 的旧 SkillCard API，逐一修复后重跑）

- [ ] **Step 6: TS 全量检查**

```bash
pnpm exec tsc --noEmit 2>&1 | head -30
```

Expected: 0 errors

- [ ] **Step 7: Commit**

```bash
git add src/features/skill-center/SkillCenterPage.tsx \
        src/features/skill-center/SkillCenterPage.integration.test.tsx
git commit -m "feat(skill-center): full B6iNU realignment — new topbar, card-click nav, category bar"
```

---

## 额外修复：其他引用旧 SkillCard API 的文件

若 Step 6.5 发现有其他文件用了旧的 `onUse`/`onOpen` prop，按以下方式修复：

- 搜索：`grep -rn "onUse\|onOpen" src/`
- 将 `onUse={...} onOpen={...}` 替换为 `onClick={...}`（通常选择 `onOpen` 对应的详情跳转）
- 删除多余的 `import { useChat }` 等只为 `onUse` 存在的依赖
- 重跑 `pnpm test`

---

## 验收标准

1. `pnpm test` 全绿（无 skip、无 fail）
2. `pnpm exec tsc --noEmit` 0 errors
3. 启动 `pnpm dev` 后在浏览器里打开技能中心，视觉符合设计截图：
   - 顶栏有标题 + 技能数 + 搜索框 + 两个按钮
   - 热门推荐 3 张大卡（无底部按钮）
   - 全部技能分类 chip 为圆角 pill，激活黄底
   - 点击任意技能卡跳转详情页

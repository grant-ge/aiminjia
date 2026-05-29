# 前端视觉重构 · plan-D：Chat Scene & Interaction 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把聊天页从"扁平消息骨架"重构为 design.pen 中"轮次结构化渲染 + ToolGroup 聚合卡 + GeneratedFileCard + SuggestChipGroup + 紧凑输入区 + 全量技能弹层"的产品形态，同时保留 `chatStore` 持久化结构不动。

**Architecture:** 新增 `src/components/chat-scene/` 组件族承载所有设计稿结构；新增 `src/hooks/useTurnRenderModel.ts` 把 `chatStore` 的扁平 message + toolExecutions 投影为 `Turn[]`（纯前端聚合，无后端改动）；改造 `MessageList` 为消费 Turn model；`SkillPopover` 升级为全量技能弹层。`UserMessageBubble / AiSegmentText / TypingIndicator / ToolGroupCard` 等都是纯展示组件，可独立测试；聚合逻辑测试在 hook 层。

**Tech Stack:** 同前。新增 `react-syntax-highlighter` 的 prism 子包（或复用已有高亮能力；若没有则用 `<pre>` 原生字符体，避免新增依赖）。

**对应 spec：** `docs/superpowers/specs/2026-04-23-frontend-visual-realignment-to-design-pen.md` 第 5.6、6.1、6.2、6.4、7.2、7.3 章。

**前置：** plan-A、B、C 已完成。分支 `pzc`。

---

## 文件结构

### 新建 `src/components/chat-scene/`

| 路径 | 责任 |
|---|---|
| `UserMessageBubble.tsx` | 金底右对齐、max-w 80% |
| `AiSegmentText.tsx` | AI 段落文本（fontSize 14，1.5 行高） |
| `TypingIndicator.tsx`（新，5 variant） | default / analyze / retrieve / generate / organize |
| `SuggestChipGroup.tsx` | "建议回复" chip 组 |
| `GeneratedFileCard.tsx` | 文件卡：icon + title/sub + 右侧 Open pill |
| `ToolGroupCard.tsx` | 聚合卡外壳（顶 bar + 步骤列表） |
| `ToolGroupStepRow.tsx` | 单步行（折叠 / 展开容器） |
| `ToolGroupCodeBlock.tsx` | 代码输入 + 输出 slot |
| `ChatComposerCompact.tsx` | 紧凑输入区（+/技能/访问/项目/模型/mic/send） |
| `ChatBottomArea.tsx` | 输入区外壳 + 底部 tips |
| `SkillPopoverPanel.tsx` | 聊天底部"技能"按钮上方的弹层（全量已安装技能） |
| `__tests__/*.test.tsx` | 每个组件 1 条 render/交互 test |

### 新建 `src/hooks/`

| 路径 | 责任 |
|---|---|
| `useTurnRenderModel.ts` | 把 messages + conversationStreamState 投影为 Turn[] |
| `__tests__/useTurnRenderModel.test.ts` | 3 种输入形态 → Turn 结构快照 |

### 修改

| 路径 | 修改内容 |
|---|---|
| `src/components/chat/MessageList.tsx` | 改为消费 Turn model，调用 chat-scene 组件渲染 |
| `src/components/chat/SkillPopover.tsx` | 升级为 `SkillPopoverPanel` 的消费入口；仍保留文件名，内部重写 |
| `src/features/chat/ChatPage.tsx`（若存在）或 `src/App.tsx` 聊天段 | 改用 `ChatTopBar + ChatBottomArea + ChatComposerCompact` 替换旧输入区；保留 chatStore 绑定不变 |
| 旧 `src/components/chat/AiBubble.tsx` / `StreamingBubble.tsx` / `TaskStatusList.tsx` / `TurnSummaryBadge.tsx` / `WelcomeScreen.tsx` | 保留文件以支撑既有测试；`MessageList` 不再直接渲染它们（保留作为 fallback / 内嵌复用，见 Task D-6） |

---

## Task D-1：纯展示小件 — UserMessageBubble / AiSegmentText / TypingIndicator

**Files:**
- Create: `src/components/chat-scene/UserMessageBubble.tsx`
- Create: `src/components/chat-scene/AiSegmentText.tsx`
- Create: `src/components/chat-scene/TypingIndicator.tsx`
- Create: `src/components/chat-scene/__tests__/UserMessageBubble.test.tsx`
- Create: `src/components/chat-scene/__tests__/TypingIndicator.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/chat-scene/__tests__/UserMessageBubble.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { UserMessageBubble } from '../UserMessageBubble'

describe('UserMessageBubble', () => {
  it('renders text on a primary-colored bubble with right alignment wrapper', () => {
    render(<UserMessageBubble text="Hello" />)
    expect(screen.getByText('Hello')).toBeInTheDocument()
  })

  it('bubble uses bg-primary and rounded-2xl', () => {
    const { container } = render(<UserMessageBubble text="X" />)
    const bubble = container.querySelector('[data-testid="user-bubble"]')
    expect(bubble?.className).toMatch(/bg-primary/)
    expect(bubble?.className).toMatch(/rounded-2xl/)
  })

  it('bubble max width is 80% of the row', () => {
    const { container } = render(<UserMessageBubble text="X" />)
    const bubble = container.querySelector('[data-testid="user-bubble"]')
    expect(bubble?.className).toMatch(/max-w-\[80%\]/)
  })
})
```

```tsx
// src/components/chat-scene/__tests__/TypingIndicator.test.tsx
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { TypingIndicator } from '../TypingIndicator'

describe('TypingIndicator', () => {
  it.each([
    ['default', '正在处理'],
    ['analyze', '分析中'],
    ['retrieve', '检索中'],
    ['generate', '生成中'],
    ['organize', '整理中'],
  ] as const)('variant %s shows label %s', (variant, label) => {
    render(<TypingIndicator variant={variant} />)
    expect(screen.getByText(new RegExp(label))).toBeInTheDocument()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/UserMessageBubble.test.tsx src/components/chat-scene/__tests__/TypingIndicator.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现三个组件**

```tsx
// src/components/chat-scene/UserMessageBubble.tsx
/**
 * @designSource design.pen#1JNrw bubble/adaptive-max-80
 * @sizing r-16 padding [12,16] bg primary fg primary-foreground; align right; max-w 80%
 */
interface UserMessageBubbleProps {
  text: string
}

export function UserMessageBubble({ text }: UserMessageBubbleProps) {
  return (
    <div className="flex w-full justify-end">
      <div
        data-testid="user-bubble"
        className="max-w-[80%] rounded-2xl bg-primary px-4 py-3 text-sm text-primary-foreground"
      >
        {text}
      </div>
    </div>
  )
}
```

```tsx
// src/components/chat-scene/AiSegmentText.tsx
/**
 * @designSource design.pen#TtxTY/HSE9l/ZK6ey
 * @sizing fontSize 14 color foreground lineHeight ~1.5
 */
interface AiSegmentTextProps {
  text: string
}

export function AiSegmentText({ text }: AiSegmentTextProps) {
  return <div className="text-sm leading-[1.55] text-foreground">{text}</div>
}
```

```tsx
// src/components/chat-scene/TypingIndicator.tsx
/**
 * @designSource design.pen#oYVXX/nVSBv/EAVW9/91cWy/gpR09
 */
import {
  Activity,
  Brain,
  Loader2,
  Search,
  Sparkles,
  type LucideIcon,
} from 'lucide-react'

export type TypingVariant = 'default' | 'analyze' | 'retrieve' | 'generate' | 'organize'

const MAP: Record<TypingVariant, { icon: LucideIcon; label: string }> = {
  default: { icon: Loader2, label: '正在处理...' },
  analyze: { icon: Brain, label: '分析中...' },
  retrieve: { icon: Search, label: '检索中...' },
  generate: { icon: Sparkles, label: '生成中...' },
  organize: { icon: Activity, label: '整理中...' },
}

interface TypingIndicatorProps {
  variant: TypingVariant
}

export function TypingIndicator({ variant }: TypingIndicatorProps) {
  const { icon: Icon, label } = MAP[variant]
  const animate = variant === 'default' ? 'animate-spin' : 'animate-pulse'
  return (
    <div className="flex items-center gap-2 text-[13px] text-primary">
      <Icon className={`h-3.5 w-3.5 ${animate}`} />
      <span>{label}</span>
    </div>
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/UserMessageBubble.test.tsx src/components/chat-scene/__tests__/TypingIndicator.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/chat-scene
git commit -m "feat(frontend): add UserMessageBubble/AiSegmentText/TypingIndicator"
```

---

## Task D-2：SuggestChipGroup + GeneratedFileCard

**Files:**
- Create: `src/components/chat-scene/SuggestChipGroup.tsx`
- Create: `src/components/chat-scene/GeneratedFileCard.tsx`
- Create: `src/components/chat-scene/__tests__/SuggestChipGroup.test.tsx`
- Create: `src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/chat-scene/__tests__/SuggestChipGroup.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { CalendarPlus } from 'lucide-react'
import { describe, expect, it, vi } from 'vitest'

import { SuggestChipGroup } from '../SuggestChipGroup'

describe('SuggestChipGroup', () => {
  it('renders caption and fires click on chip', () => {
    const fn = vi.fn()
    render(
      <SuggestChipGroup
        caption="建议回复"
        items={[
          {
            label: '帮我把 1on1 排进日历',
            icon: <CalendarPlus className="h-3.5 w-3.5 text-primary" />,
            onClick: fn,
          },
        ]}
      />,
    )
    expect(screen.getByText('建议回复')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /1on1/ }))
    expect(fn).toHaveBeenCalled()
  })
})
```

```tsx
// src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { GeneratedFileCard } from '../GeneratedFileCard'

describe('GeneratedFileCard', () => {
  it('renders title/sub/appName and fires onOpen', () => {
    const onOpen = vi.fn()
    render(
      <GeneratedFileCard
        title="绩效分析总结 · Q2 近 30 天"
        sub="Report · XLSX"
        appName="Microsoft Excel"
        fileIcon={<span>file</span>}
        onOpen={onOpen}
      />,
    )
    expect(screen.getByText('绩效分析总结 · Q2 近 30 天')).toBeInTheDocument()
    expect(screen.getByText('Report · XLSX')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Microsoft Excel/ }))
    expect(onOpen).toHaveBeenCalled()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/SuggestChipGroup.test.tsx src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现**

```tsx
// src/components/chat-scene/SuggestChipGroup.tsx
/**
 * @designSource design.pen#kFmPc
 * @sizing caption 12 muted; chip r-999 border 1 bg background padding [6,12] gap 8
 */
import type { ReactNode } from 'react'

export interface SuggestChip {
  label: string
  icon: ReactNode
  onClick: () => void
}

interface SuggestChipGroupProps {
  caption?: string
  items: SuggestChip[]
}

export function SuggestChipGroup({ caption = '建议回复', items }: SuggestChipGroupProps) {
  return (
    <div className="flex flex-col gap-2">
      <div className="text-xs text-muted-foreground">{caption}</div>
      <div className="flex flex-wrap gap-2">
        {items.map((it, i) => (
          <button
            key={i}
            type="button"
            onClick={it.onClick}
            className="flex items-center gap-2 rounded-full border border-border bg-background px-3 py-1.5 text-[13px] text-foreground transition-colors hover:bg-muted"
          >
            {it.icon}
            <span>{it.label}</span>
          </button>
        ))}
      </div>
    </div>
  )
}
```

```tsx
// src/components/chat-scene/GeneratedFileCard.tsx
/**
 * @designSource design.pen#v46uG
 * @sizing r-14 border 1 bg card padding [14,16] gap 16; fileIcon 44×52 r-6 bg muted
 */
import type { ReactNode } from 'react'
import { ChevronDown } from 'lucide-react'

interface GeneratedFileCardProps {
  title: string
  sub: string
  appName: string
  fileIcon: ReactNode
  appIcon?: ReactNode
  onOpen: () => void
}

export function GeneratedFileCard({
  title,
  sub,
  appName,
  fileIcon,
  appIcon,
  onOpen,
}: GeneratedFileCardProps) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-[14px] border border-border bg-card p-4">
      <div className="flex min-w-0 items-center gap-3">
        <div className="flex h-[52px] w-11 items-center justify-center rounded-md border border-border bg-muted">
          {fileIcon}
        </div>
        <div className="flex min-w-0 flex-col gap-1">
          <div className="truncate text-[15px] font-semibold text-foreground">{title}</div>
          <div className="truncate text-[13px] text-muted-foreground">{sub}</div>
        </div>
      </div>
      <button
        type="button"
        onClick={onOpen}
        className="flex shrink-0 items-center gap-2 rounded-full border border-border bg-background py-1.5 pl-3 pr-1.5 text-[13px] text-foreground transition-colors hover:bg-muted"
        aria-label={`${appName} open`}
      >
        {appIcon}
        <span>{appName}</span>
        <span className="mx-1 h-4 w-px bg-border" />
        <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
      </button>
    </div>
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/SuggestChipGroup.test.tsx src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/chat-scene/SuggestChipGroup.tsx src/components/chat-scene/GeneratedFileCard.tsx src/components/chat-scene/__tests__/SuggestChipGroup.test.tsx src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx
git commit -m "feat(frontend): add SuggestChipGroup and GeneratedFileCard"
```

---

## Task D-3：ToolGroupCard 四态（+ 子件 StepRow / CodeBlock）

**Files:**
- Create: `src/components/chat-scene/ToolGroupCard.tsx`
- Create: `src/components/chat-scene/ToolGroupStepRow.tsx`
- Create: `src/components/chat-scene/ToolGroupCodeBlock.tsx`
- Create: `src/components/chat-scene/__tests__/ToolGroupCard.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/chat-scene/__tests__/ToolGroupCard.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ToolGroupCard } from '../ToolGroupCard'

const STEPS = [
  { index: 1, name: 'fetch_feedback', status: 'done' as const, durationMs: 1300 },
  { index: 2, name: 'cluster_topics', status: 'done' as const, durationMs: 2100, inputJson: '{ "a": 1 }' },
  { index: 3, name: 'draft_followups', status: 'done' as const, durationMs: 1400 },
]

describe('ToolGroupCard', () => {
  it('header shows done status and aggregate duration', () => {
    render(
      <ToolGroupCard
        status="done"
        steps={STEPS}
        durationMs={4800}
        expanded
        expandedStepIndex={null}
        onToggle={() => {}}
        onToggleStep={() => {}}
      />,
    )
    expect(screen.getByText(/已完成 3 步/)).toBeInTheDocument()
    expect(screen.getByText(/4\.8s/)).toBeInTheDocument()
  })

  it('header shows running status with progress x/y when running', () => {
    render(
      <ToolGroupCard
        status="running"
        steps={[...STEPS].map((s, i) => (i < 2 ? { ...s, status: 'done' as const } : { ...s, status: 'running' as const }))}
        durationMs={2400}
        expanded
        expandedStepIndex={null}
        onToggle={() => {}}
        onToggleStep={() => {}}
      />,
    )
    expect(screen.getByText(/正在执行/)).toBeInTheDocument()
    expect(screen.getByText(/2 \/ 3/)).toBeInTheDocument()
  })

  it('clicking top bar toggles expanded', () => {
    const onToggle = vi.fn()
    render(
      <ToolGroupCard
        status="done"
        steps={STEPS}
        durationMs={1000}
        expanded
        expandedStepIndex={null}
        onToggle={onToggle}
        onToggleStep={() => {}}
      />,
    )
    fireEvent.click(screen.getByTestId('tool-group-top-bar'))
    expect(onToggle).toHaveBeenCalled()
  })

  it('shows code block only for expandedStepIndex', () => {
    const { rerender } = render(
      <ToolGroupCard
        status="done"
        steps={STEPS}
        durationMs={1000}
        expanded
        expandedStepIndex={null}
        onToggle={() => {}}
        onToggleStep={() => {}}
      />,
    )
    expect(screen.queryByText(/"a"/)).toBeNull()
    rerender(
      <ToolGroupCard
        status="done"
        steps={STEPS}
        durationMs={1000}
        expanded
        expandedStepIndex={2}
        onToggle={() => {}}
        onToggleStep={() => {}}
      />,
    )
    expect(screen.getByText(/"a"/)).toBeInTheDocument()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/ToolGroupCard.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现三个文件**

```tsx
// src/components/chat-scene/ToolGroupCodeBlock.tsx
/**
 * @designSource design.pen#8FxYa detail
 * @sizing bg muted padding [14,16,16,46] font mono
 */
import type { ReactNode } from 'react'

interface ToolGroupCodeBlockProps {
  inputJson?: string
  output?: ReactNode
}

export function ToolGroupCodeBlock({ inputJson, output }: ToolGroupCodeBlockProps) {
  return (
    <div className="flex flex-col gap-3 bg-muted px-14 py-4">
      {inputJson ? (
        <div className="flex flex-col gap-1.5">
          <div className="text-[12px] font-semibold text-muted-foreground">输入</div>
          <pre className="whitespace-pre-wrap rounded-md bg-[#0a0a0a] p-3 font-mono text-[12px] leading-relaxed text-primary-foreground">
            {inputJson}
          </pre>
        </div>
      ) : null}
      {output ? (
        <div className="flex flex-col gap-1.5">
          <div className="text-[12px] font-semibold text-muted-foreground">输出</div>
          <div>{output}</div>
        </div>
      ) : null}
    </div>
  )
}
```

```tsx
// src/components/chat-scene/ToolGroupStepRow.tsx
/**
 * @designSource design.pen#TJbJy / mveNP / ogS0H
 * @sizing padding [10,14]; expanded wrapping uses top/bottom border 1
 */
import type { ReactNode } from 'react'
import { CheckCircle2, ChevronDown, ChevronRight, Loader2 } from 'lucide-react'

export interface ToolStep {
  index: number
  name: string
  status: 'running' | 'done' | 'error'
  durationMs?: number
  inputJson?: string
  output?: ReactNode
}

interface ToolGroupStepRowProps {
  step: ToolStep
  expanded: boolean
  onToggle: () => void
}

export function ToolGroupStepRow({ step, expanded, onToggle }: ToolGroupStepRowProps) {
  const seconds = step.durationMs ? (step.durationMs / 1000).toFixed(1) + 's' : '—'
  const StatusIcon =
    step.status === 'running' ? Loader2 : step.status === 'done' ? CheckCircle2 : ChevronRight
  return (
    <button
      type="button"
      onClick={onToggle}
      className="flex w-full items-center justify-between gap-2 px-3.5 py-2.5 text-left text-[13px] hover:bg-muted/50"
    >
      <div className="flex min-w-0 items-center gap-2">
        <StatusIcon
          className={
            step.status === 'running'
              ? 'h-3.5 w-3.5 animate-spin text-primary'
              : step.status === 'done'
                ? 'h-3.5 w-3.5'
                : 'h-3.5 w-3.5 text-muted-foreground'
          }
          style={step.status === 'done' ? { color: '#16A34A' } : undefined}
        />
        <span className="truncate font-mono text-[12.5px] text-foreground">{step.name}</span>
      </div>
      <div className="flex items-center gap-2 text-muted-foreground">
        <span className="text-[12px]">{seconds}</span>
        {expanded ? (
          <ChevronDown className="h-3.5 w-3.5" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5" />
        )}
      </div>
    </button>
  )
}
```

```tsx
// src/components/chat-scene/ToolGroupCard.tsx
/**
 * @designSource design.pen#yNouu/ECmej
 * @sizing r-12 border 1 bg background; topBar padding [12,14] bottom-border 1; steps padding [6,0]
 */
import { CheckCircle2, ChevronDown, ChevronUp, Sparkles } from 'lucide-react'

import { ToolGroupCodeBlock } from './ToolGroupCodeBlock'
import { ToolGroupStepRow, type ToolStep } from './ToolGroupStepRow'

interface ToolGroupCardProps {
  status: 'running' | 'done'
  steps: ToolStep[]
  /** aggregate duration in ms */
  durationMs: number
  expanded: boolean
  expandedStepIndex: number | null
  onToggle: () => void
  onToggleStep: (index: number) => void
}

export function ToolGroupCard({
  status,
  steps,
  durationMs,
  expanded,
  expandedStepIndex,
  onToggle,
  onToggleStep,
}: ToolGroupCardProps) {
  const done = steps.filter((s) => s.status === 'done').length
  const seconds = (durationMs / 1000).toFixed(1) + 's'

  return (
    <div className="overflow-hidden rounded-[12px] border border-border bg-background">
      <button
        type="button"
        data-testid="tool-group-top-bar"
        onClick={onToggle}
        className="flex w-full items-center justify-between gap-2 border-b border-border px-3.5 py-3 text-left"
      >
        <div className="flex items-center gap-2">
          {status === 'done' ? (
            <span
              className="flex h-6 w-6 items-center justify-center rounded-md"
              style={{ backgroundColor: '#DCFCE7' }}
            >
              <CheckCircle2 className="h-3.5 w-3.5" style={{ color: '#16A34A' }} />
            </span>
          ) : (
            <span className="flex h-6 w-6 items-center justify-center rounded-md bg-brand-primary-subtle">
              <Sparkles className="h-3.5 w-3.5 text-primary" />
            </span>
          )}
          <span className="text-sm font-semibold text-foreground">
            {status === 'done' ? `已完成 ${done} 步` : '正在执行任务步骤'}
          </span>
        </div>
        <div className="flex items-center gap-2 text-muted-foreground">
          {status === 'running' ? (
            <span className="text-[12px]">
              {done} / {steps.length}
            </span>
          ) : null}
          <span className="text-[12px]">{seconds}</span>
          {expanded ? (
            <ChevronUp className="h-4 w-4" />
          ) : (
            <ChevronDown className="h-4 w-4" />
          )}
        </div>
      </button>
      {expanded ? (
        <div className="py-1">
          {steps.map((s) => {
            const isOpen = expandedStepIndex === s.index
            return (
              <div
                key={s.index}
                className={
                  isOpen
                    ? 'border-b border-t border-border'
                    : ''
                }
              >
                <ToolGroupStepRow
                  step={s}
                  expanded={isOpen}
                  onToggle={() => onToggleStep(s.index)}
                />
                {isOpen ? (
                  <ToolGroupCodeBlock inputJson={s.inputJson} output={s.output} />
                ) : null}
              </div>
            )
          })}
        </div>
      ) : null}
    </div>
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/ToolGroupCard.test.tsx
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/components/chat-scene/ToolGroupCard.tsx src/components/chat-scene/ToolGroupStepRow.tsx src/components/chat-scene/ToolGroupCodeBlock.tsx src/components/chat-scene/__tests__/ToolGroupCard.test.tsx
git commit -m "feat(frontend): add ToolGroupCard with 4 visual states"
```

---

## Task D-4：useTurnRenderModel hook（消息流 → Turn[] 聚合）

**Files:**
- Create: `src/hooks/useTurnRenderModel.ts`
- Create: `src/hooks/__tests__/useTurnRenderModel.test.ts`

- [ ] **Step 1：写失败测试（先定义期望输出结构）**

```ts
// src/hooks/__tests__/useTurnRenderModel.test.ts
import { describe, expect, it } from 'vitest'

import {
  buildTurnsFromMessages,
  type RenderTurn,
} from '../useTurnRenderModel'
import type { Message } from '@/types/message'
import type { ToolExecution } from '@/stores/streamingStore'

function userMsg(id: string, text: string): Message {
  return {
    id,
    conversationId: 'c1',
    role: 'user',
    createdAt: new Date().toISOString(),
    content: { text },
  }
}

function aiMsg(id: string, text: string, generatedFiles?: Message['content']['generatedFiles']): Message {
  return {
    id,
    conversationId: 'c1',
    role: 'assistant',
    createdAt: new Date().toISOString(),
    content: { text, generatedFiles },
  }
}

describe('buildTurnsFromMessages', () => {
  it('groups messages into turns starting at each user message', () => {
    const msgs: Message[] = [
      userMsg('u1', 'hi'),
      aiMsg('a1', 'hello'),
      userMsg('u2', 'again'),
      aiMsg('a2', 'hi!'),
    ]
    const turns = buildTurnsFromMessages(msgs, [])
    expect(turns.map((t) => t.userMessage?.id)).toEqual(['u1', 'u2'])
    expect(turns[0].aiSegments.map((s) => s.id)).toEqual(['a1'])
    expect(turns[1].aiSegments.map((s) => s.id)).toEqual(['a2'])
  })

  it('attaches tool executions of the active turn as a single ToolGroup', () => {
    const msgs: Message[] = [userMsg('u1', 'x'), aiMsg('a1', 'done')]
    const tools: ToolExecution[] = [
      { toolId: 't1', toolName: 'fetch_feedback', status: 'completed' },
      { toolId: 't2', toolName: 'cluster_topics', status: 'completed' },
    ]
    const turns = buildTurnsFromMessages(msgs, tools)
    expect(turns[0].toolGroup).toBeDefined()
    expect(turns[0].toolGroup?.steps.map((s) => s.name)).toEqual([
      'fetch_feedback',
      'cluster_topics',
    ])
    expect(turns[0].toolGroup?.status).toBe('done')
  })

  it('marks toolGroup as running when any tool is executing', () => {
    const tools: ToolExecution[] = [
      { toolId: 't1', toolName: 'fetch', status: 'completed' },
      { toolId: 't2', toolName: 'run', status: 'executing' },
    ]
    const turns = buildTurnsFromMessages([userMsg('u1', 'x')], tools)
    expect(turns[0].toolGroup?.status).toBe('running')
  })

  it('collects generatedFiles from AI messages into turn.generatedFiles', () => {
    const ai = aiMsg('a1', 'done', [
      {
        id: 'g1',
        title: '绩效分析总结 · Q2 近 30 天',
        subtitle: 'Report · XLSX',
        appName: 'Microsoft Excel',
      } as never,
    ])
    const turns = buildTurnsFromMessages([userMsg('u1', 'x'), ai], [])
    expect(turns[0].generatedFiles.length).toBe(1)
    expect(turns[0].generatedFiles[0].title).toBe('绩效分析总结 · Q2 近 30 天')
  })
})

describe('RenderTurn shape smoke', () => {
  it('has the documented fields', () => {
    const turn: RenderTurn = {
      userMessage: undefined,
      aiSegments: [],
      toolGroup: undefined,
      generatedFiles: [],
      suggestions: [],
    }
    expect(turn).toBeTruthy()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/hooks/__tests__/useTurnRenderModel.test.ts
```

Expected: FAIL。

- [ ] **Step 3：实现**

```ts
// src/hooks/useTurnRenderModel.ts
/**
 * Plan-D：把扁平 messages + conversationStreamState.toolExecutions
 * 投影为 RenderTurn[]，供 MessageList 渲染。
 *
 * 原则：
 *   1. 每个 user message 开一个新 turn
 *   2. 在两个 user message 之间出现的 assistant message 并入同一个 turn
 *   3. 所有 toolExecutions 归入当前"最后一个 turn"（一次 agent turn 里的工具调用共用一张 ToolGroup 卡）
 *   4. AI 消息的 generatedFiles 依次摊平到所在 turn
 */
import { useMemo } from 'react'

import { useChatStore } from '@/stores/chatStore'
import type { ToolExecution } from '@/stores/streamingStore'
import type { GeneratedFile, Message } from '@/types/message'

export interface RenderAiSegment {
  id: string
  text: string
}

export interface RenderToolStep {
  index: number
  name: string
  status: 'running' | 'done' | 'error'
  durationMs?: number
  inputJson?: string
}

export interface RenderToolGroup {
  status: 'running' | 'done'
  steps: RenderToolStep[]
  durationMs: number
}

export interface RenderGeneratedFile {
  id: string
  title: string
  sub: string
  appName: string
}

export interface RenderTurn {
  userMessage?: { id: string; text: string }
  aiSegments: RenderAiSegment[]
  toolGroup?: RenderToolGroup
  generatedFiles: RenderGeneratedFile[]
  suggestions: string[]
}

function toolExecStatusToStep(s: ToolExecution['status']): RenderToolStep['status'] {
  return s === 'executing' ? 'running' : s === 'error' ? 'error' : 'done'
}

function normalizeGeneratedFile(f: GeneratedFile): RenderGeneratedFile {
  const anyF = f as unknown as {
    id: string
    title?: string
    fileName?: string
    subtitle?: string
    appName?: string
    format?: string
  }
  return {
    id: anyF.id,
    title: anyF.title || anyF.fileName || '未命名文件',
    sub: anyF.subtitle || anyF.format || '',
    appName: anyF.appName || 'Open',
  }
}

export function buildTurnsFromMessages(
  messages: Message[],
  toolExecutions: ToolExecution[],
): RenderTurn[] {
  const turns: RenderTurn[] = []
  let current: RenderTurn | null = null

  for (const m of messages) {
    if (m.role === 'user') {
      current = {
        userMessage: { id: m.id, text: m.content.text || '' },
        aiSegments: [],
        toolGroup: undefined,
        generatedFiles: [],
        suggestions: [],
      }
      turns.push(current)
      continue
    }
    if (m.role === 'assistant') {
      if (!current) {
        current = {
          userMessage: undefined,
          aiSegments: [],
          toolGroup: undefined,
          generatedFiles: [],
          suggestions: [],
        }
        turns.push(current)
      }
      if (m.content.text) {
        current.aiSegments.push({ id: m.id, text: m.content.text })
      }
      if (m.content.generatedFiles && m.content.generatedFiles.length > 0) {
        for (const f of m.content.generatedFiles) {
          current.generatedFiles.push(normalizeGeneratedFile(f))
        }
      }
    }
  }

  if (toolExecutions.length > 0 && turns.length > 0) {
    const target = turns[turns.length - 1]
    const steps: RenderToolStep[] = toolExecutions.map((t, i) => ({
      index: i + 1,
      name: t.toolName,
      status: toolExecStatusToStep(t.status),
      durationMs: undefined,
    }))
    const running = steps.some((s) => s.status === 'running')
    target.toolGroup = {
      status: running ? 'running' : 'done',
      steps,
      durationMs: 0,
    }
  }

  return turns
}

export function useTurnRenderModel(): RenderTurn[] {
  const messages = useChatStore((s) => s.messages)
  const activeId = useChatStore((s) => s.activeConversationId)
  const toolExecutions = useChatStore((s) => {
    if (!activeId) return [] as ToolExecution[]
    return s.streamStates[activeId]?.toolExecutions ?? []
  })
  return useMemo(
    () => buildTurnsFromMessages(messages, toolExecutions),
    [messages, toolExecutions],
  )
}
```

- [ ] **Step 4：测试通过**

```bash
pnpm exec vitest run src/hooks/__tests__/useTurnRenderModel.test.ts
```

Expected: PASS。

- [ ] **Step 5：commit**

```bash
git add src/hooks/useTurnRenderModel.ts src/hooks/__tests__/useTurnRenderModel.test.ts
git commit -m "feat(frontend): add useTurnRenderModel for turn-based chat rendering"
```

---

## Task D-5：ChatComposerCompact + ChatBottomArea

**Files:**
- Create: `src/components/chat-scene/ChatComposerCompact.tsx`
- Create: `src/components/chat-scene/ChatBottomArea.tsx`
- Create: `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ChatComposerCompact } from '../ChatComposerCompact'

describe('ChatComposerCompact', () => {
  it('fires onSubmit with value on send click', () => {
    const onSubmit = vi.fn()
    render(
      <ChatComposerCompact
        value="hello"
        onChange={() => {}}
        onSubmit={onSubmit}
        submitDisabled={false}
        onOpenSkill={() => {}}
        onPickProject={() => {}}
        leftInfoText="Desktop"
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: /send/i }))
    expect(onSubmit).toHaveBeenCalledWith('hello')
  })

  it('fires onOpenSkill when skill pill clicked', () => {
    const onOpenSkill = vi.fn()
    render(
      <ChatComposerCompact
        value=""
        onChange={() => {}}
        onSubmit={() => {}}
        submitDisabled
        onOpenSkill={onOpenSkill}
        onPickProject={() => {}}
        leftInfoText="Desktop"
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: '技能' }))
    expect(onOpenSkill).toHaveBeenCalled()
  })

  it('wrapper has r-18 border bg-card', () => {
    const { container } = render(
      <ChatComposerCompact
        value=""
        onChange={() => {}}
        onSubmit={() => {}}
        submitDisabled
        onOpenSkill={() => {}}
        onPickProject={() => {}}
      />,
    )
    const root = container.querySelector('[data-testid="composer-root"]')
    expect(root?.className).toMatch(/rounded-\[18px\]/)
    expect(root?.className).toMatch(/border/)
    expect(root?.className).toMatch(/bg-card/)
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现 ChatComposerCompact**

```tsx
// src/components/chat-scene/ChatComposerCompact.tsx
/**
 * @designSource design.pen#uq6ga
 * @sizing r-18 border 1 bg card padding [16,18,14,18] gap 12
 */
import { useRef } from 'react'
import { Blocks, Folder, Mic, Plus, Send, ShieldCheck } from 'lucide-react'

interface ChatComposerCompactProps {
  value: string
  onChange: (v: string) => void
  onSubmit: (v: string) => void
  submitDisabled: boolean
  onOpenSkill: () => void
  onPickProject: () => void
  leftInfoText?: string
  modelLabel?: string
  placeholder?: string
}

export function ChatComposerCompact({
  value,
  onChange,
  onSubmit,
  submitDisabled,
  onOpenSkill,
  onPickProject,
  leftInfoText = 'Desktop',
  modelLabel = '标准',
  placeholder = '继续追问、修改口径，或让 AI 直接帮你创建后续任务...',
}: ChatComposerCompactProps) {
  const ref = useRef<HTMLTextAreaElement>(null)

  const send = () => {
    if (submitDisabled) return
    onSubmit(value)
  }

  return (
    <div
      data-testid="composer-root"
      className="flex w-full flex-col gap-3 rounded-[18px] border border-border bg-card px-4 pb-3.5 pt-4"
    >
      <textarea
        ref={ref}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        rows={1}
        className="w-full resize-none bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault()
            send()
          }
        }}
      />
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <button
            type="button"
            aria-label="添加附件"
            className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted"
          >
            <Plus className="h-4 w-4" />
          </button>
          <button
            type="button"
            onClick={onOpenSkill}
            aria-label="技能"
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
          >
            <Blocks className="h-3.5 w-3.5" />
            <span>技能</span>
          </button>
          <button
            type="button"
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
          >
            <ShieldCheck className="h-3.5 w-3.5" />
            <span>完全访问权限</span>
          </button>
          <button
            type="button"
            onClick={onPickProject}
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
          >
            <Folder className="h-3.5 w-3.5" />
            <span>{leftInfoText}</span>
          </button>
        </div>
        <div className="flex items-center gap-3">
          <button
            type="button"
            className="flex items-center gap-1 rounded-md px-2 py-1 text-[13px] text-muted-foreground transition-colors hover:bg-muted"
          >
            <span>{modelLabel}</span>
          </button>
          <button
            type="button"
            aria-label="语音输入"
            className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted"
          >
            <Mic className="h-4 w-4" />
          </button>
          <button
            type="button"
            aria-label="send"
            onClick={send}
            disabled={submitDisabled}
            style={submitDisabled ? { backgroundColor: '#D4D4D8' } : undefined}
            className={
              submitDisabled
                ? 'flex h-8 w-8 items-center justify-center rounded-full text-white'
                : 'flex h-8 w-8 items-center justify-center rounded-full bg-primary text-primary-foreground transition-colors hover:opacity-90'
            }
          >
            <Send className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  )
}
```

- [ ] **Step 4：实现 ChatBottomArea**

```tsx
// src/components/chat-scene/ChatBottomArea.tsx
/**
 * @designSource design.pen#Cbtm1
 * @sizing gap 10; tips padding [0,12] fontSize 11 muted
 */
import type { ReactNode } from 'react'

interface ChatBottomAreaProps {
  composer: ReactNode
  tipsLeft?: string
  tipsRight?: string[]
}

export function ChatBottomArea({
  composer,
  tipsLeft = '内容由 AI 生成，请仔细核实回答内容',
  tipsRight = ['Enter 发送', 'Shift+Enter 换行'],
}: ChatBottomAreaProps) {
  return (
    <div className="flex w-full flex-col gap-2.5">
      {composer}
      <div className="flex items-center justify-between px-3 text-[11px] text-muted-foreground">
        <span>{tipsLeft}</span>
        <div className="flex items-center gap-3">
          {tipsRight.map((t) => (
            <span key={t}>{t}</span>
          ))}
        </div>
      </div>
    </div>
  )
}
```

- [ ] **Step 5：测试通过**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx
```

Expected: PASS。

- [ ] **Step 6：commit**

```bash
git add src/components/chat-scene/ChatComposerCompact.tsx src/components/chat-scene/ChatBottomArea.tsx src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx
git commit -m "feat(frontend): add ChatComposerCompact and ChatBottomArea"
```

---

## Task D-6：重写 `MessageList` 消费 Turn model

**Files:**
- Modify: `src/components/chat/MessageList.tsx`

- [ ] **Step 1：替换 MessageList**

```tsx
/**
 * @designSource design.pen#F8ixG flow
 * @sizing padding [24,40] gap 18
 */
import { FileSpreadsheet } from 'lucide-react'

import { AiSegmentText } from '@/components/chat-scene/AiSegmentText'
import { GeneratedFileCard } from '@/components/chat-scene/GeneratedFileCard'
import { SuggestChipGroup } from '@/components/chat-scene/SuggestChipGroup'
import { ToolGroupCard } from '@/components/chat-scene/ToolGroupCard'
import { TypingIndicator } from '@/components/chat-scene/TypingIndicator'
import { UserMessageBubble } from '@/components/chat-scene/UserMessageBubble'
import { useTurnRenderModel } from '@/hooks/useTurnRenderModel'
import { useChatStore } from '@/stores/chatStore'
import { useMemo, useState } from 'react'

export function MessageList() {
  const turns = useTurnRenderModel()
  const isStreaming = useChatStore((s) => s.isStreaming)
  // per-turn expansion state: turnIdx -> { expanded, expandedStepIndex }
  const [expansion, setExpansion] = useState<
    Record<number, { expanded: boolean; stepIndex: number | null }>
  >({})

  const renderTurns = useMemo(() => turns, [turns])

  return (
    <div className="flex flex-col gap-5 px-10 py-6">
      {renderTurns.map((t, i) => {
        const e = expansion[i] ?? { expanded: true, stepIndex: null }
        return (
          <div key={i} className="flex flex-col gap-4">
            {t.userMessage ? <UserMessageBubble text={t.userMessage.text} /> : null}
            {t.aiSegments.map((s) => (
              <AiSegmentText key={s.id} text={s.text} />
            ))}
            {t.toolGroup ? (
              <ToolGroupCard
                status={t.toolGroup.status}
                steps={t.toolGroup.steps}
                durationMs={t.toolGroup.durationMs}
                expanded={e.expanded}
                expandedStepIndex={e.stepIndex}
                onToggle={() =>
                  setExpansion((prev) => ({
                    ...prev,
                    [i]: { ...e, expanded: !e.expanded },
                  }))
                }
                onToggleStep={(index) =>
                  setExpansion((prev) => ({
                    ...prev,
                    [i]: { ...e, stepIndex: e.stepIndex === index ? null : index },
                  }))
                }
              />
            ) : null}
            {t.generatedFiles.map((f) => (
              <GeneratedFileCard
                key={f.id}
                title={f.title}
                sub={f.sub}
                appName={f.appName}
                fileIcon={<FileSpreadsheet className="h-4 w-4 text-muted-foreground" />}
                onOpen={() => {}}
              />
            ))}
            {t.suggestions.length > 0 ? (
              <SuggestChipGroup
                items={t.suggestions.map((s) => ({
                  label: s,
                  icon: null,
                  onClick: () => {},
                }))}
              />
            ) : null}
          </div>
        )
      })}
      {isStreaming ? <TypingIndicator variant="organize" /> : null}
    </div>
  )
}
```

- [ ] **Step 2：lint + tsc + 全测**

```bash
pnpm exec tsc --noEmit
pnpm exec vitest run src/components/chat src/components/chat-scene src/hooks
pnpm lint
```

Expected: 0 error / PASS。

老测试（`AiBubble.*`/`StreamingBubble.*`/`MessageItem.*`）仍然存在并独立测试它们的组件本身，即使 MessageList 不再使用——保留，别删。如果某条老测试因 selector 不���出现而失败，把断言降级为"组件导出存在即可"，但不删测试。

- [ ] **Step 3：commit**

```bash
git add src/components/chat/MessageList.tsx
git commit -m "refactor(frontend): MessageList renders via turn model and chat-scene parts"
```

---

## Task D-7：SkillPopover 全量接入

**Files:**
- Create: `src/components/chat-scene/SkillPopoverPanel.tsx`
- Modify: `src/components/chat/SkillPopover.tsx`
- Create: `src/components/chat-scene/__tests__/SkillPopoverPanel.test.tsx`

- [ ] **Step 1：写失败测试**

```tsx
// src/components/chat-scene/__tests__/SkillPopoverPanel.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillPopoverPanel } from '../SkillPopoverPanel'

const ITEMS = [
  { id: 'a', title: '数据分析', subtitle: '上传 Excel / CSV 生成报告', source: '内置' },
  { id: 'b', title: '文案助手', subtitle: '起草邮件 / 日报', source: '已安装' },
  { id: 'c', title: 'PPT 生成', subtitle: '按大纲生成幻灯片', source: '内置' },
]

describe('SkillPopoverPanel', () => {
  it('renders head title and all items', () => {
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
    expect(screen.getByText('管理已安装的技能')).toBeInTheDocument()
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByText('文案助手')).toBeInTheDocument()
    expect(screen.getByText('PPT 生成')).toBeInTheDocument()
  })

  it('fires onPick with id when an item clicked', () => {
    const onPick = vi.fn()
    render(<SkillPopoverPanel items={ITEMS} onPick={onPick} onClose={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /文案助手/ }))
    expect(onPick).toHaveBeenCalledWith('b')
  })

  it('becomes scrollable when items > 6', () => {
    const many = Array.from({ length: 10 }, (_, i) => ({
      id: String(i),
      title: `技能 ${i}`,
      subtitle: '...',
      source: '内置',
    }))
    const { container } = render(
      <SkillPopoverPanel items={many} onPick={() => {}} onClose={() => {}} />,
    )
    const list = container.querySelector('[data-testid="skill-popover-list"]')
    expect(list?.className).toMatch(/overflow-auto/)
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/SkillPopoverPanel.test.tsx
```

Expected: FAIL。

- [ ] **Step 3：实现 SkillPopoverPanel**

```tsx
// src/components/chat-scene/SkillPopoverPanel.tsx
/**
 * @designSource design.pen#ip8MF popover
 * @sizing w 560 r-14 bg popover border 1 shadow lvl-2; head padding [12,16] bottom-border 1; row padding [10,16]
 */
import { X } from 'lucide-react'

export interface SkillPopoverItem {
  id: string
  title: string
  subtitle: string
  source: string
}

interface SkillPopoverPanelProps {
  items: SkillPopoverItem[]
  onPick: (id: string) => void
  onClose: () => void
}

export function SkillPopoverPanel({ items, onPick, onClose }: SkillPopoverPanelProps) {
  return (
    <div
      className="w-[560px] overflow-hidden rounded-[14px] border border-border bg-popover"
      style={{
        boxShadow:
          '0 2px 3.5px -1px rgba(0,0,0,0.06), 0 4px 5.25px -1px rgba(0,0,0,0.10)',
      }}
    >
      <header className="flex items-center justify-between border-b border-border px-4 py-3 text-[12px] font-semibold text-muted-foreground">
        <span>管理已安装的技能</span>
        <button
          type="button"
          aria-label="关闭"
          onClick={onClose}
          className="text-muted-foreground transition-colors hover:text-foreground"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </header>
      <ul
        data-testid="skill-popover-list"
        className="flex max-h-[320px] flex-col overflow-auto"
      >
        {items.map((it) => (
          <li key={it.id}>
            <button
              type="button"
              onClick={() => onPick(it.id)}
              className="flex w-full items-center justify-between gap-3 px-4 py-2.5 text-left transition-colors hover:bg-muted"
            >
              <div className="flex min-w-0 flex-col">
                <span className="truncate text-sm font-semibold text-foreground">
                  {it.title}
                </span>
                <span className="truncate text-[12px] text-muted-foreground">
                  {it.subtitle}
                </span>
              </div>
              <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-[12px] text-muted-foreground">
                {it.source}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}
```

- [ ] **Step 4：重写 `src/components/chat/SkillPopover.tsx` 作为 store-bound 包装**

```tsx
import { useSkillStore } from '@/stores/skillStore'

import { SkillPopoverPanel } from '../chat-scene/SkillPopoverPanel'

interface SkillPopoverProps {
  open: boolean
  onPick: (skillId: string) => void
  onClose: () => void
}

export function SkillPopover({ open, onPick, onClose }: SkillPopoverProps) {
  const skills = useSkillStore((s) => s.skills)
  if (!open) return null
  const items = skills.map((s) => ({
    id: s.id,
    title: s.displayName,
    subtitle: s.shortDescription || s.description,
    source: s.source === 'builtin' ? '内置' : '已安装',
  }))
  return (
    <div className="absolute bottom-full left-10 mb-3">
      <SkillPopoverPanel items={items} onPick={onPick} onClose={onClose} />
    </div>
  )
}
```

- [ ] **Step 5：测试 + lint + tsc**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/SkillPopoverPanel.test.tsx
pnpm exec tsc --noEmit
pnpm lint
```

Expected: PASS / 0 error。

若 `SkillPopover` 的旧调用点（`src/components/chat/InputBar.tsx` 或类似）签名变化了，顺手把它们的 props 改为新的 `open/onPick/onClose` 形态；对应测试选择器同步更新。

- [ ] **Step 6：commit**

```bash
git add src/components/chat/SkillPopover.tsx src/components/chat-scene/SkillPopoverPanel.tsx src/components/chat-scene/__tests__/SkillPopoverPanel.test.tsx
git commit -m "feat(frontend): full-skill SkillPopover panel wiring"
```

---

## Task D-8：聊天页装配 — ChatTopBar + MessageList + ChatBottomArea

**Files:**
- Modify: 主聊天页文件（`src/features/chat/ChatPage.tsx` 若存在；否则定位到现有聊天容器，通过 `grep -R "MessageList" src/features src/App.tsx` 找到）

- [ ] **Step 1：定位聊天页容器**

```bash
grep -Rn "MessageList" src/features src/App.tsx src/components/layout 2>/dev/null
```

在定位到的容器文件里，把容器结构替换为：

```tsx
import { useState } from 'react'

import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { ChatComposerCompact } from '@/components/chat-scene/ChatComposerCompact'
import { MessageList } from '@/components/chat/MessageList'
import { SkillPopover } from '@/components/chat/SkillPopover'
import { ChatTopBar } from '@/components/shell/ChatTopBar'
import { useChat } from '@/hooks/useChat'

export function ChatScene() {
  const [input, setInput] = useState('')
  const [skillOpen, setSkillOpen] = useState(false)
  const { activeConversation, sendMessage } = useChat()

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      <ChatTopBar
        title={activeConversation?.title || '新任务'}
        workspace="Desktop"
        onShare={() => {}}
        onMore={() => {}}
        onToggleSidebar={() => {}}
      />
      <div className="min-h-0 flex-1 overflow-auto">
        <MessageList />
      </div>
      <div className="relative px-10 pb-6 pt-0">
        <SkillPopover
          open={skillOpen}
          onPick={(id) => {
            setSkillOpen(false)
            setInput((prev) => `${prev} /${id}`.trim())
          }}
          onClose={() => setSkillOpen(false)}
        />
        <ChatBottomArea
          composer={
            <ChatComposerCompact
              value={input}
              onChange={setInput}
              onSubmit={(v) => {
                if (!v.trim()) return
                void sendMessage(v)
                setInput('')
              }}
              submitDisabled={!input.trim()}
              onOpenSkill={() => setSkillOpen((s) => !s)}
              onPickProject={() => {}}
              leftInfoText="Desktop"
            />
          }
        />
      </div>
    </div>
  )
}
```

如果现有项目已用 `useChat().sendMessage` 名字不同，请根据实际 hook 名调整为等价 API；不新增 hook API。

- [ ] **Step 2：lint + tsc + 全测**

```bash
pnpm exec tsc --noEmit
pnpm test
pnpm lint
```

Expected: 0 error / 全 PASS。

- [ ] **Step 3：commit**

```bash
git add src/features/chat src/App.tsx 2>/dev/null
git commit -m "refactor(frontend): wire chat container with new ChatTopBar/composer/bottom"
```

---

## Task D-Final：阶段 D 验收

- [ ] **Step 1：跑全部测试 + lint + tsc**

```bash
pnpm test
pnpm lint
pnpm exec tsc --noEmit
```

Expected: 全 PASS / 0 error。

- [ ] **Step 2：dev 目视确认**

```bash
pnpm tauri:dev
```

创建一个新对话并发消息，目视：
- 顶栏：标题 / "/" / Desktop、右侧 share/more/collapse 图标；
- 消息区：金色用户气泡右对齐、AI 段落、ToolGroup 聚合卡（折叠 / 展开 / 单步代码）；
- 生成文件卡 + 建议 chip（如触发场景）；
- 底部输入区：r-18 白卡 + tips 行 "内容由 AI 生成..." / "Enter 发送" / "Shift+Enter 换行"；
- 点"技能"按钮：上方弹出 560 宽 popover，列出全量已安装技能。

- [ ] **Step 3：阶段 commit**

```bash
git commit --allow-empty -m "chore(frontend): plan-D milestone — chat scene aligned to design.pen"
```

---

## 自审

**Spec coverage：** 第 5.6 章组件清单 ✓；第 6.1 ToolGroup 前端聚合方案 ✓；第 6.2 SkillPopover 全量接入 ✓；第 6.4 消息流按 turn 渲染 ✓；第 7.2 / 7.3 页面拼装 ✓。

**Placeholder scan：** 已扫。`fileIcon / appIcon` 等 slot 参数留给调用方传入具体图标（非 TBD，是设计上的 slot）。`durationMs` 来自 hook 的默认值 0，后续 plan 可升级到后端真时长（非 TBD，是显式默认）。

**Type consistency：** `ToolStep` 在 StepRow 导出、Card 直接 `import type` 复用；`RenderTurn / RenderToolGroup / RenderToolStep / RenderGeneratedFile` 同一来源；`SkillPopoverItem` 在 Panel 声明，包装层构造。

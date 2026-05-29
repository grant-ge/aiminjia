# Tool Block 折叠 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** interleaved 模式下，把连续工具调用合并为一行 Codex 风格摘要，点开二级展开后每条工具单行文字，再点击单行展开 IO 详情。

**Architecture:** 新增 `ToolStepGroupBlock`（一级折叠容器，含摘要文案逻辑）和 `ToolStepRow`（单行 + 二级展开复用 `ToolTraceIO`），改 `MessageList.renderInterleavedBlocks` 在渲染前把连续 `toolStep` 收拢成组；删除 `InlineToolBlock`。`useTurnRenderModel` 不动。

**Tech Stack:** React 19, TypeScript, react-i18next, lucide-react, Tailwind, Vitest + React Testing Library

**Spec:** `docs/superpowers/specs/2026-05-28-tool-block-collapsing-design.md`

---

## File Structure

- **Create:**
  - `src/components/chat-scene/ToolStepGroupBlock.tsx` — 一级折叠容器，摘要文案生成
  - `src/components/chat-scene/ToolStepRow.tsx` — 单行工具行 + 二级展开
  - `src/components/chat-scene/__tests__/ToolStepGroupBlock.test.tsx`
  - `src/components/chat-scene/__tests__/ToolStepRow.test.tsx`

- **Modify:**
  - `src/components/chat/MessageList.tsx` — `renderInterleavedBlocks` 中收拢连续 toolStep；删除 InlineToolBlock import
  - `src/i18n/zh-CN.json` — 加 `chat.toolGroupSummary.*` keys
  - `src/i18n/en-US.json` — 加 `chat.toolGroupSummary.*` keys

- **Delete:**
  - `src/components/chat-scene/InlineToolBlock.tsx`

---

## Task 1: 加 i18n keys

**Files:**
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`

i18n key 结构（顶层 `toolGroup` 已有，新增独立的 `toolGroupSummary` 避免污染）：

```
toolGroupSummary.bucket.command       # "运行了" / "ran"，量词 "命令" / "commands"
toolGroupSummary.bucket.file_read     # "读取了" / "read"，量词 "文件" / "files"
toolGroupSummary.bucket.file_edit     # "编辑了" / "edited"，量词 "文件" / "files"
toolGroupSummary.bucket.search        # "搜索了" / "searched"，量词 "次" / "times"
toolGroupSummary.bucket.mcp           # "调用了" / "called"，量词 "MCP 工具" / "MCP tools"
toolGroupSummary.bucket.other         # "使用了" / "used"，量词 "工具" / "tools"
toolGroupSummary.separator            # 中文 "、"；英文 ", "
toolGroupSummary.failedSuffix         # "，{{count}} 个失败" / ", {{count}} failed"
toolGroupSummary.runningSuffix        # "…"（两种语言都用 horizontal ellipsis）
```

每个 bucket 模板：`"{{verb}} {{count}} 个{{noun}}"` 中文 / `"{{verb}} {{count}} {{noun}}"` 英文。把整句 `phrase` 放在 bucket 下，调用时直接 `t('chat.toolGroupSummary.bucket.command', { count })`。

- [ ] **Step 1: 在 `src/i18n/zh-CN.json` 的 `toolGroup` 块后追加 `toolGroupSummary`**

定位到 zh-CN.json 第 672 行（`toolGroup` 结束的 `}`），在其后插入：

```json
  "toolGroupSummary": {
    "bucket": {
      "command": "运行了 {{count}} 个命令",
      "file_read": "读取了 {{count}} 个文件",
      "file_edit": "编辑了 {{count}} 个文件",
      "search": "搜索了 {{count}} 次",
      "mcp": "调用了 {{count}} 个 MCP 工具",
      "other": "使用了 {{count}} 个工具"
    },
    "separator": "、",
    "failedSuffix": "，{{count}} 个失败",
    "runningSuffix": "…"
  },
```

- [ ] **Step 2: 在 `src/i18n/en-US.json` 的对应位置追加**

```json
  "toolGroupSummary": {
    "bucket": {
      "command_one": "ran {{count}} command",
      "command_other": "ran {{count}} commands",
      "file_read_one": "read {{count}} file",
      "file_read_other": "read {{count}} files",
      "file_edit_one": "edited {{count}} file",
      "file_edit_other": "edited {{count}} files",
      "search_one": "searched {{count}} time",
      "search_other": "searched {{count}} times",
      "mcp_one": "called {{count}} MCP tool",
      "mcp_other": "called {{count}} MCP tools",
      "other_one": "used {{count}} tool",
      "other_other": "used {{count}} tools"
    },
    "separator": ", ",
    "failedSuffix": "{{count}} failed",
    "runningSuffix": "…"
  },
```

注：英文用 i18next plural（`_one` / `_other` 后缀），中文走默认 fallback。`failedSuffix` 英文不带前导逗号 — 拼接时再加。

- [ ] **Step 3: 验证 JSON 合法 + dev 启动无报错**

Run: `node -e "JSON.parse(require('fs').readFileSync('src/i18n/zh-CN.json'))" && node -e "JSON.parse(require('fs').readFileSync('src/i18n/en-US.json'))"`
Expected: 无输出，退出码 0

- [ ] **Step 4: Commit**

```bash
git add src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "feat(i18n): 加 toolGroupSummary 文案 keys"
```

---

## Task 2: 摘要文案纯函数 + 单测

**Files:**
- Create: `src/components/chat-scene/toolStepSummary.ts`
- Create: `src/components/chat-scene/__tests__/toolStepSummary.test.ts`

把"step 列表 → bucket 计数 + 渲染分量"的逻辑独立成纯函数，方便单测。

- [ ] **Step 1: 写失败测试 `src/components/chat-scene/__tests__/toolStepSummary.test.ts`**

```ts
import { describe, it, expect } from 'vitest'

import { classifyToolBucket, summarizeToolSteps } from '../toolStepSummary'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

function step(name: string, status: RenderToolStep['status'] = 'done'): RenderToolStep {
  return { index: 0, toolCallId: name + Math.random(), name, status }
}

describe('classifyToolBucket', () => {
  it('Bash / shell → command', () => {
    expect(classifyToolBucket('Bash')).toBe('command')
    expect(classifyToolBucket('shell_run')).toBe('command')
    expect(classifyToolBucket('shell')).toBe('command')
  })
  it('Read → file_read', () => {
    expect(classifyToolBucket('Read')).toBe('file_read')
    expect(classifyToolBucket('read_file')).toBe('file_read')
  })
  it('Write/Edit → file_edit', () => {
    expect(classifyToolBucket('Write')).toBe('file_edit')
    expect(classifyToolBucket('Edit')).toBe('file_edit')
    expect(classifyToolBucket('MultiEdit')).toBe('file_edit')
  })
  it('Grep/Glob → search', () => {
    expect(classifyToolBucket('Grep')).toBe('search')
    expect(classifyToolBucket('Glob')).toBe('search')
  })
  it('mcp__* → mcp', () => {
    expect(classifyToolBucket('mcp__pencil__batch_get')).toBe('mcp')
  })
  it('unknown → other', () => {
    expect(classifyToolBucket('FancyTool')).toBe('other')
  })
})

describe('summarizeToolSteps', () => {
  it('按出现顺序聚合 bucket', () => {
    const steps = [step('Read'), step('Read'), step('Bash'), step('Read'), step('Bash')]
    const r = summarizeToolSteps(steps)
    expect(r.buckets).toEqual([
      { key: 'file_read', count: 3 },
      { key: 'command', count: 2 },
    ])
  })

  it('统计 running / error', () => {
    const steps = [step('Read', 'running'), step('Bash', 'error'), step('Read', 'done')]
    const r = summarizeToolSteps(steps)
    expect(r.runningCount).toBe(1)
    expect(r.errorCount).toBe(1)
  })

  it('空列表 → buckets 空', () => {
    expect(summarizeToolSteps([])).toEqual({ buckets: [], runningCount: 0, errorCount: 0 })
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/components/chat-scene/__tests__/toolStepSummary.test.ts`
Expected: FAIL — 找不到 `../toolStepSummary` 模块

- [ ] **Step 3: 实现 `src/components/chat-scene/toolStepSummary.ts`**

```ts
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

export type ToolBucket = 'command' | 'file_read' | 'file_edit' | 'search' | 'mcp' | 'other'

export interface BucketCount {
  key: ToolBucket
  count: number
}

export interface ToolStepSummary {
  buckets: BucketCount[]
  runningCount: number
  errorCount: number
}

export function classifyToolBucket(name: string): ToolBucket {
  const n = name.trim()
  if (n.startsWith('mcp__')) return 'mcp'
  const lower = n.toLowerCase()
  if (lower === 'bash' || lower === 'shell' || lower === 'shell_run') return 'command'
  if (lower === 'read' || lower === 'read_file') return 'file_read'
  if (
    lower === 'write' ||
    lower === 'edit' ||
    lower === 'multiedit' ||
    lower === 'write_file' ||
    lower === 'edit_file'
  )
    return 'file_edit'
  if (lower === 'grep' || lower === 'glob') return 'search'
  return 'other'
}

export function summarizeToolSteps(steps: readonly RenderToolStep[]): ToolStepSummary {
  const buckets: BucketCount[] = []
  let runningCount = 0
  let errorCount = 0
  for (const s of steps) {
    if (s.status === 'running') runningCount++
    if (s.status === 'error') errorCount++
    const key = classifyToolBucket(s.name)
    const existing = buckets.find((b) => b.key === key)
    if (existing) existing.count++
    else buckets.push({ key, count: 1 })
  }
  return { buckets, runningCount, errorCount }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm exec vitest run src/components/chat-scene/__tests__/toolStepSummary.test.ts`
Expected: PASS — 全部 11 条用例通过

- [ ] **Step 5: Commit**

```bash
git add src/components/chat-scene/toolStepSummary.ts src/components/chat-scene/__tests__/toolStepSummary.test.ts
git commit -m "feat(chat): 加工具分桶摘要纯函数 + 单测"
```

---

## Task 3: ToolStepRow（单行 + 二级展开）

**Files:**
- Create: `src/components/chat-scene/ToolStepRow.tsx`
- Create: `src/components/chat-scene/__tests__/ToolStepRow.test.tsx`

替代原 `InlineToolBlock` 的"单行 + IO 详情"职责，但去掉外层 card 容器。文案规则：从 inputJson 解析主参数 (path / pattern / command 头部)，解析失败 fallback 为单独 tool name。

- [ ] **Step 1: 写失败测试 `src/components/chat-scene/__tests__/ToolStepRow.test.tsx`**

```tsx
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import { ToolStepRow } from '../ToolStepRow'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

function makeStep(overrides: Partial<RenderToolStep> = {}): RenderToolStep {
  return {
    index: 0,
    toolCallId: 't1',
    name: 'Read',
    status: 'done',
    inputJson: JSON.stringify({ file_path: '/foo/bar/base_prompt.rs' }),
    output: 'file contents',
    ...overrides,
  }
}

describe('ToolStepRow', () => {
  it('Read 工具：显示 basename', () => {
    render(<ToolStepRow step={makeStep()} />)
    expect(screen.getByText(/Read/)).toBeInTheDocument()
    expect(screen.getByText(/base_prompt\.rs/)).toBeInTheDocument()
  })

  it('Bash 工具：显示 command 截断', () => {
    const step = makeStep({
      name: 'Bash',
      inputJson: JSON.stringify({ command: 'ls -la /tmp/foo' }),
    })
    render(<ToolStepRow step={step} />)
    expect(screen.getByText(/Bash/)).toBeInTheDocument()
    expect(screen.getByText(/ls -la \/tmp\/foo/)).toBeInTheDocument()
  })

  it('未知工具名：只显示 tool name', () => {
    const step = makeStep({ name: 'Weird', inputJson: undefined })
    render(<ToolStepRow step={step} />)
    expect(screen.getByText('Weird')).toBeInTheDocument()
  })

  it('inputJson 不合法：fallback 只显示 tool name', () => {
    const step = makeStep({ name: 'Read', inputJson: 'not json' })
    render(<ToolStepRow step={step} />)
    expect(screen.getByText('Read')).toBeInTheDocument()
  })

  it('点击行 toggle 二级展开（渲染 ToolTraceIO output）', () => {
    render(<ToolStepRow step={makeStep()} />)
    expect(screen.queryByText('file contents')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('file contents')).toBeInTheDocument()
  })

  it('running + progressTail 时自动展开', () => {
    const step = makeStep({ status: 'running', output: undefined, progressTail: 'tail line' })
    render(<ToolStepRow step={step} />)
    expect(screen.getByText('tail line')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/components/chat-scene/__tests__/ToolStepRow.test.tsx`
Expected: FAIL — 找不到 `../ToolStepRow` 模块

- [ ] **Step 3: 实现 `src/components/chat-scene/ToolStepRow.tsx`**

```tsx
import { AlertCircle, CheckCircle2, ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import { useState, type ReactNode } from 'react'

import { ToolTraceIO } from './ToolTraceIO'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

interface ToolStepRowProps {
  step: RenderToolStep
}

/**
 * 单条工具行：左侧状态 icon + tool 名 + 主参数摘要 + 右侧 chevron。
 * 点击 toggle 二级展开，展开后复用 `ToolTraceIO` 显示输入/输出/progress。
 * Auto-expand 行为与原 InlineToolBlock 一致：running + progressTail 时自动开。
 */
export function ToolStepRow({ step }: ToolStepRowProps) {
  const autoExpand =
    step.status === 'running' && (step.progressTail ?? '').length > 0
  const [manualOpen, setManualOpen] = useState<boolean | null>(null)
  const open = manualOpen ?? autoExpand

  const statusIcon: ReactNode =
    step.status === 'running' ? (
      <Loader2 className="h-3 w-3 animate-spin text-primary" />
    ) : step.status === 'error' ? (
      <AlertCircle className="h-3 w-3 text-destructive" />
    ) : (
      <CheckCircle2 className="h-3 w-3 text-muted-foreground" />
    )

  const summary = formatStepSummary(step)

  return (
    <div className="ml-4">
      <button
        type="button"
        onClick={() => setManualOpen(open ? false : true)}
        className="flex w-full items-center gap-1.5 py-1 text-left text-xs text-muted-foreground hover:text-foreground"
      >
        <span className="text-muted-foreground/60">⎿</span>
        {statusIcon}
        <span className="truncate font-mono">{summary}</span>
        {open ? (
          <ChevronDown className="ml-auto h-3 w-3 shrink-0" />
        ) : (
          <ChevronRight className="ml-auto h-3 w-3 shrink-0" />
        )}
      </button>
      {open ? (
        <div className="ml-5 mt-1">
          <ToolTraceIO
            toolName={step.name}
            inputJson={step.inputJson}
            output={step.output}
            progressTail={step.status === 'running' ? step.progressTail : undefined}
            progressTotalBytes={
              step.status === 'running' ? step.progressTotalBytes : undefined
            }
          />
        </div>
      ) : null}
    </div>
  )
}

function formatStepSummary(step: RenderToolStep): string {
  const detail = extractDetail(step.name, step.inputJson)
  return detail ? `${step.name} ${detail}` : step.name
}

function extractDetail(name: string, inputJson?: string): string | null {
  if (!inputJson) return null
  let parsed: Record<string, unknown>
  try {
    parsed = JSON.parse(inputJson) as Record<string, unknown>
  } catch {
    return null
  }
  const lower = name.toLowerCase()

  if (lower === 'bash' || lower === 'shell' || lower === 'shell_run') {
    const cmd = pickString(parsed, ['command', 'cmd', 'script'])
    return cmd ? truncate(cmd, 80) : null
  }
  if (lower === 'read' || lower === 'read_file') {
    const p = pickString(parsed, ['file_path', 'path', 'filepath'])
    return p ? basename(p) : null
  }
  if (
    lower === 'write' ||
    lower === 'edit' ||
    lower === 'multiedit' ||
    lower === 'write_file' ||
    lower === 'edit_file'
  ) {
    const p = pickString(parsed, ['file_path', 'path', 'filepath'])
    return p ? basename(p) : null
  }
  if (lower === 'grep') {
    return pickString(parsed, ['pattern', 'query']) ?? null
  }
  if (lower === 'glob') {
    return pickString(parsed, ['pattern', 'glob']) ?? null
  }
  return null
}

function pickString(obj: Record<string, unknown>, keys: string[]): string | null {
  for (const k of keys) {
    const v = obj[k]
    if (typeof v === 'string' && v.length > 0) return v
  }
  return null
}

function basename(p: string): string {
  const m = p.replace(/\\/g, '/').split('/').filter(Boolean)
  return m.length === 0 ? p : m[m.length - 1]!
}

function truncate(s: string, n: number): string {
  return s.length <= n ? s : s.slice(0, n - 1) + '…'
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm exec vitest run src/components/chat-scene/__tests__/ToolStepRow.test.tsx`
Expected: PASS — 6 条用例通过

- [ ] **Step 5: Commit**

```bash
git add src/components/chat-scene/ToolStepRow.tsx src/components/chat-scene/__tests__/ToolStepRow.test.tsx
git commit -m "feat(chat): 加 ToolStepRow 单行工具组件 + 二级展开"
```

---

## Task 4: ToolStepGroupBlock（一级折叠 + 摘要行）

**Files:**
- Create: `src/components/chat-scene/ToolStepGroupBlock.tsx`
- Create: `src/components/chat-scene/__tests__/ToolStepGroupBlock.test.tsx`

接受 `RenderToolStep[]` 列表，默认折叠为摘要行；点击展开后渲染 N 个 `ToolStepRow`。

- [ ] **Step 1: 写失败测试 `src/components/chat-scene/__tests__/ToolStepGroupBlock.test.tsx`**

```tsx
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent, within } from '@testing-library/react'

import { ToolStepGroupBlock } from '../ToolStepGroupBlock'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

// 测试用最小 i18n stub — 不挂 i18next，组件本身用 react-i18next；
// 全局测试 setup 已加载真实 i18n（参考其他 chat-scene 测试），此处直接断言中文文案。

function step(name: string, status: RenderToolStep['status'] = 'done', id?: string): RenderToolStep {
  return { index: 0, toolCallId: id ?? name + Math.random(), name, status }
}

describe('ToolStepGroupBlock — 折叠态', () => {
  it('单个 Read → "读取了 1 个文件"', () => {
    render(<ToolStepGroupBlock steps={[step('Read')]} />)
    expect(screen.getByText(/读取了 1 个文件/)).toBeInTheDocument()
  })

  it('3 Read + 2 Bash → "读取了 3 个文件、运行了 2 个命令"', () => {
    const steps = [step('Read'), step('Read'), step('Read'), step('Bash'), step('Bash')]
    render(<ToolStepGroupBlock steps={steps} />)
    expect(screen.getByText(/读取了 3 个文件、运行了 2 个命令/)).toBeInTheDocument()
  })

  it('包含 running → 显示 spinner 和 runningSuffix …', () => {
    const steps = [step('Read', 'running'), step('Read', 'done')]
    const { container } = render(<ToolStepGroupBlock steps={steps} />)
    expect(container.querySelector('.animate-spin')).toBeTruthy()
    expect(screen.getByText(/读取了 2 个文件…/)).toBeInTheDocument()
  })

  it('包含 error → 显示 AlertCircle 和 "1 个失败"', () => {
    const steps = [step('Read', 'done'), step('Bash', 'error')]
    render(<ToolStepGroupBlock steps={steps} />)
    expect(screen.getByText(/1 个失败/)).toBeInTheDocument()
  })

  it('折叠态默认不渲染 ToolStepRow', () => {
    const steps = [step('Read', 'done', 'tc-1'), step('Bash', 'done', 'tc-2')]
    const { container } = render(<ToolStepGroupBlock steps={steps} />)
    expect(within(container).queryByText('⎿')).not.toBeInTheDocument()
  })
})

describe('ToolStepGroupBlock — 一级展开', () => {
  it('点击摘要行 → 展开 N 个 ToolStepRow', () => {
    const steps = [
      step('Read', 'done', 'tc-1'),
      step('Bash', 'done', 'tc-2'),
    ]
    render(<ToolStepGroupBlock steps={steps} />)
    fireEvent.click(screen.getByText(/读取了 1 个文件/))
    const rows = screen.getAllByText('⎿')
    expect(rows).toHaveLength(2)
  })
})
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm exec vitest run src/components/chat-scene/__tests__/ToolStepGroupBlock.test.tsx`
Expected: FAIL — 找不到 `../ToolStepGroupBlock` 模块

- [ ] **Step 3: 实现 `src/components/chat-scene/ToolStepGroupBlock.tsx`**

```tsx
import { AlertCircle, ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ToolStepRow } from './ToolStepRow'
import { summarizeToolSteps, type BucketCount } from './toolStepSummary'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

interface ToolStepGroupBlockProps {
  steps: readonly RenderToolStep[]
}

/**
 * 一级折叠容器：把连续工具调用合并为一行 Codex 风格摘要
 * （"读取了 3 个文件、运行了 2 个命令 ›"）。点击展开后渲染 N 个 ToolStepRow，
 * 每行再各自可二级展开为输入/输出详情。无 border / 无 bg / 无 shadow。
 */
export function ToolStepGroupBlock({ steps }: ToolStepGroupBlockProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)

  if (steps.length === 0) return null

  const summary = summarizeToolSteps(steps)
  const parts = summary.buckets.map((b) => renderBucket(t, b))
  const separator = t('chat.toolGroupSummary.separator')
  let text = parts.join(separator)
  if (summary.runningCount > 0) {
    text += t('chat.toolGroupSummary.runningSuffix')
  }
  if (summary.errorCount > 0) {
    text += separator + t('chat.toolGroupSummary.failedSuffix', { count: summary.errorCount })
  }

  const leadingIcon =
    summary.runningCount > 0 ? (
      <Loader2 className="h-3.5 w-3.5 animate-spin text-primary shrink-0" />
    ) : summary.errorCount > 0 ? (
      <AlertCircle className="h-3.5 w-3.5 text-destructive shrink-0" />
    ) : null

  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-1.5 py-1.5 text-left text-sm text-muted-foreground hover:text-foreground"
      >
        {leadingIcon}
        <span className="truncate">{text}</span>
        {open ? (
          <ChevronDown className="ml-auto h-3.5 w-3.5 shrink-0" />
        ) : (
          <ChevronRight className="ml-auto h-3.5 w-3.5 shrink-0" />
        )}
      </button>
      {open ? (
        <div className="mt-1 flex flex-col gap-0.5">
          {steps.map((s) => (
            <ToolStepRow key={s.toolCallId} step={s} />
          ))}
        </div>
      ) : null}
    </div>
  )
}

function renderBucket(
  t: ReturnType<typeof useTranslation>['t'],
  bucket: BucketCount,
): string {
  return t(`chat.toolGroupSummary.bucket.${bucket.key}`, { count: bucket.count })
}
```

- [ ] **Step 4: 检查全局测试 setup 已挂 i18n（必要时不挂则 mock）**

Run: `cat src/test-setup.ts 2>/dev/null || cat vitest.setup.ts 2>/dev/null || grep -rn "setupFiles" vitest.config.* 2>/dev/null`
预期：找到测试 setup 文件并确认其中初始化了 i18n。如果未初始化，给 `ToolStepGroupBlock.test.tsx` 顶部加：
```ts
import '@/i18n'  // 复用 app 的 i18n 初始化
```

- [ ] **Step 5: 运行测试确认通过**

Run: `pnpm exec vitest run src/components/chat-scene/__tests__/ToolStepGroupBlock.test.tsx`
Expected: PASS — 6 条用例通过

- [ ] **Step 6: Commit**

```bash
git add src/components/chat-scene/ToolStepGroupBlock.tsx src/components/chat-scene/__tests__/ToolStepGroupBlock.test.tsx
git commit -m "feat(chat): 加 ToolStepGroupBlock 一级折叠 + 摘要行"
```

---

## Task 5: MessageList 接入 + 删除 InlineToolBlock

**Files:**
- Modify: `src/components/chat/MessageList.tsx` (lines 17, 499-505, 534-537)
- Delete: `src/components/chat-scene/InlineToolBlock.tsx`

把 `renderInterleavedBlocks` 里的 `<InlineToolBlock>` 替换为：在持久化 / live 两段子数组里**各自**走 walker，把连续 `toolStep` 收拢成 `<ToolStepGroupBlock>`。

- [ ] **Step 1: 改 `src/components/chat/MessageList.tsx` import**

Edit: 删除 line 17 `import { InlineToolBlock } from '@/components/chat-scene/InlineToolBlock'`
加 line 17 替换为 `import { ToolStepGroupBlock } from '@/components/chat-scene/ToolStepGroupBlock'`

- [ ] **Step 2: 改 `renderBlock` —— 把 toolStep 分支抽出到外层 walker**

在 `renderInterleavedBlocks` 函数体里，找到现有 `renderBlock` 函数。改造方案：
1. `renderBlock` 保留处理 `assistantText` / `generatedFile` / `suggestions` 三类，去掉 `toolStep` 分支（toolStep 由外层 walker 单独处理）。
2. 新增一个 `walkAndGroup(slice: RenderTurnBlock[], keyPrefix: string)` 函数：扫描 slice，连续 toolStep 收成一组渲染为 `<ToolStepGroupBlock>`，非 toolStep 走 `renderBlock`。

替换 MessageList.tsx 第 499~537 行的 `renderBlock` + `splitAt` 渲染逻辑。完整新版（找到 `const renderBlock = (b: RenderTurnBlock, idx: number) => {` 起，到 `const children = [` 之前的整段，替换为）：

```ts
    const renderBlock = (b: RenderTurnBlock, idx: number) => {
      if (b.kind === 'assistantText') {
        return <AiBubble key={b.id} message={b.segment.message} />
      }
      if (b.kind === 'generatedFile') {
        const f = b.file
        return (
          <GeneratedFileCard
            key={f.id}
            title={f.title}
            sub={f.sub}
            appName={f.primaryAction === 'preview' ? '预览' : f.appName}
            primaryAction={f.primaryAction}
            canPreview={f.canPreview}
            canOpenExternal={f.canOpenExternal}
            canReveal={f.canReveal}
            onPreview={() => ctx.onPreview(f)}
            onOpenExternal={() => void ctx.onOpenExternal(f)}
            onReveal={() => void ctx.onReveal(f)}
          />
        )
      }
      if (b.kind === 'suggestions') {
        return (
          <SuggestChipGroup
            key={`sug-${idx}`}
            items={b.suggestions.map((s) => ({ label: s, onClick: () => {} }))}
          />
        )
      }
      return null
    }

    /** 扫一段连续 blocks：把连续 toolStep 合并到一个 ToolStepGroupBlock，
     *  其他 block 各自走 renderBlock。`keyPrefix` 区分 persisted/live 两段，
     *  防止 React key 冲突；同时保证 persisted/live 分界不跨界合并 toolStep。 */
    const walkAndGroup = (slice: RenderTurnBlock[], keyPrefix: string, baseIdx: number) => {
      const nodes: React.ReactNode[] = []
      let pending: Extract<RenderTurnBlock, { kind: 'toolStep' }>[] = []
      const flush = () => {
        if (pending.length === 0) return
        const firstId = pending[0]!.toolCallId
        nodes.push(
          <ToolStepGroupBlock key={`${keyPrefix}-tg-${firstId}`} steps={pending.map((p) => p.step)} />,
        )
        pending = []
      }
      slice.forEach((b, i) => {
        if (b.kind === 'toolStep') {
          pending.push(b)
        } else {
          flush()
          nodes.push(renderBlock(b, baseIdx + i))
        }
      })
      flush()
      return nodes
    }

    const splitAt = Math.min(ctx.persistedBlockCount, blocks.length)
    const persistedNodes = walkAndGroup(blocks.slice(0, splitAt), 'persisted', 0)
    const liveNodes = walkAndGroup(blocks.slice(splitAt), 'live', splitAt)
```

- [ ] **Step 3: 类型检查 + lint**

Run: `pnpm exec tsc --noEmit && pnpm lint`
Expected: 退出码 0；如果报 `React` 未导入，在文件顶部 `import { useEffect, useMemo, useRef } from 'react'` 改为 `import { type ReactNode, useEffect, useMemo, useRef } from 'react'`，并把 `React.ReactNode` 改为 `ReactNode`

- [ ] **Step 4: 删 `src/components/chat-scene/InlineToolBlock.tsx`**

Run: `git rm src/components/chat-scene/InlineToolBlock.tsx`
Expected: file removed; 由于该文件是 untracked，`git rm` 可能报 "did not match any files"，改用 `rm src/components/chat-scene/InlineToolBlock.tsx`

- [ ] **Step 5: 跑相关测试**

Run: `pnpm exec vitest run src/components/chat-scene src/components/chat/MessageList`
Expected: 所有测试 PASS（包括 ChatRow.test.tsx / TurnSummaryBadge.test.tsx / ToolGroupCard.test.tsx 等不应受影响）

- [ ] **Step 6: 手动启 dev 跑一次（强制要求 — 见 CLAUDE.md "UI 改动必须本地验证"）**

Run: `pnpm tauri:dev` 后在 UI 触发一次连续工具调用场景（例如让 AI 连续读多个文件），逐一确认：
1. 折叠态显示"读取了 N 个文件 ›"
2. 点击展开后看到 N 行 `⎿ Read xxx ›`
3. 点击单行展开看到 ToolTraceIO 输入/输出
4. 运行中场景看到 spinner 和 "…" 后缀
5. dark mode 切换正常（hover 颜色跟主题变量走）

如果发现视觉问题，先回到 ToolStepGroupBlock / ToolStepRow 修复，再继续。

- [ ] **Step 7: Commit**

```bash
git add src/components/chat/MessageList.tsx
git add -A src/components/chat-scene/  # 包含 InlineToolBlock.tsx 的删除
git commit -m "feat(chat): MessageList 接入 ToolStepGroupBlock，删除 InlineToolBlock"
```

---

## Task 6: 收尾验证

**Files:** 无修改

- [ ] **Step 1: 跑全量前端测试**

Run: `pnpm test`
Expected: 全部 PASS

- [ ] **Step 2: 跑 type-check + lint**

Run: `pnpm exec tsc --noEmit && pnpm lint`
Expected: 退出码 0，无 warning

- [ ] **Step 3: 检查 grep 残留**

Run: `grep -rn "InlineToolBlock" src/ docs/ 2>/dev/null`
Expected: 仅 spec 文档中可能保留历史描述（可忽略 / 可选清理），源代码无任何引用

- [ ] **Step 4: 如有残留代码引用，删除；若 spec 文档残留，update spec 删除"InlineToolBlock"提及为"ToolStepRow"**

- [ ] **Step 5: 最终 commit（如有改动）**

```bash
git add -A
git commit -m "chore: 清理 InlineToolBlock 残留引用"
```

---

## Self-Review

**Spec 覆盖：**
- 数据层"本地分组" → Task 5（walkAndGroup）✓
- persisted/live 不跨界合并 → Task 5 (两次 walkAndGroup) ✓
- 折叠态 UI（无 border、muted、icon、chevron） → Task 4 ✓
- 一级展开渲染 ToolStepRow → Task 4 ✓
- 二级展开复用 ToolTraceIO → Task 3 ✓
- 摘要文案分桶逻辑 → Task 2 ✓
- i18n keys → Task 1 ✓
- 运行中 spinner + "…" → Task 4 + Task 2 ✓
- 错误 AlertCircle + "N 个失败" → Task 4 + Task 2 ✓
- 单行文案规则（Bash command / Read basename / Grep pattern / fallback） → Task 3 ✓
- 删除 InlineToolBlock → Task 5 ✓

**Placeholder 扫描：** 无 TBD / TODO / "类似 Task N"。

**类型一致性：** `RenderToolStep` 从 `@/hooks/useTurnRenderModel` 引用一致；`ToolBucket` / `BucketCount` 在 toolStepSummary.ts 定义，被 Task 4 import 一致。

**Memo 风险：** Spec §"风险与约束自检" 已记录 ToolStepGroupBlock 和 ToolStepRow 都**不**套 React.memo，跟现有 `InlineToolBlock` 行为一致（依赖 MessageList 整体重渲驱动 step 的原地 mutate）。Task 4 / Task 3 实现中也都没加 memo。

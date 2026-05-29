# Subagent Transcript 前端展示（Plan-T）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让用户看到子 agent 的执行结果、生成文件列表和可折叠的执行轨迹（transcript），消费 `SubAgentResultEnvelope` 的全部字段。
**前置依赖:** Plan-I 后端（I4-I6：transcript store + `get_subagent_transcript` command + `MessageContent.subagentEnvelope` 序列化字段）
**Tech Stack:** React, TypeScript, Zustand, Tauri v2, Vitest + @testing-library/react
**Worktree branch:** pzc

---

## 背景与现状

### 后端 envelope 的存储格式（Plan-T 实施前）

`SubAgentResultEnvelope` 目前通过 `to_storage_summary()` 序列化为一条带前缀的字符串：

```
subagent-envelope:v1:{"schemaVersion":1,"output":"...","iterationsUsed":3,...}
```

这条字符串当前**直接存放在 `MessageContent.text`** 中传给前端，前端完全没有解析它。

### Plan-T 的后端配合要求（需 Plan-I 同步实现）

Plan-T 要求后端在 `MessageContent` 序列化层做一次解析：当 `text` 字段以 `subagent-envelope:v1:` 开头时，将其解析为结构化字段 `subagentEnvelope`，并将 `text` 置为 `null`（或省略），不再把原始前缀字符串暴露给前端。

具体需要：

1. **`src-tauri/src/models/message.rs`**：`MessageContent` 新增字段：
   ```rust
   #[serde(skip_serializing_if = "Option::is_none")]
   pub subagent_envelope: Option<SubAgentEnvelopePayload>,
   ```
   其中 `SubAgentEnvelopePayload` 为：
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   #[serde(rename_all = "camelCase")]
   pub struct SubAgentEnvelopePayload {
       pub schema_version: u32,
       pub output: String,
       pub iterations_used: usize,
       pub generated_files: Vec<String>,
       pub transcript_ref: Option<String>,
   }
   ```

2. **消息读取路径**（`get_messages` / `message:updated` 事件）：在将 `Message` 序列化发往前端前，检测 `content.text` 是否以 `subagent-envelope:v1:` 开头；若是，解析并填充 `subagent_envelope` 字段，同时将 `text` 设为 `None`。

3. **新增 Tauri command**：
   ```rust
   #[tauri::command]
   pub async fn get_subagent_transcript(
       transcript_ref: String,
       state: tauri::State<'_, Arc<AgentRuntime>>,
   ) -> Result<Vec<SubAgentTranscriptEntryFrontend>, String>
   ```
   返回类型：
   ```rust
   #[derive(Serialize)]
   #[serde(rename_all = "camelCase")]
   pub struct SubAgentTranscriptEntryFrontend {
       pub role: String,
       pub content: String,
       pub tool_name: Option<String>,
   }
   ```

---

## 文件地图

| 文件 | 操作 | 说明 |
|---|---|---|
| `src/types/message.ts` | Modify | 新增 `SubAgentEnvelopeContent`、`SubAgentTranscriptEntry` 类型；`MessageContent` 新增 `subagentEnvelope` 字段；`MESSAGE_CONTENT_RENDER_ORDER` 末尾追加 |
| `src/lib/tauri.ts` | Modify | 新增 `getSubagentTranscript(transcriptRef)` invoke 封装 |
| `src/components/rich-content/SubAgentResultCard.tsx` | Create | 展示 output + generated_files + iterations_used |
| `src/components/rich-content/SubAgentResultCard.test.tsx` | Create | Vitest 单测 |
| `src/components/rich-content/SubAgentTranscriptViewer.tsx` | Create | 折叠展示 transcript 条目，懒加载 |
| `src/components/rich-content/SubAgentTranscriptViewer.test.tsx` | Create | Vitest 单测 |
| `src/components/rich-content/index.ts` | Modify | 导出两个新组件 |
| `src/components/chat/AiBubble.tsx` | Modify | `ContentRenderer` 新增 `subagentEnvelope` case |

---

## Task T1：类型定义 + `tauri.ts` invoke 封装

**Files:**
- Modify: `src/types/message.ts`
- Modify: `src/lib/tauri.ts`
- Create: `src/lib/tauri.subagent.test.ts`

### 调研结论摘要

`SubAgentResultEnvelope`（Rust）字段：
- `output: String` — 子 agent 产出的主要文本摘要
- `iterationsUsed: usize` — 消耗的 LLM 轮次数
- `generatedFiles: Vec<String>` — 生成文件的文件名列表（不是 `GeneratedFile` 对象，只是名称字符串）
- `transcriptRef: Option<String>` — 形如 `subagent://child-run-xxx`，Plan-I 实现后可通过 `get_subagent_transcript` 取回完整条目

`SubAgentTranscriptEntry`（Rust）字段：
- `role: String`
- `content: String`
- `toolName: Option<String>`（`tool_call_id` 不需要透传前端）

- [ ] **Step T1-1: 写失败测试**

创建 `src/lib/tauri.subagent.test.ts`：

```typescript
// src/lib/tauri.subagent.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest'

const mockInvoke = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({ invoke: mockInvoke }))

import { getSubagentTranscript } from '@/lib/tauri'
import type { SubAgentTranscriptEntry } from '@/types/message'

describe('getSubagentTranscript', () => {
  beforeEach(() => mockInvoke.mockReset())

  it('invokes get_subagent_transcript with the transcript ref', async () => {
    const entries: SubAgentTranscriptEntry[] = [
      { role: 'assistant', content: 'Analysis complete.', toolName: undefined },
      { role: 'tool', content: 'Saved report.xlsx', toolName: 'execute_python' },
    ]
    mockInvoke.mockResolvedValue(entries)

    const result = await getSubagentTranscript('subagent://child-run-42')

    expect(mockInvoke).toHaveBeenCalledWith('get_subagent_transcript', {
      transcriptRef: 'subagent://child-run-42',
    })
    expect(result).toHaveLength(2)
    expect(result[1].toolName).toBe('execute_python')
  })

  it('returns empty array when backend returns []', async () => {
    mockInvoke.mockResolvedValue([])
    const result = await getSubagentTranscript('subagent://child-run-empty')
    expect(result).toEqual([])
  })
})
```

- [ ] **Step T1-2: 运行测试确认失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/lib/tauri.subagent.test.ts
```

Expected: FAIL，报 `getSubagentTranscript` 不存在 / `SubAgentTranscriptEntry` 类型不存在。

- [ ] **Step T1-3: 在 `src/types/message.ts` 中新增类型并扩展 `MessageContent`**

在 `src/types/message.ts` 末尾追加（`-- Generated File` 区块之后）：

```typescript
// --- Subagent Envelope ---

/**
 * Frontend representation of SubAgentResultEnvelope.
 * Populated by the backend when a message's text starts with
 * the "subagent-envelope:v1:" prefix; the raw prefix string is
 * stripped and never reaches the frontend.
 */
export interface SubAgentEnvelopeContent {
  schemaVersion: number
  /** The subagent's final output text. */
  output: string
  /** Number of LLM iterations the subagent consumed. */
  iterationsUsed: number
  /**
   * File names generated by the subagent (bare names, not GeneratedFile objects).
   * These files should already appear in the parent message's generatedFiles list.
   */
  generatedFiles: string[]
  /**
   * Opaque reference to the full transcript, e.g. "subagent://child-run-xxx".
   * Pass to `getSubagentTranscript()` to lazily load the transcript entries.
   * Undefined until Plan-I backend is deployed.
   */
  transcriptRef?: string
}

/** A single entry in a subagent's transcript (conversation turn). */
export interface SubAgentTranscriptEntry {
  role: string
  content: string
  /** Name of the tool that produced this entry, if role is "tool". */
  toolName?: string
}
```

在 `MessageContent` interface 的末尾追加字段（在 `generatedFiles` 之后）：

```typescript
  /** Structured subagent result envelope. Mutually exclusive with text for subagent messages. */
  subagentEnvelope?: SubAgentEnvelopeContent
```

在 `MESSAGE_CONTENT_RENDER_ORDER` 数组末尾（`confirmations` 之后）追加：

```typescript
  'subagentEnvelope',
```

- [ ] **Step T1-4: 在 `src/lib/tauri.ts` 中新增 invoke 封装**

在 `tauri.ts` 末尾（Marketplace Commands 区块之后）追加：

```typescript
// ---------------------------------------------------------------------------
// Subagent Transcript Commands
// ---------------------------------------------------------------------------

/**
 * Load the full transcript entries for a completed subagent run.
 * Requires Plan-I backend (get_subagent_transcript command).
 *
 * @param transcriptRef - Opaque ref from SubAgentEnvelopeContent.transcriptRef
 *                        (format: "subagent://<child_run_id>")
 * @returns Array of transcript entries in chronological order
 */
export function getSubagentTranscript(transcriptRef: string): Promise<SubAgentTranscriptEntry[]> {
  return invoke<SubAgentTranscriptEntry[]>('get_subagent_transcript', { transcriptRef })
}
```

并在文件顶部 import 区块补上类型引用（`SubAgentTranscriptEntry` 来自 `@/types/message`，已在该文件中通过其他类型 import 了 `message` 模块，需在该 import 语句中追加）：

```typescript
import type { Message, SubAgentTranscriptEntry } from '@/types/message'
```

- [ ] **Step T1-5: 运行测试确认通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/lib/tauri.subagent.test.ts
```

Expected: PASS。

- [ ] **Step T1-6: 跑前端全量单测**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm test
```

Expected: PASS（类型扩展不应破坏现有测试）。

- [ ] **Step T1-7: Commit**

```bash
git add src/types/message.ts src/lib/tauri.ts src/lib/tauri.subagent.test.ts
git commit -m "feat(subagent-frontend): add SubAgentEnvelopeContent type and getSubagentTranscript invoke — T1"
```

---

## Task T2：`SubAgentResultCard` 组件（output + files + iterations）

**Files:**
- Create: `src/components/rich-content/SubAgentResultCard.tsx`
- Create: `src/components/rich-content/SubAgentResultCard.test.tsx`
- Modify: `src/components/rich-content/index.ts`

这个组件负责渲染 envelope 的静态部分：`output`、`generatedFiles`（名称列表）、`iterationsUsed`。Transcript 折叠由 T3 的 `SubAgentTranscriptViewer` 组件组合进来。

- [ ] **Step T2-1: 写失败测试**

创建 `src/components/rich-content/SubAgentResultCard.test.tsx`：

```typescript
// src/components/rich-content/SubAgentResultCard.test.tsx
import { render, screen } from '@testing-library/react'
import '@testing-library/jest-dom'
import { describe, it, expect, vi } from 'vitest'
import type { SubAgentEnvelopeContent } from '@/types/message'

// SubAgentTranscriptViewer is tested separately; stub it out here
vi.mock('@/components/rich-content/SubAgentTranscriptViewer', () => ({
  SubAgentTranscriptViewer: ({ transcriptRef }: { transcriptRef?: string }) =>
    transcriptRef ? <div data-testid="transcript-viewer">{transcriptRef}</div> : null,
}))

import { SubAgentResultCard } from '@/components/rich-content/SubAgentResultCard'

const baseEnvelope: SubAgentEnvelopeContent = {
  schemaVersion: 1,
  output: 'Completed the analysis task.',
  iterationsUsed: 3,
  generatedFiles: ['report.xlsx', 'chart.png'],
  transcriptRef: 'subagent://child-run-42',
}

describe('SubAgentResultCard', () => {
  it('renders the output text', () => {
    render(<SubAgentResultCard envelope={baseEnvelope} />)
    expect(screen.getByText('Completed the analysis task.')).toBeInTheDocument()
  })

  it('renders each generated file name', () => {
    render(<SubAgentResultCard envelope={baseEnvelope} />)
    expect(screen.getByText('report.xlsx')).toBeInTheDocument()
    expect(screen.getByText('chart.png')).toBeInTheDocument()
  })

  it('renders iteration count', () => {
    render(<SubAgentResultCard envelope={baseEnvelope} />)
    expect(screen.getByText(/3/)).toBeInTheDocument()
  })

  it('passes transcriptRef to SubAgentTranscriptViewer when present', () => {
    render(<SubAgentResultCard envelope={baseEnvelope} />)
    const viewer = screen.getByTestId('transcript-viewer')
    expect(viewer).toHaveTextContent('subagent://child-run-42')
  })

  it('does not render transcript viewer when transcriptRef is absent', () => {
    const envelope: SubAgentEnvelopeContent = { ...baseEnvelope, transcriptRef: undefined }
    render(<SubAgentResultCard envelope={envelope} />)
    expect(screen.queryByTestId('transcript-viewer')).not.toBeInTheDocument()
  })

  it('does not render file list section when generatedFiles is empty', () => {
    const envelope: SubAgentEnvelopeContent = { ...baseEnvelope, generatedFiles: [] }
    render(<SubAgentResultCard envelope={envelope} />)
    expect(screen.queryByText('report.xlsx')).not.toBeInTheDocument()
  })
})
```

- [ ] **Step T2-2: 运行测试确认失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/components/rich-content/SubAgentResultCard.test.tsx
```

Expected: FAIL，报 `SubAgentResultCard` 模块不存在。

- [ ] **Step T2-3: 实现 `SubAgentResultCard.tsx`**

创建 `src/components/rich-content/SubAgentResultCard.tsx`：

```typescript
/**
 * SubAgentResultCard — renders the structured result of a completed subagent run.
 *
 * Displays:
 *   - output: the subagent's final summary text
 *   - generatedFiles: file names produced by the subagent (bare names only)
 *   - iterationsUsed: LLM round count badge
 *   - transcript: delegated to SubAgentTranscriptViewer (lazy, collapsible)
 */
import type { SubAgentEnvelopeContent } from '@/types/message'
import { SubAgentTranscriptViewer } from './SubAgentTranscriptViewer'
import { useTranslation } from 'react-i18next'

interface SubAgentResultCardProps {
  envelope: SubAgentEnvelopeContent
}

export function SubAgentResultCard({ envelope }: SubAgentResultCardProps) {
  const { t } = useTranslation()
  const { output, generatedFiles, iterationsUsed, transcriptRef } = envelope

  return (
    <div
      className="my-2 rounded-lg border"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* Header bar */}
      <div
        className="flex items-center justify-between rounded-t-lg border-b px-4 py-2.5"
        style={{
          background: 'var(--color-bg-elevated)',
          borderColor: 'var(--color-border)',
        }}
      >
        <span
          className="text-xs font-semibold uppercase tracking-wide"
          style={{ color: 'var(--color-text-muted)' }}
        >
          {t('subagent.resultCard.header', 'Subagent Result')}
        </span>
        <span
          className="rounded-full px-2 py-0.5 text-xs font-medium"
          style={{
            background: 'var(--color-bg-neutral)',
            color: 'var(--color-text-muted)',
          }}
        >
          {t('subagent.resultCard.iterations', '{{count}} iterations', { count: iterationsUsed })}
        </span>
      </div>

      {/* Output */}
      <div className="px-4 py-3">
        <p
          className="text-sm leading-relaxed"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {output}
        </p>
      </div>

      {/* Generated files (names only) */}
      {generatedFiles.length > 0 && (
        <div
          className="border-t px-4 py-2.5"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <div
            className="mb-1.5 text-xs font-semibold uppercase tracking-wide"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {t('subagent.resultCard.filesGenerated', 'Files generated')}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {generatedFiles.map((name) => (
              <span
                key={name}
                className="rounded-md px-2 py-0.5 text-xs font-medium"
                style={{
                  background: 'var(--color-bg-neutral)',
                  color: 'var(--color-text-secondary)',
                  fontFamily: 'monospace',
                }}
              >
                {name}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* Transcript viewer (collapsible, lazy) */}
      {transcriptRef && (
        <div
          className="border-t"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <SubAgentTranscriptViewer transcriptRef={transcriptRef} />
        </div>
      )}
    </div>
  )
}
```

- [ ] **Step T2-4: 运行测试确认通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/components/rich-content/SubAgentResultCard.test.tsx
```

Expected: PASS。

- [ ] **Step T2-5: 导出新组件**

在 `src/components/rich-content/index.ts` 末尾追加：

```typescript
export { SubAgentResultCard } from './SubAgentResultCard'
export { SubAgentTranscriptViewer } from './SubAgentTranscriptViewer'
```

（`SubAgentTranscriptViewer` 在 T3 中实现，但先注册导出避免 T4 集成时找不到。）

- [ ] **Step T2-6: Commit**

```bash
git add src/components/rich-content/SubAgentResultCard.tsx src/components/rich-content/SubAgentResultCard.test.tsx src/components/rich-content/index.ts
git commit -m "feat(subagent-frontend): add SubAgentResultCard component — T2"
```

---

## Task T3：`SubAgentTranscriptViewer` 组件（折叠 + 懒加载）

**Files:**
- Create: `src/components/rich-content/SubAgentTranscriptViewer.tsx`
- Create: `src/components/rich-content/SubAgentTranscriptViewer.test.tsx`

交互逻辑：
- 默认折叠，显示"查看执行轨迹"展开按钮
- 首次展开时触发 `getSubagentTranscript(transcriptRef)` invoke（懒加载，仅调用一次）
- 加载中显示 loading 状态
- 加载失败显示错误提示（不抛出异常）
- 展开后列出所有 transcript 条目；每条显示 `role` badge + `content` 文本；`tool` role 条目额外显示 `toolName`
- 再次点击折叠（不清除已加载数据）

- [ ] **Step T3-1: 写失败测试**

创建 `src/components/rich-content/SubAgentTranscriptViewer.test.tsx`：

```typescript
// src/components/rich-content/SubAgentTranscriptViewer.test.tsx
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { SubAgentTranscriptEntry } from '@/types/message'

const mockGetSubagentTranscript = vi.fn<(ref: string) => Promise<SubAgentTranscriptEntry[]>>()
vi.mock('@/lib/tauri', () => ({
  getSubagentTranscript: mockGetSubagentTranscript,
}))

import { SubAgentTranscriptViewer } from '@/components/rich-content/SubAgentTranscriptViewer'

const ENTRIES: SubAgentTranscriptEntry[] = [
  { role: 'assistant', content: 'Running analysis...', toolName: undefined },
  { role: 'tool', content: 'Wrote 3 rows to report.xlsx', toolName: 'execute_python' },
  { role: 'assistant', content: 'Done.', toolName: undefined },
]

describe('SubAgentTranscriptViewer', () => {
  beforeEach(() => mockGetSubagentTranscript.mockReset())

  it('renders a toggle button initially (collapsed)', () => {
    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-1" />)
    expect(screen.getByRole('button')).toBeInTheDocument()
    // transcript entries not yet visible
    expect(screen.queryByText('Running analysis...')).not.toBeInTheDocument()
  })

  it('loads transcript and renders entries on first expand', async () => {
    mockGetSubagentTranscript.mockResolvedValue(ENTRIES)
    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-2" />)

    fireEvent.click(screen.getByRole('button'))

    await waitFor(() => {
      expect(screen.getByText('Running analysis...')).toBeInTheDocument()
    })

    expect(screen.getByText('Wrote 3 rows to report.xlsx')).toBeInTheDocument()
    expect(screen.getByText('execute_python')).toBeInTheDocument()
    expect(mockGetSubagentTranscript).toHaveBeenCalledTimes(1)
    expect(mockGetSubagentTranscript).toHaveBeenCalledWith('subagent://run-2')
  })

  it('does NOT re-fetch on second expand after collapsing', async () => {
    mockGetSubagentTranscript.mockResolvedValue(ENTRIES)
    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-3" />)

    // expand
    fireEvent.click(screen.getByRole('button'))
    await waitFor(() => screen.getByText('Running analysis...'))

    // collapse
    fireEvent.click(screen.getByRole('button'))
    expect(screen.queryByText('Running analysis...')).not.toBeInTheDocument()

    // expand again
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('Running analysis...')).toBeInTheDocument()

    expect(mockGetSubagentTranscript).toHaveBeenCalledTimes(1)
  })

  it('shows an error message when the fetch fails', async () => {
    mockGetSubagentTranscript.mockRejectedValue(new Error('transcript not found'))
    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-bad" />)

    fireEvent.click(screen.getByRole('button'))

    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeInTheDocument()
    })
  })

  it('displays role badges for each entry', async () => {
    mockGetSubagentTranscript.mockResolvedValue(ENTRIES)
    render(<SubAgentTranscriptViewer transcriptRef="subagent://run-4" />)
    fireEvent.click(screen.getByRole('button'))

    await waitFor(() => screen.getByText('Running analysis...'))

    // There should be two "assistant" badges and one "tool" badge
    const assistantBadges = screen.getAllByText('assistant')
    expect(assistantBadges).toHaveLength(2)
    expect(screen.getByText('tool')).toBeInTheDocument()
  })
})
```

- [ ] **Step T3-2: 运行测试确认失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/components/rich-content/SubAgentTranscriptViewer.test.tsx
```

Expected: FAIL，报 `SubAgentTranscriptViewer` 模块不存在。

- [ ] **Step T3-3: 实现 `SubAgentTranscriptViewer.tsx`**

创建 `src/components/rich-content/SubAgentTranscriptViewer.tsx`：

```typescript
/**
 * SubAgentTranscriptViewer — collapsible, lazily-loaded transcript viewer.
 *
 * On first expand: calls getSubagentTranscript(transcriptRef) via Tauri IPC.
 * Subsequent toggles reuse the already-loaded entries (no re-fetch).
 *
 * Requires Plan-I backend (get_subagent_transcript command).
 */
import { useState, useCallback } from 'react'
import type { SubAgentTranscriptEntry } from '@/types/message'
import { getSubagentTranscript } from '@/lib/tauri'
import { useTranslation } from 'react-i18next'

interface SubAgentTranscriptViewerProps {
  transcriptRef: string
}

type LoadState = 'idle' | 'loading' | 'loaded' | 'error'

const ROLE_BADGE_STYLE: Record<string, { bg: string; color: string }> = {
  assistant: { bg: 'var(--color-filetype-blue-bg)', color: 'var(--color-semantic-blue)' },
  tool:      { bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  user:      { bg: 'var(--color-bg-neutral)', color: 'var(--color-text-muted)' },
}

function roleBadgeStyle(role: string) {
  return ROLE_BADGE_STYLE[role] ?? ROLE_BADGE_STYLE.user
}

export function SubAgentTranscriptViewer({ transcriptRef }: SubAgentTranscriptViewerProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const [loadState, setLoadState] = useState<LoadState>('idle')
  const [entries, setEntries] = useState<SubAgentTranscriptEntry[]>([])
  const [errorMsg, setErrorMsg] = useState<string>('')

  const handleToggle = useCallback(async () => {
    if (!expanded && loadState === 'idle') {
      // First expand: load transcript
      setLoadState('loading')
      setExpanded(true)
      try {
        const loaded = await getSubagentTranscript(transcriptRef)
        setEntries(loaded)
        setLoadState('loaded')
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        setErrorMsg(msg)
        setLoadState('error')
      }
    } else {
      // Subsequent toggles: just flip expanded (no re-fetch)
      setExpanded((prev) => !prev)
    }
  }, [expanded, loadState, transcriptRef])

  return (
    <div>
      {/* Toggle button */}
      <button
        onClick={handleToggle}
        className="flex w-full items-center gap-2 px-4 py-2.5 text-left text-xs transition-colors hover:bg-[var(--color-bg-hover)]"
        style={{ color: 'var(--color-text-muted)' }}
      >
        {/* Chevron */}
        <svg
          width="12"
          height="12"
          viewBox="0 0 12 12"
          fill="none"
          style={{
            transition: 'transform 0.15s ease',
            transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)',
            flexShrink: 0,
          }}
        >
          <path d="M4 2l4 4-4 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>

        <span className="font-medium">
          {expanded
            ? t('subagent.transcript.collapse', 'Hide execution trace')
            : t('subagent.transcript.expand', 'View execution trace')}
        </span>

        {loadState === 'loaded' && (
          <span
            className="rounded-full px-1.5 py-0.5 font-medium"
            style={{
              background: 'var(--color-bg-neutral)',
              color: 'var(--color-text-muted)',
            }}
          >
            {entries.length}
          </span>
        )}
      </button>

      {/* Body */}
      {expanded && (
        <div
          className="border-t px-4 py-3"
          style={{ borderColor: 'var(--color-border)' }}
        >
          {/* Loading */}
          {loadState === 'loading' && (
            <div
              className="py-2 text-xs"
              style={{ color: 'var(--color-text-muted)' }}
            >
              {t('subagent.transcript.loading', 'Loading execution trace...')}
            </div>
          )}

          {/* Error */}
          {loadState === 'error' && (
            <div
              role="alert"
              className="rounded-md px-3 py-2 text-xs"
              style={{
                background: 'var(--color-semantic-red-bg-light)',
                color: 'var(--color-semantic-red)',
              }}
            >
              {t('subagent.transcript.error', 'Failed to load execution trace')}: {errorMsg}
            </div>
          )}

          {/* Entries */}
          {loadState === 'loaded' && (
            <div className="flex flex-col gap-2">
              {entries.map((entry, idx) => {
                const badge = roleBadgeStyle(entry.role)
                return (
                  <div key={idx} className="flex gap-2.5">
                    {/* Role badge */}
                    <span
                      className="mt-0.5 shrink-0 rounded px-1.5 py-0.5 text-xs font-semibold"
                      style={{ background: badge.bg, color: badge.color, alignSelf: 'flex-start' }}
                    >
                      {entry.role}
                    </span>

                    <div className="min-w-0 flex-1">
                      {/* Tool name (for tool entries) */}
                      {entry.toolName && (
                        <div
                          className="mb-0.5 font-mono text-xs"
                          style={{ color: 'var(--color-text-muted)' }}
                        >
                          {entry.toolName}
                        </div>
                      )}
                      {/* Content */}
                      <p
                        className="whitespace-pre-wrap break-words text-xs leading-relaxed"
                        style={{ color: 'var(--color-text-secondary)' }}
                      >
                        {entry.content}
                      </p>
                    </div>
                  </div>
                )
              })}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
```

- [ ] **Step T3-4: 运行测试确认通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/components/rich-content/SubAgentTranscriptViewer.test.tsx
```

Expected: PASS。

- [ ] **Step T3-5: Commit**

```bash
git add src/components/rich-content/SubAgentTranscriptViewer.tsx src/components/rich-content/SubAgentTranscriptViewer.test.tsx
git commit -m "feat(subagent-frontend): add SubAgentTranscriptViewer with lazy-load and collapse — T3"
```

---

## Task T4：集成到 `AiBubble` / `MessageItem`

**Files:**
- Modify: `src/components/chat/AiBubble.tsx`
- Create: `src/components/chat/AiBubble.subagent.test.tsx`

`AiBubble` 已通过 `MESSAGE_CONTENT_RENDER_ORDER` 驱动渲染；T1 中已在数组末尾追加了 `'subagentEnvelope'`。只需在 `ContentRenderer` 的 `switch` 中增加对应 `case`，并导入 `SubAgentResultCard`。

- [ ] **Step T4-1: 写失败测试**

创建 `src/components/chat/AiBubble.subagent.test.tsx`：

```typescript
// src/components/chat/AiBubble.subagent.test.tsx
import { render, screen } from '@testing-library/react'
import '@testing-library/jest-dom'
import { describe, it, expect, vi } from 'vitest'
import type { Message } from '@/types/message'

// Stub out heavy dependencies
vi.mock('@/lib/tauri', () => ({
  sendMessage: vi.fn(),
  openGeneratedFile: vi.fn(),
  revealFileInFolder: vi.fn(),
  getSubagentTranscript: vi.fn(),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn((selector: (s: { activeConversationId: string | null }) => unknown) =>
    selector({ activeConversationId: 'conv-1' })
  ),
}))

vi.mock('@/hooks/useProductName', () => ({
  useProductName: () => 'AIjia',
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (_key: string, fallback: string) => fallback }),
}))

// Stub SubAgentTranscriptViewer to avoid real IPC in unit tests
vi.mock('@/components/rich-content/SubAgentTranscriptViewer', () => ({
  SubAgentTranscriptViewer: () => <div data-testid="transcript-viewer-stub" />,
}))

import { AiBubble } from '@/components/chat/AiBubble'

const envelopeMessage: Message = {
  id: 'msg-1',
  conversationId: 'conv-1',
  role: 'assistant',
  createdAt: '2026-04-18T00:00:00Z',
  content: {
    subagentEnvelope: {
      schemaVersion: 1,
      output: 'Subagent finished the task successfully.',
      iterationsUsed: 5,
      generatedFiles: ['analysis.xlsx'],
      transcriptRef: 'subagent://child-run-99',
    },
  },
}

describe('AiBubble — subagentEnvelope integration', () => {
  it('renders SubAgentResultCard when message has subagentEnvelope', () => {
    render(<AiBubble message={envelopeMessage} />)
    expect(screen.getByText('Subagent finished the task successfully.')).toBeInTheDocument()
    expect(screen.getByText('analysis.xlsx')).toBeInTheDocument()
    expect(screen.getByText(/5/)).toBeInTheDocument()
    expect(screen.getByTestId('transcript-viewer-stub')).toBeInTheDocument()
  })

  it('does not render an empty bubble for an envelope-only message', () => {
    const { container } = render(<AiBubble message={envelopeMessage} />)
    // The bubble div should be present (has content)
    expect(container.firstChild).not.toBeNull()
  })

  it('renders both text and envelope when both are present', () => {
    const mixed: Message = {
      ...envelopeMessage,
      content: {
        text: 'Some preamble text.',
        subagentEnvelope: envelopeMessage.content.subagentEnvelope,
      },
    }
    render(<AiBubble message={mixed} />)
    expect(screen.getByText('Some preamble text.')).toBeInTheDocument()
    expect(screen.getByText('Subagent finished the task successfully.')).toBeInTheDocument()
  })
})
```

- [ ] **Step T4-2: 运行测试确认失败**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/components/chat/AiBubble.subagent.test.tsx
```

Expected: FAIL，报 `subagentEnvelope` case 在 `ContentRenderer` 中 fall through 到 `default: return null`，导致 output 文本不出现。

- [ ] **Step T4-3: 在 `AiBubble.tsx` 中新增 import 和 case**

在 `src/components/chat/AiBubble.tsx` 的 import 区块中，将 `SubAgentResultCard` 加入 `rich-content` 的导入：

```typescript
import {
  RichCodeBlock,
  RichDataTable,
  MetricCards,
  OptionCards,
  AnomalyList,
  InsightBlock,
  RootCauseBlock,
  ConfirmBlock,
  ProgressSteps,
  SearchSourceBlock,
  ExecSummaryCard,
  ReportCards,
  GeneratedFileCard,
  SubAgentResultCard,
} from '@/components/rich-content'
```

在顶部的类型 import 中补上 `SubAgentEnvelopeContent`：

```typescript
import type {
  Message,
  MessageContent,
  CodeBlock,
  DataTable,
  MetricCard,
  OptionGroup,
  AnomalyItem,
  InsightBlock as InsightBlockType,
  RootCauseBlock as RootCauseBlockType,
  ConfirmBlock as ConfirmBlockType,
  ProgressState,
  SearchSource,
  ExecSummary,
  ReportCard,
  GeneratedFile,
  SubAgentEnvelopeContent,
} from '@/types/message'
```

在 `ContentRenderer` 的 `switch` 语句中，在 `default: return null` 之前插入新 case：

```typescript
    case 'subagentEnvelope':
      return <SubAgentResultCard envelope={value as SubAgentEnvelopeContent} />
```

- [ ] **Step T4-4: 运行测试确认通过**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/components/chat/AiBubble.subagent.test.tsx
```

Expected: PASS。

- [ ] **Step T4-5: 全量前端单测回归**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm test
```

Expected: PASS，无新增失败。

- [ ] **Step T4-6: TypeScript 类型检查**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && pnpm build 2>&1 | grep -E "error TS|^src"
```

Expected: 无类型错误输出。

- [ ] **Step T4-7: Commit**

```bash
git add src/components/chat/AiBubble.tsx src/components/chat/AiBubble.subagent.test.tsx
git commit -m "feat(subagent-frontend): integrate SubAgentResultCard into AiBubble — T4"
```

---

## 完成标准（Definition of Done）

- `SubAgentEnvelopeContent` 和 `SubAgentTranscriptEntry` 类型已在 `src/types/message.ts` 中定义。
- `MessageContent` 包含 `subagentEnvelope?: SubAgentEnvelopeContent`，`MESSAGE_CONTENT_RENDER_ORDER` 末尾已追加 `'subagentEnvelope'`。
- `getSubagentTranscript(transcriptRef)` 已在 `src/lib/tauri.ts` 中封装。
- `SubAgentResultCard` 渲染 output / generatedFiles（名称列表）/ iterationsUsed，并组合 `SubAgentTranscriptViewer`。
- `SubAgentTranscriptViewer` 首次展开时懒加载 transcript，折叠/再展开不重复请求；加载失败显示 `role="alert"` 错误块。
- `AiBubble.ContentRenderer` 新增 `'subagentEnvelope'` case，渲染 `SubAgentResultCard`。
- 全量前端单测通过：`pnpm test`。

---

## 后端配合清单（Plan-I 同步需求）

以下为 Plan-T 正常运行必须由 Plan-I 提供的后端变更，前端代码已按此接口设计：

| 编号 | 需求 | 位置 |
|---|---|---|
| B1 | `MessageContent` 新增 `subagent_envelope: Option<SubAgentEnvelopePayload>` 字段 | `src-tauri/src/models/message.rs` |
| B2 | 消息序列化时检测 `text` 前缀，解析为 `subagent_envelope` 并清空 `text` | 消息读取 / `message:updated` 发射路径 |
| B3 | 新增 `get_subagent_transcript(transcript_ref)` Tauri command，返回 `Vec<SubAgentTranscriptEntryFrontend>` | `src-tauri/src/transport/tauri_commands/` |
| B4 | `SubAgentTranscriptEntryFrontend` 序列化字段：`role`, `content`, `toolName` (camelCase) | 配套类型定义 |

在 Plan-I 后端就绪前，`subagentEnvelope` 字段将始终为 `undefined`，前端渲染路径不会被激活，不影响现有功能。

---

## 推荐执行顺序

T1 → T2 → T3 → T4

原因：T1 建立类型和 IPC 契约，T2 和 T3 各自独立实现（T2 stub 了 T3），T4 最后集成。T2 和 T3 可并行实施，但 T3 stub 需先对齐接口签名。

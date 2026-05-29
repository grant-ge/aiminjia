# Generated File Preview Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generated file cards open real files, expose a real action dropdown, and use the existing RightPanel as a wide workspace preview area with artifact switching.

**Architecture:** Keep `GeneratedFileCard` as a presentational event component. Put preview selection in a small Zustand UI store so `MessageList` and `RightPanel` can coordinate without restructuring `ChatArea`; add a secure Tauri `get_file_preview` command for generated and uploaded files. `RightPanel` has a narrow task-monitor mode and a wide preview mode with `FilePreviewPane` on the left and task/artifact navigation on the right.

**Tech Stack:** React 19, TypeScript, Zustand, Radix DropdownMenu, Vitest + Testing Library, Tauri v2, Rust, existing file store and `FileManager`.

---

## Source Spec

- Spec: `docs/superpowers/specs/2026-04-28-generated-file-open-menu-design.md`
- Design decision: use `RightPanel Workspace Preview`, not a desktop modal.
- Current caveat: the worktree already has many unrelated uncommitted changes. Do not revert or overwrite unrelated changes; stage only files touched by the current task.

## File Structure

### Frontend files

- Create: `src/components/chat/generatedFileActions.ts`
  - Central pure helpers for previewable file types, enabled file actions, preview target creation, and primary action selection.
- Create: `src/components/chat/generatedFileActions.test.ts`
  - Unit tests for all helper decisions.
- Create: `src/stores/generatedFilePreviewStore.ts`
  - Zustand store holding the active preview target and close/open operations.
- Create: `src/stores/generatedFilePreviewStore.test.ts`
  - Store tests for open/close and conversation switching safety.
- Modify: `src/types/message.ts`
  - Broaden `GeneratedFile.fileType` to accept preview-oriented values from storage such as `markdown`, `md`, `text`, `jpg`, `jpeg`, while keeping existing values valid.
  - Make `GeneratedFile.actions` tolerant of old messages by allowing it to be optional in UI-facing data.
- Modify: `src/hooks/useTurnRenderModel.ts`
  - Preserve `fileType/actions`, derive `canPreview` and `primaryAction`, keep existing title/sub/appName behavior.
- Modify: `src/hooks/__tests__/useTurnRenderModel.test.ts`
  - Add coverage for metadata preservation and old generated-file records.
- Modify: `src/components/chat-scene/GeneratedFileCard.tsx`
  - Replace the fake chevron button with a real split button and Radix dropdown.
- Modify: `src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx`
  - Add split button and menu action tests.
- Modify: `src/components/chat/MessageList.tsx`
  - Wire `openGeneratedFile`, `revealFileInFolder`, toast failures, and preview-store open.
- Create: `src/components/chat/MessageList.test.tsx`
  - Tests for open/reveal/preview wiring and failure toast.
- Modify: `src/components/chat/RightPanel.tsx`
  - Add narrow/wide layout, `FilePreviewPane`, clickable artifact list, current artifact highlight, conversation filtering, and close preview.
- Create: `src/components/chat/RightPanel.test.tsx`
  - Tests for narrow mode, wide mode, artifact filtering, switching, and close behavior.
- Create: `src/components/chat/FilePreviewPane.tsx`
  - Renders loading, unsupported, error, markdown, text/json/csv, and sandboxed HTML preview states.
- Create: `src/components/chat/FilePreviewPane.test.tsx`
  - Tests for each render state and external-open fallback.
- Modify: `src/lib/tauri.ts`
  - Add `FilePreview` type and `getFilePreview(fileId, conversationId)` wrapper.
- Create: `src/lib/tauri.file-preview.test.ts`
  - Tests Tauri command name and camelCase payload.

### Backend files

- Modify: `src-tauri/src/storage/file_manager.rs`
  - Add a public `resolve_existing_file(stored_path) -> Result<PathBuf>` that uses existing safe path validation and rejects directories/missing files.
- Modify: `src-tauri/src/commands/file.rs`
  - Add preview response types, file-record resolver with metadata, preview classification helper, and `get_file_preview` command.
  - Migrate `open_generated_file` and `reveal_file_in_folder` from `full_path` fallback to `resolve_existing_file`.
  - Add focused helper tests under `#[cfg(test)]`.
- Modify: `src-tauri/src/lib.rs`
  - Register `file::get_file_preview` in `tauri::generate_handler!`.
- Optional follow-up if command migration requires it: `src-tauri/src/transport/tauri_commands/file.rs`
  - Keep adapter behavior aligned with `commands/file.rs`; do not make it the primary command path unless the app has switched command registration to transport adapters.

## Implementation Strategy

Use small commits per task. Prefer subagents with disjoint write sets:

1. Frontend helper/render/card tasks can run before backend preview content exists.
2. Backend secure preview API is independent of RightPanel layout.
3. RightPanel preview shell can use `unsupported`/loading states before real preview content is wired.
4. Final integration must be done after both frontend and backend tasks land.

---

### Task 1: Generated File Action Helpers And Render Model

**Files:**
- Create: `src/components/chat/generatedFileActions.ts`
- Create: `src/components/chat/generatedFileActions.test.ts`
- Modify: `src/types/message.ts`
- Modify: `src/hooks/useTurnRenderModel.ts`
- Modify: `src/hooks/__tests__/useTurnRenderModel.test.ts`

- [ ] **Step 1: Write failing helper tests**

Create `src/components/chat/generatedFileActions.test.ts`:

```ts
import { describe, expect, it } from 'vitest'

import {
  getGeneratedFilePrimaryAction,
  isFileActionEnabled,
  isPreviewableFileType,
  toPreviewTarget,
} from './generatedFileActions'

const conversationId = 'conv-1'

describe('generatedFileActions', () => {
  it.each([
    ['markdown', 'report.md'],
    ['md', 'report.md'],
    ['html', 'preview.html'],
    ['text', 'notes.txt'],
    ['json', 'data.json'],
    ['csv', 'rows.csv'],
  ])('treats %s as previewable', (fileType, fileName) => {
    expect(isPreviewableFileType(fileType, fileName)).toBe(true)
    expect(getGeneratedFilePrimaryAction({ fileType, title: fileName })).toBe('preview')
  })

  it.each([
    ['excel', 'book.xlsx'],
    ['xlsx', 'book.xlsx'],
    ['pdf', 'report.pdf'],
    ['png', 'chart.png'],
    ['jpg', 'photo.jpg'],
    ['py', 'script.py'],
    [undefined, 'unknown.bin'],
  ])('treats %s as external-open by default', (fileType, fileName) => {
    expect(isPreviewableFileType(fileType, fileName)).toBe(false)
    expect(getGeneratedFilePrimaryAction({ fileType, title: fileName })).toBe('open')
  })

  it('falls back to the filename extension when fileType is missing', () => {
    expect(isPreviewableFileType(undefined, 'summary.md')).toBe(true)
    expect(isPreviewableFileType(undefined, 'summary.xlsx')).toBe(false)
  })

  it('treats missing actions as enabled for backward compatibility', () => {
    expect(isFileActionEnabled(undefined, 'open')).toBe(true)
    expect(isFileActionEnabled([], 'open')).toBe(true)
  })

  it('uses explicit disabled actions when actions are present', () => {
    expect(isFileActionEnabled([{ type: 'open', label: 'Open', enabled: false }], 'open')).toBe(false)
    expect(isFileActionEnabled([{ type: 'reveal', label: 'Reveal', enabled: true }], 'reveal')).toBe(true)
  })

  it('creates a preview target bound to the current conversation', () => {
    expect(toPreviewTarget({ id: 'file-1', title: 'report.md', fileType: 'markdown' }, conversationId)).toEqual({
      fileId: 'file-1',
      conversationId,
      fileName: 'report.md',
      fileType: 'markdown',
    })
  })
})
```

- [ ] **Step 2: Run helper test and verify it fails**

Run:

```bash
pnpm vitest run src/components/chat/generatedFileActions.test.ts
```

Expected: FAIL because `generatedFileActions.ts` does not exist.

- [ ] **Step 3: Implement helper module**

Create `src/components/chat/generatedFileActions.ts`:

```ts
import type { FileAction } from '@/types/message'

export type GeneratedFilePrimaryAction = 'preview' | 'open'

export interface PreviewTarget {
  fileId: string
  conversationId: string
  fileName: string
  fileType?: string
}

const PREVIEWABLE_TYPES = new Set(['md', 'markdown', 'html', 'txt', 'text', 'json', 'csv'])

function extensionFromFileName(fileName?: string): string | undefined {
  const ext = fileName?.split('.').pop()?.trim().toLowerCase()
  return ext && ext !== fileName?.toLowerCase() ? ext : undefined
}

function normalizeFileType(fileType?: string, fileName?: string): string | undefined {
  const raw = fileType?.trim().toLowerCase() || extensionFromFileName(fileName)
  if (!raw) return undefined
  if (raw === 'md') return 'markdown'
  if (raw === 'txt') return 'text'
  return raw
}

export function isPreviewableFileType(fileType?: string, fileName?: string): boolean {
  const normalized = normalizeFileType(fileType, fileName)
  return normalized ? PREVIEWABLE_TYPES.has(normalized) : false
}

export function getGeneratedFilePrimaryAction(file: { fileType?: string; title?: string; fileName?: string }): GeneratedFilePrimaryAction {
  return isPreviewableFileType(file.fileType, file.title ?? file.fileName) ? 'preview' : 'open'
}

export function isFileActionEnabled(actions: FileAction[] | undefined, type: FileAction['type']): boolean {
  if (!actions || actions.length === 0) return true
  const action = actions.find((candidate) => candidate.type === type)
  return action?.enabled ?? false
}

export function toPreviewTarget(
  file: { id: string; title?: string; fileName?: string; fileType?: string },
  conversationId: string,
): PreviewTarget {
  return {
    fileId: file.id,
    conversationId,
    fileName: file.title ?? file.fileName ?? '未命名文件',
    fileType: file.fileType,
  }
}
```

- [ ] **Step 4: Broaden generated file types safely**

Modify `src/types/message.ts` generated file area to this shape:

```ts
export type GeneratedFileType =
  | 'excel'
  | 'xlsx'
  | 'html'
  | 'pdf'
  | 'csv'
  | 'json'
  | 'png'
  | 'jpg'
  | 'jpeg'
  | 'py'
  | 'markdown'
  | 'md'
  | 'text'
  | 'txt'
  | string

export interface GeneratedFile {
  id: string
  fileName: string
  filePath: string
  fileType: GeneratedFileType
  fileSize: number
  category: 'report' | 'chart' | 'data' | 'analysis' | 'script' | 'temp' | string
  version: number
  isLatest: boolean
  supersededBy?: string
  createdAt: string
  createdByStep?: number
  description: string
  actions?: FileAction[]
  isDegraded?: boolean
  degradationNotice?: string | null
  requestedFormat?: string
}
```

- [ ] **Step 5: Write failing render model tests**

Append these tests to `src/hooks/__tests__/useTurnRenderModel.test.ts`:

```ts
  it('preserves generated file action metadata for file card interactions', () => {
    const turns = buildTurns([
      user('u1', 'create file'),
      assistant('a1', {
        text: 'done',
        generatedFiles: [
          {
            id: 'gf-md',
            fileName: 'summary.md',
            filePath: 'generated/summary.md',
            fileType: 'markdown',
            fileSize: 2048,
            category: 'report',
            version: 1,
            isLatest: true,
            createdAt: new Date().toISOString(),
            description: 'Summary',
            actions: [
              { type: 'preview', label: 'Preview', enabled: true },
              { type: 'open', label: 'Open', enabled: true },
              { type: 'reveal', label: 'Reveal', enabled: true },
            ],
          },
        ],
      }),
    ])

    expect(turns[0].generatedFiles[0]).toMatchObject({
      id: 'gf-md',
      title: 'summary.md',
      fileType: 'markdown',
      canPreview: true,
      primaryAction: 'preview',
    })
    expect(turns[0].generatedFiles[0].actions).toEqual([
      { type: 'preview', label: 'Preview', enabled: true },
      { type: 'open', label: 'Open', enabled: true },
      { type: 'reveal', label: 'Reveal', enabled: true },
    ])
  })

  it('uses safe defaults for old generated file records without actions', () => {
    const turns = buildTurns([
      user('u1', 'create file'),
      assistant('a1', {
        text: 'done',
        generatedFiles: [
          {
            id: 'gf-xlsx',
            fileName: 'table.xlsx',
            filePath: 'generated/table.xlsx',
            fileType: 'excel',
            fileSize: 4096,
            category: 'data',
            version: 1,
            isLatest: true,
            createdAt: new Date().toISOString(),
            description: 'Table',
          },
        ],
      }),
    ])

    expect(turns[0].generatedFiles[0]).toMatchObject({
      fileType: 'excel',
      actions: [],
      canPreview: false,
      primaryAction: 'open',
    })
  })
```

If the local test helper names differ, adapt the values to the existing helper names in that file while keeping these assertions identical.

- [ ] **Step 6: Run render model test and verify it fails**

Run:

```bash
pnpm vitest run src/hooks/__tests__/useTurnRenderModel.test.ts
```

Expected: FAIL because `RenderGeneratedFile` does not expose `fileType/actions/canPreview/primaryAction` yet.

- [ ] **Step 7: Update render model**

Modify `src/hooks/useTurnRenderModel.ts` imports and interface:

```ts
import type { FileAction, GeneratedFile, Message, SkillCommandBreadcrumb } from '@/types/message'
import { getGeneratedFilePrimaryAction, isPreviewableFileType } from '@/components/chat/generatedFileActions'

export interface RenderGeneratedFile {
  id: string
  title: string
  sub: string
  appName: string
  fileType?: string
  actions: FileAction[]
  canPreview: boolean
  primaryAction: 'preview' | 'open'
}
```

Update `normalizeGeneratedFile` return value:

```ts
function normalizeGeneratedFile(f: GeneratedFile): RenderGeneratedFile {
  const anyF = f as unknown as {
    id: string; title?: string; fileName?: string;
    subtitle?: string; appName?: string; format?: string;
  }
  const title = anyF.title || anyF.fileName || '未命名文件'
  const fileType = typeof f.fileType === 'string' ? f.fileType : undefined

  return {
    id: anyF.id,
    title,
    sub: buildGeneratedFileMeta(f, anyF.format, anyF.subtitle),
    appName: anyF.appName || 'Open',
    fileType,
    actions: f.actions ?? [],
    canPreview: isPreviewableFileType(fileType, title),
    primaryAction: getGeneratedFilePrimaryAction({ fileType, title }),
  }
}
```

- [ ] **Step 8: Run targeted tests and verify pass**

Run:

```bash
pnpm vitest run src/components/chat/generatedFileActions.test.ts src/hooks/__tests__/useTurnRenderModel.test.ts
```

Expected: PASS.

- [ ] **Step 9: Commit Task 1**

```bash
git add src/components/chat/generatedFileActions.ts src/components/chat/generatedFileActions.test.ts src/types/message.ts src/hooks/useTurnRenderModel.ts src/hooks/__tests__/useTurnRenderModel.test.ts
git commit -m "feat: derive generated file actions"
```

---

### Task 2: Split Button Generated File Card

**Files:**
- Modify: `src/components/chat-scene/GeneratedFileCard.tsx`
- Modify: `src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx`

- [ ] **Step 1: Replace the existing card test with split-action coverage**

Update `src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx` with these cases:

```tsx
import '@testing-library/jest-dom'
import { render, screen, fireEvent, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { GeneratedFileCard } from '../GeneratedFileCard'

function renderCard(overrides: Partial<React.ComponentProps<typeof GeneratedFileCard>> = {}) {
  const props: React.ComponentProps<typeof GeneratedFileCard> = {
    title: '绩效分析总结.md',
    sub: '2 KB · 报告',
    appName: 'Preview',
    primaryAction: 'preview',
    canPreview: true,
    canOpenExternal: true,
    canReveal: true,
    onPreview: vi.fn(),
    onOpenExternal: vi.fn(),
    onReveal: vi.fn(),
    ...overrides,
  }
  render(<GeneratedFileCard {...props} />)
  return props
}

describe('GeneratedFileCard', () => {
  it('renders title/sub and a split preview button', () => {
    renderCard()
    expect(screen.getByText('绩效分析总结.md')).toBeInTheDocument()
    expect(screen.getByText('2 KB · 报告')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Preview 绩效分析总结.md' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'More actions for 绩效分析总结.md' })).toBeInTheDocument()
  })

  it('runs preview when preview is the primary action', () => {
    const props = renderCard()
    fireEvent.click(screen.getByRole('button', { name: 'Preview 绩效分析总结.md' }))
    expect(props.onPreview).toHaveBeenCalledTimes(1)
    expect(props.onOpenExternal).not.toHaveBeenCalled()
  })

  it('runs external open when open is the primary action', () => {
    const props = renderCard({ title: 'table.xlsx', appName: 'Open', primaryAction: 'open', canPreview: false })
    fireEvent.click(screen.getByRole('button', { name: 'Open table.xlsx' }))
    expect(props.onOpenExternal).toHaveBeenCalledTimes(1)
    expect(props.onPreview).not.toHaveBeenCalled()
  })

  it('exposes preview, open, and reveal menu actions', async () => {
    const props = renderCard()
    fireEvent.click(screen.getByRole('button', { name: 'More actions for 绩效分析总结.md' }))
    const menu = await screen.findByRole('menu')

    fireEvent.click(within(menu).getByRole('menuitem', { name: /Preview inside/ }))
    expect(props.onPreview).toHaveBeenCalledTimes(1)

    fireEvent.click(screen.getByRole('button', { name: 'More actions for 绩效分析总结.md' }))
    const reopened = await screen.findByRole('menu')
    fireEvent.click(within(reopened).getByRole('menuitem', { name: /Open with default app/ }))
    expect(props.onOpenExternal).toHaveBeenCalledTimes(1)

    fireEvent.click(screen.getByRole('button', { name: 'More actions for 绩效分析总结.md' }))
    const third = await screen.findByRole('menu')
    fireEvent.click(within(third).getByRole('menuitem', { name: /Show in folder/ }))
    expect(props.onReveal).toHaveBeenCalledTimes(1)
  })

  it('disables preview menu item when preview is unavailable', async () => {
    renderCard({ title: 'table.xlsx', appName: 'Open', primaryAction: 'open', canPreview: false })
    fireEvent.click(screen.getByRole('button', { name: 'More actions for table.xlsx' }))
    const menu = await screen.findByRole('menu')
    expect(within(menu).getByRole('menuitem', { name: /Preview unavailable/ })).toHaveAttribute('aria-disabled', 'true')
  })
})
```

- [ ] **Step 2: Run card test and verify it fails**

Run:

```bash
pnpm vitest run src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx
```

Expected: FAIL because new props/menu behavior are not implemented.

- [ ] **Step 3: Implement split button with DropdownMenu**

Modify `src/components/chat-scene/GeneratedFileCard.tsx` imports and props:

```tsx
import type { ReactNode } from 'react'
import { ChevronDown, Eye, ExternalLink, FolderOpen } from 'lucide-react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import type { GeneratedFilePrimaryAction } from '@/components/chat/generatedFileActions'

interface GeneratedFileCardProps {
  title: string
  sub: string
  appName: string
  appIcon?: ReactNode
  primaryAction: GeneratedFilePrimaryAction
  canPreview: boolean
  canOpenExternal?: boolean
  canReveal?: boolean
  onPreview: () => void
  onOpenExternal: () => void
  onReveal: () => void
}
```

Replace the current right-side `<button>` with:

```tsx
      <div className="flex shrink-0 overflow-hidden rounded-full border border-border bg-background text-[13px] text-foreground transition-colors hover:bg-muted">
        <button
          type="button"
          onClick={primaryAction === 'preview' ? onPreview : onOpenExternal}
          aria-label={`${primaryAction === 'preview' ? 'Preview' : 'Open'} ${title}`}
          className="flex items-center gap-2 py-1.5 pl-3 pr-2"
          disabled={primaryAction === 'open' && canOpenExternal === false}
        >
          {appIcon}
          <span>{primaryAction === 'preview' ? 'Preview' : appName}</span>
        </button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={`More actions for ${title}`}
              className="flex items-center border-l border-border px-2 text-muted-foreground hover:text-foreground"
            >
              <ChevronDown className="h-3.5 w-3.5" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-52">
            <DropdownMenuItem disabled={!canPreview} onClick={onPreview}>
              <Eye className="h-3.5 w-3.5" />
              {canPreview ? 'Preview inside' : 'Preview unavailable'}
            </DropdownMenuItem>
            <DropdownMenuItem disabled={canOpenExternal === false} onClick={onOpenExternal}>
              <ExternalLink className="h-3.5 w-3.5" />
              Open with default app
            </DropdownMenuItem>
            <DropdownMenuItem disabled={canReveal === false} onClick={onReveal}>
              <FolderOpen className="h-3.5 w-3.5" />
              Show in folder
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
```

Keep `TiltedFileIcon` and file-label behavior unchanged.

- [ ] **Step 4: Run card test and verify pass**

Run:

```bash
pnpm vitest run src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit Task 2**

```bash
git add src/components/chat-scene/GeneratedFileCard.tsx src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx
git commit -m "feat: add generated file action menu"
```

---

### Task 3: Preview UI Store And MessageList File Actions

**Files:**
- Create: `src/stores/generatedFilePreviewStore.ts`
- Create: `src/stores/generatedFilePreviewStore.test.ts`
- Modify: `src/components/chat/MessageList.tsx`
- Create: `src/components/chat/MessageList.test.tsx`

- [ ] **Step 1: Write failing preview store tests**

Create `src/stores/generatedFilePreviewStore.test.ts`:

```ts
import { beforeEach, describe, expect, it } from 'vitest'

import { useGeneratedFilePreviewStore } from './generatedFilePreviewStore'

const target = {
  fileId: 'gf-1',
  conversationId: 'conv-1',
  fileName: 'summary.md',
  fileType: 'markdown',
}

describe('generatedFilePreviewStore', () => {
  beforeEach(() => {
    useGeneratedFilePreviewStore.getState().closePreview()
  })

  it('opens and closes a preview target', () => {
    useGeneratedFilePreviewStore.getState().openPreview(target)
    expect(useGeneratedFilePreviewStore.getState().target).toEqual(target)

    useGeneratedFilePreviewStore.getState().closePreview()
    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('clears stale preview when the active conversation changes', () => {
    useGeneratedFilePreviewStore.getState().openPreview(target)
    useGeneratedFilePreviewStore.getState().clearIfConversationChanged('conv-2')
    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('keeps preview when the same conversation remains active', () => {
    useGeneratedFilePreviewStore.getState().openPreview(target)
    useGeneratedFilePreviewStore.getState().clearIfConversationChanged('conv-1')
    expect(useGeneratedFilePreviewStore.getState().target).toEqual(target)
  })
})
```

- [ ] **Step 2: Run store test and verify it fails**

Run:

```bash
pnpm vitest run src/stores/generatedFilePreviewStore.test.ts
```

Expected: FAIL because store file does not exist.

- [ ] **Step 3: Implement preview store**

Create `src/stores/generatedFilePreviewStore.ts`:

```ts
import { create } from 'zustand'
import type { PreviewTarget } from '@/components/chat/generatedFileActions'

interface GeneratedFilePreviewState {
  target: PreviewTarget | null
  openPreview: (target: PreviewTarget) => void
  closePreview: () => void
  clearIfConversationChanged: (conversationId: string) => void
}

export const useGeneratedFilePreviewStore = create<GeneratedFilePreviewState>((set, get) => ({
  target: null,
  openPreview: (target) => set({ target }),
  closePreview: () => set({ target: null }),
  clearIfConversationChanged: (conversationId) => {
    const current = get().target
    if (current && current.conversationId !== conversationId) {
      set({ target: null })
    }
  },
}))
```

- [ ] **Step 4: Write failing MessageList tests**

Create `src/components/chat/MessageList.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  openGeneratedFile: vi.fn(),
  revealFileInFolder: vi.fn(),
}))

vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    openGeneratedFile: tauriMock.openGeneratedFile,
    revealFileInFolder: tauriMock.revealFileInFolder,
  }
})

import { MessageList } from './MessageList'
import { useChatStore } from '@/stores/chatStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { useNotificationStore } from '@/stores/notificationStore'

function seedMessages(fileType = 'markdown') {
  useChatStore.setState({
    activeConversationId: 'conv-1',
    messages: [
      {
        id: 'a1',
        conversationId: 'conv-1',
        role: 'assistant',
        createdAt: new Date().toISOString(),
        content: {
          text: 'done',
          generatedFiles: [
            {
              id: 'gf-1',
              fileName: fileType === 'excel' ? 'table.xlsx' : 'summary.md',
              filePath: 'generated/file',
              fileType,
              fileSize: 1024,
              category: 'report',
              version: 1,
              isLatest: true,
              createdAt: new Date().toISOString(),
              description: 'Generated file',
              actions: [
                { type: 'preview', label: 'Preview', enabled: true },
                { type: 'open', label: 'Open', enabled: true },
                { type: 'reveal', label: 'Reveal', enabled: true },
              ],
            },
          ],
        },
      },
    ],
  })
}

describe('MessageList generated file actions', () => {
  beforeEach(() => {
    tauriMock.openGeneratedFile.mockReset().mockResolvedValue(undefined)
    tauriMock.revealFileInFolder.mockReset().mockResolvedValue(undefined)
    useGeneratedFilePreviewStore.getState().closePreview()
    useNotificationStore.setState({ notifications: [] })
    seedMessages()
  })

  it('opens preview mode for previewable files', () => {
    render(<MessageList />)

    fireEvent.click(screen.getByRole('button', { name: 'Preview summary.md' }))

    expect(useGeneratedFilePreviewStore.getState().target).toEqual({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })
  })

  it('opens non-previewable files with the system default app', () => {
    seedMessages('excel')
    render(<MessageList />)

    fireEvent.click(screen.getByRole('button', { name: 'Open table.xlsx' }))

    expect(tauriMock.openGeneratedFile).toHaveBeenCalledWith('gf-1', 'conv-1')
  })

  it('reveals files from the dropdown menu', async () => {
    render(<MessageList />)

    fireEvent.click(screen.getByRole('button', { name: 'More actions for summary.md' }))
    const menu = await screen.findByRole('menu')
    fireEvent.click(within(menu).getByRole('menuitem', { name: /Show in folder/ }))

    expect(tauriMock.revealFileInFolder).toHaveBeenCalledWith('gf-1', 'conv-1')
  })

  it('shows a toast when external open fails', async () => {
    seedMessages('excel')
    tauriMock.openGeneratedFile.mockRejectedValue(new Error('missing'))
    render(<MessageList />)

    fireEvent.click(screen.getByRole('button', { name: 'Open table.xlsx' }))

    await waitFor(() => {
      expect(useNotificationStore.getState().notifications[0]).toMatchObject({
        level: 'error',
        title: '无法打开文件',
      })
    })
  })
})
```

- [ ] **Step 5: Run MessageList test and verify it fails**

Run:

```bash
pnpm vitest run src/components/chat/MessageList.test.tsx
```

Expected: FAIL because `MessageList` still passes empty callbacks.

- [ ] **Step 6: Implement MessageList action wiring**

Modify `src/components/chat/MessageList.tsx` imports:

```tsx
import { openGeneratedFile, revealFileInFolder } from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { isFileActionEnabled, toPreviewTarget } from '@/components/chat/generatedFileActions'
import type { RenderGeneratedFile } from '@/hooks/useTurnRenderModel'
```

Add helpers inside `MessageList`:

```tsx
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)

  function notifyFileError(title: string, fileName: string, error: unknown) {
    useNotificationStore.getState().push({
      level: 'error',
      title,
      message: error instanceof Error ? `${fileName}: ${error.message}` : fileName,
      actions: [],
      dismissible: true,
      autoHide: 5,
      context: 'toast',
    })
  }

  function conversationIdForFile(): string | null {
    return activeConversationId ?? null
  }

  function handlePreview(file: RenderGeneratedFile) {
    const conversationId = conversationIdForFile()
    if (!conversationId) {
      notifyFileError('无法预览文件', file.title, new Error('No active conversation'))
      return
    }
    openPreview(toPreviewTarget(file, conversationId))
  }

  async function handleOpenExternal(file: RenderGeneratedFile) {
    const conversationId = conversationIdForFile()
    if (!conversationId) {
      notifyFileError('无法打开文件', file.title, new Error('No active conversation'))
      return
    }
    try {
      await openGeneratedFile(file.id, conversationId)
    } catch (error) {
      notifyFileError('无法打开文件', file.title, error)
    }
  }

  async function handleReveal(file: RenderGeneratedFile) {
    const conversationId = conversationIdForFile()
    if (!conversationId) {
      notifyFileError('无法在文件夹中显示', file.title, new Error('No active conversation'))
      return
    }
    try {
      await revealFileInFolder(file.id, conversationId)
    } catch (error) {
      notifyFileError('无法在文件夹中显示', file.title, error)
    }
  }
```

Update `GeneratedFileCard` usage:

```tsx
              <GeneratedFileCard
                key={f.id}
                title={f.title}
                sub={f.sub}
                appName={f.primaryAction === 'preview' ? 'Preview' : f.appName}
                primaryAction={f.primaryAction}
                canPreview={f.canPreview && isFileActionEnabled(f.actions, 'preview')}
                canOpenExternal={isFileActionEnabled(f.actions, 'open')}
                canReveal={isFileActionEnabled(f.actions, 'reveal')}
                onPreview={() => handlePreview(f)}
                onOpenExternal={() => void handleOpenExternal(f)}
                onReveal={() => void handleReveal(f)}
              />
```

- [ ] **Step 7: Run Task 3 tests and verify pass**

Run:

```bash
pnpm vitest run src/stores/generatedFilePreviewStore.test.ts src/components/chat/MessageList.test.tsx
```

Expected: PASS.

- [ ] **Step 8: Commit Task 3**

```bash
git add src/stores/generatedFilePreviewStore.ts src/stores/generatedFilePreviewStore.test.ts src/components/chat/MessageList.tsx src/components/chat/MessageList.test.tsx
git commit -m "feat: wire generated file card actions"
```

---

### Task 4: RightPanel Workspace Preview Shell

**Files:**
- Create: `src/components/chat/FilePreviewPane.tsx`
- Create: `src/components/chat/FilePreviewPane.test.tsx`
- Modify: `src/components/chat/RightPanel.tsx`
- Create: `src/components/chat/RightPanel.test.tsx`
- Modify: `src/features/chat/ChatPage.tsx`

- [ ] **Step 1: Write failing FilePreviewPane shell tests**

Create `src/components/chat/FilePreviewPane.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { FilePreviewPane } from './FilePreviewPane'
import type { PreviewTarget } from './generatedFileActions'

const target: PreviewTarget = {
  fileId: 'gf-1',
  conversationId: 'conv-1',
  fileName: 'summary.md',
  fileType: 'markdown',
}

describe('FilePreviewPane', () => {
  it('renders an empty state without a target', () => {
    render(<FilePreviewPane target={null} onOpenExternal={() => {}} />)
    expect(screen.getByText('选择一个产物进行预览')).toBeInTheDocument()
  })

  it('renders an unsupported placeholder and external-open fallback for a target', () => {
    const onOpenExternal = vi.fn()
    render(<FilePreviewPane target={target} onOpenExternal={onOpenExternal} />)

    expect(screen.getByText('summary.md')).toBeInTheDocument()
    expect(screen.getByText('预览内容加载能力将在下一阶段接入')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Open with default app' }))
    expect(onOpenExternal).toHaveBeenCalledWith(target)
  })
})
```

- [ ] **Step 2: Run FilePreviewPane test and verify it fails**

Run:

```bash
pnpm vitest run src/components/chat/FilePreviewPane.test.tsx
```

Expected: FAIL because component does not exist.

- [ ] **Step 3: Implement FilePreviewPane shell**

Create `src/components/chat/FilePreviewPane.tsx`:

```tsx
import { ExternalLink, FileText } from 'lucide-react'
import type { PreviewTarget } from './generatedFileActions'

interface FilePreviewPaneProps {
  target: PreviewTarget | null
  onOpenExternal: (target: PreviewTarget) => void
}

export function FilePreviewPane({ target, onOpenExternal }: FilePreviewPaneProps) {
  if (!target) {
    return (
      <div className="flex h-full items-center justify-center rounded-2xl border border-dashed border-border bg-card/60 p-8 text-center">
        <div className="max-w-xs text-[13px] text-muted-foreground">选择一个产物进行预览</div>
      </div>
    )
  }

  return (
    <section className="flex h-full min-w-0 flex-col overflow-hidden rounded-2xl border border-border bg-card shadow-sm">
      <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="h-4 w-4 shrink-0 text-muted-foreground" />
          <h3 className="truncate text-[13px] font-semibold text-foreground">{target.fileName}</h3>
        </div>
        <button
          type="button"
          onClick={() => onOpenExternal(target)}
          className="flex items-center gap-1 rounded-full border border-border px-3 py-1.5 text-[12px] text-foreground hover:bg-muted"
        >
          <ExternalLink className="h-3.5 w-3.5" />
          Open with default app
        </button>
      </header>
      <div className="flex flex-1 items-center justify-center overflow-auto p-6">
        <div className="max-w-sm text-center text-[13px] leading-6 text-muted-foreground">
          预览内容加载能力将在下一阶段接入
        </div>
      </div>
    </section>
  )
}
```

- [ ] **Step 4: Write failing RightPanel tests**

Create `src/components/chat/RightPanel.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { RightPanel } from './RightPanel'
import { useChatStore } from '@/stores/chatStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'

function generatedFile(id: string, conversationId: string, fileName: string, fileType = 'markdown') {
  return {
    id: `msg-${id}`,
    conversationId,
    role: 'assistant' as const,
    createdAt: new Date().toISOString(),
    content: {
      text: 'done',
      generatedFiles: [
        {
          id,
          fileName,
          filePath: `generated/${fileName}`,
          fileType,
          fileSize: 1024,
          category: 'report',
          version: 1,
          isLatest: true,
          createdAt: new Date().toISOString(),
          description: 'file',
          actions: [],
        },
      ],
    },
  }
}

describe('RightPanel workspace preview', () => {
  beforeEach(() => {
    useGeneratedFilePreviewStore.getState().closePreview()
    useChatStore.setState({
      messages: [
        generatedFile('gf-1', 'conv-1', 'summary.md'),
        generatedFile('gf-2', 'conv-1', 'table.xlsx', 'excel'),
        generatedFile('gf-other', 'conv-2', 'other.md'),
      ],
      taskStates: {},
    })
  })

  it('renders the narrow task monitor by default', () => {
    render(<RightPanel conversationId="conv-1" onOpenExternal={vi.fn()} />)

    expect(screen.getByText('任务监控')).toBeInTheDocument()
    expect(screen.getByTestId('right-panel')).toHaveClass('w-[260px]')
    expect(screen.queryByText('选择一个产物进行预览')).not.toBeInTheDocument()
  })

  it('renders wide preview mode when a target is selected', () => {
    useGeneratedFilePreviewStore.getState().openPreview({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })

    render(<RightPanel conversationId="conv-1" onOpenExternal={vi.fn()} />)

    expect(screen.getByTestId('right-panel')).toHaveClass('w-[720px]')
    expect(screen.getByText('summary.md')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Close preview' })).toBeInTheDocument()
  })

  it('filters artifacts by conversation and switches preview target on click', () => {
    render(<RightPanel conversationId="conv-1" onOpenExternal={vi.fn()} />)

    const artifacts = screen.getByTestId('artifact-list')
    expect(within(artifacts).getByText('summary.md')).toBeInTheDocument()
    expect(within(artifacts).getByText('table.xlsx')).toBeInTheDocument()
    expect(within(artifacts).queryByText('other.md')).not.toBeInTheDocument()

    fireEvent.click(within(artifacts).getByRole('button', { name: 'Preview summary.md' }))

    expect(useGeneratedFilePreviewStore.getState().target).toMatchObject({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
    })
  })

  it('closes preview mode', () => {
    useGeneratedFilePreviewStore.getState().openPreview({
      fileId: 'gf-1',
      conversationId: 'conv-1',
      fileName: 'summary.md',
      fileType: 'markdown',
    })

    render(<RightPanel conversationId="conv-1" onOpenExternal={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Close preview' }))

    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })
})
```

- [ ] **Step 5: Run RightPanel test and verify it fails**

Run:

```bash
pnpm vitest run src/components/chat/RightPanel.test.tsx
```

Expected: FAIL because `RightPanel` is still fixed width and artifacts are not clickable/filtered.

- [ ] **Step 6: Implement RightPanel preview mode**

Modify `src/components/chat/RightPanel.tsx`:

- Add imports:

```tsx
import { X } from 'lucide-react'
import { FilePreviewPane } from './FilePreviewPane'
import { toPreviewTarget, type PreviewTarget } from './generatedFileActions'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
```

- Change props and root:

```tsx
interface RightPanelProps {
  conversationId: string
  onOpenExternal?: (target: PreviewTarget) => void
}

export function RightPanel({ conversationId, onOpenExternal }: RightPanelProps) {
  const target = useGeneratedFilePreviewStore((s) => s.target)
  const closePreview = useGeneratedFilePreviewStore((s) => s.closePreview)
  const clearIfConversationChanged = useGeneratedFilePreviewStore((s) => s.clearIfConversationChanged)
  const previewOpen = target?.conversationId === conversationId

  clearIfConversationChanged(conversationId)

  if (previewOpen) {
    return (
      <div data-testid="right-panel" className="flex h-full w-[720px] shrink-0 overflow-hidden border-l border-border bg-background">
        <div className="min-w-0 flex-1 p-3">
          <FilePreviewPane target={target} onOpenExternal={(next) => onOpenExternal?.(next)} />
        </div>
        <div className="flex h-full w-[240px] shrink-0 flex-col overflow-y-auto border-l border-border bg-background">
          <div className="flex items-center justify-between px-4 py-4">
            <h2 className="text-[15px] font-semibold text-foreground">任务监控</h2>
            <button type="button" aria-label="Close preview" onClick={closePreview} className="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground">
              <X className="h-4 w-4" />
            </button>
          </div>
          <TaskSection conversationId={conversationId} />
          <ArtifactSection conversationId={conversationId} activeFileId={target.fileId} />
          <SkillMcpSection />
        </div>
      </div>
    )
  }

  return (
    <div data-testid="right-panel" className="flex h-full w-[260px] shrink-0 flex-col overflow-y-auto border-l border-border bg-background">
      <div className="px-4 py-4">
        <h2 className="text-[15px] font-semibold text-foreground">任务监控</h2>
      </div>
      <TaskSection conversationId={conversationId} />
      <ArtifactSection conversationId={conversationId} activeFileId={null} />
      <SkillMcpSection />
    </div>
  )
}
```

- Update `ArtifactSection` signature and filtering:

```tsx
function ArtifactSection({ conversationId, activeFileId }: { conversationId: string; activeFileId: string | null }) {
  const messages = useChatStore((s) => s.messages)
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)
  const [open, setOpen] = useState(true)

  const files = useMemo(() => {
    const seen = new Set<string>()
    const result: GeneratedFile[] = []
    for (const msg of messages) {
      if (msg.conversationId !== conversationId) continue
      for (const f of msg.content.generatedFiles ?? []) {
        if (!seen.has(f.id) && f.isLatest) {
          seen.add(f.id)
          result.push(f)
        }
      }
    }
    return result
  }, [messages, conversationId])
```

- Update artifact list container and item call:

```tsx
            <div data-testid="artifact-list" className="flex flex-col gap-1">
              {files.map((f) => (
                <ArtifactItem
                  key={f.id}
                  file={f}
                  active={f.id === activeFileId}
                  onPreview={() => openPreview(toPreviewTarget({ id: f.id, title: f.fileName, fileType: f.fileType }, conversationId))}
                />
              ))}
            </div>
```

- Replace `ArtifactItem` with clickable button:

```tsx
function ArtifactItem({ file, active, onPreview }: { file: GeneratedFile; active: boolean; onPreview: () => void }) {
  return (
    <button
      type="button"
      aria-label={`Preview ${file.fileName}`}
      onClick={onPreview}
      className={cn(
        'flex items-center gap-2 rounded-lg px-2 py-1 text-left hover:bg-muted',
        active && 'bg-muted text-foreground',
      )}
    >
      <ArtifactFileIcon fileType={file.fileType} />
      <span className="min-w-0 flex-1 truncate text-[12px] text-foreground">
        {file.fileName}
      </span>
    </button>
  )
}
```

- [ ] **Step 7: Wire ChatPage external-open callback into RightPanel**

Modify `src/features/chat/ChatPage.tsx` imports:

```tsx
import { openGeneratedFile } from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import type { PreviewTarget } from '@/components/chat/generatedFileActions'
```

Add handler inside `ChatPage`:

```tsx
  async function handleOpenPreviewTarget(target: PreviewTarget) {
    try {
      await openGeneratedFile(target.fileId, target.conversationId)
    } catch (error) {
      useNotificationStore.getState().push({
        level: 'error',
        title: '无法打开文件',
        message: error instanceof Error ? `${target.fileName}: ${error.message}` : target.fileName,
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    }
  }
```

Update render:

```tsx
        <RightPanel conversationId={conversationId} onOpenExternal={(target) => void handleOpenPreviewTarget(target)} />
```

- [ ] **Step 8: Run Task 4 tests and verify pass**

Run:

```bash
pnpm vitest run src/components/chat/FilePreviewPane.test.tsx src/components/chat/RightPanel.test.tsx
```

Expected: PASS.

- [ ] **Step 9: Commit Task 4**

```bash
git add src/components/chat/FilePreviewPane.tsx src/components/chat/FilePreviewPane.test.tsx src/components/chat/RightPanel.tsx src/components/chat/RightPanel.test.tsx src/features/chat/ChatPage.tsx
git commit -m "feat: add right panel file preview workspace"
```

---

### Task 5: Tauri Preview Wrapper

**Files:**
- Modify: `src/lib/tauri.ts`
- Create: `src/lib/tauri.file-preview.test.ts`

- [ ] **Step 1: Write failing Tauri wrapper tests**

Create `src/lib/tauri.file-preview.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'

const coreMock = vi.hoisted(() => ({
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: coreMock.invoke,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

import { getFilePreview, type FilePreview } from './tauri'

describe('tauri file preview command', () => {
  beforeEach(() => {
    coreMock.invoke.mockReset()
  })

  it('invokes get_file_preview with fileId and conversationId', async () => {
    const preview: FilePreview = {
      kind: 'markdown',
      fileName: 'summary.md',
      mimeType: 'text/markdown',
      content: '# Summary',
    }
    coreMock.invoke.mockResolvedValue(preview)

    await expect(getFilePreview('gf-1', 'conv-1')).resolves.toEqual(preview)

    expect(coreMock.invoke).toHaveBeenCalledWith('get_file_preview', {
      fileId: 'gf-1',
      conversationId: 'conv-1',
    })
  })
})
```

- [ ] **Step 2: Run wrapper test and verify it fails**

Run:

```bash
pnpm vitest run src/lib/tauri.file-preview.test.ts
```

Expected: FAIL because `getFilePreview` does not exist.

- [ ] **Step 3: Add FilePreview type and wrapper**

Modify `src/lib/tauri.ts` near existing file commands:

```ts
export type FilePreview =
  | {
      kind: 'markdown' | 'text' | 'json' | 'csv'
      fileName: string
      mimeType: string
      content: string
    }
  | {
      kind: 'html'
      fileName: string
      mimeType: 'text/html'
      content: string
      sandbox: true
    }
  | {
      kind: 'unsupported'
      fileName: string
      reason: string
    }

export function getFilePreview(fileId: string, conversationId: string): Promise<FilePreview> {
  return invoke<FilePreview>('get_file_preview', {
    fileId,
    conversationId,
  })
}
```

Keep the old `previewFile(fileId, conversationId): Promise<string>` for compatibility until callers are migrated.

- [ ] **Step 4: Run wrapper test and verify pass**

Run:

```bash
pnpm vitest run src/lib/tauri.file-preview.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit Task 5**

```bash
git add src/lib/tauri.ts src/lib/tauri.file-preview.test.ts
git commit -m "feat: add structured file preview api"
```

---

### Task 6: Secure Rust File Preview Command

**Files:**
- Modify: `src-tauri/src/storage/file_manager.rs`
- Modify: `src-tauri/src/commands/file.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing Rust tests for preview helpers**

Add a `#[cfg(test)] mod preview_tests` block near the bottom of `src-tauri/src/commands/file.rs`:

```rust
#[cfg(test)]
mod preview_tests {
    use super::*;
    use crate::storage::file_manager::FileManager;
    use std::fs;
    use tempfile::TempDir;

    fn workspace() -> (TempDir, FileManager) {
        let dir = TempDir::new().expect("temp dir");
        fs::create_dir_all(dir.path().join("generated")).expect("generated dir");
        (dir, FileManager::new(dir.path()))
    }

    #[test]
    fn classify_markdown_preview() {
        let preview = preview_from_bytes("summary.md", "markdown", b"# Hello".to_vec()).expect("preview");
        match preview {
            FilePreview::Markdown { file_name, mime_type, content } => {
                assert_eq!(file_name, "summary.md");
                assert_eq!(mime_type, "text/markdown");
                assert_eq!(content, "# Hello");
            }
            other => panic!("expected markdown preview, got {:?}", other),
        }
    }

    #[test]
    fn unsupported_binary_type_does_not_return_content() {
        let preview = preview_from_bytes("table.xlsx", "excel", b"not really excel".to_vec()).expect("preview");
        match preview {
            FilePreview::Unsupported { file_name, reason } => {
                assert_eq!(file_name, "table.xlsx");
                assert!(reason.contains("not supported"));
            }
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn oversized_file_returns_unsupported() {
        let bytes = vec![b'a'; (MAX_PREVIEW_BYTES as usize) + 1];
        let preview = preview_from_bytes("large.md", "markdown", bytes).expect("preview");
        match preview {
            FilePreview::Unsupported { reason, .. } => assert!(reason.contains("too large")),
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn non_utf8_file_returns_unsupported() {
        let preview = preview_from_bytes("bad.md", "markdown", vec![0xff, 0xfe, 0xfd]).expect("preview");
        match preview {
            FilePreview::Unsupported { reason, .. } => assert!(reason.contains("valid UTF-8")),
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn resolve_existing_file_rejects_path_traversal() {
        let (_dir, file_mgr) = workspace();
        let result = file_mgr.resolve_existing_file("../outside.txt");
        assert!(result.is_err());
    }

    #[test]
    fn resolve_existing_file_rejects_directories() {
        let (_dir, file_mgr) = workspace();
        let result = file_mgr.resolve_existing_file("generated");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run Rust helper tests and verify failure**

Run:

```bash
cd src-tauri
cargo test --lib commands::file::preview_tests
```

Expected: FAIL because `FilePreview`, `MAX_PREVIEW_BYTES`, `preview_from_bytes`, and `resolve_existing_file` are not implemented.

- [ ] **Step 3: Add safe existing-file resolver**

Modify `src-tauri/src/storage/file_manager.rs` after `full_path` or before it:

```rust
    /// Resolve an existing stored file and reject missing files, directories, and path traversal.
    pub fn resolve_existing_file(&self, stored_path: &str) -> Result<PathBuf> {
        let path = self.safe_resolve(stored_path)?;
        if !path.is_file() {
            return Err(anyhow!("Stored file does not exist: {}", stored_path));
        }
        Ok(path)
    }
```

Keep `full_path` unchanged for existing callers not touched in this task.

- [ ] **Step 4: Add preview types and pure helper**

Modify `src-tauri/src/commands/file.rs` near the top:

```rust
const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;

#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FilePreview {
    #[serde(rename = "markdown")]
    Markdown { file_name: String, mime_type: String, content: String },
    #[serde(rename = "text")]
    Text { file_name: String, mime_type: String, content: String },
    #[serde(rename = "json")]
    Json { file_name: String, mime_type: String, content: String },
    #[serde(rename = "csv")]
    Csv { file_name: String, mime_type: String, content: String },
    #[serde(rename = "html")]
    Html { file_name: String, mime_type: String, content: String, sandbox: bool },
    #[serde(rename = "unsupported")]
    Unsupported { file_name: String, reason: String },
}

struct ResolvedFileRecord {
    file_name: String,
    stored_path: String,
    file_type: String,
    file_size: i64,
}
```

Add helper functions:

```rust
fn preview_mime_type(kind: &str) -> &'static str {
    match kind {
        "markdown" | "md" => "text/markdown",
        "html" => "text/html",
        "json" => "application/json",
        "csv" => "text/csv",
        "text" | "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn normalize_preview_kind(file_name: &str, file_type: &str) -> String {
    let lowered = file_type.trim().to_lowercase();
    let from_ext = Path::new(file_name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let raw = if lowered.is_empty() { from_ext } else { lowered };
    match raw.as_str() {
        "md" | "markdown" => "markdown".to_string(),
        "txt" | "text" => "text".to_string(),
        "html" | "json" | "csv" => raw,
        _ => raw,
    }
}

fn preview_from_bytes(file_name: &str, file_type: &str, bytes: Vec<u8>) -> Result<FilePreview, String> {
    if bytes.len() as u64 > MAX_PREVIEW_BYTES {
        return Ok(FilePreview::Unsupported {
            file_name: file_name.to_string(),
            reason: format!("File is too large for preview; maximum is {} bytes", MAX_PREVIEW_BYTES),
        });
    }

    let kind = normalize_preview_kind(file_name, file_type);
    let supported = matches!(kind.as_str(), "markdown" | "html" | "text" | "json" | "csv");
    if !supported {
        return Ok(FilePreview::Unsupported {
            file_name: file_name.to_string(),
            reason: format!("Preview for '{}' files is not supported", file_type),
        });
    }

    let content = match String::from_utf8(bytes) {
        Ok(value) => value,
        Err(_) => {
            return Ok(FilePreview::Unsupported {
                file_name: file_name.to_string(),
                reason: "File preview requires valid UTF-8 text".to_string(),
            })
        }
    };

    let mime_type = preview_mime_type(&kind).to_string();
    Ok(match kind.as_str() {
        "markdown" => FilePreview::Markdown { file_name: file_name.to_string(), mime_type, content },
        "html" => FilePreview::Html { file_name: file_name.to_string(), mime_type: "text/html".to_string(), content, sandbox: true },
        "json" => FilePreview::Json { file_name: file_name.to_string(), mime_type, content },
        "csv" => FilePreview::Csv { file_name: file_name.to_string(), mime_type, content },
        _ => FilePreview::Text { file_name: file_name.to_string(), mime_type, content },
    })
}
```

- [ ] **Step 5: Add metadata resolver and command**

Add resolver in `src-tauri/src/commands/file.rs`:

```rust
fn resolve_file_record(
    facade: &RuntimeRepositoryFacade,
    file_id: &str,
    conversation_id: &str,
) -> Result<ResolvedFileRecord, String> {
    let store = facade.file_record_store();

    if let Some(record) = store
        .get_uploaded_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        let stored_path = record.get("storedPath").and_then(|v| v.as_str()).ok_or_else(|| "Invalid file record".to_string())?;
        let file_name = record
            .get("originalName")
            .and_then(|v| v.as_str())
            .or_else(|| record.get("fileName").and_then(|v| v.as_str()))
            .unwrap_or("unknown")
            .to_string();
        let file_type = record.get("fileType").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let file_size = record.get("fileSize").and_then(|v| v.as_i64()).unwrap_or(0);
        return Ok(ResolvedFileRecord { file_name, stored_path: stored_path.to_string(), file_type, file_size });
    }

    if let Some(record) = store
        .get_generated_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        let stored_path = record.get("storedPath").and_then(|v| v.as_str()).ok_or_else(|| "Invalid file record".to_string())?;
        let file_name = record.get("fileName").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let file_type = record.get("fileType").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let file_size = record.get("fileSize").and_then(|v| v.as_i64()).unwrap_or(0);
        return Ok(ResolvedFileRecord { file_name, stored_path: stored_path.to_string(), file_type, file_size });
    }

    Err("File not found or does not belong to this conversation".to_string())
}
```

Add command:

```rust
#[tauri::command]
pub async fn get_file_preview(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<FilePreview, String> {
    let record = resolve_file_record(&facade, &file_id, &conversation_id)?;
    if record.file_size as u64 > MAX_PREVIEW_BYTES {
        return Ok(FilePreview::Unsupported {
            file_name: record.file_name,
            reason: format!("File is too large for preview; maximum is {} bytes", MAX_PREVIEW_BYTES),
        });
    }
    let full_path = file_mgr.resolve_existing_file(&record.stored_path).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&full_path).map_err(|e| e.to_string())?;
    preview_from_bytes(&record.file_name, &record.file_type, bytes)
}
```

- [ ] **Step 6: Migrate open/reveal away from full_path fallback**

In `open_generated_file`, replace:

```rust
    let full_path = file_mgr.full_path(&stored_path);
```

with:

```rust
    let full_path = file_mgr.resolve_existing_file(&stored_path).map_err(|e| e.to_string())?;
```

In `reveal_file_in_folder`, make the same replacement.

- [ ] **Step 7: Register command**

Modify `src-tauri/src/lib.rs` command list near file commands:

```rust
            file::get_file_preview,
```

Place it next to `file::preview_file` or immediately before it.

- [ ] **Step 8: Run Rust tests and cargo check**

Run:

```bash
cd src-tauri
cargo test --lib commands::file::preview_tests
cargo check
```

Expected: both commands exit 0.

- [ ] **Step 9: Commit Task 6**

```bash
git add src-tauri/src/storage/file_manager.rs src-tauri/src/commands/file.rs src-tauri/src/lib.rs
git commit -m "feat: add secure file preview command"
```

---

### Task 7: FilePreviewPane Real Content Loading

**Files:**
- Modify: `src/components/chat/FilePreviewPane.tsx`
- Modify: `src/components/chat/FilePreviewPane.test.tsx`

- [ ] **Step 1: Extend FilePreviewPane tests for loaded content**

Update `src/components/chat/FilePreviewPane.test.tsx` with mocked `getFilePreview`:

```tsx
const previewMock = vi.hoisted(() => ({
  getFilePreview: vi.fn(),
}))

vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    getFilePreview: previewMock.getFilePreview,
  }
})
```

Add tests:

```tsx
  it('loads and renders markdown content', async () => {
    previewMock.getFilePreview.mockResolvedValue({
      kind: 'markdown',
      fileName: 'summary.md',
      mimeType: 'text/markdown',
      content: '# Summary',
    })

    render(<FilePreviewPane target={target} onOpenExternal={() => {}} />)

    expect(await screen.findByRole('heading', { name: 'Summary' })).toBeInTheDocument()
    expect(previewMock.getFilePreview).toHaveBeenCalledWith('gf-1', 'conv-1')
  })

  it('renders unsupported preview responses', async () => {
    previewMock.getFilePreview.mockResolvedValue({
      kind: 'unsupported',
      fileName: 'table.xlsx',
      reason: 'Preview for excel files is not supported',
    })

    render(<FilePreviewPane target={{ ...target, fileName: 'table.xlsx', fileType: 'excel' }} onOpenExternal={() => {}} />)

    expect(await screen.findByText('Preview for excel files is not supported')).toBeInTheDocument()
  })

  it('renders preview loading errors', async () => {
    previewMock.getFilePreview.mockRejectedValue(new Error('not found'))

    render(<FilePreviewPane target={target} onOpenExternal={() => {}} />)

    expect(await screen.findByText('not found')).toBeInTheDocument()
  })
```

- [ ] **Step 2: Run FilePreviewPane test and verify it fails**

Run:

```bash
pnpm vitest run src/components/chat/FilePreviewPane.test.tsx
```

Expected: FAIL because component does not call `getFilePreview` or render content.

- [ ] **Step 3: Implement preview loading and rendering**

Modify `src/components/chat/FilePreviewPane.tsx`:

```tsx
import { useEffect, useState } from 'react'
import { ExternalLink, FileText, Loader2 } from 'lucide-react'
import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { getFilePreview, type FilePreview } from '@/lib/tauri'
import type { PreviewTarget } from './generatedFileActions'
```

Add state inside component:

```tsx
  const [preview, setPreview] = useState<FilePreview | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!target) {
      setPreview(null)
      setError(null)
      setLoading(false)
      return
    }
    let cancelled = false
    setLoading(true)
    setError(null)
    setPreview(null)
    getFilePreview(target.fileId, target.conversationId)
      .then((next) => {
        if (!cancelled) setPreview(next)
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : '无法预览文件')
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [target?.fileId, target?.conversationId])
```

Replace body placeholder with:

```tsx
      <div className="flex-1 overflow-auto p-6">
        {loading ? (
          <div className="flex h-full items-center justify-center text-[13px] text-muted-foreground">
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            正在加载预览
          </div>
        ) : error ? (
          <div className="rounded-xl border border-destructive/20 bg-destructive/5 p-4 text-[13px] text-destructive">{error}</div>
        ) : preview ? (
          <PreviewContent preview={preview} />
        ) : (
          <div className="flex h-full items-center justify-center text-[13px] text-muted-foreground">预览内容加载能力将在下一阶段接入</div>
        )}
      </div>
```

Add helper component below:

```tsx
function PreviewContent({ preview }: { preview: FilePreview }) {
  switch (preview.kind) {
    case 'markdown':
      return <AssistantMarkdown text={preview.content} />
    case 'html':
      return <iframe title={preview.fileName} sandbox="" srcDoc={preview.content} className="h-full min-h-[520px] w-full rounded-xl border border-border bg-background" />
    case 'json':
    case 'csv':
    case 'text':
      return <pre className="whitespace-pre-wrap rounded-xl bg-muted p-4 text-[12px] leading-6 text-foreground">{preview.content}</pre>
    case 'unsupported':
      return <div className="rounded-xl border border-border bg-muted/40 p-4 text-[13px] text-muted-foreground">{preview.reason}</div>
  }
}
```

- [ ] **Step 4: Run FilePreviewPane test and verify pass**

Run:

```bash
pnpm vitest run src/components/chat/FilePreviewPane.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit Task 7**

```bash
git add src/components/chat/FilePreviewPane.tsx src/components/chat/FilePreviewPane.test.tsx
git commit -m "feat: render generated file previews"
```

---

### Task 8: Integration Verification And Cleanup

**Files:**
- Review all files touched in Tasks 1-7.
- Do not edit unrelated dirty files.

- [ ] **Step 1: Run focused frontend regression**

Run:

```bash
pnpm vitest run \
  src/components/chat/generatedFileActions.test.ts \
  src/hooks/__tests__/useTurnRenderModel.test.ts \
  src/components/chat-scene/__tests__/GeneratedFileCard.test.tsx \
  src/stores/generatedFilePreviewStore.test.ts \
  src/components/chat/MessageList.test.tsx \
  src/components/chat/RightPanel.test.tsx \
  src/components/chat/FilePreviewPane.test.tsx \
  src/lib/tauri.file-preview.test.ts
```

Expected: PASS.

- [ ] **Step 2: Run Rust verification**

Run:

```bash
cd src-tauri
cargo test --lib commands::file::preview_tests
cargo check
```

Expected: PASS.

- [ ] **Step 3: Run lint only if the touched files are isolated enough**

Run:

```bash
pnpm lint
```

Expected: PASS. If lint fails only because of pre-existing unrelated dirty files, record the unrelated failures and continue with focused test evidence; do not edit unrelated files.

- [ ] **Step 4: Inspect git diff for accidental unrelated changes**

Run:

```bash
git status --short
git diff --name-only HEAD
```

Expected: only task-owned files appear in the new commits, plus pre-existing unrelated dirty files that were already present before implementation.

- [ ] **Step 5: Manual smoke check in app browser**

Run the app if not already running:

```bash
pnpm dev
```

Open the chat route in the in-app browser and verify:

1. A generated markdown/html/text/csv/json card shows `Preview` as the primary action.
2. A generated excel/pdf/png card shows `Open` as the primary action.
3. The chevron opens a menu with `Preview inside`, `Open with default app`, and `Show in folder`.
4. Clicking `Preview` expands the right panel into the wide workspace.
5. The right artifact list filters to the current conversation.
6. Clicking another artifact switches the preview target.
7. Closing preview returns the panel to the 260px task monitor.
8. Unsupported files show an unsupported message and the external-open fallback.

- [ ] **Step 6: Final commit if verification required a cleanup edit**

If Task 8 required any cleanup changes, commit only those files:

```bash
git add <cleanup-files>
git commit -m "fix: polish generated file preview workspace"
```

If no cleanup changes were needed, do not create an empty commit.

---

## Self-Review Checklist

- Spec goal 1 (`Open` no longer empty): Task 3 wires `openGeneratedFile`; Task 8 verifies.
- Spec goal 2 (real split button): Task 2 implements and tests split button.
- Spec goal 3 (dropdown actions): Task 2 implements and tests menu actions.
- Spec goal 4 (Preview/Open primary action): Task 1 derives primary action; Task 2 renders it; Task 3 executes it.
- Spec goal 5 (RightPanel workspace preview): Task 4 implements wide preview mode and artifact switching.
- Spec goal 6 (simple preview content): Task 5 adds wrapper, Task 6 adds backend command, Task 7 renders content.
- Security requirement (no bare `filePath`): Task 6 uses `fileId + conversationId` and safe resolver.
- Non-goal (no Office/PDF embedded preview): Task 6 returns `unsupported`; Task 7 renders unsupported fallback.
- Conversation isolation: Task 4 filters artifacts; Task 6 resolver uses conversation-scoped store methods.
- Path traversal fallback risk: Task 6 adds `resolve_existing_file` and migrates open/reveal.

# Unified TableView Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace both `markdown.ts` (handwritten parser) and `RichDataTable` with a single schema-driven `TableView` component, while migrating markdown rendering to `react-markdown + remark-gfm`. Visual style aligns with the qob project's minimal data-style table.

**Architecture:** A new `src/components/data-table/` directory owns the table component family. `AssistantMarkdown` switches to `react-markdown`, with `code` / `a` / `table` overrides living in `src/components/chat-scene/markdown/`. The handwritten `markdown.ts`, `RichDataTable`, and the related event-delegation code in `ChatArea.tsx` all get deleted.

**Tech Stack:** React 19, TypeScript, Vitest 4, Tailwind v4 utility classes via `globals.css` design tokens, `react-markdown`, `remark-gfm`.

**Spec:** `docs/superpowers/specs/2026-04-27-unified-table-view-design.md`

---

## Task ordering & rationale

The plan builds in this order:

1. **Tokens** — design tokens land first; later tasks reference them (Task 1)
2. **Types & utils** — pure-TS layer, no React (Tasks 2–3)
3. **TableView component family** — TDD, leaf to root (Tasks 4–8)
4. **DataTable schema mapper** — bridges existing `types/message.ts::DataTable` to TableView (Task 9)
5. **Wire AiBubble** — switch the existing `RichDataTable` consumer (Task 10)
6. **Markdown migration** — install deps, build override components, replace `AssistantMarkdown` (Tasks 11–15)
7. **Cleanup** — delete `markdown.ts`, `RichDataTable`, `ChatArea` event delegation (Task 16)
8. **Final verification** — full test + manual smoke (Task 17)

Each task is independently committable. Task 10 ↔ 16 ordering ensures the app stays functional throughout.

---

## Task 1: Add table design tokens to globals.css

**Files:**
- Modify: `src/styles/globals.css`

**Context:** The spec §3.1 defines the table design tokens. Existing palette uses `--color-semantic-{green,orange,red,blue,purple}` and `--color-accent`, not `--color-accent-{success,warning,danger,info}`. Map the spec's tone names to the existing palette.

- [ ] **Step 1: Read existing token block to find a good insertion point**

Run: `grep -n '^:root\|^}' src/styles/globals.css | head -20`

Expected: Locate the closing `}` of the main `:root` block (or `@theme` block — match whichever pattern existing color tokens live under) so the new tokens append cleanly to the same scope.

- [ ] **Step 2: Append the table token block inside the same `:root` (or `@theme`) scope**

Insert immediately before the closing `}` of the scope that already defines `--color-bg-base`, `--color-border`, etc.:

```css
  /* ────────────── Table ────────────── */
  /* Font family is intentionally not declared — tables inherit from parent */
  --table-font-size: 14px;
  --table-line-height: 1.5;

  --table-bg: var(--color-bg-card);
  --table-radius: 8px;
  --table-border: var(--color-border);
  --table-divider: var(--color-border-subtle);

  --table-header-bg: var(--color-bg-base);
  --table-header-fg: var(--color-text-secondary);
  --table-header-weight: 600;
  --table-header-pad-y: 8px;
  --table-header-pad-x: 12px;

  --table-cell-fg: var(--color-text-primary);
  --table-cell-pad-y: 8px;
  --table-cell-pad-x: 12px;
  --table-row-zebra: color-mix(in srgb, var(--color-bg-base) 40%, transparent);
  --table-row-hover: color-mix(in srgb, var(--color-bg-base) 60%, transparent);

  /* Cell tones — map spec names to existing semantic palette */
  --table-tone-neutral-bg: color-mix(in srgb, var(--color-text-secondary) 10%, transparent);
  --table-tone-neutral-fg: var(--color-text-primary);
  --table-tone-success-bg: var(--color-semantic-green-bg);
  --table-tone-success-fg: var(--color-semantic-green);
  --table-tone-warning-bg: var(--color-semantic-orange-bg);
  --table-tone-warning-fg: var(--color-semantic-orange);
  --table-tone-danger-bg:  var(--color-semantic-red-bg);
  --table-tone-danger-fg:  var(--color-semantic-red);
  --table-tone-info-bg:    var(--color-semantic-blue-bg);
  --table-tone-info-fg:    var(--color-semantic-blue);
  --table-tone-accent-bg:  var(--color-accent-subtle);
  --table-tone-accent-fg:  var(--color-accent);
```

If a `[data-theme='dark']` block already exists, also add inside it:

```css
  --table-row-zebra: color-mix(in srgb, var(--color-bg-base) 25%, transparent);
```

If no dark scope exists, skip the dark override; leave a follow-up note in the commit body.

- [ ] **Step 3: Verify token names referenced (`--color-semantic-green-bg`, `--color-semantic-orange-bg`, `--color-semantic-red-bg`, `--color-semantic-blue-bg`, `--color-accent-subtle`) actually exist**

Run: `grep -n -- '--color-semantic-green-bg\|--color-semantic-orange-bg\|--color-semantic-red-bg\|--color-semantic-blue-bg\|--color-accent-subtle' src/styles/globals.css`

Expected: All 5 names are defined somewhere in the file. If any is missing, add a fallback definition inside the same scope:

```css
--color-semantic-green-bg: rgba(34, 139, 34, 0.12);
```

(Use values that visually match the project — peek at `--color-accent-subtle` for the alpha pattern.)

- [ ] **Step 4: Verify build still compiles**

Run: `pnpm build`

Expected: PASS. CSS-only change should not break TypeScript build, but Vite will catch any malformed CSS.

- [ ] **Step 5: Commit**

```bash
git add src/styles/globals.css
git commit -m "feat(styles): add --table-* design tokens"
```

---

## Task 2: Create the table schema types

**Files:**
- Create: `src/components/data-table/tableSchema.ts`

**Context:** Spec §2.1 defines all types. This file is pure TypeScript with no runtime, so no test is needed.

- [ ] **Step 1: Create the file with full type definitions**

```ts
// src/components/data-table/tableSchema.ts

export type CellAlign = 'left' | 'center' | 'right'

export type CellTone =
  | 'neutral'
  | 'success'
  | 'warning'
  | 'danger'
  | 'info'
  | 'accent'

export interface TableCellSpec {
  text: string
  tone?: CellTone
  variant?: 'pill' | 'plain' | 'bold'
}

export type TableCellValue = string | number | null | TableCellSpec

export interface TableColumn {
  key: string
  label: string
  align?: CellAlign
  width?: number | string
  wrap?: 'truncate' | 'wrap'
  sortable?: boolean
  sortType?: 'string' | 'number' | 'date'
  tabularNums?: boolean
}

export type TableRow = Record<string, TableCellValue>

export interface TableMeta {
  title?: string
  badge?: string
  footnote?: string
}

/** Type guard: is the cell a TableCellSpec object? */
export function isCellSpec(v: TableCellValue): v is TableCellSpec {
  return typeof v === 'object' && v !== null
}

/** Extract a cell's plain-text representation for sort/copy/render. */
export function cellText(v: TableCellValue): string {
  if (v == null) return ''
  if (typeof v === 'string') return v
  if (typeof v === 'number') return String(v)
  return v.text
}
```

- [ ] **Step 2: Verify types compile**

Run: `pnpm tsc --noEmit`

Expected: PASS, no errors related to the new file.

- [ ] **Step 3: Commit**

```bash
git add src/components/data-table/tableSchema.ts
git commit -m "feat(data-table): add table schema types"
```

---

## Task 3: Implement table utilities (sort + CSV/TSV serialize)

**Files:**
- Create: `src/components/data-table/tableUtils.ts`
- Test: `src/components/data-table/__tests__/tableUtils.test.ts`

**Context:** Pure functions, perfect for TDD. Spec §4.1 (sort comparator), §4.4 (CSV/TSV escape rules).

- [ ] **Step 1: Write failing tests**

```ts
// src/components/data-table/__tests__/tableUtils.test.ts
import { describe, it, expect } from 'vitest'
import { sortRows, toCsv, toTsv } from '../tableUtils'
import type { TableColumn, TableRow } from '../tableSchema'

const cols: TableColumn[] = [
  { key: 'name', label: 'Name', sortable: true },
  { key: 'qty', label: 'Qty', sortable: true, sortType: 'number' },
  { key: 'when', label: 'When', sortable: true, sortType: 'date' },
]

const rows: TableRow[] = [
  { name: 'banana', qty: 2, when: '2024-01-02' },
  { name: 'apple', qty: 10, when: '2024-01-01' },
  { name: 'cherry', qty: { text: '5' }, when: 'invalid' },
]

describe('sortRows', () => {
  it('returns input unchanged when sort is null', () => {
    expect(sortRows(rows, null, cols)).toEqual(rows)
  })

  it('sorts by string column ascending', () => {
    const out = sortRows(rows, { key: 'name', dir: 'asc' }, cols)
    expect(out.map((r) => r.name)).toEqual(['apple', 'banana', 'cherry'])
  })

  it('sorts by string column descending', () => {
    const out = sortRows(rows, { key: 'name', dir: 'desc' }, cols)
    expect(out.map((r) => r.name)).toEqual(['cherry', 'banana', 'apple'])
  })

  it('sorts by number column treating cells as numbers', () => {
    const out = sortRows(rows, { key: 'qty', dir: 'asc' }, cols)
    expect(out.map((r) => r.name)).toEqual(['banana', 'cherry', 'apple'])
  })

  it('puts NaN at the end for number sort', () => {
    const r = [
      { v: 'a' as const, n: 'x' },
      { v: 'b' as const, n: 1 },
    ]
    const c: TableColumn[] = [{ key: 'n', label: 'N', sortable: true, sortType: 'number' }]
    const out = sortRows(r, { key: 'n', dir: 'asc' }, c)
    expect(out[0].n).toBe(1)
  })

  it('puts invalid dates at the end for date sort', () => {
    const out = sortRows(rows, { key: 'when', dir: 'asc' }, cols)
    expect(out.map((r) => r.name)).toEqual(['apple', 'banana', 'cherry'])
  })

  it('does not mutate the input array', () => {
    const before = [...rows]
    sortRows(rows, { key: 'name', dir: 'asc' }, cols)
    expect(rows).toEqual(before)
  })
})

describe('toCsv', () => {
  it('emits header row from column labels', () => {
    expect(toCsv(cols, [])).toBe('Name,Qty,When\r\n')
  })

  it('serializes rows in column order', () => {
    const r: TableRow[] = [{ name: 'a', qty: 1, when: '2024' }]
    expect(toCsv(cols, r)).toBe('Name,Qty,When\r\na,1,2024\r\n')
  })

  it('quotes fields containing commas', () => {
    const r: TableRow[] = [{ name: 'a,b', qty: 1, when: '2024' }]
    expect(toCsv(cols, r)).toContain('"a,b"')
  })

  it('quotes fields containing double quotes and doubles them', () => {
    const r: TableRow[] = [{ name: 'a"b', qty: 1, when: '2024' }]
    expect(toCsv(cols, r)).toContain('"a""b"')
  })

  it('quotes fields containing newlines', () => {
    const r: TableRow[] = [{ name: 'a\nb', qty: 1, when: '2024' }]
    expect(toCsv(cols, r)).toContain('"a\nb"')
  })

  it('renders TableCellSpec via cell.text', () => {
    const r: TableRow[] = [{ name: { text: 'pill' }, qty: 1, when: '2024' }]
    expect(toCsv(cols, r)).toContain('pill')
  })

  it('renders null as empty', () => {
    const r: TableRow[] = [{ name: null, qty: 1, when: '2024' }]
    expect(toCsv(cols, r)).toBe('Name,Qty,When\r\n,1,2024\r\n')
  })
})

describe('toTsv', () => {
  it('uses tabs as field separator', () => {
    const r: TableRow[] = [{ name: 'a', qty: 1, when: '2024' }]
    expect(toTsv(cols, r)).toBe('Name\tQty\tWhen\r\na\t1\t2024\r\n')
  })

  it('replaces tabs and newlines inside fields with single space', () => {
    const r: TableRow[] = [{ name: 'a\tb', qty: 1, when: 'c\nd' }]
    expect(toTsv(cols, r)).toContain('a b')
    expect(toTsv(cols, r)).toContain('c d')
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm exec vitest run src/components/data-table/__tests__/tableUtils.test.ts`

Expected: FAIL — module not found.

- [ ] **Step 3: Implement the utilities**

```ts
// src/components/data-table/tableUtils.ts
import type { TableColumn, TableRow, TableCellValue } from './tableSchema'
import { cellText } from './tableSchema'

export interface SortState {
  key: string
  dir: 'asc' | 'desc'
}

const collator = new Intl.Collator(undefined, { numeric: false, sensitivity: 'base' })

function compareString(a: TableCellValue, b: TableCellValue): number {
  return collator.compare(cellText(a), cellText(b))
}

function compareNumber(a: TableCellValue, b: TableCellValue): number {
  const na = Number(cellText(a))
  const nb = Number(cellText(b))
  const aBad = Number.isNaN(na)
  const bBad = Number.isNaN(nb)
  if (aBad && bBad) return 0
  if (aBad) return 1   // NaN sorts last regardless of direction
  if (bBad) return -1
  return na - nb
}

function compareDate(a: TableCellValue, b: TableCellValue): number {
  const ta = Date.parse(cellText(a))
  const tb = Date.parse(cellText(b))
  const aBad = Number.isNaN(ta)
  const bBad = Number.isNaN(tb)
  if (aBad && bBad) return 0
  if (aBad) return 1   // invalid sorts last regardless of direction
  if (bBad) return -1
  return ta - tb
}

export function sortRows(
  rows: TableRow[],
  state: SortState | null,
  columns: TableColumn[],
): TableRow[] {
  if (!state) return rows
  const col = columns.find((c) => c.key === state.key)
  if (!col) return rows
  const cmp =
    col.sortType === 'number' ? compareNumber :
    col.sortType === 'date'   ? compareDate   :
    compareString

  const out = [...rows].sort((ra, rb) => cmp(ra[state.key], rb[state.key]))
  // Invalid/NaN values always go last; only flip valid pairs for direction
  if (state.dir === 'desc') {
    return out.reverse().sort((ra, rb) => {
      const va = cellText(ra[state.key])
      const vb = cellText(rb[state.key])
      // Keep "bad" values pinned to bottom even in desc
      if (col.sortType === 'number') {
        const aBad = Number.isNaN(Number(va))
        const bBad = Number.isNaN(Number(vb))
        if (aBad && !bBad) return 1
        if (!aBad && bBad) return -1
      }
      if (col.sortType === 'date') {
        const aBad = Number.isNaN(Date.parse(va))
        const bBad = Number.isNaN(Date.parse(vb))
        if (aBad && !bBad) return 1
        if (!aBad && bBad) return -1
      }
      return 0
    })
  }
  return out
}

function csvEscape(field: string): string {
  if (/[",\n\r]/.test(field)) {
    return `"${field.replace(/"/g, '""')}"`
  }
  return field
}

function tsvScrub(field: string): string {
  return field.replace(/[\t\n\r]/g, ' ')
}

export function toCsv(columns: TableColumn[], rows: TableRow[]): string {
  const lines: string[] = []
  lines.push(columns.map((c) => csvEscape(c.label)).join(','))
  for (const row of rows) {
    lines.push(columns.map((c) => csvEscape(cellText(row[c.key]))).join(','))
  }
  return lines.join('\r\n') + '\r\n'
}

export function toTsv(columns: TableColumn[], rows: TableRow[]): string {
  const lines: string[] = []
  lines.push(columns.map((c) => tsvScrub(c.label)).join('\t'))
  for (const row of rows) {
    lines.push(columns.map((c) => tsvScrub(cellText(row[c.key]))).join('\t'))
  }
  return lines.join('\r\n') + '\r\n'
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm exec vitest run src/components/data-table/__tests__/tableUtils.test.ts`

Expected: PASS, all cases.

- [ ] **Step 5: Commit**

```bash
git add src/components/data-table/tableUtils.ts src/components/data-table/__tests__/tableUtils.test.ts
git commit -m "feat(data-table): add sort + CSV/TSV utilities"
```

---

## Task 4: Build TableToolbar (title + badge + copy button)

**Files:**
- Create: `src/components/data-table/TableToolbar.tsx`

**Context:** Spec §3.3 toolbar layout. Renders only when one of `title`, `badge`, `enableCopy` is truthy. Copy button uses Shift modifier to switch CSV ↔ TSV. Tests cover this in Task 8 (TableView integration).

- [ ] **Step 1: Implement the component**

```tsx
// src/components/data-table/TableToolbar.tsx
import { useState, useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import type { TableColumn, TableRow, TableMeta } from './tableSchema'
import { toCsv, toTsv } from './tableUtils'

interface Props {
  meta?: TableMeta
  enableCopy?: boolean
  columns: TableColumn[]
  rows: TableRow[]
}

export function TableToolbar({ meta, enableCopy, columns, rows }: Props) {
  const { t } = useTranslation()
  const [shiftHeld, setShiftHeld] = useState(false)
  const [copied, setCopied] = useState<'idle' | 'ok' | 'fail'>('idle')

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => setShiftHeld(e.shiftKey)
    window.addEventListener('keydown', onKey)
    window.addEventListener('keyup', onKey)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('keyup', onKey)
    }
  }, [])

  const handleCopy = useCallback(
    (e: React.MouseEvent) => {
      const useTsv = e.shiftKey
      const text = useTsv ? toTsv(columns, rows) : toCsv(columns, rows)
      navigator.clipboard
        .writeText(text)
        .then(() => {
          setCopied('ok')
          setTimeout(() => setCopied('idle'), 2000)
        })
        .catch(() => {
          setCopied('fail')
          setTimeout(() => setCopied('idle'), 2000)
        })
    },
    [columns, rows],
  )

  if (!meta?.title && !meta?.badge && !enableCopy) return null

  const tooltip = shiftHeld
    ? t('dataTable.copyTsv', 'Copy as TSV')
    : t('dataTable.copyCsv', 'Copy as CSV (hold Shift for TSV)')

  return (
    <div
      className="flex items-center justify-between border-b px-3 py-2"
      style={{
        background: 'var(--table-header-bg)',
        borderColor: 'var(--table-divider)',
        fontSize: 'var(--table-font-size)',
      }}
      data-testid="table-toolbar"
    >
      <div className="flex items-center gap-2 min-w-0">
        {meta?.title && (
          <span
            className="truncate font-semibold"
            style={{ color: 'var(--color-text-primary)' }}
          >
            {meta.title}
          </span>
        )}
        {meta?.badge && (
          <span
            className="inline-block rounded-full px-2 py-0.5 text-xs font-medium"
            style={{
              background: 'var(--table-tone-neutral-bg)',
              color: 'var(--table-tone-neutral-fg)',
            }}
          >
            {meta.badge}
          </span>
        )}
      </div>
      {enableCopy && (
        <button
          type="button"
          onClick={handleCopy}
          title={tooltip}
          className="text-xs transition-colors"
          style={{
            color:
              copied === 'ok'
                ? 'var(--color-semantic-green)'
                : copied === 'fail'
                  ? 'var(--color-semantic-red)'
                  : 'var(--color-text-muted)',
          }}
          data-testid="table-copy-button"
        >
          {copied === 'ok'
            ? t('common.copied', 'Copied')
            : copied === 'fail'
              ? t('common.copyFailed', 'Copy failed')
              : t('common.copy', 'Copy')}
        </button>
      )}
    </div>
  )
}
```

- [ ] **Step 2: Verify it compiles**

Run: `pnpm tsc --noEmit`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/data-table/TableToolbar.tsx
git commit -m "feat(data-table): add TableToolbar with copy button"
```

---

## Task 5: Build TableHeader (sortable th)

**Files:**
- Create: `src/components/data-table/TableHeader.tsx`

**Context:** Spec §3.2 header rules + §4.1 sort cycle. Header lives inside `<thead>`, supports sticky via parent container's CSS.

- [ ] **Step 1: Implement the component**

```tsx
// src/components/data-table/TableHeader.tsx
import type { TableColumn } from './tableSchema'
import type { SortState } from './tableUtils'

interface Props {
  columns: TableColumn[]
  enableSort?: boolean
  sortState: SortState | null
  onToggleSort: (key: string) => void
  sticky?: boolean
}

export function TableHeader({ columns, enableSort, sortState, onToggleSort, sticky }: Props) {
  return (
    <thead
      className={sticky ? 'sticky top-0 z-10' : undefined}
      style={{
        background: 'var(--table-header-bg)',
        // Sticky header needs a bottom shadow because borders don't paint with sticky positioning
        ...(sticky ? { boxShadow: '0 1px 0 var(--table-border)' } : {}),
      }}
    >
      <tr style={{ borderBottom: '1px solid var(--table-border)' }}>
        {columns.map((col) => {
          const sortable = !!enableSort && !!col.sortable
          const isActive = sortState?.key === col.key
          const dir = isActive ? sortState!.dir : null
          const ariaSort: 'ascending' | 'descending' | 'none' =
            dir === 'asc' ? 'ascending' : dir === 'desc' ? 'descending' : 'none'

          const content = (
            <span className="inline-flex items-center gap-1">
              <span>{col.label}</span>
              {sortable && (
                <span
                  aria-hidden
                  style={{
                    fontSize: '10px',
                    opacity: isActive ? 1 : 0.4,
                  }}
                >
                  {dir === 'desc' ? '▼' : '▲'}
                </span>
              )}
            </span>
          )

          return (
            <th
              key={col.key}
              scope="col"
              aria-sort={sortable ? ariaSort : undefined}
              style={{
                padding: 'var(--table-header-pad-y) var(--table-header-pad-x)',
                color: 'var(--table-header-fg)',
                fontWeight: 'var(--table-header-weight)' as unknown as number,
                textAlign: col.align ?? 'left',
                width: col.width,
                whiteSpace: 'nowrap',
                cursor: sortable ? 'pointer' : 'default',
                userSelect: 'none',
              }}
              onClick={sortable ? () => onToggleSort(col.key) : undefined}
              onKeyDown={
                sortable
                  ? (e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault()
                        onToggleSort(col.key)
                      }
                    }
                  : undefined
              }
              tabIndex={sortable ? 0 : undefined}
            >
              {content}
            </th>
          )
        })}
      </tr>
    </thead>
  )
}
```

- [ ] **Step 2: Verify it compiles**

Run: `pnpm tsc --noEmit`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/data-table/TableHeader.tsx
git commit -m "feat(data-table): add TableHeader with sort affordance"
```

---

## Task 6: Build TableBody (cell rendering with tone/pill/null/empty)

**Files:**
- Create: `src/components/data-table/TableBody.tsx`

**Context:** Spec §4.5 cell rendering rules + §4.6 empty state.

- [ ] **Step 1: Implement the component**

```tsx
// src/components/data-table/TableBody.tsx
import type { TableColumn, TableRow, TableCellValue, CellTone } from './tableSchema'
import { isCellSpec } from './tableSchema'

interface Props {
  columns: TableColumn[]
  rows: TableRow[]
  emptyText: string
}

const toneVar = (tone: CellTone | undefined, kind: 'bg' | 'fg') =>
  `var(--table-tone-${tone ?? 'neutral'}-${kind})`

function renderCell(value: TableCellValue, col: TableColumn) {
  if (value == null) {
    return (
      <span style={{ color: 'var(--color-text-muted)' }} aria-label="empty">
        —
      </span>
    )
  }

  if (!isCellSpec(value)) {
    return String(value)
  }

  const text = value.text
  if (value.variant === 'pill') {
    return (
      <span
        className="inline-block rounded px-1.5 leading-[1.4]"
        style={{
          background: toneVar(value.tone, 'bg'),
          color: toneVar(value.tone, 'fg'),
        }}
      >
        {text}
      </span>
    )
  }

  if (value.variant === 'bold') {
    return (
      <span
        style={{
          fontWeight: 600,
          color: value.tone ? toneVar(value.tone, 'fg') : 'var(--table-cell-fg)',
        }}
      >
        {text}
      </span>
    )
  }

  // 'plain' or undefined
  return (
    <span style={{ color: value.tone ? toneVar(value.tone, 'fg') : 'var(--table-cell-fg)' }}>
      {text}
    </span>
  )
}

function cellTitle(value: TableCellValue): string | undefined {
  if (value == null) return undefined
  if (typeof value === 'string') return value
  if (typeof value === 'number') return String(value)
  return value.text
}

export function TableBody({ columns, rows, emptyText }: Props) {
  if (rows.length === 0) {
    return (
      <tbody>
        <tr>
          <td
            colSpan={columns.length}
            style={{
              padding: 'var(--table-cell-pad-y) var(--table-cell-pad-x)',
              color: 'var(--color-text-secondary)',
              textAlign: 'center',
            }}
          >
            {emptyText}
          </td>
        </tr>
      </tbody>
    )
  }

  return (
    <tbody>
      {rows.map((row, rowIdx) => (
        <tr
          key={rowIdx}
          className="transition-colors"
          style={{
            background: rowIdx % 2 === 1 ? 'var(--table-row-zebra)' : undefined,
            borderBottom: '1px solid var(--table-divider)',
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = 'var(--table-row-hover)'
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background =
              rowIdx % 2 === 1 ? 'var(--table-row-zebra)' : ''
          }}
        >
          {columns.map((col) => {
            const value = row[col.key]
            const wrapStyle: React.CSSProperties =
              col.wrap === 'wrap'
                ? { whiteSpace: 'normal' }
                : {
                    whiteSpace: 'nowrap',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    maxWidth: col.width ?? 320,
                  }
            return (
              <td
                key={col.key}
                title={col.wrap === 'wrap' ? undefined : cellTitle(value)}
                style={{
                  padding: 'var(--table-cell-pad-y) var(--table-cell-pad-x)',
                  color: 'var(--table-cell-fg)',
                  textAlign: col.align ?? 'left',
                  fontVariantNumeric: col.tabularNums ? 'tabular-nums' : undefined,
                  ...wrapStyle,
                }}
              >
                {renderCell(value, col)}
              </td>
            )
          })}
        </tr>
      ))}
    </tbody>
  )
}
```

- [ ] **Step 2: Verify it compiles**

Run: `pnpm tsc --noEmit`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/data-table/TableBody.tsx
git commit -m "feat(data-table): add TableBody with cell tone/pill/empty rendering"
```

---

## Task 7: Build TableView (composition + truncate/footer + sort state)

**Files:**
- Create: `src/components/data-table/TableView.tsx`
- Create: `src/components/data-table/index.ts`

**Context:** Spec §3.3 container structure, §4.2 truncate+expand, §4.3 sticky validation, §2.2 props.

- [ ] **Step 1: Implement TableView**

```tsx
// src/components/data-table/TableView.tsx
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TableColumn, TableRow, TableMeta } from './tableSchema'
import { sortRows } from './tableUtils'
import type { SortState } from './tableUtils'
import { TableToolbar } from './TableToolbar'
import { TableHeader } from './TableHeader'
import { TableBody } from './TableBody'

export interface TableViewProps {
  columns: TableColumn[]
  rows: TableRow[]
  meta?: TableMeta

  enableSort?: boolean
  enableCopy?: boolean
  stickyHeader?: boolean
  maxHeight?: number | string
  truncateRows?: number

  className?: string
}

export function TableView({
  columns,
  rows,
  meta,
  enableSort,
  enableCopy,
  stickyHeader,
  maxHeight,
  truncateRows,
  className,
}: TableViewProps) {
  const { t } = useTranslation()
  const [sortState, setSortState] = useState<SortState | null>(null)
  const [expanded, setExpanded] = useState(false)

  useEffect(() => {
    if (stickyHeader && maxHeight === undefined) {
      console.warn(
        '[TableView] stickyHeader requires maxHeight; sticky has no effect without a scroll container.',
      )
    }
  }, [stickyHeader, maxHeight])

  const sorted = useMemo(
    () => sortRows(rows, sortState, columns),
    [rows, sortState, columns],
  )

  const isTruncated =
    truncateRows !== undefined && !expanded && sorted.length > truncateRows
  const visibleRows = isTruncated ? sorted.slice(0, truncateRows!) : sorted

  const toggleSort = (key: string) => {
    setSortState((prev) => {
      if (!prev || prev.key !== key) return { key, dir: 'asc' }
      if (prev.dir === 'asc') return { key, dir: 'desc' }
      return null
    })
  }

  const showFooter = isTruncated || expanded || meta?.footnote
  const footerText = isTruncated
    ? t('dataTable.truncatedFooter', {
        total: sorted.length,
        shown: truncateRows!,
        defaultValue: 'Showing {{shown}} of {{total}} rows',
      })
    : expanded && truncateRows !== undefined && sorted.length > truncateRows
      ? t('dataTable.expandedFooter', {
          total: sorted.length,
          defaultValue: 'Showing all {{total}} rows',
        })
      : meta?.footnote ?? ''

  return (
    <div
      className={`overflow-hidden ${className ?? ''}`}
      style={{
        background: 'var(--table-bg)',
        border: '1px solid var(--table-border)',
        borderRadius: 'var(--table-radius)',
        fontSize: 'var(--table-font-size)',
        lineHeight: 'var(--table-line-height)',
      }}
      data-testid="table-view"
    >
      <TableToolbar
        meta={meta}
        enableCopy={enableCopy}
        columns={columns}
        rows={sorted}
      />

      <div
        className="overflow-auto"
        style={maxHeight !== undefined ? { maxHeight } : undefined}
      >
        <table
          className="w-full"
          style={{ borderCollapse: 'collapse', tableLayout: 'auto' }}
        >
          <TableHeader
            columns={columns}
            enableSort={enableSort}
            sortState={sortState}
            onToggleSort={toggleSort}
            sticky={stickyHeader && maxHeight !== undefined}
          />
          <TableBody
            columns={columns}
            rows={visibleRows}
            emptyText={t('dataTable.empty', 'No data')}
          />
        </table>
      </div>

      {showFooter && (
        <div
          className="flex items-center justify-between border-t px-3 py-2 text-xs"
          style={{
            background: 'var(--table-header-bg)',
            borderColor: 'var(--table-divider)',
            color: 'var(--color-text-secondary)',
          }}
          data-testid="table-footer"
        >
          <span>{footerText}</span>
          {truncateRows !== undefined && sorted.length > truncateRows && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="text-xs underline-offset-2 hover:underline"
              style={{ color: 'var(--color-accent)' }}
              data-testid="table-expand-toggle"
            >
              {expanded
                ? t('dataTable.collapse', 'Collapse')
                : t('dataTable.expandAll', 'Expand all')}
            </button>
          )}
        </div>
      )}
    </div>
  )
}
```

- [ ] **Step 2: Create the barrel export**

```ts
// src/components/data-table/index.ts
export { TableView } from './TableView'
export type { TableViewProps } from './TableView'
export type {
  TableColumn,
  TableRow,
  TableCellValue,
  TableCellSpec,
  TableMeta,
  CellAlign,
  CellTone,
} from './tableSchema'
```

- [ ] **Step 3: Verify it compiles**

Run: `pnpm tsc --noEmit`

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/components/data-table/TableView.tsx src/components/data-table/index.ts
git commit -m "feat(data-table): assemble TableView with truncate/sort/sticky"
```

---

## Task 8: TableView integration tests

**Files:**
- Create: `src/components/data-table/__tests__/TableView.test.tsx`

**Context:** Spec §5.1 — covers all behaviors end-to-end via React Testing Library.

- [ ] **Step 1: Write the test file**

```tsx
// src/components/data-table/__tests__/TableView.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, within } from '@testing-library/react'
import { TableView } from '../TableView'
import type { TableColumn, TableRow } from '../tableSchema'

// i18n: react-i18next falls back to defaultValue when not initialized
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (_key: string, optsOrFallback?: unknown, opts?: { defaultValue?: string }) => {
      // Support both t(key, defaultValue) and t(key, { defaultValue, ...vars })
      if (typeof optsOrFallback === 'string') return optsOrFallback
      if (optsOrFallback && typeof optsOrFallback === 'object' && 'defaultValue' in optsOrFallback) {
        let s = (optsOrFallback as { defaultValue: string }).defaultValue
        for (const [k, v] of Object.entries(optsOrFallback)) {
          if (k === 'defaultValue') continue
          s = s.replace(`{{${k}}}`, String(v))
        }
        return s
      }
      return opts?.defaultValue ?? _key
    },
  }),
}))

const cols: TableColumn[] = [
  { key: 'a', label: 'A', sortable: true },
  { key: 'b', label: 'B', sortable: true, sortType: 'number', align: 'right', tabularNums: true },
]

const rows: TableRow[] = [
  { a: 'banana', b: 2 },
  { a: 'apple', b: 10 },
  { a: 'cherry', b: 5 },
]

describe('TableView — basic rendering', () => {
  it('renders headers and rows', () => {
    render(<TableView columns={cols} rows={rows} />)
    expect(screen.getByText('A')).toBeInTheDocument()
    expect(screen.getByText('B')).toBeInTheDocument()
    expect(screen.getByText('banana')).toBeInTheDocument()
  })

  it('renders empty state when rows is empty', () => {
    render(<TableView columns={cols} rows={[]} />)
    expect(screen.getByText('No data')).toBeInTheDocument()
  })

  it('renders null cell as em-dash', () => {
    render(<TableView columns={cols} rows={[{ a: null, b: 1 }]} />)
    expect(screen.getByText('—')).toBeInTheDocument()
  })

  it('renders TableCellSpec pill', () => {
    render(
      <TableView
        columns={cols}
        rows={[{ a: { text: 'OK', tone: 'success', variant: 'pill' }, b: 1 }]}
      />,
    )
    expect(screen.getByText('OK')).toBeInTheDocument()
  })

  it('does not render toolbar when no meta or copy', () => {
    render(<TableView columns={cols} rows={rows} />)
    expect(screen.queryByTestId('table-toolbar')).not.toBeInTheDocument()
  })

  it('renders toolbar with title', () => {
    render(<TableView columns={cols} rows={rows} meta={{ title: 'My Table' }} />)
    expect(screen.getByText('My Table')).toBeInTheDocument()
  })
})

describe('TableView — sort', () => {
  it('does not show sort affordance when enableSort is off', () => {
    const { container } = render(<TableView columns={cols} rows={rows} />)
    const headers = container.querySelectorAll('th[aria-sort]')
    expect(headers.length).toBe(0)
  })

  it('cycles null → asc → desc → null on click', () => {
    render(<TableView columns={cols} rows={rows} enableSort />)
    const aHeader = screen.getByText('A').closest('th') as HTMLTableCellElement
    expect(aHeader.getAttribute('aria-sort')).toBe('none')

    fireEvent.click(aHeader)
    expect(aHeader.getAttribute('aria-sort')).toBe('ascending')
    let cells = screen.getAllByText(/apple|banana|cherry/)
    expect(cells[0].textContent).toBe('apple')

    fireEvent.click(aHeader)
    expect(aHeader.getAttribute('aria-sort')).toBe('descending')
    cells = screen.getAllByText(/apple|banana|cherry/)
    expect(cells[0].textContent).toBe('cherry')

    fireEvent.click(aHeader)
    expect(aHeader.getAttribute('aria-sort')).toBe('none')
  })

  it('sorts numbers correctly', () => {
    render(<TableView columns={cols} rows={rows} enableSort />)
    const bHeader = screen.getByText('B').closest('th') as HTMLTableCellElement
    fireEvent.click(bHeader)
    const tbodyRows = document.querySelectorAll('tbody tr')
    expect(within(tbodyRows[0] as HTMLElement).getByText('2')).toBeInTheDocument()
    expect(within(tbodyRows[2] as HTMLElement).getByText('10')).toBeInTheDocument()
  })
})

describe('TableView — truncate + expand', () => {
  const many: TableRow[] = Array.from({ length: 5 }, (_, i) => ({ a: `row${i}`, b: i }))

  it('truncates rows when truncateRows is set and shows footer', () => {
    render(<TableView columns={cols} rows={many} truncateRows={2} />)
    expect(screen.getByText('row0')).toBeInTheDocument()
    expect(screen.getByText('row1')).toBeInTheDocument()
    expect(screen.queryByText('row2')).not.toBeInTheDocument()
    expect(screen.getByText(/Showing 2 of 5/)).toBeInTheDocument()
  })

  it('expands all rows when toggle clicked, then collapses again', () => {
    render(<TableView columns={cols} rows={many} truncateRows={2} />)
    fireEvent.click(screen.getByTestId('table-expand-toggle'))
    expect(screen.getByText('row4')).toBeInTheDocument()
    expect(screen.getByText(/Showing all 5/)).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('table-expand-toggle'))
    expect(screen.queryByText('row4')).not.toBeInTheDocument()
  })

  it('sort + truncate: shows the first N rows of the sorted set', () => {
    render(<TableView columns={cols} rows={many} truncateRows={2} enableSort />)
    const bHeader = screen.getByText('B').closest('th') as HTMLTableCellElement
    fireEvent.click(bHeader)  // asc by b
    fireEvent.click(bHeader)  // desc by b
    // After desc by b, first two rows should have the largest b values: 4, 3
    const tbodyRows = document.querySelectorAll('tbody tr')
    expect(within(tbodyRows[0] as HTMLElement).getByText('4')).toBeInTheDocument()
    expect(within(tbodyRows[1] as HTMLElement).getByText('3')).toBeInTheDocument()
  })
})

describe('TableView — copy', () => {
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    })
  })

  it('does not show copy button when enableCopy is off', () => {
    render(<TableView columns={cols} rows={rows} meta={{ title: 'X' }} />)
    expect(screen.queryByTestId('table-copy-button')).not.toBeInTheDocument()
  })

  it('copies CSV by default', async () => {
    render(<TableView columns={cols} rows={rows} enableCopy />)
    fireEvent.click(screen.getByTestId('table-copy-button'))
    expect(navigator.clipboard.writeText).toHaveBeenCalled()
    const text = (navigator.clipboard.writeText as unknown as ReturnType<typeof vi.fn>).mock.calls[0][0]
    expect(text).toContain('A,B')
    expect(text).toContain('banana,2')
  })

  it('copies TSV when Shift is held', async () => {
    render(<TableView columns={cols} rows={rows} enableCopy />)
    fireEvent.click(screen.getByTestId('table-copy-button'), { shiftKey: true })
    const text = (navigator.clipboard.writeText as unknown as ReturnType<typeof vi.fn>).mock.calls[0][0]
    expect(text).toContain('A\tB')
    expect(text).toContain('banana\t2')
  })

  it('always copies the full row set even when truncated', async () => {
    const many: TableRow[] = Array.from({ length: 5 }, (_, i) => ({ a: `row${i}`, b: i }))
    render(<TableView columns={cols} rows={many} truncateRows={2} enableCopy />)
    fireEvent.click(screen.getByTestId('table-copy-button'))
    const text = (navigator.clipboard.writeText as unknown as ReturnType<typeof vi.fn>).mock.calls[0][0]
    expect(text).toContain('row4')
  })
})

describe('TableView — sticky warning', () => {
  it('warns when stickyHeader is set without maxHeight', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    render(<TableView columns={cols} rows={rows} stickyHeader />)
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('stickyHeader requires maxHeight'))
    warn.mockRestore()
  })

  it('does not warn when stickyHeader and maxHeight are both set', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    render(<TableView columns={cols} rows={rows} stickyHeader maxHeight={300} />)
    expect(warn).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
```

- [ ] **Step 2: Run the tests — they should pass against the components from Tasks 4–7**

Run: `pnpm exec vitest run src/components/data-table/__tests__/TableView.test.tsx`

Expected: PASS, all cases. If something fails, the most likely culprit is the i18n mock — check the mock returns a sensible string for the actual call patterns used.

- [ ] **Step 3: Commit**

```bash
git add src/components/data-table/__tests__/TableView.test.tsx
git commit -m "test(data-table): integration tests for TableView"
```

---

## Task 9: Add the DataTable schema mapper

**Files:**
- Create: `src/components/data-table/mapDataTable.ts`
- Create: `src/components/data-table/__tests__/mapDataTable.test.ts`

**Context:** Spec §2.3. Bridges `src/types/message.ts::DataTable` (with `color: 'green'|...|'accent'` and `bold: boolean`) into the new TableView schema (with `tone` + `variant`).

- [ ] **Step 1: Write failing tests**

```ts
// src/components/data-table/__tests__/mapDataTable.test.ts
import { describe, it, expect } from 'vitest'
import { mapDataTableColumns, mapDataTableRows } from '../mapDataTable'
import type { DataTable } from '@/types/message'

describe('mapDataTableColumns', () => {
  it('passes through key/label/align', () => {
    const input: DataTable['columns'] = [
      { key: 'a', label: 'A' },
      { key: 'b', label: 'B', align: 'right' },
    ]
    expect(mapDataTableColumns(input)).toEqual([
      { key: 'a', label: 'A' },
      { key: 'b', label: 'B', align: 'right' },
    ])
  })
})

describe('mapDataTableRows', () => {
  it('maps color: green → tone: success', () => {
    const rows: DataTable['rows'] = [{ a: { text: 'ok', color: 'green' } }]
    const out = mapDataTableRows(rows)
    expect(out[0].a).toEqual({ text: 'ok', tone: 'success', variant: 'plain' })
  })

  it('maps color: orange → tone: warning', () => {
    const rows: DataTable['rows'] = [{ a: { text: 'warn', color: 'orange' } }]
    expect(mapDataTableRows(rows)[0].a).toMatchObject({ tone: 'warning' })
  })

  it('maps color: red → tone: danger', () => {
    expect(mapDataTableRows([{ a: { text: 'x', color: 'red' } }])[0].a).toMatchObject({ tone: 'danger' })
  })

  it('maps color: blue → tone: info', () => {
    expect(mapDataTableRows([{ a: { text: 'x', color: 'blue' } }])[0].a).toMatchObject({ tone: 'info' })
  })

  it('maps color: accent → tone: accent', () => {
    expect(mapDataTableRows([{ a: { text: 'x', color: 'accent' } }])[0].a).toMatchObject({ tone: 'accent' })
  })

  it('maps bold: true → variant: bold (overrides plain)', () => {
    const rows: DataTable['rows'] = [{ a: { text: 'b', bold: true } }]
    expect(mapDataTableRows(rows)[0].a).toMatchObject({ variant: 'bold' })
  })

  it('produces variant: plain when no bold and no special color', () => {
    expect(mapDataTableRows([{ a: { text: 'x' } }])[0].a).toMatchObject({ variant: 'plain' })
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm exec vitest run src/components/data-table/__tests__/mapDataTable.test.ts`

Expected: FAIL — module not found.

- [ ] **Step 3: Implement the mapper**

```ts
// src/components/data-table/mapDataTable.ts
import type { DataTable, TableColumn as MsgColumn, TableRow as MsgRow, TableCellValue as MsgCell } from '@/types/message'
import type { TableColumn, TableRow, TableCellValue, CellTone } from './tableSchema'

const COLOR_TO_TONE: Record<string, CellTone> = {
  green: 'success',
  orange: 'warning',
  red: 'danger',
  blue: 'info',
  accent: 'accent',
}

export function mapDataTableColumns(cols: MsgColumn[]): TableColumn[] {
  return cols.map((c) => ({ key: c.key, label: c.label, align: c.align }))
}

function mapCell(cell: MsgCell | undefined): TableCellValue {
  if (cell == null) return null
  const tone: CellTone | undefined = cell.color ? COLOR_TO_TONE[cell.color] : undefined
  const variant = cell.bold ? 'bold' : 'plain'
  return { text: cell.text, tone, variant }
}

export function mapDataTableRows(rows: MsgRow[]): TableRow[] {
  return rows.map((row) => {
    const out: TableRow = {}
    for (const k of Object.keys(row)) {
      out[k] = mapCell(row[k])
    }
    return out
  })
}

export function toTableMeta(table: DataTable): { title?: string; badge?: string } {
  return {
    title: table.title,
    badge: table.badge?.text,
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm exec vitest run src/components/data-table/__tests__/mapDataTable.test.ts`

Expected: PASS, all cases.

- [ ] **Step 5: Commit**

```bash
git add src/components/data-table/mapDataTable.ts src/components/data-table/__tests__/mapDataTable.test.ts
git commit -m "feat(data-table): add DataTable → TableView schema mapper"
```

---

## Task 10: Switch AiBubble from RichDataTable to TableView

**Files:**
- Modify: `src/components/chat/AiBubble.tsx`

**Context:** Spec §2.3 AiBubble end. After this task, `RichDataTable` is unused (deleted in Task 16). The app must remain functional in-between.

- [ ] **Step 1: Replace the import**

In `src/components/chat/AiBubble.tsx`, find the imports from `@/components/rich-content` (around line 25-40) and remove `RichDataTable` from that group. Then add at the top:

```tsx
import { TableView } from '@/components/data-table'
import { mapDataTableColumns, mapDataTableRows, toTableMeta } from '@/components/data-table/mapDataTable'
```

- [ ] **Step 2: Replace the `tables` case in the switch**

Find this block (around line 184–191):

```tsx
case 'tables':
  return (
    <>
      {(value as DataTable[]).map((table) => (
        <RichDataTable key={table.id} table={table} />
      ))}
    </>
  )
```

Replace with:

```tsx
case 'tables':
  return (
    <>
      {(value as DataTable[]).map((table) => (
        <div key={table.id} className="my-3">
          <TableView
            columns={mapDataTableColumns(table.columns)}
            rows={mapDataTableRows(table.rows)}
            meta={toTableMeta(table)}
            enableCopy
            truncateRows={50}
          />
        </div>
      ))}
    </>
  )
```

- [ ] **Step 3: Verify TypeScript compiles and existing AiBubble tests still pass**

Run: `pnpm tsc --noEmit && pnpm exec vitest run src/components/chat/AiBubble.subagent.test.tsx`

Expected: PASS. If `AiBubble.subagent.test.tsx` was rendering tables, it might need a small tweak — inspect output and add a test-id selector update if needed.

- [ ] **Step 4: Commit**

```bash
git add src/components/chat/AiBubble.tsx
git commit -m "feat(chat): switch AiBubble tables to TableView"
```

---

## Task 11: Install react-markdown and remark-gfm

**Files:**
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`

**Context:** React 19 compatibility. `react-markdown` v9+ supports React 18/19. `remark-gfm` v4 is the current major.

- [ ] **Step 1: Install both packages**

Run: `pnpm add react-markdown remark-gfm`

Expected: Both packages added to `dependencies`. Versions should be `react-markdown ^9.x` and `remark-gfm ^4.x`.

- [ ] **Step 2: Verify the install did not break the build**

Run: `pnpm build`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add package.json pnpm-lock.yaml
git commit -m "chore(deps): add react-markdown and remark-gfm"
```

---

## Task 12: Build the markdown CodeBlock override

**Files:**
- Create: `src/components/chat-scene/markdown/MarkdownCodeBlock.tsx`

**Context:** Replaces the `data-copy-code` event-delegation pattern. react-markdown calls the `code` override with `inline` to distinguish backtick code vs fenced blocks.

- [ ] **Step 1: Implement the component**

```tsx
// src/components/chat-scene/markdown/MarkdownCodeBlock.tsx
import { useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'

interface CodeProps {
  inline?: boolean
  className?: string
  children?: React.ReactNode
}

/**
 * react-markdown `code` override.
 * Renders inline code as <code>; fenced code blocks as a card with a copy button.
 */
export function MarkdownCodeBlock({ inline, className, children }: CodeProps) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState<'idle' | 'ok' | 'fail'>('idle')

  if (inline) {
    return (
      <code
        style={{
          background: 'var(--color-bg-base)',
          padding: '1px 5px',
          borderRadius: 3,
          fontFamily: 'var(--font-mono)',
          fontSize: '0.82em',
          color: 'var(--color-text-primary)',
        }}
      >
        {children}
      </code>
    )
  }

  // Fenced block: react-markdown puts <code class="language-xxx"> inside <pre>.
  // We extract the language from className.
  const match = /language-(\w+)/.exec(className ?? '')
  const lang = match?.[1] ?? 'code'
  const codeText = String(children ?? '').replace(/\n$/, '')

  const handleCopy = useCallback(() => {
    navigator.clipboard
      .writeText(codeText)
      .then(() => {
        setCopied('ok')
        setTimeout(() => setCopied('idle'), 2000)
      })
      .catch(() => {
        setCopied('fail')
        setTimeout(() => setCopied('idle'), 2000)
      })
  }, [codeText])

  return (
    <div
      style={{
        margin: '12px 0',
        borderRadius: 8,
        overflow: 'hidden',
        border: '1px solid var(--color-border-subtle)',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '6px 12px',
          background: 'var(--color-bg-base)',
          fontSize: '0.75rem',
          color: 'var(--color-text-muted)',
          fontFamily: 'var(--font-mono)',
        }}
      >
        <span>{lang}</span>
        <button
          type="button"
          onClick={handleCopy}
          style={{
            cursor: 'pointer',
            border: 'none',
            background: 'none',
            fontSize: '0.7rem',
            color:
              copied === 'ok'
                ? 'var(--color-semantic-green)'
                : copied === 'fail'
                  ? 'var(--color-semantic-red)'
                  : 'var(--color-text-muted)',
            fontFamily: 'var(--font-mono)',
            padding: '2px 6px',
            borderRadius: 3,
          }}
        >
          {copied === 'ok'
            ? t('common.copied', 'Copied')
            : copied === 'fail'
              ? t('common.copyFailed', 'Copy failed')
              : t('common.copy', 'Copy')}
        </button>
      </div>
      <pre
        style={{
          margin: 0,
          padding: '12px 14px',
          overflowX: 'auto',
          background: 'var(--color-bg-elevated, var(--color-bg-card))',
          fontSize: '0.82rem',
          lineHeight: 1.55,
          fontFamily: 'var(--font-mono)',
          color: 'var(--color-text-primary)',
        }}
      >
        <code>{codeText}</code>
      </pre>
    </div>
  )
}
```

- [ ] **Step 2: Verify it compiles**

Run: `pnpm tsc --noEmit`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/chat-scene/markdown/MarkdownCodeBlock.tsx
git commit -m "feat(markdown): add code block override with copy button"
```

---

## Task 13: Wire react-markdown components map (table + link + code)

**Files:**
- Create: `src/components/chat-scene/markdown/markdownComponents.tsx`

**Context:** Spec §2.3 markdown side. Extracts GFM `<table>` AST into TableView; routes file:// links to existing handler; delegates code blocks to Task 12.

- [ ] **Step 1: Implement the components map and the GFM table extractor**

```tsx
// src/components/chat-scene/markdown/markdownComponents.tsx
import { Children, isValidElement, type ReactElement, type ReactNode } from 'react'
import type { Components } from 'react-markdown'
import { useTranslation } from 'react-i18next'
import { TableView } from '@/components/data-table'
import type { TableColumn, TableRow } from '@/components/data-table'
import { useNotificationStore } from '@/stores/notificationStore'
import { openFileByName } from '@/lib/tauri'
import { MarkdownCodeBlock } from './MarkdownCodeBlock'

/** Walk react-markdown's <table> children and extract columns + rows. */
function extractTableFromGfm(node: ReactNode): { columns: TableColumn[]; rows: TableRow[] } {
  const columns: TableColumn[] = []
  const rows: TableRow[] = []

  const collectText = (n: ReactNode): string => {
    if (n == null || typeof n === 'boolean') return ''
    if (typeof n === 'string' || typeof n === 'number') return String(n)
    if (Array.isArray(n)) return n.map(collectText).join('')
    if (isValidElement(n)) {
      const props = n.props as { children?: ReactNode }
      return collectText(props.children)
    }
    return ''
  }

  // react-markdown's table children: [<thead>, <tbody>] (whitespace text nodes are skipped)
  Children.forEach(node, (section) => {
    if (!isValidElement(section)) return
    const sectionEl = section as ReactElement<{ children?: ReactNode }>
    const sectionType = String((sectionEl.type as { displayName?: string; name?: string } | string) || '')
      .toLowerCase()
    const isHead = sectionType.includes('thead') || sectionEl.type === 'thead'
    const isBody = sectionType.includes('tbody') || sectionEl.type === 'tbody'

    Children.forEach(sectionEl.props.children, (tr) => {
      if (!isValidElement(tr)) return
      const trEl = tr as ReactElement<{ children?: ReactNode }>
      const cells: string[] = []
      Children.forEach(trEl.props.children, (cell) => {
        if (!isValidElement(cell)) return
        cells.push(collectText((cell as ReactElement<{ children?: ReactNode }>).props.children).trim())
      })

      if (isHead) {
        cells.forEach((label, idx) => {
          columns.push({ key: String(idx), label })
        })
      } else if (isBody) {
        const row: TableRow = {}
        cells.forEach((text, idx) => {
          row[String(idx)] = text
        })
        rows.push(row)
      }
    })
  })

  return { columns, rows }
}

function FileLink({ href, children }: { href?: string; children?: ReactNode }) {
  const { t } = useTranslation()
  const isFileUrl = href?.startsWith('file:///')
  const isHttp = href?.startsWith('http://') || href?.startsWith('https://')

  if (isFileUrl) {
    const fileName = (() => {
      try {
        return decodeURIComponent(href!.slice(7)).split('/').pop() ?? ''
      } catch {
        return ''
      }
    })()
    return (
      <span
        role="link"
        tabIndex={0}
        title={t('common.openFile', 'Open file')}
        style={{
          cursor: 'pointer',
          textDecoration: 'underline',
          textDecorationStyle: 'dashed',
          textUnderlineOffset: 3,
          color: 'var(--color-primary)',
        }}
        onClick={() => {
          if (!fileName) return
          openFileByName(fileName).catch(() => {
            useNotificationStore.getState().push({
              level: 'error',
              title: t('chatArea.fileNotFound', 'File not found'),
              message: t('chatArea.cannotOpenFile', { fileName, defaultValue: `Cannot open ${fileName}` }),
              actions: [],
              dismissible: true,
              autoHide: 5,
              context: 'toast',
            })
          })
        }}
      >
        {children}
      </span>
    )
  }

  if (isHttp) {
    return (
      <a
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        style={{ color: 'var(--color-primary)', textDecoration: 'underline' }}
      >
        {children}
      </a>
    )
  }

  return <>{children}</>
}

export const markdownComponents: Components = {
  table({ children }) {
    const { columns, rows } = extractTableFromGfm(children)
    if (columns.length === 0) {
      // Could not parse — let react-markdown fall back to a default <table>
      console.warn('[AssistantMarkdown] Could not extract table; falling back to native render.')
      return <table>{children}</table>
    }
    return (
      <div className="my-3">
        <TableView columns={columns} rows={rows} />
      </div>
    )
  },
  a({ href, children }) {
    return <FileLink href={href}>{children}</FileLink>
  },
  code: MarkdownCodeBlock,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `pnpm tsc --noEmit`

Expected: PASS. If TypeScript complains about `Components` import or signature, check installed `react-markdown` version — for v9 the import is `import type { Components } from 'react-markdown'`.

- [ ] **Step 3: Commit**

```bash
git add src/components/chat-scene/markdown/markdownComponents.tsx
git commit -m "feat(markdown): wire components map (table/link/code overrides)"
```

---

## Task 14: Rewrite AssistantMarkdown to use react-markdown

**Files:**
- Modify: `src/components/chat-scene/AssistantMarkdown.tsx`

**Context:** Replaces `dangerouslySetInnerHTML` + `markdownToHtml`. `skipHtml` is on by default in react-markdown v9 — the option is `disallowedElements`/`urlTransform` for further hardening; we rely on default behavior.

- [ ] **Step 1: Replace the file contents**

```tsx
// src/components/chat-scene/AssistantMarkdown.tsx
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { markdownComponents } from './markdown/markdownComponents'

interface AssistantMarkdownProps {
  text: string
}

export function AssistantMarkdown({ text }: AssistantMarkdownProps) {
  if (!text.trim()) return null

  return (
    <div className="assistant-markdown text-[15px] leading-7">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        skipHtml
        components={markdownComponents}
      >
        {text}
      </ReactMarkdown>
    </div>
  )
}
```

- [ ] **Step 2: Verify TypeScript and existing AssistantMarkdown consumers compile**

Run: `pnpm tsc --noEmit`

Expected: PASS. The component signature is unchanged (`text: string` → `JSX.Element | null`), so consumers do not need updates.

- [ ] **Step 3: Run frontend tests to catch regressions**

Run: `pnpm test`

Expected: Existing markdown.test.ts may FAIL because we are about to delete it (Task 16). For now, **all other tests must pass**. If the markdown test fails on this step, that is expected — the file will be deleted in Task 16.

If any *non-markdown* test fails (e.g. a chat snapshot that expected old HTML), inspect and fix the assertion to match the new React tree before continuing.

- [ ] **Step 4: Commit**

```bash
git add src/components/chat-scene/AssistantMarkdown.tsx
git commit -m "feat(markdown): switch AssistantMarkdown to react-markdown"
```

---

## Task 15: AssistantMarkdown integration tests

**Files:**
- Create: `src/components/chat-scene/markdown/__tests__/AssistantMarkdown.test.tsx`

**Context:** Spec §5.2. We test the full pipeline (string → React tree → DOM) for the cases that matter: GFM tables routed to TableView, raw HTML stripped, code blocks with copy, and basic markdown still works.

- [ ] **Step 1: Write the test file**

```tsx
// src/components/chat-scene/markdown/__tests__/AssistantMarkdown.test.tsx
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { AssistantMarkdown } from '../../AssistantMarkdown'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (_key: string, fallback?: unknown) => {
      if (typeof fallback === 'string') return fallback
      if (fallback && typeof fallback === 'object' && 'defaultValue' in fallback) {
        return (fallback as { defaultValue: string }).defaultValue
      }
      return _key
    },
  }),
}))

vi.mock('@/lib/tauri', () => ({
  openFileByName: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: { getState: () => ({ push: vi.fn() }) },
}))

describe('AssistantMarkdown', () => {
  it('renders nothing for empty input', () => {
    const { container } = render(<AssistantMarkdown text="   " />)
    expect(container.firstChild).toBeNull()
  })

  it('renders a GFM table via TableView', () => {
    const md = `| Name | Qty |
|---|---|
| apple | 1 |
| banana | 2 |`
    render(<AssistantMarkdown text={md} />)
    expect(screen.getByTestId('table-view')).toBeInTheDocument()
    expect(screen.getByText('Name')).toBeInTheDocument()
    expect(screen.getByText('apple')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
  })

  it('strips raw HTML tags from input (skipHtml default)', () => {
    render(<AssistantMarkdown text={'plain <script>alert(1)</script> text'} />)
    expect(screen.queryByText(/script/)).not.toBeInTheDocument()
    expect(screen.getByText(/plain/)).toBeInTheDocument()
    expect(screen.getByText(/text/)).toBeInTheDocument()
  })

  it('renders fenced code blocks with a copy button', () => {
    render(<AssistantMarkdown text={'```js\nconst x = 1\n```'} />)
    expect(screen.getByText('js')).toBeInTheDocument()
    expect(screen.getByText('Copy')).toBeInTheDocument()
    expect(screen.getByText('const x = 1')).toBeInTheDocument()
  })

  it('renders inline code', () => {
    render(<AssistantMarkdown text={'use `npm install` to install'} />)
    expect(screen.getByText('npm install')).toBeInTheDocument()
  })

  it('renders bold and italic without escaping', () => {
    render(<AssistantMarkdown text={'**bold** and *italic*'} />)
    expect(screen.getByText('bold')).toBeInTheDocument()
    expect(screen.getByText('italic')).toBeInTheDocument()
  })

  it('renders standard markdown links', () => {
    render(<AssistantMarkdown text={'[Click](https://example.com)'} />)
    const link = screen.getByText('Click').closest('a') as HTMLAnchorElement
    expect(link).toBeTruthy()
    expect(link.href).toBe('https://example.com/')
  })

  it('renders empty GFM table (header only) as TableView with empty state', () => {
    const md = `| A | B |
|---|---|`
    render(<AssistantMarkdown text={md} />)
    expect(screen.getByTestId('table-view')).toBeInTheDocument()
    expect(screen.getByText('No data')).toBeInTheDocument()
  })
})
```

- [ ] **Step 2: Run tests**

Run: `pnpm exec vitest run src/components/chat-scene/markdown/__tests__/AssistantMarkdown.test.tsx`

Expected: PASS. If the GFM table test fails because `extractTableFromGfm` doesn't recognize the section types, log `console.log(section.type)` inside the extractor temporarily to see what react-markdown passes (might be `'thead'` string or a component reference), and adjust the type-detection branch in Task 13's extractor accordingly.

- [ ] **Step 3: Commit**

```bash
git add src/components/chat-scene/markdown/__tests__/AssistantMarkdown.test.tsx
git commit -m "test(markdown): integration tests for react-markdown pipeline"
```

---

## Task 16: Delete legacy markdown.ts, RichDataTable, and ChatArea event delegation

**Files:**
- Delete: `src/lib/markdown.ts`
- Delete: `src/lib/markdown.test.ts`
- Delete: `src/components/rich-content/RichDataTable.tsx`
- Modify: `src/components/rich-content/index.ts`
- Modify: `src/components/layout/ChatArea.tsx`

**Context:** Cleanup after the migration. Spec §6 migration notes.

- [ ] **Step 1: Confirm RichDataTable has no remaining importers**

Run: `grep -rn "RichDataTable" src --include="*.ts" --include="*.tsx"`

Expected: Only one match: `src/components/rich-content/index.ts`. Anything else means Task 10 missed a consumer — fix that consumer first before proceeding.

- [ ] **Step 2: Confirm markdownToHtml has no remaining importers**

Run: `grep -rn "markdownToHtml\|from '@/lib/markdown'" src --include="*.ts" --include="*.tsx"`

Expected: No matches. If any remain, switch them to `AssistantMarkdown` or fix as needed.

- [ ] **Step 3: Delete the legacy files**

Run:
```bash
rm src/lib/markdown.ts
rm src/lib/markdown.test.ts
rm src/components/rich-content/RichDataTable.tsx
```

- [ ] **Step 4: Remove the export from rich-content barrel**

Edit `src/components/rich-content/index.ts` and remove the line:

```ts
export { RichDataTable } from './RichDataTable'
```

- [ ] **Step 5: Remove the data-copy-code / data-file-link event delegation in ChatArea.tsx**

In `src/components/layout/ChatArea.tsx`, find the `useEffect` block that begins with the comment:

```tsx
// Copy-to-clipboard event delegation for markdown code blocks.
```

and delete the entire `useEffect` (currently around lines 61–117 — but use the comment as the anchor, not the line numbers). Also remove now-unused imports: check whether `i18n`, `useNotificationStore`, `openFileByName` are still used elsewhere in the file; only delete imports that have no remaining usage in this file.

Run: `grep -n "data-copy-code\|data-file-link" src/components/layout/ChatArea.tsx`

Expected: No matches.

- [ ] **Step 6: Verify build + full test suite**

Run: `pnpm tsc --noEmit && pnpm test`

Expected: PASS for everything. Specifically:
- No "Cannot find module '@/lib/markdown'" errors
- No "Cannot find name 'RichDataTable'" errors
- All chat-related tests still pass

If any test fails because it asserted on the old HTML output, update it to assert on the new React tree.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore(markdown): remove legacy markdown.ts, RichDataTable, event delegation"
```

---

## Task 17: Final verification

**Context:** Smoke test the whole change in dev mode and run the full regression set called out in CLAUDE.md.

- [ ] **Step 1: Run the frontend full test suite**

Run: `pnpm test`

Expected: PASS, including:
- `src/components/data-table/` (Tasks 3, 8, 9)
- `src/components/chat-scene/markdown/` (Task 15)
- All previously-existing tests (no regressions)

- [ ] **Step 2: Run the project's documented integration test set**

Run:
```bash
pnpm exec vitest run \
  src/lib/tauri.events.test.ts \
  src/hooks/useStreaming.integration.test.tsx \
  src/stores/chatStore.test.ts
```

Expected: PASS.

- [ ] **Step 3: Lint**

Run: `pnpm lint`

Expected: PASS. Fix any new lint warnings introduced by the new files.

- [ ] **Step 4: Manual smoke test in dev mode**

Run: `pnpm tauri:dev`

Manually verify in the running app:
- Open a chat with an AI message that contains a markdown table — confirm it renders with the new visual style (rounded outer border, zebra rows, 14px text)
- Open a chat with an AI message that contains a fenced code block — confirm the copy button works
- Open a chat with a `tables` rich-content payload (subagent results, file analyzer output, etc.) — confirm title + badge + Copy button render, and Copy puts CSV on the clipboard
- Hold Shift and click Copy on a table — confirm the clipboard now has TSV
- If a test conversation has 50+ rows in a table, confirm truncate footer appears with "Expand all"

- [ ] **Step 5: Final commit (if any test/lint touchups needed)**

If steps 1–4 needed any small fixes, commit them:

```bash
git add -A
git commit -m "fix: address final verification feedback"
```

If everything passed cleanly, no commit needed — the plan is done.

---

## Self-Review Notes

- Spec §1 (architecture) → Task 1 (tokens), Task 7 (TableView), Task 14 (AssistantMarkdown), Task 16 (cleanup)
- Spec §2 (interface) → Tasks 2, 3, 4, 5, 6, 7, 9
- Spec §3 (visual) → Task 1 (tokens), Tasks 4–7 (apply tokens via inline style+className)
- Spec §4.1 (sort) → Tasks 3, 5, 7, 8
- Spec §4.2 (truncate) → Task 7, 8
- Spec §4.3 (sticky) → Task 7, 8 (warning test)
- Spec §4.4 (copy CSV/TSV) → Tasks 3, 4, 8
- Spec §4.5 (cell rendering) → Tasks 6, 8
- Spec §4.6 (empty state) → Tasks 6, 8
- Spec §4.7 (error fallback) → Task 13 (extractTableFromGfm fallback)
- Spec §5 (testing) → Tasks 3, 8, 9, 15, 17
- Spec §6 (migration) → Tasks 11–16

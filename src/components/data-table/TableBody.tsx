import type { CSSProperties } from 'react'
import type { TableColumn, TableRow, TableCellValue, CellTone } from './tableSchema'
import { isCellSpec } from './tableSchema'

interface Props {
  columns: TableColumn[]
  rows: TableRow[]
  emptyText: string
}

const toneVar = (tone: CellTone | undefined, kind: 'bg' | 'fg') =>
  `var(--table-tone-${tone ?? 'neutral'}-${kind})`

function renderCell(value: TableCellValue) {
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
        // rowIdx is the visual row position (including post-sort), used for
        // zebra striping. TableRow has no stable id in the schema; when one
        // is added, switch to it.
        <tr
          key={rowIdx}
          className="transition-colors hover:bg-[var(--table-row-hover)]"
          style={{
            background: rowIdx % 2 === 1 ? 'var(--table-row-zebra)' : undefined,
            borderBottom: rowIdx === rows.length - 1 ? undefined : '1px solid var(--table-divider)',
          }}
        >
          {columns.map((col) => {
            const value = row[col.key]
            const wrapStyle: CSSProperties =
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
                {renderCell(value)}
              </td>
            )
          })}
        </tr>
      ))}
    </tbody>
  )
}

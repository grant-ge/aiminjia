/**
 * RichDataTable — data table with optional title and badge.
 * Based on visual-prototype-zh.html .tbl-wrap styles.
 */
import { useState, useCallback } from 'react'
import type { DataTable, TableCellValue } from '@/types/message'
import { Badge } from '@/components/common/Badge'

interface RichDataTableProps {
  table: DataTable
}

const CELL_COLOR_MAP: Record<string, string> = {
  green: 'var(--color-semantic-green)',
  orange: 'var(--color-semantic-orange)',
  red: 'var(--color-semantic-red)',
  blue: 'var(--color-semantic-blue)',
  accent: 'var(--color-accent)',
}

export function RichDataTable({ table }: RichDataTableProps) {
  const [copied, setCopied] = useState(false)

  /** Copy table data as TSV (tab-separated values) for pasting into Excel. */
  const handleCopy = useCallback(() => {
    const header = table.columns.map((col) => col.label).join('\t')
    const rows = table.rows.map((row) =>
      table.columns.map((col) => row[col.key]?.text ?? '').join('\t'),
    )
    const tsv = [header, ...rows].join('\n')
    navigator.clipboard.writeText(tsv).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    })
  }, [table])

  return (
    <div
      className="my-3 overflow-hidden rounded-lg border"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* Title row */}
      {(table.title || table.badge) && (
        <div
          className="flex items-center justify-between border-b px-4 py-3"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <div className="flex items-center gap-2">
            <span className="text-sm font-semibold" style={{ color: 'var(--color-text-primary)' }}>
              {table.title}
            </span>
            {table.badge && (
              <Badge variant={table.badge.variant}>{table.badge.text}</Badge>
            )}
          </div>
          <button
            onClick={handleCopy}
            className="flex items-center gap-1 text-xs transition-colors"
            style={{ color: copied ? 'var(--color-semantic-green)' : 'var(--color-text-muted)' }}
          >
            {copied ? '已复制' : '复制表格'}
          </button>
        </div>
      )}

      {/* Table */}
      <div className="overflow-x-auto">
        <table className="w-full text-left">
          <thead>
            <tr style={{ borderBottom: '1px solid var(--color-border)' }}>
              {table.columns.map((col) => (
                <th
                  key={col.key}
                  className="px-4 py-2.5 text-xs font-semibold uppercase tracking-wide"
                  style={{
                    color: 'var(--color-text-muted)',
                    textAlign: col.align ?? 'left',
                  }}
                >
                  {col.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {table.rows.map((row, rowIdx) => (
              <tr
                key={rowIdx}
                style={{ borderBottom: '1px solid var(--color-border-subtle)' }}
              >
                {table.columns.map((col) => {
                  const cell: TableCellValue | undefined = row[col.key]
                  return (
                    <td
                      key={col.key}
                      className="px-4 py-2 text-sm"
                      style={{
                        color: cell?.color
                          ? CELL_COLOR_MAP[cell.color] ?? 'var(--color-text-secondary)'
                          : 'var(--color-text-secondary)',
                        fontWeight: cell?.bold ? 600 : 400,
                        textAlign: col.align ?? 'left',
                      }}
                    >
                      {cell?.text ?? '—'}
                    </td>
                  )
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

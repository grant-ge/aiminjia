import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TableColumn, TableRow, TableMeta } from './tableSchema'
import { sortRows } from './tableUtils'
import type { SortState } from './tableUtils'
import { TableToolbar } from './TableToolbar'
import { TableHeader } from './TableHeader'
import { TableBody } from './TableBody'
import { Button } from '@/components/ui/button'

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

  const showFooter = isTruncated || expanded || !!meta?.footnote
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
      className={className ? `mt-5 mb-3 ${className}` : 'mt-5 mb-3'}
      data-testid="table-view"
    >
      <div
        className="overflow-hidden"
        style={{
          background: 'var(--table-bg)',
          border: '1px solid var(--table-border)',
          borderRadius: 'var(--radius-md)',
          fontSize: 'var(--table-font-size)',
          lineHeight: 'var(--table-line-height)',
        }}
      >
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
            className="flex items-center justify-between border-t px-3 py-2 text-xs border-border"
            style={{
              background: 'var(--table-header-bg)',
              borderColor: 'var(--table-divider)',
              color: 'var(--color-text-secondary)',
            }}
            data-testid="table-footer"
          >
            <span>{footerText}</span>
            {truncateRows !== undefined && sorted.length > truncateRows && (
              <Button unstyled
                type="button"
                onClick={() => setExpanded((v) => !v)}
                className="text-xs underline-offset-2 hover:underline"
                style={{ color: 'var(--color-accent)' }}
                data-testid="table-expand-toggle"
              >
                {expanded
                  ? t('dataTable.collapse', 'Collapse')
                  : t('dataTable.expandAll', 'Expand all')}
              </Button>
            )}
          </div>
        )}
      </div>

      <div className="mt-1.5 flex justify-start">
        <TableToolbar
          enableCopy={enableCopy}
          columns={columns}
          // Copy exports the full sorted dataset, not the truncated visible slice.
          rows={sorted}
        />
      </div>
    </div>
  )
}

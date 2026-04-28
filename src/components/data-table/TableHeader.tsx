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
                    fontSize: '0.625rem',
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

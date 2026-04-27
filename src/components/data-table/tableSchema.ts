// Schema for the unified TableView (src/components/data-table/).
// Note: src/types/message.ts declares its own legacy `TableColumn` / `TableRow` /
// `TableCellValue` for the chat DataTable payload. The two type families are
// intentionally separate; bridge them in mapDataTable.ts, not here.

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

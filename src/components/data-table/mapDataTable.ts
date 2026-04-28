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

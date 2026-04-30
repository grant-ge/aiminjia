import type { TableColumn, TableRow, TableCellValue } from './tableSchema'
import { cellText } from './tableSchema'

export interface SortState {
  key: string
  dir: 'asc' | 'desc'
}

const collator = new Intl.Collator(undefined, { numeric: false, sensitivity: 'base' })

type SortType = NonNullable<TableColumn['sortType']>

/** A "bad" value is one that cannot be ordered by the chosen sortType
 *  (NaN for numbers, unparseable string for dates). Strings have no bad case. */
function isBadForType(sortType: SortType | undefined, value: TableCellValue): boolean {
  if (sortType === 'number') return Number.isNaN(Number(cellText(value)))
  if (sortType === 'date')   return Number.isNaN(Date.parse(cellText(value)))
  return false
}

function compareValid(sortType: SortType | undefined, a: TableCellValue, b: TableCellValue): number {
  if (sortType === 'number') return Number(cellText(a)) - Number(cellText(b))
  if (sortType === 'date')   return Date.parse(cellText(a)) - Date.parse(cellText(b))
  return collator.compare(cellText(a), cellText(b))
}

export function sortRows(
  rows: TableRow[],
  state: SortState | null,
  columns: TableColumn[],
): TableRow[] {
  if (!state) return rows
  const col = columns.find((c) => c.key === state.key)
  if (!col) return rows

  const dirMul = state.dir === 'desc' ? -1 : 1

  return [...rows].sort((ra, rb) => {
    const va = ra[state.key]
    const vb = rb[state.key]
    const aBad = isBadForType(col.sortType, va)
    const bBad = isBadForType(col.sortType, vb)
    // Bad values always go last regardless of direction.
    if (aBad && bBad) return 0
    if (aBad) return 1
    if (bBad) return -1
    return dirMul * compareValid(col.sortType, va, vb)
  })
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

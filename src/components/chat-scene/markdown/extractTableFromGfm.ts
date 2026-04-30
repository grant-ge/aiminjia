import { Children, isValidElement, type ReactElement, type ReactNode } from 'react'
import type { TableColumn, TableRow } from '@/components/data-table'

/** Walk react-markdown's <table> children and extract columns + rows. */
export function extractTableFromGfm(node: ReactNode): { columns: TableColumn[]; rows: TableRow[] } {
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

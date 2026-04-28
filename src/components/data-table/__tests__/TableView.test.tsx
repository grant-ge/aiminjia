import '@testing-library/jest-dom'
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

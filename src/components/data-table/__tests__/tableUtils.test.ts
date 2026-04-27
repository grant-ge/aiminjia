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

  it('keeps NaN pinned to the bottom for number desc sort', () => {
    const r: TableRow[] = [{ n: 'x' }, { n: 1 }, { n: 5 }]
    const c: TableColumn[] = [{ key: 'n', label: 'N', sortable: true, sortType: 'number' }]
    const out = sortRows(r, { key: 'n', dir: 'desc' }, c)
    expect(out.map((row) => row.n)).toEqual([5, 1, 'x'])
  })

  it('keeps invalid dates pinned to the bottom for date desc sort', () => {
    const out = sortRows(rows, { key: 'when', dir: 'desc' }, cols)
    expect(out.map((r) => r.name)).toEqual(['banana', 'apple', 'cherry'])
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

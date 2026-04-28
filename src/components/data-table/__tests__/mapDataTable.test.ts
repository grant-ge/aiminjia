import { describe, it, expect } from 'vitest'
import { mapDataTableColumns, mapDataTableRows } from '../mapDataTable'
import type { DataTable } from '@/types/message'

describe('mapDataTableColumns', () => {
  it('passes through key/label/align', () => {
    const input: DataTable['columns'] = [
      { key: 'a', label: 'A' },
      { key: 'b', label: 'B', align: 'right' },
    ]
    expect(mapDataTableColumns(input)).toEqual([
      { key: 'a', label: 'A' },
      { key: 'b', label: 'B', align: 'right' },
    ])
  })
})

describe('mapDataTableRows', () => {
  it('maps color: green → tone: success', () => {
    const rows: DataTable['rows'] = [{ a: { text: 'ok', color: 'green' } }]
    const out = mapDataTableRows(rows)
    expect(out[0].a).toEqual({ text: 'ok', tone: 'success', variant: 'plain' })
  })

  it('maps color: orange → tone: warning', () => {
    const rows: DataTable['rows'] = [{ a: { text: 'warn', color: 'orange' } }]
    expect(mapDataTableRows(rows)[0].a).toMatchObject({ tone: 'warning' })
  })

  it('maps color: red → tone: danger', () => {
    expect(mapDataTableRows([{ a: { text: 'x', color: 'red' } }])[0].a).toMatchObject({ tone: 'danger' })
  })

  it('maps color: blue → tone: info', () => {
    expect(mapDataTableRows([{ a: { text: 'x', color: 'blue' } }])[0].a).toMatchObject({ tone: 'info' })
  })

  it('maps color: accent → tone: accent', () => {
    expect(mapDataTableRows([{ a: { text: 'x', color: 'accent' } }])[0].a).toMatchObject({ tone: 'accent' })
  })

  it('maps bold: true → variant: bold (overrides plain)', () => {
    const rows: DataTable['rows'] = [{ a: { text: 'b', bold: true } }]
    expect(mapDataTableRows(rows)[0].a).toMatchObject({ variant: 'bold' })
  })

  it('produces variant: plain when no bold and no special color', () => {
    expect(mapDataTableRows([{ a: { text: 'x' } }])[0].a).toMatchObject({ variant: 'plain' })
  })
})

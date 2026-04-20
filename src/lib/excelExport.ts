/**
 * Excel export — converts conversation data tables into an XLSX workbook.
 * Uses SheetJS (xlsx) to generate spreadsheets from DataTable structures.
 */
import type { DataTable } from '@/types/message'

/**
 * Export data tables as an Excel workbook and return the binary data.
 *
 * Creates a workbook with one sheet per table. Each sheet uses the table
 * title as the sheet name, with headers in the first row and data rows below.
 *
 * @param title - File name (without extension) for the exported file
 * @param tables - Array of DataTable objects to include in the workbook
 * @returns Uint8Array of the xlsx binary data
 */
export async function exportAsExcel(_title: string, tables: DataTable[]): Promise<Uint8Array> {
  const XLSX = await import('xlsx')
  const wb = XLSX.utils.book_new()

  if (tables.length === 0) {
    // Create a single empty sheet so the workbook is valid
    const ws = XLSX.utils.aoa_to_sheet([['No data tables found in this conversation.']])
    XLSX.utils.book_append_sheet(wb, ws, 'Sheet1')
  } else {
    const usedNames = new Set<string>()

    for (const table of tables) {
      // Build sheet name — XLSX sheet names max 31 chars, no special chars
      let rawName = (table.title ?? 'Sheet').replace(/[\\/*?[\]:]/g, '').slice(0, 31)
      if (!rawName) rawName = 'Sheet'

      // Deduplicate sheet names
      let sheetName = rawName
      let counter = 2
      while (usedNames.has(sheetName)) {
        const suffix = ` (${counter})`
        sheetName = rawName.slice(0, 31 - suffix.length) + suffix
        counter++
      }
      usedNames.add(sheetName)

      // Build the data array: headers + rows
      const headers = table.columns.map((col) => col.label)
      const rows = table.rows.map((row) =>
        table.columns.map((col) => row[col.key]?.text ?? ''),
      )

      const ws = XLSX.utils.aoa_to_sheet([headers, ...rows])

      // Set column widths based on content
      ws['!cols'] = table.columns.map((col, colIdx) => {
        let maxLen = col.label.length
        for (const row of rows) {
          const cellLen = (row[colIdx] ?? '').length
          if (cellLen > maxLen) maxLen = cellLen
        }
        return { wch: Math.min(Math.max(maxLen + 2, 10), 50) }
      })

      XLSX.utils.book_append_sheet(wb, ws, sheetName)
    }
  }

  // Return binary data as Uint8Array for Tauri fs write
  return new Uint8Array(XLSX.write(wb, { bookType: 'xlsx', type: 'array' }) as ArrayBuffer)
}

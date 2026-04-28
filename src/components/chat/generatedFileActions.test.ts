import { describe, expect, it } from 'vitest'

import {
  getGeneratedFilePrimaryAction,
  isFileActionEnabled,
  isPreviewableFileType,
  toPreviewTarget,
} from './generatedFileActions'

const conversationId = 'conv-1'

describe('generatedFileActions', () => {
  it.each([
    ['markdown', 'report.md'],
    ['md', 'report.md'],
    ['html', 'preview.html'],
    ['text', 'notes.txt'],
    ['json', 'data.json'],
    ['csv', 'rows.csv'],
  ])('treats %s as previewable', (fileType, fileName) => {
    expect(isPreviewableFileType(fileType, fileName)).toBe(true)
    expect(getGeneratedFilePrimaryAction({ fileType, title: fileName })).toBe('preview')
  })

  it.each([
    ['excel', 'book.xlsx'],
    ['xlsx', 'book.xlsx'],
    ['pdf', 'report.pdf'],
    ['png', 'chart.png'],
    ['jpg', 'photo.jpg'],
    ['py', 'script.py'],
    [undefined, 'unknown.bin'],
  ])('treats %s as external-open by default', (fileType, fileName) => {
    expect(isPreviewableFileType(fileType, fileName)).toBe(false)
    expect(getGeneratedFilePrimaryAction({ fileType, title: fileName })).toBe('open')
  })

  it('falls back to the filename extension when fileType is missing', () => {
    expect(isPreviewableFileType(undefined, 'summary.md')).toBe(true)
    expect(isPreviewableFileType(undefined, 'summary.xlsx')).toBe(false)
  })

  it('uses the real filename extension before the display title for primary action fallback', () => {
    expect(getGeneratedFilePrimaryAction({ title: 'Readable Report', fileName: 'report.md' })).toBe('preview')
  })

  it('treats missing actions as enabled for backward compatibility', () => {
    expect(isFileActionEnabled(undefined, 'open')).toBe(true)
    expect(isFileActionEnabled([], 'open')).toBe(true)
  })

  it('uses explicit disabled actions when actions are present', () => {
    expect(isFileActionEnabled([{ type: 'open', label: 'Open', enabled: false }], 'open')).toBe(false)
    expect(isFileActionEnabled([{ type: 'reveal', label: 'Reveal', enabled: true }], 'reveal')).toBe(true)
  })

  it('creates a preview target bound to the current conversation', () => {
    expect(toPreviewTarget({ id: 'file-1', title: 'report.md', fileType: 'markdown' }, conversationId)).toEqual({
      fileId: 'file-1',
      conversationId,
      fileName: 'report.md',
      fileType: 'markdown',
    })
  })

  it('uses the real filename before the display title for preview targets', () => {
    expect(
      toPreviewTarget({ id: 'file-1', title: 'Readable Report', fileName: 'report.md', fileType: undefined }, conversationId),
    ).toEqual({
      fileId: 'file-1',
      conversationId,
      fileName: 'report.md',
      fileType: undefined,
    })
  })
})

import { describe, expect, it } from 'vitest'

import type { FileAction } from '@/types/message'

import {
  getGeneratedFilePrimaryAction,
  isFileActionEnabled,
  isPreviewActionEnabledForFile,
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
    ['png', 'chart.png'],
    ['jpg', 'photo.jpg'],
    ['jpeg', 'photo.jpeg'],
    ['webp', 'preview.webp'],
    ['gif', 'animation.gif'],
    ['bmp', 'bitmap.bmp'],
    ['svg', 'vector.svg'],
  ])('treats %s as previewable', (fileType, fileName) => {
    expect(isPreviewableFileType(fileType, fileName)).toBe(true)
    expect(getGeneratedFilePrimaryAction({ fileType, title: fileName })).toBe('preview')
  })

  it.each([
    ['excel', 'book.xlsx'],
    ['xlsx', 'book.xlsx'],
    ['pdf', 'report.pdf'],
    ['py', 'script.py'],
    [undefined, 'unknown.bin'],
  ])('treats %s as external-open by default', (fileType, fileName) => {
    expect(isPreviewableFileType(fileType, fileName)).toBe(false)
    expect(getGeneratedFilePrimaryAction({ fileType, title: fileName })).toBe('open')
  })

  it('falls back to the filename extension when fileType is missing', () => {
    expect(isPreviewableFileType(undefined, 'summary.md')).toBe(true)
    expect(isPreviewableFileType(undefined, 'chart.png')).toBe(true)
    expect(isPreviewableFileType(undefined, 'summary.xlsx')).toBe(false)
  })

  it('falls back to previewable filename extension when fileType is generic metadata', () => {
    expect(isPreviewableFileType('image', 'mock-status-chart.png')).toBe(true)
    expect(isPreviewableFileType('chart', 'mock-status-chart.png')).toBe(true)
    expect(getGeneratedFilePrimaryAction({ fileType: 'image', fileName: 'mock-status-chart.png' })).toBe('preview')
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

  it('uses file type for preview when actions omit preview', () => {
    const actions: FileAction[] = [
      { type: 'open', label: 'Open', enabled: true },
      { type: 'reveal', label: 'Open Folder', enabled: true },
    ]

    expect(isPreviewActionEnabledForFile(actions, 'png', 'mock-status-chart.png')).toBe(true)
    expect(isPreviewActionEnabledForFile(actions, 'html', 'mock-coverage-report.html')).toBe(true)
    expect(isPreviewActionEnabledForFile(actions, 'markdown', 'mock-markdown-brief.md')).toBe(true)
    expect(isPreviewActionEnabledForFile(actions, 'json', 'mock-fallback-data.json')).toBe(true)
    expect(isPreviewActionEnabledForFile(actions, 'csv', 'mock-data-matrix.csv')).toBe(true)
    expect(isPreviewActionEnabledForFile(actions, 'pdf', 'mock-audit-summary.pdf')).toBe(false)
  })

  it('does not let actions disable type-based preview', () => {
    expect(isPreviewActionEnabledForFile([
      { type: 'preview', label: 'Preview', enabled: false },
      { type: 'open', label: 'Open', enabled: true },
    ], 'png', 'mock-status-chart.png')).toBe(true)
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

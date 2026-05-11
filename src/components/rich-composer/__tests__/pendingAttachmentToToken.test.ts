import { describe, expect, it } from 'vitest'
import {
  pendingAttachmentToToken,
  pendingAttachmentsToTokens,
} from '../pendingAttachmentToToken'
import type { PendingAttachment } from '@/hooks/useChatAttachments'

const mkPending = (overrides: Partial<PendingAttachment> = {}): PendingAttachment => ({
  id: '/abs/plan.pdf',
  fileName: 'plan.pdf',
  path: '/abs/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 1024,
  mimeType: undefined,
  source: 'picker',
  ...overrides,
})

describe('pendingAttachmentToToken', () => {
  it('file kind 完整字段映射', () => {
    const pending = mkPending()
    const token = pendingAttachmentToToken(pending)
    expect(token).toEqual({
      id: '/abs/plan.pdf',
      fileName: 'plan.pdf',
      path: '/abs/plan.pdf',
      kind: 'file',
      fileType: 'pdf',
      fileSize: 1024,
      mimeType: undefined,
      source: 'picker',
    })
  })

  it('image kind 映射', () => {
    const token = pendingAttachmentToToken(
      mkPending({ kind: 'image', fileType: 'image', source: 'clipboard-image' }),
    )
    expect(token.kind).toBe('image')
    expect(token.fileType).toBe('image')
    expect(token.source).toBe('clipboard-image')
  })

  it('folder kind 映射', () => {
    const token = pendingAttachmentToToken(
      mkPending({ kind: 'folder', fileType: 'folder', source: 'drop' }),
    )
    expect(token.kind).toBe('folder')
    expect(token.fileType).toBe('folder')
    expect(token.source).toBe('drop')
  })

  it('mimeType present', () => {
    const token = pendingAttachmentToToken(mkPending({ mimeType: 'application/pdf' }))
    expect(token.mimeType).toBe('application/pdf')
  })

  it('pendingAttachmentsToTokens 顺序保留', () => {
    const list = [
      mkPending({ id: 'a' }),
      mkPending({ id: 'b' }),
      mkPending({ id: 'c' }),
    ]
    const tokens = pendingAttachmentsToTokens(list)
    expect(tokens.map((t) => t.id)).toEqual(['a', 'b', 'c'])
  })
})

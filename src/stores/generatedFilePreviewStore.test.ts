import { beforeEach, describe, expect, it } from 'vitest'

import type { PreviewTarget } from '@/components/chat/generatedFileActions'
import { useGeneratedFilePreviewStore } from './generatedFilePreviewStore'

const target: PreviewTarget = {
  fileId: 'gf-1',
  conversationId: 'conv-1',
  fileName: 'summary.md',
  fileType: 'markdown',
}

beforeEach(() => {
  useGeneratedFilePreviewStore.setState({ target: null })
})

describe('generatedFilePreviewStore', () => {
  it('opens and closes the current preview target', () => {
    useGeneratedFilePreviewStore.getState().openPreview(target)

    expect(useGeneratedFilePreviewStore.getState().target).toEqual(target)

    useGeneratedFilePreviewStore.getState().closePreview()

    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('clears the preview target when conversation changed', () => {
    useGeneratedFilePreviewStore.getState().openPreview(target)

    useGeneratedFilePreviewStore.getState().clearIfConversationChanged('conv-2')

    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('keeps the preview target for the same conversation', () => {
    useGeneratedFilePreviewStore.getState().openPreview(target)

    useGeneratedFilePreviewStore.getState().clearIfConversationChanged('conv-1')

    expect(useGeneratedFilePreviewStore.getState().target).toEqual(target)
  })
})

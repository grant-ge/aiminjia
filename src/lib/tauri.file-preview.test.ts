import { beforeEach, describe, expect, it, vi } from 'vitest'

const coreMock = vi.hoisted(() => ({ invoke: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: coreMock.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

import { getFilePreview, type FilePreview } from './tauri'

describe('tauri file preview command', () => {
  beforeEach(() => { coreMock.invoke.mockReset() })

  it('invokes get_file_preview with fileId and conversationId', async () => {
    const preview: FilePreview = {
      kind: 'markdown',
      fileName: 'summary.md',
      mimeType: 'text/markdown',
      content: '# Summary',
    }
    coreMock.invoke.mockResolvedValue(preview)

    await expect(getFilePreview('gf-1', 'conv-1')).resolves.toEqual(preview)

    expect(coreMock.invoke).toHaveBeenCalledWith('get_file_preview', {
      fileId: 'gf-1',
      conversationId: 'conv-1',
    })
  })

  it('passes through image previews returned by the backend', async () => {
    const preview: FilePreview = {
      kind: 'image',
      fileName: 'chart.png',
      mimeType: 'image/png',
      dataUrl: 'data:image/png;base64,iVBORw==',
    }
    coreMock.invoke.mockResolvedValue(preview)

    await expect(getFilePreview('gf-chart', 'conv-1')).resolves.toEqual(preview)
  })
})

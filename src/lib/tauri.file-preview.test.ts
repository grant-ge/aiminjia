import { beforeEach, describe, expect, it, vi } from 'vitest'

const coreMock = vi.hoisted(() => ({ invoke: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: coreMock.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

import {
  getFilePreview,
  isGeneratedFileAvailable,
  isLocalDirectoryAvailable,
  isLocalFileAvailable,
  saveGeneratedFileAs,
  saveLocalFileAs,
  type FilePreview,
} from './tauri'

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

  it('invokes generated file availability through the conversation file index', async () => {
    coreMock.invoke.mockResolvedValue(true)

    await expect(isGeneratedFileAvailable('gf-chart', 'conv-1')).resolves.toBe(true)

    expect(coreMock.invoke).toHaveBeenCalledWith('is_generated_file_available', {
      fileId: 'gf-chart',
      conversationId: 'conv-1',
    })
  })

  it('invokes local file availability for explicit local artifact paths', async () => {
    coreMock.invoke.mockResolvedValue(false)

    await expect(isLocalFileAvailable('/tmp/missing.png')).resolves.toBe(false)

    expect(coreMock.invoke).toHaveBeenCalledWith('is_local_file_available', {
      path: '/tmp/missing.png',
    })
  })

  it('invokes local directory availability for workspace paths', async () => {
    coreMock.invoke.mockResolvedValue(false)

    await expect(isLocalDirectoryAvailable('/tmp/missing-workspace')).resolves.toBe(false)

    expect(coreMock.invoke).toHaveBeenCalledWith('is_local_directory_available', {
      path: '/tmp/missing-workspace',
    })
  })

  it('invokes save_generated_file_as with the selected destination path', async () => {
    coreMock.invoke.mockResolvedValue('/Users/me/Downloads/chart.png')

    await expect(saveGeneratedFileAs('gf-chart', 'conv-1', '/Users/me/Downloads/chart.png'))
      .resolves.toBe('/Users/me/Downloads/chart.png')

    expect(coreMock.invoke).toHaveBeenCalledWith('save_generated_file_as', {
      fileId: 'gf-chart',
      conversationId: 'conv-1',
      destinationPath: '/Users/me/Downloads/chart.png',
    })
  })

  it('invokes save_local_file_as for local preview targets', async () => {
    coreMock.invoke.mockResolvedValue('/Users/me/Downloads/source.png')

    await expect(saveLocalFileAs('/tmp/source.png', '/Users/me/Downloads/source.png'))
      .resolves.toBe('/Users/me/Downloads/source.png')

    expect(coreMock.invoke).toHaveBeenCalledWith('save_local_file_as', {
      path: '/tmp/source.png',
      destinationPath: '/Users/me/Downloads/source.png',
    })
  })
})

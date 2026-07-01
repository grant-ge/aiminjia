import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useBrandingStore } from '@/stores/brandingStore'

import { RuntimePanel } from './RuntimePanel'

vi.mock('@/lib/tauri', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/tauri')>()),
  getSettings: vi.fn(),
  runtimeDiagnostics: vi.fn(() => new Promise(() => {})),
  updateSettings: vi.fn(),
}))

describe('RuntimePanel', () => {
  beforeEach(() => {
    useBrandingStore.getState().reset()
  })

  it('uses tenant product name in managed runtime copy', async () => {
    useBrandingStore.setState({ productName: '小新助手' })

    render(<RuntimePanel />)

    expect(screen.getByText('小新助手 托管 Node、Python、uv 运行时，可通过 OSS 下载和更新。')).toBeInTheDocument()
    expect(screen.getByText('优先使用 小新助手 托管运行时')).toBeInTheDocument()
    expect(screen.getByText(/优先使用 小新助手 托管的 Node、Python、uv/)).toBeInTheDocument()
    expect(screen.queryByText(/AIjia/)).not.toBeInTheDocument()
  })
})

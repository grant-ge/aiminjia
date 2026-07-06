import '@testing-library/jest-dom'
import { act, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useUpdaterStore } from '@/lib/updaterStore'
import { SidebarUpdateButton } from './SidebarUpdateButton'

vi.mock('react-i18next', () => ({
  initReactI18next: {
    type: '3rdParty',
    init: vi.fn(),
  },
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) => {
      if (key === 'updater.sidebarButton') return '更新'
      if (key === 'updater.sidebarButtonDownloading') return '下载中'
      if (key === 'updater.sidebarButtonReady') return '安装'
      if (key === 'updater.sidebarButtonFailed') return '更新失败'
      if (key === 'updater.sidebarButtonInstalling') return '安装中'
      if (key === 'updater.sidebarButtonTooltip') return `打开更新 v${values?.version}`
      return key
    },
  }),
}))

function resetUpdaterStore() {
  act(() => useUpdaterStore.setState({
    phase: 'idle',
    version: null,
    panelOpen: false,
    _update: null,
    _cachedBytes: null,
    _bootstrapPromise: null,
  }))
}

describe('SidebarUpdateButton', () => {
  afterEach(() => {
    resetUpdaterStore()
  })

  it.each([
    ['available', '更新'],
    ['downloading', '下载中'],
    ['ready', '安装'],
    ['failed', '更新失败'],
    ['installing', '安装中'],
  ] as const)(
    'renders and opens the updater panel for %s state',
    async (phase, label) => {
      const user = userEvent.setup()
      act(() => useUpdaterStore.setState({
        phase,
        version: '0.5.99',
      }))

      render(<SidebarUpdateButton />)

      const button = screen.getByRole('button', { name: label })
      expect(button).toHaveAttribute('data-aijia-updater-sidebar-button')
      expect(button).toHaveAttribute('data-aijia-updater-phase', phase)
      expect(button).toHaveAttribute('data-aijia-updater-version', '0.5.99')
      expect(button).toHaveAttribute('title', '打开更新 v0.5.99')
      expect(button).toHaveClass('text-[var(--color-updater-action-foreground)]')
      expect(button).not.toHaveClass('text-white')

      await user.click(button)

      expect(useUpdaterStore.getState().panelOpen).toBe(true)
    },
  )

  it('stays hidden while idle or checking', () => {
    act(() => useUpdaterStore.setState({ phase: 'idle', version: null }))
    const { rerender } = render(<SidebarUpdateButton />)
    expect(screen.queryByRole('button', { name: '更新' })).not.toBeInTheDocument()

    act(() => useUpdaterStore.setState({ phase: 'checking', version: '0.5.99' }))
    rerender(<SidebarUpdateButton />)
    expect(screen.queryByRole('button', { name: '更新' })).not.toBeInTheDocument()
  })
})

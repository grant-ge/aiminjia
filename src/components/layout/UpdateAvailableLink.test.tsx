import '@testing-library/jest-dom'
import { act, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { UpdateAvailableLink } from './UpdateAvailableLink'
import { useUpdaterStore } from '@/lib/updaterStore'

vi.mock('react-i18next', () => ({
  initReactI18next: {
    type: '3rdParty',
    init: vi.fn(),
  },
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) => {
      if (key === 'updater.linkReady') return `v${values?.version} ready`
      return key
    },
  }),
}))

describe('UpdateAvailableLink', () => {
  afterEach(() => {
    act(() => useUpdaterStore.setState({
      phase: 'idle',
      version: null,
      panelOpen: false,
      _update: null,
      _cachedBytes: null,
      _bootstrapPromise: null,
    }))
  })

  it('uses a low-profile sidebar-title-bar treatment so the update text remains visible without a white pill', () => {
    act(() => useUpdaterStore.setState({
      phase: 'ready',
      version: '0.5.22',
      _update: { install: vi.fn() } as never,
      _cachedBytes: new Uint8Array([1]),
    }))

    render(<UpdateAvailableLink />)

    const button = screen.getByRole('button', { name: /v0\.5\.22 ready/ })
    expect(button).toHaveAttribute('data-aijia-updater-link')
    expect(button).toHaveAttribute('data-aijia-updater-version', '0.5.22')
    expect(button).toHaveClass('text-[rgba(var(--sidebar-foreground-rgb),0.80)]')
    expect(button).toHaveClass('hover:bg-[rgba(var(--sidebar-accent-rgb),0.70)]')
    expect(button).not.toHaveClass('bg-white/95')
  })
})

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
      if (key === 'updater.linkText') return `v${values?.version} ready`
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
      _downloaded: false,
      _bootstrapPromise: null,
    }))
  })

  it('uses a low-profile title-bar treatment so the update text remains visible without a white pill', () => {
    act(() => useUpdaterStore.setState({
      phase: 'ready',
      version: '0.5.22',
      _update: { install: vi.fn() } as never,
      _downloaded: true,
    }))

    render(<UpdateAvailableLink />)

    const button = screen.getByRole('button', { name: /v0\.5\.22 ready/ })
    expect(button).toHaveClass('text-primary-foreground/95')
    expect(button).toHaveClass('hover:bg-white/10')
    expect(button).not.toHaveClass('bg-white/95')
  })
})

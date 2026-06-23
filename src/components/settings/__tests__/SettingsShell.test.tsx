import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SettingsShell } from '../SettingsShell'

describe('SettingsShell', () => {
  it('renders menu and content slots', () => {
    render(
      <SettingsShell open menu={<div>menu-slot</div>} content={<div>content-slot</div>} onClose={() => {}} />,
    )
    expect(screen.getByText('menu-slot')).toBeInTheDocument()
    expect(screen.getByText('content-slot')).toBeInTheDocument()
  })

  it('does not render when open=false', () => {
    render(
      <SettingsShell open={false} menu={<div>m</div>} content={<div>c</div>} onClose={() => {}} />,
    )
    expect(screen.queryByText('m')).toBeNull()
  })

  it('uses the spec §7.9 lighter overlay token (modal lives over the sidebar surface, not a critical alert)', () => {
    render(
      <SettingsShell open menu={<div>m</div>} content={<div>c</div>} onClose={() => {}} />,
    )

    expect(screen.getByTestId('settings-overlay')).toHaveClass('bg-[var(--color-overlay-light)]')
  })

  it('clicking the overlay invokes onClose', () => {
    const onClose = vi.fn()
    render(
      <SettingsShell open menu={<div>m</div>} content={<div>c</div>} onClose={onClose} />,
    )
    fireEvent.click(screen.getByTestId('settings-overlay'))
    expect(onClose).toHaveBeenCalled()
  })

  it('modal box uses the desktop visual standard xl size (980×720) with rounded-md', () => {
    const { container } = render(
      <SettingsShell open menu={<div />} content={<div />} onClose={() => {}} />,
    )
    const modal = container.querySelector('[data-testid="settings-modal-box"]')
    expect(modal?.className).toMatch(/w-\[980px\]/)
    expect(modal?.className).toMatch(/h-\[720px\]/)
    expect(modal?.className).toMatch(/rounded-md/)
  })

  it('pressing Escape invokes onClose', () => {
    const onClose = vi.fn()
    render(
      <SettingsShell open menu={<div />} content={<div />} onClose={onClose} />,
    )
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})

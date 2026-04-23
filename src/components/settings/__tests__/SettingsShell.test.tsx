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

  it('clicking the overlay invokes onClose', () => {
    const onClose = vi.fn()
    render(
      <SettingsShell open menu={<div>m</div>} content={<div>c</div>} onClose={onClose} />,
    )
    fireEvent.click(screen.getByTestId('settings-overlay'))
    expect(onClose).toHaveBeenCalled()
  })

  it('modal box uses w-[980px] with rounded-[18px]', () => {
    const { container } = render(
      <SettingsShell open menu={<div />} content={<div />} onClose={() => {}} />,
    )
    const modal = container.querySelector('[data-testid="settings-modal-box"]')
    expect(modal?.className).toMatch(/w-\[980px\]/)
    expect(modal?.className).toMatch(/rounded-\[18px\]/)
  })
})

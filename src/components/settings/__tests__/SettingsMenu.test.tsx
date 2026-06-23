import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SettingsMenu, SETTINGS_MENU_ITEMS } from '../SettingsMenu'

// Vitest setup wires react-i18next via the actual i18n bundle, so `t(key)`
// returns the resolved zh-CN string (default). The tests below assert on
// the menu structure + click handler, not on the language-specific labels.

describe('SettingsMenu', () => {
  const visibleItems = SETTINGS_MENU_ITEMS.filter((it) => !it.disabled)

  it('renders only visible menu items (one button per enabled entry)', () => {
    render(<SettingsMenu activeKey="account" onSelect={() => {}} />)
    const buttons = screen.getAllByRole('button')
    expect(buttons.length).toBe(visibleItems.length)
  })

  it('marks active item with bg-card class', () => {
    render(<SettingsMenu activeKey="account" onSelect={() => {}} />)
    // The active entry has class "bg-card font-semibold"; locate any button
    // with that class.
    const buttons = screen.getAllByRole('button')
    const active = buttons.find((b) => /bg-card/.test(b.className))
    expect(active).toBeTruthy()
  })

  it('fires onSelect with enabled key', () => {
    const onSelect = vi.fn()
    render(<SettingsMenu activeKey="account" onSelect={onSelect} />)
    const buttons = screen.getAllByRole('button')
    const idx = visibleItems.findIndex((it) => it.key === 'archived')
    fireEvent.click(buttons[idx])
    expect(onSelect).toHaveBeenCalledWith('archived')
  })

  it('hides disabled entries', () => {
    render(<SettingsMenu activeKey="account" onSelect={() => {}} />)
    const buttons = screen.getAllByRole('button')
    expect(buttons.length).toBe(visibleItems.length)
    // Sanity: at least one entry should be disabled (i.e. menu has hidden rows).
    expect(SETTINGS_MENU_ITEMS.some((it) => it.disabled)).toBe(true)
  })
})

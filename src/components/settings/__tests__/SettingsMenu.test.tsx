import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SettingsMenu, SETTINGS_MENU_ITEMS } from '../SettingsMenu'

describe('SettingsMenu', () => {
  it('renders all 7 menu items', () => {
    render(<SettingsMenu activeKey="account" onSelect={() => {}} />)
    for (const it of SETTINGS_MENU_ITEMS) {
      expect(screen.getByRole('button', { name: it.label })).toBeInTheDocument()
    }
  })

  it('marks active item with bg-card class', () => {
    render(<SettingsMenu activeKey="usage" onSelect={() => {}} />)
    const active = screen.getByRole('button', { name: '用量' })
    expect(active.className).toMatch(/bg-card/)
  })

  it('fires onSelect with key', () => {
    const onSelect = vi.fn()
    render(<SettingsMenu activeKey="account" onSelect={onSelect} />)
    fireEvent.click(screen.getByRole('button', { name: 'MCP 服务' }))
    expect(onSelect).toHaveBeenCalledWith('mcp')
  })
})

import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SettingsMenu, SETTINGS_MENU_ITEMS } from '../SettingsMenu'

describe('SettingsMenu', () => {
  it('renders all menu items', () => {
    render(<SettingsMenu activeKey="account" onSelect={() => {}} />)
    for (const it of SETTINGS_MENU_ITEMS) {
      const name = it.disabled ? `${it.label}（未开放）` : it.label
      expect(screen.getByRole('button', { name })).toBeInTheDocument()
    }
  })

  it('marks active item with bg-card class', () => {
    render(<SettingsMenu activeKey="usage" onSelect={() => {}} />)
    const active = screen.getByRole('button', { name: '用量（未开放）' })
    expect(active.className).toMatch(/bg-card/)
  })

  it('fires onSelect with enabled key', () => {
    const onSelect = vi.fn()
    render(<SettingsMenu activeKey="account" onSelect={onSelect} />)
    fireEvent.click(screen.getByRole('button', { name: '归档记录' }))
    expect(onSelect).toHaveBeenCalledWith('archived')
  })

  it('disables settings that are not implemented yet', () => {
    const onSelect = vi.fn()
    render(<SettingsMenu activeKey="account" onSelect={onSelect} />)

    for (const label of ['用量', '系统权限', 'MCP 服务', 'SSO 集成', '快捷键']) {
      const item = screen.getByRole('button', { name: `${label}（未开放）` })
      expect(item).toBeDisabled()
      fireEvent.click(item)
    }

    expect(onSelect).not.toHaveBeenCalled()
  })
})

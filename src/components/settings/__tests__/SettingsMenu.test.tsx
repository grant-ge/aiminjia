import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SettingsMenu, SETTINGS_MENU_ITEMS } from '../SettingsMenu'

describe('SettingsMenu', () => {
  it('renders only visible menu items', () => {
    render(<SettingsMenu activeKey="account" onSelect={() => {}} />)
    for (const it of SETTINGS_MENU_ITEMS.filter((item) => !item.disabled)) {
      expect(screen.getByRole('button', { name: it.label })).toBeInTheDocument()
    }
    for (const it of SETTINGS_MENU_ITEMS.filter((item) => item.disabled)) {
      expect(screen.queryByRole('button', { name: `${it.label}（未开放）` })).not.toBeInTheDocument()
    }
  })

  it('marks active item with bg-card class', () => {
    render(<SettingsMenu activeKey="account" onSelect={() => {}} />)
    const active = screen.getByRole('button', { name: '通用' })
    expect(active.className).toMatch(/bg-card/)
  })

  it('fires onSelect with enabled key', () => {
    const onSelect = vi.fn()
    render(<SettingsMenu activeKey="account" onSelect={onSelect} />)
    fireEvent.click(screen.getByRole('button', { name: '归档记录' }))
    expect(onSelect).toHaveBeenCalledWith('archived')
  })

  it('hides settings that are not implemented yet', () => {
    const onSelect = vi.fn()
    render(<SettingsMenu activeKey="account" onSelect={onSelect} />)

    for (const label of ['用量', '系统权限', 'MCP 服务', 'SSO 集成', '快捷键']) {
      expect(screen.queryByRole('button', { name: `${label}（未开放）` })).not.toBeInTheDocument()
      expect(screen.queryByText(label)).not.toBeInTheDocument()
    }

    expect(onSelect).not.toHaveBeenCalled()
  })
})

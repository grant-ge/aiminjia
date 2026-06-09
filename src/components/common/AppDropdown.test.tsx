import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { MoreHorizontal } from 'lucide-react'
import { describe, expect, it, vi } from 'vitest'

import { AppDropdown } from './AppDropdown'

function renderDropdown() {
  const onEnabled = vi.fn()
  const onDisabled = vi.fn()

  render(
    <AppDropdown
      ariaLabel="更多操作"
      trigger={
        <button type="button">
          <MoreHorizontal aria-hidden="true" />
          打开菜单
        </button>
      }
      items={[
        { id: 'enabled', label: '可用操作', onSelect: onEnabled },
        { id: 'disabled', label: '禁用操作', disabled: true, onSelect: onDisabled },
      ]}
      contentClassName="w-40"
    />,
  )

  return { onEnabled, onDisabled }
}

describe('AppDropdown', () => {
  it('renders configured menu items and invokes enabled actions', () => {
    const { onEnabled } = renderDropdown()

    fireEvent.pointerDown(screen.getByRole('button', { name: '更多操作' }))
    fireEvent.click(screen.getByRole('menuitem', { name: '可用操作' }))

    expect(onEnabled).toHaveBeenCalledTimes(1)
  })

  it('uses the shared light menu surface styling', () => {
    renderDropdown()

    fireEvent.pointerDown(screen.getByRole('button', { name: '更多操作' }))
    const menu = screen.getByRole('menu')

    // spec §7.14 — container tier uses rounded-md (12px); was rounded-md pre-spec
    expect(menu).toHaveClass('rounded-md')
    expect(menu).toHaveClass('bg-popover')
    expect(menu).not.toHaveClass('bg-sidebar')
  })

  it('passes the Radix select event to item handlers', () => {
    const onSelect = vi.fn((event: Event) => event.preventDefault())

    render(
      <AppDropdown
        ariaLabel="事件菜单"
        trigger={<button type="button">打开事件菜单</button>}
        items={[{ id: 'dialog', label: '打开弹窗', onSelect }]}
      />,
    )

    fireEvent.pointerDown(screen.getByRole('button', { name: '事件菜单' }))
    fireEvent.click(screen.getByRole('menuitem', { name: '打开弹窗' }))

    expect(onSelect).toHaveBeenCalledTimes(1)
    expect(onSelect.mock.calls[0]?.[0]).toBeInstanceOf(Event)
  })

  it('keeps disabled menu items non-interactive', () => {
    const { onDisabled } = renderDropdown()

    fireEvent.pointerDown(screen.getByRole('button', { name: '更多操作' }))
    const item = screen.getByRole('menuitem', { name: '禁用操作' })

    expect(item).toHaveAttribute('aria-disabled', 'true')
    fireEvent.click(item)

    expect(onDisabled).not.toHaveBeenCalled()
  })
})

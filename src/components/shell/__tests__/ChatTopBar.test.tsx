import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ChatTopBar } from '../ChatTopBar'

describe('ChatTopBar', () => {
  it('renders title and workspace', () => {
    render(
      <ChatTopBar
        title="打开 BI 看板导出绩效分析数据并总结"
        workspace="Desktop"
      />,
    )
    expect(
      screen.getByText('打开 BI 看板导出绩效分析数据并总结'),
    ).toBeInTheDocument()
    expect(screen.getByText('Desktop')).toBeInTheDocument()
  })

  it('does not render updated-at metadata in the header', () => {
    const updatedAt = '2026-06-10T07:31:25.990007+00:00'
    const { container } = render(
      <ChatTopBar
        title="打开 BI 看板导出绩效分析数据并总结"
        workspace="Desktop"
        updatedAt={updatedAt}
      />,
    )

    expect(screen.queryByText(/更新于/)).not.toBeInTheDocument()
    expect(container.querySelector(`[title="${updatedAt}"]`)).not.toBeInTheDocument()
  })

  it('fires more/toggleSidebar callbacks', () => {
    const onMore = vi.fn()
    const onToggleSidebar = vi.fn()
    render(
      <ChatTopBar
        title="X"
        workspace="W"
        onMore={onMore}
        onToggleSidebar={onToggleSidebar}
      />,
    )
    screen.getByRole('button', { name: /更多/ }).click()
    screen.getByRole('button', { name: /折叠侧栏/ }).click()
    expect(onMore).toHaveBeenCalled()
    expect(onToggleSidebar).toHaveBeenCalled()
  })

  it('does not render a standalone share/export button in the header', () => {
    render(<ChatTopBar title="X" />)
    expect(screen.queryByRole('button', { name: /分享|导出对话/ })).not.toBeInTheDocument()
  })

  it('header has 48px height, px-6 and bottom border', () => {
    const { container } = render(<ChatTopBar title="X" workspace="Y" />)
    const header = container.querySelector('header')
    expect(header).toHaveClass('h-12')
    expect(header).not.toHaveClass('h-14')
    expect(header?.className).toMatch(/px-6/)
    expect(header?.className).toMatch(/border-b/)
  })

  it('header has data-tauri-drag-region', () => {
    const { container } = render(<ChatTopBar title="X" />)
    expect(container.querySelector('header')?.hasAttribute('data-tauri-drag-region')).toBe(true)
  })
})

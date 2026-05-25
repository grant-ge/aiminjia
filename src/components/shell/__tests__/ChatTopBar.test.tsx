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

  it('fires share/more/toggleSidebar callbacks', () => {
    const onShare = vi.fn()
    const onMore = vi.fn()
    const onToggleSidebar = vi.fn()
    render(
      <ChatTopBar
        title="X"
        workspace="W"
        onShare={onShare}
        onMore={onMore}
        onToggleSidebar={onToggleSidebar}
      />,
    )
    screen.getByRole('button', { name: /分享/ }).click()
    screen.getByRole('button', { name: /更多/ }).click()
    screen.getByRole('button', { name: /折叠侧栏/ }).click()
    expect(onShare).toHaveBeenCalled()
    expect(onMore).toHaveBeenCalled()
    expect(onToggleSidebar).toHaveBeenCalled()
  })

  it('header has h-10, px-6 and bottom border', () => {
    const { container } = render(<ChatTopBar title="X" workspace="Y" />)
    const header = container.querySelector('header')
    expect(header?.className).toMatch(/h-10/)
    expect(header?.className).toMatch(/px-6/)
    expect(header?.className).toMatch(/border-b/)
  })

  it('header has data-tauri-drag-region', () => {
    const { container } = render(<ChatTopBar title="X" />)
    expect(container.querySelector('header')?.hasAttribute('data-tauri-drag-region')).toBe(true)
  })
})

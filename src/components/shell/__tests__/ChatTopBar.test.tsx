import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ChatTopBar } from '../ChatTopBar'
import { useUiStore } from '@/stores/uiStore'

describe('ChatTopBar', () => {
  const originalUserAgent = navigator.userAgent

  beforeEach(() => {
    useUiStore.setState({ sidebarHidden: false })
  })

  afterEach(() => {
    Object.defineProperty(navigator, 'userAgent', {
      value: originalUserAgent,
      configurable: true,
    })
  })

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

  it('marks the workspace chip as destructive when the directory is missing', async () => {
    render(
      <ChatTopBar
        title="新对话"
        workspace="aijia-test"
        workspaceAvailable={false}
        workspacePath="/Users/me/Desktop/aijia-test"
      />,
    )

    const chip = screen.getByTestId('chat-topbar-workspace')
    expect(chip).toHaveClass('text-destructive')
    expect(chip).toHaveAttribute('data-aijia-workspace-status', 'missing')
    expect(chip).toHaveAttribute('title', '工作目录不存在：/Users/me/Desktop/aijia-test')

    await userEvent.hover(chip)
    expect(await screen.findByRole('tooltip'))
      .toHaveTextContent('工作目录不存在：/Users/me/Desktop/aijia-test')
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

  it('reserves macOS window-control space when the sidebar is hidden', () => {
    Object.defineProperty(navigator, 'userAgent', {
      value: 'Mozilla/5.0 (Macintosh)',
      configurable: true,
    })
    useUiStore.setState({ sidebarHidden: true })

    const { container } = render(<ChatTopBar title="人才盘点数据处理与分析" />)
    const header = container.querySelector('header')

    expect(header).toHaveClass('pl-48')
    expect(header).toHaveClass('transition-[padding]', 'duration-200', 'ease-out')
  })

  it('header has data-tauri-drag-region', () => {
    const { container } = render(<ChatTopBar title="X" />)
    expect(container.querySelector('header')?.hasAttribute('data-tauri-drag-region')).toBe(true)
  })
})

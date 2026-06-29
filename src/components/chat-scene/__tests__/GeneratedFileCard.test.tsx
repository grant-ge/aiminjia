import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { GeneratedFileCard } from '../GeneratedFileCard'

const defaultProps = {
  title: '绩效分析总结 · Q2',
  sub: 'Report · XLSX',
  appName: 'Microsoft Excel',
  primaryAction: 'preview' as const,
  canPreview: true,
  canOpenExternal: true,
  canDownload: true,
  canReveal: true,
  onPreview: vi.fn(),
  onOpenExternal: vi.fn(),
  onDownload: vi.fn(),
  onReveal: vi.fn(),
}

function renderCard(props: Partial<React.ComponentProps<typeof GeneratedFileCard>> = {}) {
  const callbacks = {
    onPreview: vi.fn(),
    onOpenExternal: vi.fn(),
    onDownload: vi.fn(),
    onReveal: vi.fn(),
  }

  render(<GeneratedFileCard {...defaultProps} {...callbacks} {...props} />)

  return {
    onPreview: props.onPreview ?? callbacks.onPreview,
    onOpenExternal: props.onOpenExternal ?? callbacks.onOpenExternal,
    onDownload: props.onDownload ?? callbacks.onDownload,
    onReveal: props.onReveal ?? callbacks.onReveal,
  }
}

function openMenu() {
  fireEvent.pointerDown(screen.getByRole('button', { name: '更多操作：绩效分析总结 · Q2' }))
}

describe('GeneratedFileCard', () => {
  it('renders title/sub and split preview button with the tilted file icon', () => {
    renderCard()

    expect(screen.getByText('绩效分析总结 · Q2')).toBeInTheDocument()
    expect(screen.getByText('Report · XLSX')).toBeInTheDocument()
    const icon = screen.getByLabelText('XLS file icon')
    expect(icon).toBeInTheDocument()
    expect(icon).toHaveClass('-left-[5px]')
    expect(icon.querySelector('path')).toBeInTheDocument()
    expect(icon.parentElement).not.toHaveClass('overflow-hidden')
    expect(icon.closest('[data-testid="generated-file-card"]')).toHaveClass('h-16')
    expect(screen.getByRole('button', { name: '预览 绩效分析总结 · Q2' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '更多操作：绩效分析总结 · Q2' })).toBeInTheDocument()
  })

  it('keeps short file extension labels on a single line inside the icon', () => {
    renderCard({ title: 'xiaoxin_preview.png', sub: '图片' })

    expect(screen.getByText('PNG')).toHaveClass('whitespace-nowrap')
  })

  it('keeps legacy onOpen as the default open action and disables missing reveal action', () => {
    const onOpen = vi.fn()

    render(
      <GeneratedFileCard
        title="绩效分析总结 · Q2"
        sub="Report · XLSX"
        appName="Microsoft Excel"
        onOpen={onOpen}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '打开 绩效分析总结 · Q2' }))
    openMenu()
    const openItem = screen.getByRole('menuitem', { name: '用默认应用打开' })
    expect(openItem).not.toHaveAttribute('aria-disabled', 'true')
    fireEvent.click(openItem)
    openMenu()
    const revealItem = screen.getByRole('menuitem', { name: '在文件夹中显示' })
    expect(revealItem).toHaveAttribute('aria-disabled', 'true')
    fireEvent.click(revealItem)

    expect(onOpen).toHaveBeenCalledTimes(2)
  })

  it('fires onPreview from preview primary action without opening externally', () => {
    const { onPreview, onOpenExternal } = renderCard({ primaryAction: 'preview' })

    fireEvent.click(screen.getByRole('button', { name: '预览 绩效分析总结 · Q2' }))

    expect(onPreview).toHaveBeenCalledTimes(1)
    expect(onOpenExternal).not.toHaveBeenCalled()
  })

  it('fires onOpenExternal from open primary action without previewing', () => {
    const { onPreview, onOpenExternal } = renderCard({ primaryAction: 'open' })

    fireEvent.click(screen.getByRole('button', { name: '打开 绩效分析总结 · Q2' }))

    expect(onOpenExternal).toHaveBeenCalledTimes(1)
    expect(onPreview).not.toHaveBeenCalled()
  })

  it('fires each dropdown action callback', () => {
    const { onPreview, onOpenExternal, onDownload, onReveal } = renderCard()

    openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '在侧边栏预览' }))
    openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '用默认应用打开' }))
    openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '下载到...' }))
    openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: '在文件夹中显示' }))

    expect(onPreview).toHaveBeenCalledTimes(1)
    expect(onOpenExternal).toHaveBeenCalledTimes(1)
    expect(onDownload).toHaveBeenCalledTimes(1)
    expect(onReveal).toHaveBeenCalledTimes(1)
  })

  it('uses the shared light dropdown styling for preview block actions', () => {
    renderCard()

    openMenu()

    expect(screen.getByRole('menu')).toHaveClass('bg-popover')
  })

  it('disables preview menu item and labels it unavailable when preview cannot run', () => {
    const { onPreview } = renderCard({ canPreview: false })

    openMenu()
    const item = screen.getByRole('menuitem', { name: '暂不支持预览' })
    expect(item).toHaveAttribute('aria-disabled', 'true')
    fireEvent.click(item)

    expect(onPreview).not.toHaveBeenCalled()
  })

  it('disables open primary action when opening externally cannot run', () => {
    const { onOpenExternal } = renderCard({ primaryAction: 'open', canOpenExternal: false })
    const button = screen.getByRole('button', { name: '打开 绩效分析总结 · Q2' })

    expect(button).toBeDisabled()
    fireEvent.click(button)

    expect(onOpenExternal).not.toHaveBeenCalled()
  })

  it('disables reveal menu item when reveal cannot run', () => {
    const { onReveal } = renderCard({ canReveal: false })

    openMenu()
    const item = screen.getByRole('menuitem', { name: '在文件夹中显示' })
    expect(item).toHaveAttribute('aria-disabled', 'true')
    fireEvent.click(item)

    expect(onReveal).not.toHaveBeenCalled()
  })

  it('disables download menu item when download cannot run', () => {
    const { onDownload } = renderCard({ canDownload: false })

    openMenu()
    const item = screen.getByRole('menuitem', { name: '下载到...' })
    expect(item).toHaveAttribute('aria-disabled', 'true')
    fireEvent.click(item)

    expect(onDownload).not.toHaveBeenCalled()
  })
})

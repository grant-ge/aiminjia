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
  canReveal: true,
  onPreview: vi.fn(),
  onOpenExternal: vi.fn(),
  onReveal: vi.fn(),
}

function renderCard(props: Partial<React.ComponentProps<typeof GeneratedFileCard>> = {}) {
  const callbacks = {
    onPreview: vi.fn(),
    onOpenExternal: vi.fn(),
    onReveal: vi.fn(),
  }

  render(<GeneratedFileCard {...defaultProps} {...callbacks} {...props} />)

  return {
    onPreview: props.onPreview ?? callbacks.onPreview,
    onOpenExternal: props.onOpenExternal ?? callbacks.onOpenExternal,
    onReveal: props.onReveal ?? callbacks.onReveal,
  }
}

function openMenu() {
  fireEvent.pointerDown(screen.getByRole('button', { name: 'More actions for 绩效分析总结 · Q2' }))
}

describe('GeneratedFileCard', () => {
  it('renders title/sub and split preview button', () => {
    renderCard()

    expect(screen.getByText('绩效分析总结 · Q2')).toBeInTheDocument()
    expect(screen.getByText('Report · XLSX')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Preview 绩效分析总结 · Q2' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'More actions for 绩效分析总结 · Q2' })).toBeInTheDocument()
  })

  it('keeps legacy onOpen as the default open primary action', () => {
    const onOpen = vi.fn()

    render(
      <GeneratedFileCard
        title="绩效分析总结 · Q2"
        sub="Report · XLSX"
        appName="Microsoft Excel"
        onOpen={onOpen}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Open 绩效分析总结 · Q2' }))

    expect(onOpen).toHaveBeenCalledTimes(1)
  })

  it('fires onPreview from preview primary action without opening externally', () => {
    const { onPreview, onOpenExternal } = renderCard({ primaryAction: 'preview' })

    fireEvent.click(screen.getByRole('button', { name: 'Preview 绩效分析总结 · Q2' }))

    expect(onPreview).toHaveBeenCalledTimes(1)
    expect(onOpenExternal).not.toHaveBeenCalled()
  })

  it('fires onOpenExternal from open primary action without previewing', () => {
    const { onPreview, onOpenExternal } = renderCard({ primaryAction: 'open' })

    fireEvent.click(screen.getByRole('button', { name: 'Open 绩效分析总结 · Q2' }))

    expect(onOpenExternal).toHaveBeenCalledTimes(1)
    expect(onPreview).not.toHaveBeenCalled()
  })

  it('fires each dropdown action callback', () => {
    const { onPreview, onOpenExternal, onReveal } = renderCard()

    openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Preview inside' }))
    openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Open with default app' }))
    openMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Show in folder' }))

    expect(onPreview).toHaveBeenCalledTimes(1)
    expect(onOpenExternal).toHaveBeenCalledTimes(1)
    expect(onReveal).toHaveBeenCalledTimes(1)
  })

  it('disables preview menu item and labels it unavailable when preview cannot run', () => {
    const { onPreview } = renderCard({ canPreview: false })

    openMenu()
    const item = screen.getByRole('menuitem', { name: 'Preview unavailable' })
    expect(item).toHaveAttribute('aria-disabled', 'true')
    fireEvent.click(item)

    expect(onPreview).not.toHaveBeenCalled()
  })

  it('disables open primary action when opening externally cannot run', () => {
    const { onOpenExternal } = renderCard({ primaryAction: 'open', canOpenExternal: false })
    const button = screen.getByRole('button', { name: 'Open 绩效分析总结 · Q2' })

    expect(button).toBeDisabled()
    fireEvent.click(button)

    expect(onOpenExternal).not.toHaveBeenCalled()
  })

  it('disables reveal menu item when reveal cannot run', () => {
    const { onReveal } = renderCard({ canReveal: false })

    openMenu()
    const item = screen.getByRole('menuitem', { name: 'Show in folder' })
    expect(item).toHaveAttribute('aria-disabled', 'true')
    fireEvent.click(item)

    expect(onReveal).not.toHaveBeenCalled()
  })
})

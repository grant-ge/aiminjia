import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { PageTopBar } from '../PageTopBar'
import { useUiStore } from '@/stores/uiStore'

describe('PageTopBar', () => {
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

  it('default variant: empty bar with bottom border, 48px height, px-8', () => {
    const { container } = render(<PageTopBar variant="default" />)
    const header = container.querySelector('header')
    expect(header).toHaveClass('h-12')
    expect(header).not.toHaveClass('h-14')
    expect(header?.className).toMatch(/px-8/)
    expect(header?.className).toMatch(/border-b/)
  })

  it('title variant renders the title text', () => {
    render(<PageTopBar variant="title" title="技能中心" />)
    expect(screen.getByText('技能中心')).toBeInTheDocument()
  })

  it('breadcrumb variant renders provided crumbs', () => {
    render(
      <PageTopBar
        variant="breadcrumb"
        breadcrumbs={[{ label: 'A' }, { label: 'B' }]}
      />,
    )
    expect(screen.getByText('A')).toBeInTheDocument()
    expect(screen.getByText('B')).toBeInTheDocument()
  })

  it('compact variant uses smaller text class', () => {
    const { container } = render(<PageTopBar variant="compact" title="X" />)
    expect(container.querySelector('header')?.querySelector('div')?.className).toMatch(
      /text-sm/,
    )
  })

  it('renders trailing slot when provided', () => {
    render(
      <PageTopBar
        variant="title"
        title="X"
        trailing={<span>extra</span>}
      />,
    )
    expect(screen.getByText('extra')).toBeInTheDocument()
  })

  it('keeps crowded trailing actions inside a horizontal scroll region', () => {
    render(
      <PageTopBar
        variant="title"
        title="技能中心"
        trailing={<span>extra</span>}
      />,
    )
    const trailingRegion = screen.getByText('extra').parentElement
    expect(trailingRegion?.className).toMatch(/max-w-\[70%\]/)
    expect(trailingRegion?.className).toMatch(/overflow-x-auto/)
    expect(trailingRegion?.className).toMatch(/min-w-0/)
  })

  it('header has data-tauri-drag-region', () => {
    const { container } = render(<PageTopBar variant="default" />)
    expect(container.querySelector('header')?.hasAttribute('data-tauri-drag-region')).toBe(true)
  })

  it('reserves macOS window-control space when the sidebar is hidden', () => {
    Object.defineProperty(navigator, 'userAgent', {
      value: 'Mozilla/5.0 (Macintosh)',
      configurable: true,
    })
    useUiStore.setState({ sidebarHidden: true })

    const { container } = render(<PageTopBar variant="title" title="技能中心" />)
    const header = container.querySelector('header')

    expect(header).toHaveClass('pl-48')
    expect(header).toHaveClass('transition-[padding]', 'duration-200', 'ease-out')
  })
})

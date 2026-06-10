import { render, screen, fireEvent } from '@testing-library/react'
import '@testing-library/jest-dom'
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { getDevBadgeLabel, TitleBar } from './TitleBar'

// Shared spies so window-control / drag wiring can be asserted. The inner
// closure dereferences these lazily (on getCurrentWindow() call), so they are
// already initialized by the time a test fires an event.
const startDragging = vi.fn()
const toggleMaximize = vi.fn()

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    minimize: vi.fn(),
    toggleMaximize,
    close: vi.fn(),
    startDragging,
  }),
}))

const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0)'

describe('TitleBar', () => {
  const originalUserAgent = navigator.userAgent

  beforeEach(() => {
    vi.stubEnv('DEV', false)
    startDragging.mockClear()
    toggleMaximize.mockClear()
  })

  afterEach(() => {
    Object.defineProperty(navigator, 'userAgent', { value: originalUserAgent, configurable: true })
    vi.unstubAllEnvs()
  })

  it('renders sidebar-colored strip on macOS', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    const { container } = render(<TitleBar />)
    expect(container.firstChild).toHaveStyle({ backgroundColor: 'var(--sidebar)' })
    expect(container.firstChild).toHaveClass('text-sidebar-foreground')
  })

  it('renders window controls on Windows', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Windows NT 10.0)', configurable: true })
    render(<TitleBar />)
    expect(screen.getByLabelText('Minimize')).toBeInTheDocument()
    expect(screen.getByLabelText('Maximize')).toBeInTheDocument()
    expect(screen.getByLabelText('Close')).toBeInTheDocument()
  })

  it('has a bottom border on Windows in production', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Windows NT 10.0)', configurable: true })
    const { container } = render(<TitleBar />)
    expect(container.firstChild).toHaveClass('border-b', 'border-sidebar-border')
  })

  it('shows DEV badge when import.meta.env.DEV is true', () => {
    vi.stubEnv('DEV', true)
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    render(<TitleBar />)
    expect(screen.getByText(getDevBadgeLabel())).toBeInTheDocument()
  })

  it('does not render the old diagonal stripe background in DEV', () => {
    vi.stubEnv('DEV', true)
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    const { container } = render(<TitleBar />)
    const style = (container.firstChild as HTMLElement).style
    expect(style.backgroundColor).toBe('var(--sidebar)')
    expect(style.backgroundImage).toBe('')
  })

  it('formats current dev server port in DEV badge', () => {
    expect(getDevBadgeLabel('5174')).toBe('DEV 5174')
    expect(getDevBadgeLabel('')).toBe('DEV')
  })

  it('does not show DEV badge in production build', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    render(<TitleBar />)
    expect(screen.queryByText('DEV')).not.toBeInTheDocument()
  })

  describe('Windows drag behavior', () => {
    beforeEach(() => {
      Object.defineProperty(navigator, 'userAgent', { value: WINDOWS_UA, configurable: true })
    })

    it('left-button single press starts window dragging', () => {
      const { container } = render(<TitleBar />)
      fireEvent.mouseDown(container.firstChild as Element, { buttons: 1, detail: 1 })
      expect(startDragging).toHaveBeenCalledTimes(1)
      expect(toggleMaximize).not.toHaveBeenCalled()
    })

    it('left-button double press toggles maximize instead of dragging', () => {
      const { container } = render(<TitleBar />)
      fireEvent.mouseDown(container.firstChild as Element, { buttons: 1, detail: 2 })
      expect(toggleMaximize).toHaveBeenCalledTimes(1)
      expect(startDragging).not.toHaveBeenCalled()
    })

    it('window control buttons do not trigger window dragging', () => {
      render(<TitleBar />)
      fireEvent.mouseDown(screen.getByLabelText('Close'), { buttons: 1, detail: 1 })
      expect(startDragging).not.toHaveBeenCalled()
    })
  })
})

import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { TitleBar } from './TitleBar'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore } from '@/stores/uiStore'

// Shared spies so window-control / drag wiring can be asserted. The inner
// closure dereferences these lazily (on getCurrentWindow() call), so they are
// already initialized by the time a test fires an event.
const startDragging = vi.fn()
const toggleMaximize = vi.fn()
const isFullscreen = vi.fn()
const onResized = vi.fn()

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    minimize: vi.fn(),
    toggleMaximize,
    close: vi.fn(),
    startDragging,
    isFullscreen,
    onResized,
  }),
}))

const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0)'

describe('TitleBar', () => {
  const originalUserAgent = navigator.userAgent

  beforeEach(() => {
    vi.stubEnv('DEV', false)
    localStorage.removeItem('aijia-sidebar-hidden')
    useBrandingStore.setState({
      productName: 'AI 猫',
      logoUrl: '/app-icon.png',
    })
    useUiStore.setState({ sidebarHidden: false })
    startDragging.mockClear()
    toggleMaximize.mockClear()
    isFullscreen.mockReset().mockResolvedValue(false)
    onResized.mockReset().mockResolvedValue(vi.fn())
  })

  afterEach(() => {
    Object.defineProperty(navigator, 'userAgent', { value: originalUserAgent, configurable: true })
    vi.unstubAllEnvs()
  })

  it('renders a transparent overlay strip on macOS', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    const { container } = render(<TitleBar />)
    const titleBar = container.firstChild as HTMLElement
    expect(titleBar.style.backgroundColor).toBe('transparent')
    expect(titleBar).toHaveClass('absolute', 'inset-x-0', 'top-0', 'z-10', 'h-12', 'w-full')
    expect(titleBar).toHaveClass('text-sidebar-foreground')
    const controlsLayer = container.children[1] as HTMLElement
    expect(controlsLayer).toHaveClass('pointer-events-none', 'absolute', 'z-30', 'h-12')
    expect(container.querySelector('[data-aijia-titlebar-dev-tools-dock]')).not.toBeInTheDocument()
  })

  it('renders window controls on Windows', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Windows NT 10.0)', configurable: true })
    render(<TitleBar />)
    expect(screen.getByLabelText('Minimize')).toBeInTheDocument()
    expect(screen.getByLabelText('Maximize')).toBeInTheDocument()
    expect(screen.getByLabelText('Close')).toBeInTheDocument()
  })

  it('keeps the normal occupied title bar on Windows', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Windows NT 10.0)', configurable: true })
    const { container } = render(<TitleBar />)
    const titleBar = container.firstChild as HTMLElement
    expect(titleBar).toHaveStyle({ backgroundColor: 'var(--sidebar)' })
    expect(titleBar).toHaveClass('flex', 'shrink-0')
    expect(titleBar).not.toHaveClass('absolute')
  })

  it('does not add a bottom border on Windows in production', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Windows NT 10.0)', configurable: true })
    const { container } = render(<TitleBar />)
    expect(container.firstChild).not.toHaveClass('border-b')
    expect(container.firstChild).not.toHaveClass('border-sidebar-border')
  })

  it('does not render the DEV port badge when import.meta.env.DEV is true', () => {
    vi.stubEnv('DEV', true)
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    render(<TitleBar />)
    expect(screen.queryByText(/^DEV(?:\s+\d+)?$/)).not.toBeInTheDocument()
  })

  it('does not render the dev environment switcher in the main title bar', () => {
    vi.stubEnv('DEV', true)
    Object.defineProperty(navigator, 'userAgent', { value: WINDOWS_UA, configurable: true })
    const { container } = render(<TitleBar />)

    expect(container.querySelector('[data-aijia-titlebar-dev-tools-dock]')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '环境切换' })).not.toBeInTheDocument()
  })

  it('places sidebar toggle on the left side of the macOS title bar', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    const { container } = render(<TitleBar />)

    const controlsLayer = container.children[1] as HTMLElement
    const leftGroup = controlsLayer.firstElementChild as HTMLElement
    const buttonGroup = leftGroup.firstElementChild as HTMLElement
    const toggle = screen.getByLabelText('隐藏侧栏')

    expect(leftGroup).toContainElement(toggle)
    expect(leftGroup).toHaveClass('w-64', 'justify-end', 'pr-2')
    expect(leftGroup).not.toHaveClass('pointer-events-auto')
    expect(buttonGroup).toHaveClass('pointer-events-auto')
    expect(leftGroup).not.toHaveClass('pl-20')
    expect(toggle).toHaveAttribute('data-aijia-sidebar-toggle', 'true')
    expect(container.querySelector('.lucide-panel-left')).toBeInTheDocument()
  })

  it('removes the macOS traffic-light padding while fullscreen', async () => {
    isFullscreen.mockResolvedValue(true)
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    useUiStore.setState({ sidebarHidden: true })
    const { container } = render(<TitleBar />)

    const controlsLayer = container.children[1] as HTMLElement
    const leftGroup = controlsLayer.firstElementChild as HTMLElement

    await waitFor(() => {
      expect(leftGroup).not.toHaveClass('pl-20')
    })
    expect(leftGroup).toContainElement(screen.getByLabelText('显示侧栏'))
  })

  it('keeps the macOS sidebar toggle after traffic lights when the sidebar is hidden', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    useUiStore.setState({ sidebarHidden: true })
    const { container } = render(<TitleBar />)

    const controlsLayer = container.children[1] as HTMLElement
    const leftGroup = controlsLayer.firstElementChild as HTMLElement
    const buttonGroup = leftGroup.firstElementChild as HTMLElement

    expect(leftGroup).toHaveClass('pl-20')
    expect(leftGroup).not.toHaveClass('pointer-events-auto')
    expect(leftGroup).not.toHaveClass('w-64', 'justify-end')
    expect(buttonGroup).toHaveClass('pointer-events-auto')
    expect(leftGroup).toContainElement(screen.getByLabelText('显示侧栏'))
  })

  it('renders route back and forward buttons in the macOS title bar', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    useUiStore.setState({
      route: { kind: 'skill-detail', skillId: 'sales-followup' },
      backStack: [{ kind: 'chat', conversationId: 'conv-1' }],
      forwardStack: [],
    })

    render(<TitleBar />)

    const back = screen.getByRole('button', { name: '后退' })
    const forward = screen.getByRole('button', { name: '前进' })
    expect(back).toBeEnabled()
    expect(forward).toBeDisabled()

    fireEvent.click(back)
    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'conv-1' })
    expect(screen.getByRole('button', { name: '前进' })).toBeEnabled()

    fireEvent.click(screen.getByRole('button', { name: '前进' }))
    expect(useUiStore.getState().route).toEqual({
      kind: 'skill-detail',
      skillId: 'sales-followup',
    })
  })

  it('switches to the collapsed sidebar icon after clicking the toggle', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    const { container } = render(<TitleBar />)

    fireEvent.click(screen.getByLabelText('隐藏侧栏'))

    expect(screen.getByLabelText('显示侧栏')).toBeInTheDocument()
    expect(useUiStore.getState().sidebarHidden).toBe(true)
    expect(container.querySelector('.lucide-panel-right')).toBeInTheDocument()
  })

  it('does not render the old diagonal stripe background in DEV', () => {
    vi.stubEnv('DEV', true)
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    const { container } = render(<TitleBar />)
    const style = (container.firstChild as HTMLElement).style
    expect(style.backgroundColor).toBe('transparent')
    expect(style.backgroundImage).toBe('')
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

    it('aligns sidebar controls to the sidebar edge without starting drag', () => {
      const { container } = render(<TitleBar />)
      const titleBar = container.firstElementChild as HTMLElement
      const leftGroup = titleBar.firstElementChild as HTMLElement
      const toggle = screen.getByLabelText('隐藏侧栏')

      expect(leftGroup).toContainElement(toggle)
      expect(leftGroup).toHaveClass('w-64', 'justify-end', 'pr-2')

      fireEvent.mouseDown(toggle, { buttons: 1, detail: 1 })
      expect(startDragging).not.toHaveBeenCalled()
    })

    it('does not show the tenant brand in the Windows title bar', () => {
      const { container } = render(<TitleBar />)
      const titleBar = container.firstElementChild as HTMLElement
      const leftGroup = titleBar.firstElementChild as HTMLElement
      const toggle = screen.getByLabelText('隐藏侧栏')

      expect(screen.queryByTestId('titlebar-tenant-brand')).not.toBeInTheDocument()
      expect(leftGroup).toContainElement(toggle)
      expect(screen.queryByText('AI 猫')).not.toBeInTheDocument()
    })
  })
})

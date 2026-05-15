import { render, screen } from '@testing-library/react'
import '@testing-library/jest-dom'
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { TitleBar } from './TitleBar'

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
    startDragging: vi.fn(),
  }),
}))

describe('TitleBar', () => {
  const originalUserAgent = navigator.userAgent

  beforeEach(() => {
    vi.stubEnv('DEV', false)
  })

  afterEach(() => {
    Object.defineProperty(navigator, 'userAgent', { value: originalUserAgent, configurable: true })
    vi.unstubAllEnvs()
  })

  it('renders accent strip on macOS', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    const { container } = render(<TitleBar />)
    expect(container.firstChild).toHaveClass('bg-primary')
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
    expect(container.firstChild).toHaveClass('border-b')
  })

  it('shows DEV badge when import.meta.env.DEV is true', () => {
    vi.stubEnv('DEV', true)
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    render(<TitleBar />)
    expect(screen.getByText('DEV')).toBeInTheDocument()
  })

  it('does not show DEV badge in production build', () => {
    Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Macintosh)', configurable: true })
    render(<TitleBar />)
    expect(screen.queryByText('DEV')).not.toBeInTheDocument()
  })
})

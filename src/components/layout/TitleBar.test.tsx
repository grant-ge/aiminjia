import { render, screen } from '@testing-library/react'
import '@testing-library/jest-dom'
import { describe, it, expect, vi } from 'vitest'
import { TitleBar } from './TitleBar'
import '@/i18n'

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    startDragging: vi.fn(),
    toggleMaximize: vi.fn(),
  }),
}))

describe('TitleBar', () => {
  it('title span has pointer-events-none so drag passes through to parent', () => {
    render(<TitleBar />)
    const title = screen.getByText(/AIjia|AI小家/)
    expect(title).toHaveClass('pointer-events-none')
  })
})

import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ChatComposerCompact } from '../ChatComposerCompact'

describe('ChatComposerCompact', () => {
  it('renders textarea with placeholder', () => {
    render(
      <ChatComposerCompact value="" onChange={() => {}} onSubmit={() => {}} />,
    )
    expect(
      screen.getByPlaceholderText(/继续追问/),
    ).toBeInTheDocument()
  })

  it('wrapper has rounded-[18px] border bg-card', () => {
    const { container } = render(
      <ChatComposerCompact value="" onChange={() => {}} onSubmit={() => {}} />,
    )
    const root = container.querySelector('[data-testid="composer-root"]')
    expect(root?.className).toMatch(/rounded-\[18px\]/)
    expect(root?.className).toMatch(/border/)
    expect(root?.className).toMatch(/bg-card/)
  })

  it('calls onSubmit when send button clicked with non-empty value', () => {
    const onSubmit = vi.fn()
    render(
      <ChatComposerCompact value="hello" onChange={() => {}} onSubmit={onSubmit} />,
    )
    fireEvent.click(screen.getByRole('button', { name: '发送' }))
    expect(onSubmit).toHaveBeenCalledWith('hello')
  })
})

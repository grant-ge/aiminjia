import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ConversationRow } from '../ConversationRow'

describe('ConversationRow', () => {
  it('renders title with left padding 30 (indent under project)', () => {
    const { container } = render(
      <ConversationRow title="测试会话" onClick={() => {}} />,
    )
    const btn = container.querySelector('button')
    expect(btn?.className).toMatch(/pl-\[30px\]/)
  })

  it('uses sidebar-accent bg when active', () => {
    const { container } = render(
      <ConversationRow title="X" active onClick={() => {}} />,
    )
    expect(container.querySelector('button')?.className).toMatch(/bg-sidebar-accent/)
  })

  it('shows a loader icon when loading', () => {
    const { container } = render(
      <ConversationRow title="X" loading onClick={() => {}} />,
    )
    expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
  })

  it('invokes onClick on click', () => {
    const onClick = vi.fn()
    render(<ConversationRow title="X" onClick={onClick} />)
    screen.getByRole('button').click()
    expect(onClick).toHaveBeenCalledTimes(1)
  })
})

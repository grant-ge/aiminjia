import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ConversationRow } from '../ConversationRow'

describe('ConversationRow', () => {
  it('renders title with left padding 32 on the row wrapper (indent under project)', () => {
    const { container } = render(
      <ConversationRow id="c1" title="测试会话" onClick={() => {}} />,
    )
    const wrapper = container.firstElementChild?.firstElementChild
    expect(wrapper?.className).toMatch(/pl-\[32px\]/)
  })

  it('uses sidebar-accent bg on the row wrapper when active', () => {
    const { container } = render(
      <ConversationRow id="c2" title="X" active onClick={() => {}} />,
    )
    expect(container.querySelector('.bg-sidebar-accent')).toBeInTheDocument()
  })

  it('shows a loader icon when loading', () => {
    const { container } = render(
      <ConversationRow id="c3" title="X" loading onClick={() => {}} />,
    )
    expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
  })

  it('invokes onClick on click', () => {
    const onClick = vi.fn()
    render(<ConversationRow id="c4" title="X" onClick={onClick} />)
    screen.getByRole('button', { name: 'X' }).click()
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('archive icon arms first click, fires onArchive on second click', () => {
    const onArchive = vi.fn()
    render(<ConversationRow id="c5" title="X" active onClick={() => {}} onArchive={onArchive} />)

    const archiveBtn = screen.getByRole('button', { name: '归档聊天' })
    fireEvent.click(archiveBtn)
    expect(onArchive).not.toHaveBeenCalled()
    fireEvent.click(archiveBtn)
    expect(onArchive).toHaveBeenCalledTimes(1)
  })
})

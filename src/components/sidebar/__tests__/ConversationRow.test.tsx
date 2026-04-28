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

  it('invokes configured dropdown actions', () => {
    const onArchive = vi.fn()
    const onRename = vi.fn()
    render(
      <ConversationRow
        id="c5"
        title="X"
        active
        onClick={() => {}}
        onArchive={onArchive}
        onRename={onRename}
      />,
    )

    fireEvent.pointerDown(screen.getByRole('button', { name: '聊天更多操作' }))
    fireEvent.click(screen.getByRole('menuitem', { name: '归档聊天' }))
    fireEvent.pointerDown(screen.getByRole('button', { name: '聊天更多操作' }))
    fireEvent.click(screen.getByRole('menuitem', { name: '重命名聊天' }))

    expect(onArchive).toHaveBeenCalledTimes(1)
    expect(onRename).toHaveBeenCalledTimes(1)
  })
})

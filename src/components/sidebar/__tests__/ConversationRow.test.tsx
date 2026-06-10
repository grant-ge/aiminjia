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

  it('shows a loader icon when status is loading', () => {
    const { container } = render(
      <ConversationRow id="c3" title="X" status="loading" onClick={() => {}} />,
    )
    expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
  })

  it('shows permission review chip instead of the loader while permission review is pending', () => {
    const { container } = render(
      <ConversationRow id="c3-pending" title="X" status="permission-review" onClick={() => {}} />,
    )

    expect(screen.getByText('审核')).toBeInTheDocument()
    expect(container.querySelector('[data-icon="loader"]')).not.toBeInTheDocument()
  })

  it('shows waiting reply chip while ask user question is pending', () => {
    const { container } = render(
      <ConversationRow id="c3-waiting-reply" title="X" status="waiting-reply" onClick={() => {}} />,
    )

    expect(screen.getByText('等待回复')).toBeInTheDocument()
    expect(container.querySelector('[data-icon="loader"]')).not.toBeInTheDocument()
  })

  it('keeps diagnostics in the right action slot and reveals archive controls on hover', () => {
    const onArchive = vi.fn()
    const { container } = render(
      <ConversationRow id="c3-hover" title="X" status="loading" onClick={() => {}} onArchive={onArchive} />,
    )

    expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '归档聊天' })).not.toBeInTheDocument()

    fireEvent.mouseEnter(container.querySelector('[data-aijia-conversation-row]')?.parentElement as HTMLElement)

    expect(container.querySelector('[data-icon="loader"]')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '归档聊天' })).toBeInTheDocument()
  })

  it('invokes onClick on click', () => {
    const onClick = vi.fn()
    render(<ConversationRow id="c4" title="X" onClick={onClick} />)
    screen.getByRole('button', { name: 'X' }).click()
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('archive icon arms first click, fires onArchive on second click', () => {
    const onArchive = vi.fn()
    const { container } = render(
      <ConversationRow id="c5" title="X" active onClick={() => {}} onArchive={onArchive} />,
    )

    fireEvent.mouseEnter(container.querySelector('[data-aijia-conversation-row]')?.parentElement as HTMLElement)

    const archiveBtn = screen.getByRole('button', { name: '归档聊天' })
    fireEvent.click(archiveBtn)
    expect(onArchive).not.toHaveBeenCalled()
    fireEvent.click(archiveBtn)
    expect(onArchive).toHaveBeenCalledTimes(1)
  })
})

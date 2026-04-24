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

  it('supports optional controls and tips content', () => {
    render(
      <ChatComposerCompact
        value=""
        onChange={() => {}}
        onSubmit={() => {}}
        projectLabel="Desktop"
        modelLabel="标准"
        permissionLabel="完全访问权限"
        showProjectButton={false}
        tips={<div>Enter 发送</div>}
      />,
    )

    expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
    expect(screen.getByText('标准')).toBeInTheDocument()
    expect(screen.getByText('完全访问权限')).toBeInTheDocument()
    expect(screen.getByText('Enter 发送')).toBeInTheDocument()
  })

  it('submits on Enter but not on Shift+Enter', () => {
    const onSubmit = vi.fn()
    render(
      <ChatComposerCompact value="hello" onChange={() => {}} onSubmit={onSubmit} />,
    )

    const textbox = screen.getByRole('textbox')
    fireEvent.keyDown(textbox, { key: 'Enter' })
    expect(onSubmit).toHaveBeenCalledWith('hello')

    onSubmit.mockClear()
    fireEvent.keyDown(textbox, { key: 'Enter', shiftKey: true })
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('renders pending files and stop state', () => {
    const onStop = vi.fn()
    const onOpenAttachment = vi.fn()

    render(
      <ChatComposerCompact
        value=""
        onChange={() => {}}
        onSubmit={() => {}}
        isStreaming
        onStop={onStop}
        pendingFilesSlot={<div>draft.txt</div>}
        onOpenAttachment={onOpenAttachment}
      />,
    )

    expect(screen.getByText('draft.txt')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '添加附件' }))
    expect(onOpenAttachment).toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: '停止' }))
    expect(onStop).toHaveBeenCalled()
  })

  it('uses zero gap on the left action group', () => {
    const { container } = render(
      <ChatComposerCompact value="" onChange={() => {}} onSubmit={() => {}} />,
    )
    const groups = container.querySelectorAll('.flex.items-center')
    const leftGroup = groups[1]
    expect(leftGroup?.className).toMatch(/gap-0/)
  })
})

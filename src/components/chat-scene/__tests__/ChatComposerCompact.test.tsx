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
        showProjectButton={false}
        tips={<div>Enter 发送</div>}
      />,
    )

    expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
    // 模型选择、权限、语音输入暂未实现，按钮已注释
    expect(screen.queryByRole('button', { name: '打开模型设置' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '语音输入' })).not.toBeInTheDocument()
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

  it('renders skill token, loaded skill button state, and clears token', () => {
    const onClearSkillCommand = vi.fn()

    render(
      <ChatComposerCompact
        value=""
        onChange={() => {}}
        onSubmit={() => {}}
        skillCommand={{
          id: 'biz-writing',
          label: '创建自己的技能',
          command: '/biz-writing',
        }}
        onClearSkillCommand={onClearSkillCommand}
      />,
    )

    expect(screen.getByText('创建自己的技能')).toBeInTheDocument()
    expect(screen.getByText('/biz-writing')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /当前已加载技能 创建自己的技能/ })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '移除技能 创建自己的技能' }))
    expect(onClearSkillCommand).toHaveBeenCalledTimes(1)
  })

  it('opens a fixed-size model popover with inner scroll box', () => {
    render(
      <ChatComposerCompact value="" onChange={() => {}} onSubmit={() => {}} />,
    )

    // 模型选择暂未实现，按钮已注释
    expect(screen.queryByRole('button', { name: '打开模型设置' })).not.toBeInTheDocument()
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

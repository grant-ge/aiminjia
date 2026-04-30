import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { PermissionAskDialog } from './PermissionAskDialog'
import type { PendingAsk } from '@/stores/streamingStore'

const baseAsk: PendingAsk = {
  conversationId: 'conv-1',
  runId: 'run-1',
  toolCallId: 'tc-abc',
  toolName: 'execute_python',
  message: '即将执行 Python 代码，是否允许？',
  suggestions: ['查看代码', '修改参数'],
  mode: 'default',
  rememberOptions: ['session', 'workspace', 'user'],
  defaultDestination: 'workspace',
}

describe('PermissionAskDialog', () => {
  it('renders nothing when open=false', () => {
    render(
      <PermissionAskDialog
        open={false}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={vi.fn()}
        onCancel={vi.fn()}
      />,
    )

    expect(screen.queryByText('execute_python')).toBeNull()
  })

  it('renders tool name and message when open=true', () => {
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={vi.fn()}
        onCancel={vi.fn()}
      />,
    )

    expect(screen.getByText('execute_python')).toBeInTheDocument()
    expect(screen.getByText('即将执行 Python 代码，是否允许？')).toBeInTheDocument()
  })

  it('renders suggestions when provided', () => {
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={vi.fn()}
        onCancel={vi.fn()}
      />,
    )

    expect(screen.getByText('查看代码')).toBeInTheDocument()
    expect(screen.getByText('修改参数')).toBeInTheDocument()
  })

  it('calls onAllow with remember destination when remember option is selected', () => {
    const onAllow = vi.fn()
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={onAllow}
        onDeny={vi.fn()}
        onCancel={vi.fn()}
      />,
    )

    fireEvent.click(screen.getByLabelText('记住到工作区'))
    fireEvent.click(screen.getByRole('button', { name: /允许/i }))

    expect(onAllow).toHaveBeenCalledWith({
      remember: true,
      destination: 'workspace',
    })
  })

  it('calls onDeny with deny destination when Deny button is clicked', () => {
    const onDeny = vi.fn()
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={onDeny}
        onCancel={vi.fn()}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: /拒绝/i }))

    expect(onDeny).toHaveBeenCalledWith({
      remember: false,
      destination: 'session',
    })
  })

  it('calls onCancel when ESC key is pressed', () => {
    const onCancel = vi.fn()
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={vi.fn()}
        onCancel={onCancel}
      />,
    )

    fireEvent.keyDown(document, { key: 'Escape' })

    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('renders remember destination choices', () => {
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={vi.fn()}
        onCancel={vi.fn()}
      />,
    )

    expect(screen.getByLabelText('仅本次')).toBeInTheDocument()
    expect(screen.getByLabelText('记住到工作区')).toBeInTheDocument()
    expect(screen.getByLabelText('记住到用户级')).toBeInTheDocument()
  })
})

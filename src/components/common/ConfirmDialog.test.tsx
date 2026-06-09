import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ConfirmDialog } from './ConfirmDialog'

function getOverlay() {
  return document.body.querySelector('[data-slot="confirm-dialog-overlay"]')
}

describe('ConfirmDialog', () => {
  it('renders a shared confirmation dialog with a softer gray mask', () => {
    render(
      <ConfirmDialog
        open
        title="归档此聊天？"
        description="归档后聊天将从列表中隐藏。"
        confirmLabel="归档"
        onOpenChange={vi.fn()}
        onConfirm={vi.fn()}
      />,
    )

    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    expect(screen.getByText('归档此聊天？')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '取消' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '归档' })).toBeInTheDocument()
    // spec §7.3 — confirm dialogs (AlertDialog) use full modal overlay token
    expect(getOverlay()).toHaveClass('bg-[var(--color-overlay)]')
  })

  it('uses consistent cancel styling and supports destructive confirm styling', () => {
    render(
      <ConfirmDialog
        open
        variant="destructive"
        title="彻底删除此聊天？"
        description="此操作无法撤销。"
        confirmLabel="确认删除"
        onOpenChange={vi.fn()}
        onConfirm={vi.fn()}
      />,
    )

    expect(screen.getByRole('button', { name: '取消' })).toHaveClass('border-input')
    expect(screen.getByRole('button', { name: '确认删除' })).toHaveClass('bg-destructive')
  })

  it('calls onConfirm when the confirm button is clicked', () => {
    const onConfirm = vi.fn()

    render(
      <ConfirmDialog
        open
        title="恢复此聊天？"
        description="恢复后聊天会重新出现在左侧聊天列表中。"
        confirmLabel="确认恢复"
        onOpenChange={vi.fn()}
        onConfirm={onConfirm}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '确认恢复' }))

    expect(onConfirm).toHaveBeenCalledTimes(1)
  })

  it('uses the standard modal content border', () => {
    render(
      <ConfirmDialog
        open
        title="归档此聊天？"
        description="归档后聊天将从列表中隐藏。"
        confirmLabel="归档"
        onOpenChange={vi.fn()}
        onConfirm={vi.fn()}
      />,
    )

    expect(screen.getByRole('alertdialog')).toHaveClass('border-border')
  })

})

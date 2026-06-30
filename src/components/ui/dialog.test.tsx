import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import {
  Dialog,
  DialogBody,
  DialogBodyViewport,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from './dialog'

describe('Dialog', () => {
  it('uses the spec §7.3 modal overlay token', () => {
    render(
      <Dialog open>
        <DialogContent>
          <DialogTitle>重命名聊天</DialogTitle>
          <DialogDescription>修改当前聊天标题。</DialogDescription>
        </DialogContent>
      </Dialog>,
    )

    expect(screen.getByRole('dialog')).toBeInTheDocument()
    // Tailwind arbitrary-value class for var(--color-overlay) = rgba(0,0,0,0.5)
    expect(document.body.querySelector('[data-slot="dialog-overlay"]')).toHaveClass('bg-[var(--color-overlay)]')
  })

  it('uses the standard modal content border', () => {
    render(
      <Dialog open>
        <DialogContent>
          <DialogTitle>重命名聊天</DialogTitle>
          <DialogDescription>修改当前聊天标题。</DialogDescription>
        </DialogContent>
      </Dialog>,
    )

    expect(screen.getByRole('dialog')).toHaveClass('border-border')
  })

  it('keeps modal chrome padding on header, body and footer instead of content root', () => {
    render(
      <Dialog open>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>重命名聊天</DialogTitle>
            <DialogDescription>修改当前聊天标题。</DialogDescription>
          </DialogHeader>
          <DialogBody>表单内容</DialogBody>
          <DialogFooter>操作按钮</DialogFooter>
        </DialogContent>
      </Dialog>,
    )

    const dialog = screen.getByRole('dialog')
    expect(dialog).toHaveClass('p-0', 'gap-0')
    expect(dialog).not.toHaveClass('p-6', 'gap-4')
    expect(screen.getByText('重命名聊天').closest('div')).toHaveClass('px-6', 'pt-6')
    expect(screen.getByText('表单内容')).toHaveClass('px-6', 'pt-4', 'pb-0', 'last:pb-6')
    expect(screen.getByText('操作按钮')).toHaveClass('px-6', 'pb-6')
    const closeButton = screen.getByLabelText('Close')
    expect(closeButton).toHaveClass('absolute', 'right-2', 'top-2')
    expect(closeButton.className).not.toContain('focus:ring')
    expect(closeButton).toHaveClass('focus-visible:ring-2', 'focus-visible:ring-ring')
  })

  it('provides a scroll-only body viewport for fixed header and footer dialogs', () => {
    render(
      <Dialog open>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>技能详情</DialogTitle>
            <DialogDescription>查看技能详情。</DialogDescription>
          </DialogHeader>
          <DialogBodyViewport>滚动内容</DialogBodyViewport>
          <DialogFooter>操作按钮</DialogFooter>
        </DialogContent>
      </Dialog>,
    )

    expect(screen.getByText('滚动内容')).toHaveClass('min-h-0', 'overflow-auto')
  })
})

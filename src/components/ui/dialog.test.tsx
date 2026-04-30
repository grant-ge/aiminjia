import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { Dialog, DialogContent, DialogDescription, DialogTitle } from './dialog'

describe('Dialog', () => {
  it('uses the same softer gray mask as confirm dialogs', () => {
    render(
      <Dialog open>
        <DialogContent>
          <DialogTitle>重命名聊天</DialogTitle>
          <DialogDescription>修改当前聊天标题。</DialogDescription>
        </DialogContent>
      </Dialog>,
    )

    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(document.body.querySelector('[data-slot="dialog-overlay"]')).toHaveClass('bg-gray-950/35')
  })

  it('uses a soft content border instead of the default dark border', () => {
    render(
      <Dialog open>
        <DialogContent>
          <DialogTitle>重命名聊天</DialogTitle>
          <DialogDescription>修改当前聊天标题。</DialogDescription>
        </DialogContent>
      </Dialog>,
    )

    expect(screen.getByRole('dialog')).toHaveClass('border-border/60')
  })

})

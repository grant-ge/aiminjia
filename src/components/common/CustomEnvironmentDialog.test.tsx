import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { CustomEnvironmentDialog } from './CustomEnvironmentDialog'

describe('CustomEnvironmentDialog', () => {
  it('uses the shared dialog body and footer spacing', () => {
    render(
      <CustomEnvironmentDialog
        open
        onOpenChange={vi.fn()}
        current={{ tenant: 'https://tenant.example.com', ops: 'https://ops.example.com' }}
        onConfirm={vi.fn()}
      />,
    )

    expect(screen.getByTestId('custom-environment-dialog-body')).toHaveClass('px-6')
    expect(screen.getByTestId('custom-environment-dialog-footer')).toHaveClass('px-6', 'pb-6')
  })
})

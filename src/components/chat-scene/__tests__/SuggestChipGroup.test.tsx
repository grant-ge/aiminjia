import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SuggestChipGroup } from '../SuggestChipGroup'

describe('SuggestChipGroup', () => {
  it('renders caption and fires click on chip', () => {
    const fn = vi.fn()
    render(
      <SuggestChipGroup
        caption="建议回复"
        items={[{ label: '帮我把 1on1 排进日历', onClick: fn }]}
      />,
    )
    expect(screen.getByText('建议回复')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /1on1/ }))
    expect(fn).toHaveBeenCalled()
  })
})

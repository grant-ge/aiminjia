import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { CompactBoundaryBar } from './CompactBoundaryBar'

describe('CompactBoundaryBar', () => {
  it('expands token details when clicked', () => {
    render(
      <CompactBoundaryBar
        preTokens={12000}
        postTokens={4500}
        tokensSaved={7500}
        messagesSummarized={18}
      />,
    )

    expect(screen.queryByText('压缩前')).toBeNull()

    const toggle = screen.getByRole('button', { name: /对话已压缩/ })
    expect(toggle.getAttribute('aria-expanded')).toBe('false')

    fireEvent.click(toggle)

    expect(toggle.getAttribute('aria-expanded')).toBe('true')

    expect(screen.getByText('压缩前')).not.toBeNull()
    expect(screen.getByText('压缩后')).not.toBeNull()
    expect(screen.getByText('摘要消息')).not.toBeNull()
    expect(screen.getByText('12,000')).not.toBeNull()
    expect(screen.getByText('4,500')).not.toBeNull()
    expect(screen.getByText('18')).not.toBeNull()
  })
})

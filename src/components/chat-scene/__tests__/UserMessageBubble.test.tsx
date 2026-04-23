import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { UserMessageBubble } from '../UserMessageBubble'

describe('UserMessageBubble', () => {
  it('renders text on a primary-colored bubble', () => {
    render(<UserMessageBubble text="Hello" />)
    expect(screen.getByText('Hello')).toBeInTheDocument()
  })

  it('bubble uses bg-primary and rounded-2xl', () => {
    const { container } = render(<UserMessageBubble text="X" />)
    const bubble = container.querySelector('[data-testid="user-bubble"]')
    expect(bubble?.className).toMatch(/bg-primary/)
    expect(bubble?.className).toMatch(/rounded-2xl/)
  })

  it('bubble max width is 80% of the row', () => {
    const { container } = render(<UserMessageBubble text="X" />)
    const bubble = container.querySelector('[data-testid="user-bubble"]')
    expect(bubble?.className).toMatch(/max-w-\[80%\]/)
  })
})

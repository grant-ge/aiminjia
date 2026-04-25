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

  it('renders selected skill as a visible token inside the user bubble', () => {
    const { container } = render(
      <UserMessageBubble
        text="你可以做什么"
        commandText="/salary-query 你可以做什么"
        skillCommand={{ id: 'salary-query', label: 'salary-query', command: '/salary-query' }}
      />,
    )

    const bubble = container.querySelector('[data-testid="user-bubble"]')
    const token = screen.getByTestId('user-skill-token')
    const text = screen.getByText('你可以做什么')
    expect(bubble).toContainElement(token)
    expect(bubble).toContainElement(text)
    expect(token).toHaveTextContent('salary-query')
    expect(token).toHaveAttribute('title', '/salary-query')
    expect(token.compareDocumentPosition(text) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(screen.queryByText('/salary-query')).not.toBeInTheDocument()
  })

})

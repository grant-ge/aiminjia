import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { UserMessageBubble } from '../UserMessageBubble'

describe('UserMessageBubble', () => {
  it('renders text on a primary-colored bubble', () => {
    render(<UserMessageBubble text="Hello" />)
    expect(screen.getByText('Hello')).toBeInTheDocument()
  })

  it('bubble uses bg-primary and uniform rounded-xl corners', () => {
    const { container } = render(<UserMessageBubble text="X" />)
    const bubble = container.querySelector('[data-testid="user-bubble"]')
    expect(bubble?.className).toMatch(/bg-primary/)
    expect(bubble?.className).toMatch(/rounded-xl/)
    expect(bubble?.className).not.toMatch(/rounded-b[lr]-/)
  })

  it('bubble max width is 80% of the row', () => {
    const { container } = render(<UserMessageBubble text="X" />)
    const bubble = container.querySelector('[data-testid="user-bubble"]')
    expect(bubble?.className).toMatch(/max-w-\[80%\]/)
  })

  it('bubble uses compact 12px horizontal and 8px vertical padding', () => {
    const { container } = render(<UserMessageBubble text="X" />)
    const bubble = container.querySelector('[data-testid="user-bubble"]')
    expect(bubble?.className).toMatch(/px-3/)
    expect(bubble?.className).toMatch(/py-2/)
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


  it('collapses long user messages and toggles expanded content', () => {
    const longText = Array.from({ length: 20 }, (_, i) => `第 ${i + 1} 行内容`).join('\n\n')
    const { container } = render(<UserMessageBubble text={longText} />)

    const content = screen.getByTestId('user-bubble-content')
    expect(content.className).toMatch(/max-h-\[220px\]/)
    expect(content.className).toMatch(/overflow-hidden/)

    fireEvent.click(screen.getByRole('button', { name: '展开全部' }))
    expect(content.className).not.toMatch(/max-h-\[220px\]/)
    expect(screen.getByRole('button', { name: '收起' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '收起' }))
    expect(container.querySelector('[data-testid="user-bubble-content"]')?.className).toMatch(/max-h-\[220px\]/)
    expect(screen.getByRole('button', { name: '展开全部' })).toBeInTheDocument()
  })

})

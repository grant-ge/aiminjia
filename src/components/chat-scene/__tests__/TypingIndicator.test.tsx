import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { TypingIndicator } from '../TypingIndicator'

describe('TypingIndicator', () => {
  it.each([
    ['default', '思考中…'],
    ['analyze', '分析中…'],
    ['retrieve', '检索中…'],
    ['generate', '生成中…'],
    ['organize', '整理中…'],
  ] as const)('variant %s shows label %s', (variant, label) => {
    render(<TypingIndicator variant={variant} />)
    expect(screen.getByText(label)).toBeInTheDocument()
  })

  it('uses a lighter compact icon for chat loading text', () => {
    render(<TypingIndicator variant="default" />)

    const icon = screen.getByTestId('typing-indicator-icon')
    expect(icon).toHaveClass('h-4', 'w-4')
    expect(icon).toHaveAttribute('stroke-width', '1.25')
  })
})

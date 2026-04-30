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
})

import '@testing-library/jest-dom'
import { createRef } from 'react'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { Textarea } from './textarea'

describe('Textarea', () => {
  it('renders an outlined textarea with global primary focus styling', () => {
    render(<Textarea aria-label="到点要做什么？" />)

    const textarea = screen.getByLabelText('到点要做什么？')
    expect(textarea).toHaveClass('min-h-[80px]')
    expect(textarea).toHaveClass('rounded-md')
    expect(textarea).toHaveClass('border-input')
    expect(textarea).toHaveClass('bg-card')
    expect(textarea).toHaveClass('resize-none')
    expect(textarea).toHaveClass('hover:border-primary')
    expect(textarea).toHaveClass('focus-visible:border-primary')
    expect(textarea).toHaveClass('focus-visible:ring-[rgba(var(--primary-rgb),0.15)]')
    expect(textarea).not.toHaveClass('shadow-sm')
    expect(textarea.className).not.toContain('#1677ff')
  })

  it('supports aria-invalid error styling', () => {
    render(<Textarea aria-label="到点要做什么？" aria-invalid />)

    const textarea = screen.getByLabelText('到点要做什么？')
    expect(textarea).toHaveClass('aria-invalid:border-destructive')
    expect(textarea).toHaveClass('aria-invalid:focus-visible:ring-[rgba(var(--destructive-rgb),0.15)]')
  })

  it('merges custom className and forwards refs', () => {
    const ref = createRef<HTMLTextAreaElement>()
    render(<Textarea ref={ref} aria-label="到点要做什么？" className="min-h-32" />)

    expect(screen.getByLabelText('到点要做什么？')).toHaveClass('min-h-32')
    expect(ref.current).toBe(screen.getByLabelText('到点要做什么？'))
  })
})

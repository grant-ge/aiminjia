import '@testing-library/jest-dom'
import { createRef } from 'react'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { Input } from './input'

describe('Input', () => {
  it('renders an outlined field with global primary focus styling', () => {
    render(<Input aria-label="标题" />)

    const input = screen.getByLabelText('标题')
    expect(input).toHaveClass('h-9')
    expect(input).toHaveClass('rounded-md')
    expect(input).toHaveClass('border-input')
    expect(input).toHaveClass('bg-card')
    expect(input).toHaveClass('hover:border-primary')
    expect(input).toHaveClass('focus-visible:border-primary')
    expect(input).toHaveClass('focus-visible:ring-[rgba(var(--primary-rgb),0.15)]')
    expect(input).not.toHaveClass('shadow-sm')
    expect(input.className).not.toContain('#1677ff')
  })

  it('supports aria-invalid error styling', () => {
    render(<Input aria-label="标题" aria-invalid />)

    const input = screen.getByLabelText('标题')
    expect(input).toHaveClass('aria-invalid:border-destructive')
    expect(input).toHaveClass('aria-invalid:focus-visible:ring-[rgba(var(--destructive-rgb),0.15)]')
  })

  it('merges custom className and forwards refs', () => {
    const ref = createRef<HTMLInputElement>()
    render(<Input ref={ref} aria-label="标题" className="max-w-sm" />)

    expect(screen.getByLabelText('标题')).toHaveClass('max-w-sm')
    expect(ref.current).toBe(screen.getByLabelText('标题'))
  })
})

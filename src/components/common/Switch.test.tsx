import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { Switch } from './Switch'

describe('Switch', () => {
  it('renders an accessible checked switch and calls onCheckedChange', () => {
    const onCheckedChange = vi.fn()

    render(<Switch aria-label="体验计划" checked onCheckedChange={onCheckedChange} />)

    const toggle = screen.getByRole('switch', { name: '体验计划' })
    expect(toggle).toBeChecked()
    expect(toggle).toHaveClass('bg-primary')

    fireEvent.click(toggle)

    expect(onCheckedChange).toHaveBeenCalledWith(false)
  })

  it('supports disabled state without firing changes', () => {
    const onCheckedChange = vi.fn()

    render(<Switch aria-label="开机自启动" checked={false} disabled onCheckedChange={onCheckedChange} />)

    const toggle = screen.getByRole('switch', { name: '开机自启动' })
    expect(toggle).toBeDisabled()
    expect(toggle).not.toBeChecked()
    expect(toggle).toHaveClass('h-6', 'w-11')
    expect(toggle).toHaveClass('bg-muted-foreground/20')
    expect(toggle.firstElementChild).toHaveClass('h-5', 'w-5')

    fireEvent.click(toggle)

    expect(onCheckedChange).not.toHaveBeenCalled()
  })
})

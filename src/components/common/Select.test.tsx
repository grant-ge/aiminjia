import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { Select } from './Select'

const OPTIONS = [
  { value: 'zh-CN', label: '跟随系统（简体中文）' },
  { value: 'en-US', label: 'English' },
]

describe('Select', () => {
  it('renders an accessible select with options', () => {
    render(<Select aria-label="语言" value="zh-CN" options={OPTIONS} />)

    const select = screen.getByRole('combobox', { name: '语言' })
    expect(select).toHaveValue('zh-CN')
    expect(screen.getByRole('option', { name: 'English' })).toBeInTheDocument()
  })

  it('calls onValueChange when selection changes', () => {
    const onValueChange = vi.fn()
    render(<Select aria-label="语言" value="zh-CN" options={OPTIONS} onValueChange={onValueChange} />)

    fireEvent.change(screen.getByRole('combobox', { name: '语言' }), { target: { value: 'en-US' } })

    expect(onValueChange).toHaveBeenCalledWith('en-US')
  })

  it('supports disabled state', () => {
    render(<Select aria-label="语言" value="zh-CN" options={OPTIONS} disabled />)

    expect(screen.getByRole('combobox', { name: '语言' })).toBeDisabled()
  })
})

import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { CronEditDialog } from './CronEditDialog'

describe('CronEditDialog', () => {
  it('renders with initial cron pre-selected', () => {
    render(
      <CronEditDialog open initial="0 9 * * 1" onSubmit={vi.fn()} onCancel={vi.fn()} />,
    )
    const input = screen.getByPlaceholderText(/0 9/) as HTMLInputElement
    expect(input.value).toBe('0 9 * * 1')
  })

  it('clicking preset fills the input', () => {
    render(
      <CronEditDialog open initial="" onSubmit={vi.fn()} onCancel={vi.fn()} />,
    )
    fireEvent.click(screen.getByText('每个工作日 09:00'))
    const input = screen.getByPlaceholderText(/0 9/) as HTMLInputElement
    expect(input.value).toBe('0 9 * * 1-5')
  })

  it('save calls onSubmit with trimmed value', () => {
    const onSubmit = vi.fn()
    render(
      <CronEditDialog open initial="0 9 * * 1-5" onSubmit={onSubmit} onCancel={vi.fn()} />,
    )
    fireEvent.click(screen.getByRole('button', { name: /保存/ }))
    expect(onSubmit).toHaveBeenCalledWith('0 9 * * 1-5')
  })

  it('save with empty input calls onSubmit(null)', () => {
    const onSubmit = vi.fn()
    render(
      <CronEditDialog open initial="" onSubmit={onSubmit} onCancel={vi.fn()} />,
    )
    fireEvent.click(screen.getByRole('button', { name: /保存/ }))
    expect(onSubmit).toHaveBeenCalledWith(null)
  })

  it('clear button calls onSubmit(null)', () => {
    const onSubmit = vi.fn()
    render(
      <CronEditDialog open initial="0 9 * * 1" onSubmit={onSubmit} onCancel={vi.fn()} />,
    )
    fireEvent.click(screen.getByRole('button', { name: /清除定时/ }))
    expect(onSubmit).toHaveBeenCalledWith(null)
  })
})

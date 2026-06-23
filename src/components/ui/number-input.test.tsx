import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { NumberInput } from './number-input'

describe('NumberInput', () => {
  it('clamps changed values to the configured minimum', () => {
    const onValueChange = vi.fn()

    render(
      <NumberInput
        aria-label="每 N 天"
        min={1}
        value={3}
        onValueChange={onValueChange}
      />,
    )

    fireEvent.change(screen.getByLabelText('每 N 天'), { target: { value: '0' } })

    expect(onValueChange).toHaveBeenCalledWith(1)
  })

  it('passes numeric values to onValueChange', () => {
    const onValueChange = vi.fn()

    render(
      <NumberInput
        aria-label="执行次数"
        min={1}
        value={1}
        onValueChange={onValueChange}
      />,
    )

    fireEvent.change(screen.getByLabelText('执行次数'), { target: { value: '8' } })

    expect(onValueChange).toHaveBeenCalledWith(8)
  })

  it('uses a plain text-field appearance instead of browser spin buttons', () => {
    render(
      <NumberInput
        aria-label="执行次数"
        min={1}
        value={1}
        onValueChange={() => {}}
      />,
    )

    expect(screen.getByLabelText('执行次数')).toHaveClass('[appearance:textfield]')
  })
})

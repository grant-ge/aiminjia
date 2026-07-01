import '@testing-library/jest-dom'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { beforeAll, describe, expect, it, vi } from 'vitest'

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './select'

describe('Select', () => {
  beforeAll(() => {
    Object.defineProperty(HTMLElement.prototype, 'hasPointerCapture', {
      configurable: true,
      value: () => false,
    })
    Object.defineProperty(HTMLElement.prototype, 'setPointerCapture', {
      configurable: true,
      value: () => undefined,
    })
    Object.defineProperty(HTMLElement.prototype, 'releasePointerCapture', {
      configurable: true,
      value: () => undefined,
    })
  })

  it('renders a form combobox trigger with the selected value', () => {
    render(
      <Select value="daily">
        <SelectTrigger aria-label="频率">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="one_shot">一次性</SelectItem>
          <SelectItem value="daily">每天</SelectItem>
        </SelectContent>
      </Select>,
    )

    const trigger = screen.getByRole('combobox', { name: '频率' })
    expect(trigger).toHaveTextContent('每天')
    expect(trigger).toHaveClass('border-input')
    expect(trigger).toHaveClass('hover:border-primary')
    expect(trigger).not.toHaveClass('shadow-sm')
    expect(trigger.className).not.toContain('#1677ff')
  })

  it('calls onValueChange when an option is selected', () => {
    const onValueChange = vi.fn()
    render(
      <Select value="one_shot" onValueChange={onValueChange}>
        <SelectTrigger aria-label="频率">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="one_shot">一次性</SelectItem>
          <SelectItem value="weekly">每周</SelectItem>
        </SelectContent>
      </Select>,
    )

    fireEvent.pointerDown(screen.getByRole('combobox', { name: '频率' }), {
      button: 0,
      ctrlKey: false,
      pointerType: 'mouse',
    })
    const listbox = screen.getByRole('listbox')
    fireEvent.click(within(listbox).getByRole('option', { name: '每周' }))

    expect(onValueChange).toHaveBeenCalledWith('weekly')
  })

  it('uses the global primary token for selected option emphasis', () => {
    render(
      <Select value="weekly">
        <SelectTrigger aria-label="频率">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="weekly">每周</SelectItem>
        </SelectContent>
      </Select>,
    )

    fireEvent.pointerDown(screen.getByRole('combobox', { name: '频率' }), {
      button: 0,
      ctrlKey: false,
      pointerType: 'mouse',
    })

    const option = screen.getByRole('option', { name: '每周' })
    expect(option).toHaveClass('data-[state=checked]:bg-[rgba(var(--primary-rgb),0.10)]')
    expect(option.className).not.toContain('#1677ff')
  })
})

import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ModelPickerPopover } from './ModelPickerPopover'

describe('ModelPickerPopover', () => {
  it('uses fixed shell size and inner scroll grid', () => {
    const { container } = render(
      <ModelPickerPopover open value="deepseek-v3" onChange={() => {}} onClose={() => {}} />,
    )

    const shell = container.firstElementChild as HTMLElement | null
    expect(shell?.className).toMatch(/w-\[min\(620px,calc\(100vw-48px\)\)\]/)
    expect(shell?.className).toMatch(/h-\[400px\]/)

    const gridBox = screen.getByTestId('model-popover-grid-box')
    expect(gridBox.className).toMatch(/overflow-y-auto/)
    expect(gridBox.className).toMatch(/h-full/)
    expect(gridBox.className).toMatch(/overscroll-contain/)
  })

  it('selects model and closes popover', () => {
    const onChange = vi.fn()
    const onClose = vi.fn()

    render(
      <ModelPickerPopover open value="deepseek-v3" onChange={onChange} onClose={onClose} />,
    )

    fireEvent.click(screen.getByRole('button', { name: /Custom/i }))
    expect(onChange).toHaveBeenCalledWith('custom')
    expect(onClose).toHaveBeenCalled()
  })
})

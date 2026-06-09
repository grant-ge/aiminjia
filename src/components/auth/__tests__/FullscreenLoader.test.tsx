import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { FullscreenLoader } from '../FullscreenLoader'

describe('FullscreenLoader', () => {
  it('uses runtime global theme variables for its colors', () => {
    render(<FullscreenLoader />)

    const loader = screen.getByTestId('fullscreen-loader')
    expect(loader).toHaveStyle({ background: 'var(--color-bg-main)' })
    expect(loader).toHaveStyle({ color: 'var(--color-text-primary)' })

    const spinner = screen.getByLabelText('正在恢复登录状态...')
    expect(spinner).toHaveClass('rounded-full')
    expect(spinner).not.toHaveClass('rounded-md')
    expect(spinner).toHaveAttribute('style', expect.stringContaining('border-right-color: var(--color-border)'))
    expect(spinner).toHaveAttribute('style', expect.stringContaining('border-top-color: var(--primary)'))
  })
})

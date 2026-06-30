import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { Spinner } from '../spinner'

describe('Spinner', () => {
  it('uses a thin stroke across sizes', () => {
    render(
      <>
        <Spinner aria-label="xs" size="xs" />
        <Spinner aria-label="sm" size="sm" />
        <Spinner aria-label="md" size="md" />
        <Spinner aria-label="lg" size="lg" />
      </>,
    )

    for (const label of ['xs', 'sm', 'md', 'lg']) {
      const spinner = screen.getByLabelText(label)
      expect(spinner).toHaveClass('border')
      expect(spinner).not.toHaveClass('border-2')
    }
  })
})

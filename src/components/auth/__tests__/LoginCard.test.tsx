import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { LoginCard } from '../LoginCard'

describe('LoginCard', () => {
  it('renders children inside', () => {
    render(<LoginCard><div>form-slot</div></LoginCard>)
    expect(screen.getByText('form-slot')).toBeInTheDocument()
  })

  it('uses width 460 r-18 border', () => {
    const { container } = render(<LoginCard><div /></LoginCard>)
    const card = container.querySelector('[data-testid="login-card"]')
    expect(card?.className).toMatch(/w-\[460px\]/)
    expect(card?.className).toMatch(/rounded-\[18px\]/)
    expect(card?.className).toMatch(/border/)
  })
})

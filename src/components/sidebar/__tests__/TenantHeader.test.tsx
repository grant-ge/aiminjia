import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { TenantHeader } from '../TenantHeader'

describe('TenantHeader', () => {
  it('renders the brand logo image and tenant name', () => {
    render(<TenantHeader name="仁励家网络科技(杭州)" logoUrl="/app-icon.png" />)
    const img = screen.getByRole('img', { name: /brand logo/i })
    expect(img).toHaveAttribute('src', '/app-icon.png')
    expect(screen.getByText('仁励家网络科技(杭州)')).toBeInTheDocument()
  })

  it('logo box has 28x28 sizing classes', () => {
    const { container } = render(
      <TenantHeader name="X" logoUrl="/app-icon.png" />,
    )
    const logoWrap = container.querySelector('[data-testid="tenant-logo"]')
    expect(logoWrap?.className).toMatch(/h-7/)
    expect(logoWrap?.className).toMatch(/w-7/)
  })
})

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

  it('renders a chevrons-up-down indicator on the right', () => {
    const { container } = render(
      <TenantHeader name="X" logoUrl="/app-icon.png" />,
    )
    expect(container.querySelector('[data-icon="chevrons-up-down"]')).toBeInTheDocument()
  })

  it('logo box has 24x24 sizing classes', () => {
    const { container } = render(
      <TenantHeader name="X" logoUrl="/app-icon.png" />,
    )
    const logoWrap = container.querySelector('[data-testid="tenant-logo"]')
    expect(logoWrap?.className).toMatch(/h-6/)
    expect(logoWrap?.className).toMatch(/w-6/)
  })
})

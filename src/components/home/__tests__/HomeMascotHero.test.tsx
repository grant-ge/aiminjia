import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { HomeMascotHero } from '../HomeMascotHero'

describe('HomeMascotHero', () => {
  it('renders title', () => {
    render(
      <HomeMascotHero
        mascotUrl="/app-icon.png"
        title="创建你的下一条任务"
      />,
    )
    expect(screen.getByText('创建你的下一条任务')).toBeInTheDocument()
  })

  it('mascot is 40x40 with the global md radius', () => {
    const { container } = render(
      <HomeMascotHero mascotUrl="/x.png" title="t" />,
    )
    const mascot = container.querySelector('[data-testid="home-mascot"]')
    expect(mascot?.className).toMatch(/h-10/)
    expect(mascot?.className).toMatch(/w-10/)
    expect(mascot?.className).toMatch(/rounded-md/)
  })

  it('renders mascot image with width-only sizing to avoid cropping', () => {
    const { container } = render(
      <HomeMascotHero mascotUrl="/x.png" title="t" />,
    )
    const img = container.querySelector('[data-testid="home-mascot"] img')
    expect(img?.className).toBe('w-full')
  })
})

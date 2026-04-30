import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { HomeMascotHero } from '../HomeMascotHero'

describe('HomeMascotHero', () => {
  it('renders title and subtitle', () => {
    render(
      <HomeMascotHero
        mascotUrl="/app-icon.png"
        title="创建你的下一条任务"
        subtitle="用清晰的任务描述和参数，让 AI 更快给出可执行结果。"
      />,
    )
    expect(screen.getByText('创建你的下一条任务')).toBeInTheDocument()
    expect(
      screen.getByText('用清晰的任务描述和参数，让 AI 更快给出可执行结果。'),
    ).toBeInTheDocument()
  })

  it('mascot is 64x64 without forced rounding', () => {
    const { container } = render(
      <HomeMascotHero mascotUrl="/x.png" title="t" subtitle="s" />,
    )
    const mascot = container.querySelector('[data-testid="home-mascot"]')
    expect(mascot?.className).toMatch(/h-16/)
    expect(mascot?.className).toMatch(/w-16/)
    expect(mascot?.className).not.toMatch(/rounded-full/)
  })

  it('renders mascot image with width-only sizing to avoid cropping', () => {
    const { container } = render(
      <HomeMascotHero mascotUrl="/x.png" title="t" subtitle="s" />,
    )
    const img = container.querySelector('[data-testid="home-mascot"] img')
    expect(img?.className).toBe('w-full')
  })
})

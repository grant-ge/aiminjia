import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillDetailHero } from '../SkillDetailHero'

describe('SkillDetailHero', () => {
  it('renders title, subtitle and slots', () => {
    render(
      <SkillDetailHero
        iconNode={<span>ic</span>}
        title="数据分析"
        subtitle="上传 Excel 或 CSV ..."
        actionBar={<span data-testid="ab">ab</span>}
      />,
    )
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByText(/上传 Excel/)).toBeInTheDocument()
    expect(screen.getByTestId('ab')).toBeInTheDocument()
  })

  it('heroIc box is 88×88 with brand-primary-subtle bg', () => {
    const { container } = render(
      <SkillDetailHero iconNode={null} title="t" subtitle="s" actionBar={null} />,
    )
    const box = container.querySelector('[data-testid="hero-ic"]')
    expect(box?.className).toMatch(/h-\[88px\]/)
    expect(box?.className).toMatch(/w-\[88px\]/)
    expect(box?.className).toMatch(/bg-brand-primary-subtle/)
  })
})

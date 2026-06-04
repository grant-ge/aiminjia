import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillDetailHero } from '../SkillDetailHero'

describe('SkillDetailHero', () => {
  it('renders title, subtitle and slots', () => {
    render(
      <SkillDetailHero
        title="数据分析"
        subtitle="上传 Excel 或 CSV ..."
        actionBar={<span data-testid="ab">ab</span>}
      />,
    )
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByText(/上传 Excel/)).toBeInTheDocument()
    expect(screen.getByTestId('ab')).toBeInTheDocument()
  })

  it('does not render the hero icon box', () => {
    const { container } = render(
      <SkillDetailHero title="t" subtitle="s" actionBar={null} />,
    )
    expect(container.querySelector('[data-testid="hero-ic"]')).not.toBeInTheDocument()
  })
})

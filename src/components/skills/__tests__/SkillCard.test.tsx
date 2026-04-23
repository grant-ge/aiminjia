import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillCard } from '../SkillCard'

describe('SkillCard', () => {
  it('renders title, desc and fires actions', () => {
    const onUse = vi.fn()
    const onOpen = vi.fn()
    render(
      <SkillCard
        title="数据分析"
        desc="上传 Excel 或 CSV，一键生成报告"
        iconNode={<span data-testid="ic">ic</span>}
        onUse={onUse}
        onOpen={onOpen}
      />,
    )
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByTestId('ic')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /使用/ }))
    expect(onUse).toHaveBeenCalled()
  })

  it('uses border-1 r-lg class on the card root', () => {
    const { container } = render(
      <SkillCard title="t" desc="d" iconNode={null} onUse={() => {}} onOpen={() => {}} />,
    )
    const card = container.querySelector('[data-testid="skill-card"]')
    expect(card?.className).toMatch(/border/)
    expect(card?.className).toMatch(/rounded-lg|rounded-md|rounded-\[8px\]/)
  })
})

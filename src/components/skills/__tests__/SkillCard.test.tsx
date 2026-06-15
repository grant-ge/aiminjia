import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillCard } from '../SkillCard'

describe('SkillCard', () => {
  it('renders title, meta, desc and fires onClick on card click', () => {
    const onClick = vi.fn()
    render(
      <SkillCard
        title="数据分析"
        meta="内置 · HR"
        desc="上传 Excel 或 CSV，一键生成报告"
        iconNode={<span data-testid="ic">ic</span>}
        onClick={onClick}
      />,
    )
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByText('内置 · HR')).toBeInTheDocument()
    expect(screen.getByText('上传 Excel 或 CSV，一键生成报告')).toBeInTheDocument()
    expect(screen.getByTestId('ic')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('skill-card'))
    expect(onClick).toHaveBeenCalled()
  })

  it('hot size applies min-h-32 class', () => {
    const { container } = render(
      <SkillCard title="t" meta="m" desc="d" iconNode={null} onClick={() => {}} size="hot" />,
    )
    const card = container.querySelector('[data-testid="skill-card"]')
    expect(card?.className).toMatch(/min-h-32/)
  })

  it('office size (default) applies min-h-28 class', () => {
    const { container } = render(
      <SkillCard title="t" meta="m" desc="d" iconNode={null} onClick={() => {}} />,
    )
    const card = container.querySelector('[data-testid="skill-card"]')
    expect(card?.className).toMatch(/min-h-28/)
  })

  it('has no 详情 or 使用 buttons', () => {
    render(
      <SkillCard title="t" meta="m" desc="d" iconNode={null} onClick={() => {}} />,
    )
    expect(screen.queryByRole('button', { name: /详情|使用/ })).toBeNull()
  })
})

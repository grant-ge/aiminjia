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

  it('places version next to meta instead of the main title on card layout', () => {
    const { container } = render(
      <SkillCard title="html-ppt" meta="通用" desc="d" iconNode={null} version="0.6" />,
    )

    const titleRow = screen.getByTestId('skill-card-title-row')
    const metaRow = screen.getByTestId('skill-card-meta-row')
    const version = screen.getByTestId('skill-card-version')

    expect(titleRow).not.toContainElement(version)
    expect(metaRow).toContainElement(version)
    expect(container.querySelector('[data-testid="skill-card-title-main"]')).toHaveClass('pr-0')
  })

  it('reserves right padding for card actions so title and version do not overlap', () => {
    render(
      <SkillCard
        title="薪酬市场数据查询助手"
        meta="HR"
        desc="d"
        iconNode={null}
        version="1.2"
        actionsSlot={<button type="button">+</button>}
      />,
    )

    expect(screen.getByTestId('skill-card-title-main')).toHaveClass('pr-12')
    expect(screen.getByTestId('skill-card-meta-row')).toHaveClass('pr-12')
  })

  it('uses the same chip style for version and source labels', () => {
    render(
      <SkillCard
        title="multi-search-report"
        meta="通用"
        desc="d"
        iconNode={null}
        version="0.1.0"
        sourceLabel="自建"
        layout="row"
      />,
    )

    expect(screen.getByTestId('skill-card-version').className).toBe(
      screen.getByTestId('skill-card-source').className,
    )
  })
})

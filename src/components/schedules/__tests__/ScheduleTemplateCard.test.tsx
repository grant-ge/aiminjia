import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ScheduleTemplateCard } from '../ScheduleTemplateCard'

describe('ScheduleTemplateCard', () => {
  it('renders title/desc and fires CTA', () => {
    const onCta = vi.fn()
    render(
      <ScheduleTemplateCard
        title="日报汇总"
        desc="每天 9 点把昨日数据汇总成日报。"
        cta={{ label: '使用模板', onClick: onCta }}
      />,
    )
    expect(screen.getByText('日报汇总')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '使用模板' }))
    expect(onCta).toHaveBeenCalled()
  })

  it('uses rounded-[14px] border class on card root', () => {
    const { container } = render(
      <ScheduleTemplateCard title="t" desc="d" cta={{ label: 'x', onClick: () => {} }} />,
    )
    const card = container.querySelector('[data-testid="schedule-template-card"]')
    expect(card?.className).toMatch(/rounded-\[14px\]/)
    expect(card?.className).toMatch(/border/)
  })
})

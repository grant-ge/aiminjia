import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ScheduleTemplateCard, type ScheduleTemplate } from '../ScheduleTemplateCard'

const SAMPLE: ScheduleTemplate = {
  title: '日报汇总',
  desc: '每天 9 点把昨日数据汇总成日报。',
  prompt: '每天 9 点把昨日数据汇总成日报。',
  rule: null,
}

describe('ScheduleTemplateCard', () => {
  it('renders title/desc and fires onPick with template', () => {
    const onPick = vi.fn()
    render(<ScheduleTemplateCard template={SAMPLE} onPick={onPick} />)
    expect(screen.getByText('日报汇总')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '用此模板' }))
    expect(onPick).toHaveBeenCalledWith(SAMPLE)
  })

  it('uses rounded-lg border class on card root', () => {
    const { container } = render(
      <ScheduleTemplateCard
        template={{ title: 't', desc: 'd', prompt: 'p', rule: null }}
        onPick={() => {}}
      />,
    )
    const card = container.querySelector('[data-testid="schedule-template-card"]')
    expect(card?.className).toMatch(/rounded-lg/)
    expect(card?.className).toMatch(/border/)
  })
})

import '@testing-library/jest-dom'
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { DispatchBanner } from './DispatchBanner'
import type { DispatchHeader } from './parseDispatchHeader'

function header(overrides: Partial<DispatchHeader> = {}): DispatchHeader {
  return {
    employee: '小工',
    role: '技术支持',
    trigger: 'on-demand',
    triggerTime: null,
    skillId: null,
    monitoringTargets: [],
    configLines: ['群关键词：技术、对接、集成', '响应风格：专业'],
    ...overrides,
  }
}

describe('DispatchBanner', () => {
  it('renders employee name, role, and on-demand trigger label', () => {
    render(<DispatchBanner header={header()} />)
    expect(screen.getByTestId('dispatch-banner')).toBeInTheDocument()
    expect(screen.getByText(/派活给 小工/)).toBeInTheDocument()
    expect(screen.getByText('技术支持')).toBeInTheDocument()
    expect(screen.getByText('按需派活')).toBeInTheDocument()
  })

  it('renders cron trigger with time when present', () => {
    render(
      <DispatchBanner
        header={header({ trigger: 'cron', triggerTime: '2026-05-15 09:00 UTC' })}
      />,
    )
    expect(screen.getByText(/定时 2026-05-15 09:00 UTC/)).toBeInTheDocument()
  })

  it('renders configLines split as label / value pairs', () => {
    render(<DispatchBanner header={header()} />)
    // labels stripped of "：" suffix render as their own span
    expect(screen.getByText('群关键词')).toBeInTheDocument()
    expect(screen.getByText('技术、对接、集成')).toBeInTheDocument()
    expect(screen.getByText('响应风格')).toBeInTheDocument()
    expect(screen.getByText('专业')).toBeInTheDocument()
  })

  it('omits config area when header has no facts to show', () => {
    render(<DispatchBanner header={header({ configLines: [] })} />)
    expect(screen.queryByText('群关键词')).toBeNull()
    // banner itself still renders (just title + dividers)
    expect(screen.getByTestId('dispatch-banner')).toBeInTheDocument()
  })

  it('renders skill chip when skillId is present', () => {
    render(
      <DispatchBanner
        header={header({ configLines: [], skillId: 'competitive-intelligence' })}
      />,
    )
    expect(screen.getByText('默认技能')).toBeInTheDocument()
    // displayName resolution is via useSkillStore; without a loaded skill
    // the chip falls back to the raw id.
    expect(screen.getByTestId('dispatch-skill-chip')).toBeInTheDocument()
  })

  it('renders monitoring target chips with URL on title hover', () => {
    render(
      <DispatchBanner
        header={header({
          configLines: [],
          monitoringTargets: [
            { name: '悟空', url: 'https://wukong.dingtalk.com/' },
            { name: 'workbuddy', url: null },
          ],
        })}
      />,
    )
    expect(screen.getByText('监听目标')).toBeInTheDocument()
    const chips = screen.getAllByTestId('dispatch-monitoring-chip')
    expect(chips).toHaveLength(2)
    expect(chips[0]).toHaveTextContent('悟空')
    expect(chips[0].getAttribute('title')).toBe('https://wukong.dingtalk.com/')
    expect(chips[1]).toHaveTextContent('workbuddy')
    expect(chips[1].getAttribute('title')).toBe('workbuddy')
  })
})

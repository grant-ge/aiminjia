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

  it('renders config lines verbatim', () => {
    render(<DispatchBanner header={header()} />)
    expect(screen.getByText('群关键词：技术、对接、集成')).toBeInTheDocument()
    expect(screen.getByText('响应风格：专业')).toBeInTheDocument()
  })

  it('omits config block when configLines is empty', () => {
    const { container } = render(<DispatchBanner header={header({ configLines: [] })} />)
    // Only the header line should render — no extra text rows
    expect(container.querySelectorAll('.leading-relaxed').length).toBe(0)
  })
})

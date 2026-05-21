import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillPopoverPanel } from '../SkillPopoverPanel'

const ITEMS = [
  { id: 'a', title: '数据分析', subtitle: '上传 Excel / CSV 生成报告', source: '内置' },
  { id: 'b', title: '文案助手', subtitle: '起草邮件 / 日报', source: '已安装' },
]

describe('SkillPopoverPanel', () => {
  it('renders search input and all items', () => {
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
    expect(screen.getByTestId('skill-popover-search')).toBeInTheDocument()
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByText('文案助手')).toBeInTheDocument()
  })

  it('fires onPick with id when an item clicked', () => {
    const onPick = vi.fn()
    render(<SkillPopoverPanel items={ITEMS} onPick={onPick} onClose={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /文案助手/ }))
    expect(onPick).toHaveBeenCalledWith('b')
  })

  it('closes when clicking outside the popover', () => {
    const onClose = vi.fn()
    render(
      <>
        <button type="button">外部区域</button>
        <SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={onClose} />
      </>,
    )

    fireEvent.pointerDown(screen.getByRole('button', { name: '外部区域' }))

    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('does not close when clicking inside the popover', () => {
    const onClose = vi.fn()
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={onClose} />)

    fireEvent.pointerDown(screen.getByTestId('skill-popover-search'))

    expect(onClose).not.toHaveBeenCalled()
  })

  it('filters items by query', () => {
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
    const input = screen.getByTestId('skill-popover-search') as HTMLInputElement
    fireEvent.change(input, { target: { value: '文案' } })
    expect(screen.queryByText('数据分析')).not.toBeInTheDocument()
    expect(screen.getByText('文案助手')).toBeInTheDocument()
  })

  it('ranks title matches above subtitle matches', () => {
    const items = [
      { id: 'a', title: '数据分析', subtitle: '报告生成', source: '内置' },
      { id: 'b', title: '报告助手', subtitle: '其它', source: '内置' },
    ]
    render(<SkillPopoverPanel items={items} onPick={() => {}} onClose={() => {}} />)
    fireEvent.change(screen.getByTestId('skill-popover-search'), { target: { value: '报告' } })
    const rendered = screen.getAllByRole('button').filter((b) => b.textContent?.includes('助手') || b.textContent?.includes('分析'))
    expect(rendered[0].textContent).toContain('报告助手')
    expect(rendered[1].textContent).toContain('数据分析')
  })

  it('ranks title prefix above title contains', () => {
    const items = [
      { id: 'a', title: '高级报告', subtitle: '...', source: '内置' },
      { id: 'b', title: '报告助手', subtitle: '...', source: '内置' },
    ]
    render(<SkillPopoverPanel items={items} onPick={() => {}} onClose={() => {}} />)
    fireEvent.change(screen.getByTestId('skill-popover-search'), { target: { value: '报告' } })
    const buttons = screen.getAllByRole('button').filter((b) => b.textContent?.includes('报告'))
    expect(buttons[0].textContent).toContain('报告助手')
    expect(buttons[1].textContent).toContain('高级报告')
  })

  it('shows empty state with no-match hint when query has no matches', () => {
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
    fireEvent.change(screen.getByTestId('skill-popover-search'), { target: { value: 'zzz-nope' } })
    expect(screen.getByTestId('skill-popover-empty')).toBeInTheDocument()
    expect(screen.getByText('没有匹配的技能')).toBeInTheDocument()
  })

  it('becomes scrollable when items > 6', () => {
    const many = Array.from({ length: 10 }, (_, i) => ({
      id: String(i), title: `技能 ${i}`, subtitle: '...', source: '内置',
    }))
    const { container } = render(
      <SkillPopoverPanel items={many} onPick={() => {}} onClose={() => {}} />,
    )
    const list = container.querySelector('[data-testid="skill-popover-list"]')
    expect(list?.className).toMatch(/overflow-auto/)
  })
})

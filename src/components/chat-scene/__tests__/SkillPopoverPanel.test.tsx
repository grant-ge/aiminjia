import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillPopoverPanel } from '../SkillPopoverPanel'

const ITEMS = [
  { id: 'a', title: '数据分析', subtitle: '上传 Excel / CSV 生成报告', source: '内置' },
  { id: 'b', title: '文案助手', subtitle: '起草邮件 / 日报', source: '已安装' },
]

describe('SkillPopoverPanel', () => {
  it('renders head title and all items', () => {
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
    expect(screen.getByText('管理已安装的技能')).toBeInTheDocument()
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByText('文案助手')).toBeInTheDocument()
  })

  it('fires onPick with id when an item clicked', () => {
    const onPick = vi.fn()
    render(<SkillPopoverPanel items={ITEMS} onPick={onPick} onClose={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /文案助手/ }))
    expect(onPick).toHaveBeenCalledWith('b')
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

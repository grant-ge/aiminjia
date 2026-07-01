import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillPopoverPanel } from '../SkillPopoverPanel'
import { useUiStore } from '@/stores/uiStore'

const ITEMS = [
  { id: 'a', title: '数据分析', subtitle: '上传 Excel / CSV 生成报告', icon: 'bar-chart-2', category: 'finance' },
  { id: 'b', title: '文案助手', subtitle: '起草邮件 / 日报', category: 'general' },
]

describe('SkillPopoverPanel', () => {
  beforeEach(() => {
    useUiStore.setState({ route: { kind: 'home' } })
  })

  it('renders search input and all items', () => {
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
    expect(screen.getByTestId('skill-popover-search')).toBeInTheDocument()
    expect(screen.getByText('数据分析')).toBeInTheDocument()
    expect(screen.getByText('文案助手')).toBeInTheDocument()
  })

  it('renders subtitle below the title for each row', () => {
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
    expect(screen.getByText('上传 Excel / CSV 生成报告')).toBeInTheDocument()
    expect(screen.getByText('起草邮件 / 日报')).toBeInTheDocument()
  })

  it('uses the same known skill avatar image as the skill center', () => {
    render(
      <SkillPopoverPanel
        items={[{ id: 'dingtalk-workspace', title: '玩转钉钉', subtitle: '钉钉工作台助手' }]}
        onPick={() => {}}
        onClose={() => {}}
      />,
    )

    const option = screen.getByRole('option', { name: /玩转钉钉/ })
    expect(option.querySelector('img')).toHaveAttribute('src', '/logos/dingtalk.png')
  })

  it('falls back to the title initial when the skill center has no avatar image', () => {
    render(
      <SkillPopoverPanel
        items={[{ id: 'custom-report', title: '报告助手', subtitle: '生成报告' }]}
        onPick={() => {}}
        onClose={() => {}}
      />,
    )

    const option = screen.getByRole('option', { name: /报告助手/ })
    expect(option.querySelector('img')).toBeNull()
    expect(screen.getByTestId('skill-popover-fallback-avatar')).toHaveTextContent('报')
  })

  it('fires onPick with id when an item clicked', () => {
    const onPick = vi.fn()
    render(<SkillPopoverPanel items={ITEMS} onPick={onPick} onClose={() => {}} />)
    fireEvent.click(screen.getByRole('option', { name: /文案助手/ }))
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

  it('filters slash commands by leading command text', () => {
    render(
      <SkillPopoverPanel
        items={[
          ...ITEMS,
          {
            id: 'toggle-restore-skill',
            title: 'Toggle Restore',
            subtitle: 'restore skill',
            command: '/toggle-restore-skill',
          },
        ]}
        onPick={() => {}}
        onClose={() => {}}
      />,
    )
    fireEvent.change(screen.getByTestId('skill-popover-search'), { target: { value: '/toggle' } })
    expect(screen.getByRole('option', { name: /Toggle Restore/ })).toHaveAttribute(
      'data-aijia-skill-command',
      '/toggle-restore-skill',
    )
    expect(screen.queryByText('鏁版嵁鍒嗘瀽')).not.toBeInTheDocument()
  })

  it('ranks title matches above subtitle matches', () => {
    const items = [
      { id: 'a', title: '数据分析', subtitle: '报告生成' },
      { id: 'b', title: '报告助手', subtitle: '其它' },
    ]
    render(<SkillPopoverPanel items={items} onPick={() => {}} onClose={() => {}} />)
    fireEvent.change(screen.getByTestId('skill-popover-search'), { target: { value: '报告' } })
    const rendered = screen.getAllByRole('option')
    expect(rendered[0].textContent).toContain('报告助手')
    expect(rendered[1].textContent).toContain('数据分析')
  })

  it('ranks title prefix above title contains', () => {
    const items = [
      { id: 'a', title: '高级报告', subtitle: '...' },
      { id: 'b', title: '报告助手', subtitle: '...' },
    ]
    render(<SkillPopoverPanel items={items} onPick={() => {}} onClose={() => {}} />)
    fireEvent.change(screen.getByTestId('skill-popover-search'), { target: { value: '报告' } })
    const options = screen.getAllByRole('option')
    expect(options[0].textContent).toContain('报告助手')
    expect(options[1].textContent).toContain('高级报告')
  })

  it('shows empty state with no-match hint when query has no matches', () => {
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
    fireEvent.change(screen.getByTestId('skill-popover-search'), { target: { value: 'zzz-nope' } })
    expect(screen.getByTestId('skill-popover-empty')).toBeInTheDocument()
    expect(screen.getByText('没有匹配的技能')).toBeInTheDocument()
  })

  it('renders all items and scrolls when the list overflows', () => {
    const many = Array.from({ length: 10 }, (_, i) => ({
      id: String(i),
      title: `技能 ${i}`,
      subtitle: '...',
    }))
    render(<SkillPopoverPanel items={many} onPick={() => {}} onClose={() => {}} />)
    expect(screen.getAllByRole('option')).toHaveLength(10)
  })

  describe('keyboard navigation', () => {
    it('first item is highlighted by default', () => {
      render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
      const options = screen.getAllByRole('option')
      expect(options[0]).toHaveAttribute('aria-selected', 'true')
      expect(options[1]).toHaveAttribute('aria-selected', 'false')
    })

    it('ArrowDown moves highlight to next item and wraps around', () => {
      render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
      const list = screen.getByRole('listbox')
      fireEvent.keyDown(list, { key: 'ArrowDown' })
      const options = screen.getAllByRole('option')
      expect(options[1]).toHaveAttribute('aria-selected', 'true')
      fireEvent.keyDown(list, { key: 'ArrowDown' })
      expect(screen.getAllByRole('option')[0]).toHaveAttribute('aria-selected', 'true')
    })

    it('ArrowUp from index 0 wraps to last item', () => {
      render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
      const list = screen.getByRole('listbox')
      fireEvent.keyDown(list, { key: 'ArrowUp' })
      const options = screen.getAllByRole('option')
      expect(options[1]).toHaveAttribute('aria-selected', 'true')
    })

    it('Enter picks the highlighted item', () => {
      const onPick = vi.fn()
      render(<SkillPopoverPanel items={ITEMS} onPick={onPick} onClose={() => {}} />)
      const list = screen.getByRole('listbox')
      fireEvent.keyDown(list, { key: 'ArrowDown' })
      fireEvent.keyDown(list, { key: 'Enter' })
      expect(onPick).toHaveBeenCalledWith('b')
    })

    it('Escape closes the popover', () => {
      const onClose = vi.fn()
      render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={onClose} />)
      fireEvent.keyDown(screen.getByRole('listbox'), { key: 'Escape' })
      expect(onClose).toHaveBeenCalledTimes(1)
    })
  })

  describe('explore footer', () => {
    it('renders the "explore & manage skills" entry', () => {
      render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)
      expect(screen.getByTestId('skill-popover-explore')).toBeInTheDocument()
    })

    it('clicking explore navigates to skill-center and closes', () => {
      const onClose = vi.fn()
      render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={onClose} />)
      fireEvent.click(screen.getByTestId('skill-popover-explore'))
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-center' })
      expect(onClose).toHaveBeenCalledTimes(1)
    })
  })
})

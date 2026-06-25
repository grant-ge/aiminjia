import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { Check } from 'lucide-react'
import { describe, expect, it, vi } from 'vitest'

import { SegmentedControl } from './SegmentedControl'

describe('SegmentedControl', () => {
  it('renders accessible radio options and calls onValueChange', () => {
    const onValueChange = vi.fn()

    render(
      <SegmentedControl
        ariaLabel="字号"
        testId="font-scale"
        value="medium"
        onValueChange={onValueChange}
        options={[
          { value: 'small', label: '小' },
          { value: 'medium', label: '中' },
          { value: 'large', label: '大' },
        ]}
      />,
    )

    const selected = screen.getByRole('radio', { name: '中' })
    expect(screen.getByRole('radiogroup', { name: '字号' })).toHaveClass('h-8')
    expect(selected).toHaveAttribute('aria-checked', 'true')
    expect(selected).toHaveClass('text-foreground', 'rounded')
    expect(selected).not.toHaveClass('rounded-md')
    expect(screen.getByTestId('font-scale-indicator')).toHaveClass(
      'bg-card',
      'transition-transform',
    )
    expect(screen.getByTestId('font-scale-indicator')).toHaveStyle({
      transform: 'translateX(100%)',
    })

    fireEvent.click(screen.getByRole('radio', { name: '大' }))
    expect(onValueChange).toHaveBeenCalledWith('large')
  })

  it('matches Button height tiers for sm, md, and lg', () => {
    const options = [
      { value: 'off', label: '关' },
      { value: 'on', label: '开' },
    ]

    render(
      <>
        <SegmentedControl ariaLabel="小控件" size="sm" value="off" onValueChange={() => {}} options={options} />
        <SegmentedControl ariaLabel="中控件" size="md" value="off" onValueChange={() => {}} options={options} />
        <SegmentedControl ariaLabel="大控件" size="lg" value="off" onValueChange={() => {}} options={options} />
      </>,
    )

    expect(screen.getByRole('radiogroup', { name: '小控件' })).toHaveClass('h-6')
    expect(screen.getByRole('radiogroup', { name: '中控件' })).toHaveClass('h-8')
    expect(screen.getByRole('radiogroup', { name: '大控件' })).toHaveClass('h-10')
  })

  it('supports disabled binary toggles without firing changes', () => {
    const onValueChange = vi.fn()

    render(
      <SegmentedControl
        ariaLabel="开机自启动"
        disabled
        value="off"
        onValueChange={onValueChange}
        options={[
          { value: 'off', label: '关' },
          { value: 'on', label: '开' },
        ]}
      />,
    )

    const group = screen.getByRole('radiogroup', { name: '开机自启动' })
    expect(group).toHaveAttribute('aria-disabled', 'true')
    expect(screen.getByRole('radio', { name: '关' })).toBeDisabled()
    expect(screen.getByRole('radio', { name: '开' })).toBeDisabled()

    fireEvent.click(screen.getByRole('radio', { name: '开' }))
    expect(onValueChange).not.toHaveBeenCalled()
  })

  it('supports icon-only options with explicit accessible labels', () => {
    render(
      <SegmentedControl
        ariaLabel="侧边栏分类"
        value="todo"
        onValueChange={() => {}}
        options={[
          { value: 'todo', label: '', ariaLabel: '新任务', icon: <Check data-testid="todo-icon" className="h-3.5 w-3.5" /> },
          { value: 'chat', label: '', ariaLabel: 'IM 频道', icon: <Check /> },
        ]}
      />,
    )

    expect(screen.getByRole('radio', { name: '新任务' })).toBeInTheDocument()
    expect(screen.getByTestId('todo-icon')).toHaveClass('h-3.5', 'w-3.5')
  })
})

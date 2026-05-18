import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { ToolGroupCard } from '../ToolGroupCard'

const STEPS = [
  {
    index: 1,
    name: 'Read',
    status: 'done' as const,
    durationMs: 120,
    inputJson: '{ "path": "messages.jsonl" }',
    output: 'first file content',
  },
  {
    index: 2,
    name: 'Read',
    status: 'done' as const,
    durationMs: 80,
    inputJson: '{ "path": "config.json" }',
    output: 'second file content',
  },
  { index: 3, name: 'preview_file', status: 'error' as const, durationMs: 50, output: 'preview failed' },
]

describe('ToolGroupCard — compact chip', () => {
  it('starts collapsed: trigger chip only, no step rows visible', () => {
    render(<ToolGroupCard status="done" steps={STEPS} />)
    expect(screen.getByRole('button', { name: /已完成/ })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Read/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /preview_file/ })).not.toBeInTheDocument()
  })

  it('toggling the chip reveals the step list and hides it again', () => {
    render(<ToolGroupCard status="done" steps={STEPS} />)
    const chip = screen.getByRole('button', { name: /已完成/ })
    expect(chip).toHaveAttribute('aria-expanded', 'false')

    fireEvent.click(chip)
    expect(chip).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getAllByRole('button', { name: /Read/ })).toHaveLength(2)
    expect(screen.getByRole('button', { name: /preview_file/ })).toBeInTheDocument()

    fireEvent.click(chip)
    expect(chip).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByRole('button', { name: /Read/ })).not.toBeInTheDocument()
  })

  it('renders compact "已完成 X分Y秒" when durationMs ≥ 1min', () => {
    render(<ToolGroupCard status="done" steps={STEPS} durationMs={111_000} />)
    expect(screen.getByText('已完成 1分51秒')).toBeInTheDocument()
  })

  it('renders compact "已完成 N秒" when durationMs < 1min', () => {
    render(<ToolGroupCard status="done" steps={STEPS} durationMs={4_000} />)
    expect(screen.getByText('已完成 4秒')).toBeInTheDocument()
  })

  it('falls back to step-count label when durationMs is missing or 0', () => {
    render(<ToolGroupCard status="done" steps={STEPS} />)
    expect(screen.getByText('已完成 3 步')).toBeInTheDocument()
  })

  it('shows running progress in the chip (no duration label while running)', () => {
    const runningSteps = [
      { ...STEPS[0], status: 'done' as const },
      { ...STEPS[1], status: 'running' as const },
      { ...STEPS[2], status: 'running' as const },
    ]
    render(<ToolGroupCard status="running" steps={runningSteps} durationMs={5_000} />)
    expect(screen.getByText('执行中 1 / 3')).toBeInTheDocument()
    expect(screen.queryByText(/已完成/)).not.toBeInTheDocument()
  })

  it('allows multiple tool items to be expanded at the same time once chip is open', () => {
    render(<ToolGroupCard status="done" steps={STEPS} />)
    fireEvent.click(screen.getByRole('button', { name: /已完成/ }))

    const readButtons = screen.getAllByRole('button', { name: /Read/ })
    fireEvent.click(readButtons[0])
    fireEvent.click(readButtons[1])

    expect(screen.getByText('first file content')).toBeInTheDocument()
    expect(screen.getByText('second file content')).toBeInTheDocument()
    expect(screen.getAllByText('输入')).toHaveLength(2)
  })

  it('presents failed tool steps as completed with neutral output styling', () => {
    render(<ToolGroupCard status="done" steps={STEPS} />)

    fireEvent.click(screen.getByRole('button', { name: /已完成/ }))
    fireEvent.click(screen.getByRole('button', { name: /preview_file/ }))

    expect(screen.queryByText('失败')).not.toBeInTheDocument()
    expect(screen.getAllByText('完成')).toHaveLength(3)
    expect(screen.getByText('preview failed')).toHaveStyle({
      color: 'var(--color-text-secondary)',
    })
  })
})

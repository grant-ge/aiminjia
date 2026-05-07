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

describe('ToolGroupCard', () => {
  it('renders collapsed by default and expands tool steps from the header', () => {
    render(<ToolGroupCard status="done" steps={STEPS} />)

    expect(screen.getByText('工具执行轨迹')).toBeInTheDocument()
    expect(screen.getByText('已完成 2 步')).toBeInTheDocument()
    expect(screen.queryByText('工具步骤')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /查看执行详情/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Read/ })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /工具执行轨迹/ }))

    expect(screen.getAllByRole('button', { name: /Read/ })).toHaveLength(2)
    expect(screen.getByRole('button', { name: /preview_file/ })).toBeInTheDocument()
    expect(screen.queryByText('输入')).not.toBeInTheDocument()
    expect(document.querySelector('.lucide-circle-check')).not.toBeInTheDocument()
    expect(document.querySelector('.lucide-x-circle')).not.toBeInTheDocument()
  })

  it('allows multiple tool items to be expanded at the same time', () => {
    render(<ToolGroupCard status="done" steps={STEPS} />)
    fireEvent.click(screen.getByRole('button', { name: /工具执行轨迹/ }))

    const readButtons = screen.getAllByRole('button', { name: /Read/ })
    fireEvent.click(readButtons[0])
    fireEvent.click(readButtons[1])

    expect(screen.getByText('first file content')).toBeInTheDocument()
    expect(screen.getByText('second file content')).toBeInTheDocument()
    expect(screen.getAllByText('输入')).toHaveLength(2)
  })

  it('shows running progress in the badge', () => {
    const runningSteps = [
      { ...STEPS[0], status: 'done' as const },
      { ...STEPS[1], status: 'running' as const },
      { ...STEPS[2], status: 'running' as const },
    ]

    render(<ToolGroupCard status="running" steps={runningSteps} />)

    expect(screen.getByText('执行中 1 / 3')).toBeInTheDocument()
  })

  it('collapses and expands the whole tool trace block from the header', () => {
    render(<ToolGroupCard status="done" steps={STEPS} />)

    expect(screen.queryByRole('button', { name: /Read/ })).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /工具执行轨迹/ }))

    expect(screen.getAllByRole('button', { name: /Read/ })).toHaveLength(2)
    expect(screen.getByText('已完成 2 步')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /工具执行轨迹/ }))

    expect(screen.queryByRole('button', { name: /Read/ })).not.toBeInTheDocument()
  })

})

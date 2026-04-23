import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ToolGroupCard } from '../ToolGroupCard'

const STEPS = [
  { index: 1, name: 'fetch_feedback', status: 'done' as const, durationMs: 1300 },
  { index: 2, name: 'cluster_topics', status: 'done' as const, durationMs: 2100, inputJson: '{ "a": 1 }' },
  { index: 3, name: 'draft_followups', status: 'done' as const, durationMs: 1400 },
]

describe('ToolGroupCard', () => {
  it('header shows done status and aggregate duration', () => {
    render(
      <ToolGroupCard
        status="done"
        steps={STEPS}
        durationMs={4800}
        expanded
        expandedStepIndex={null}
        onToggle={() => {}}
        onToggleStep={() => {}}
      />,
    )
    expect(screen.getByText(/已完成 3 步/)).toBeInTheDocument()
    expect(screen.getByText(/4\.8s/)).toBeInTheDocument()
  })

  it('header shows running status', () => {
    const runningSteps = STEPS.map((s, i) =>
      i < 2 ? s : { ...s, status: 'running' as const },
    )
    render(
      <ToolGroupCard
        status="running"
        steps={runningSteps}
        durationMs={2400}
        expanded
        expandedStepIndex={null}
        onToggle={() => {}}
        onToggleStep={() => {}}
      />,
    )
    expect(screen.getByText(/正在执行/)).toBeInTheDocument()
    expect(screen.getByText(/2 \/ 3/)).toBeInTheDocument()
  })

  it('clicking top bar toggles expanded', () => {
    const onToggle = vi.fn()
    render(
      <ToolGroupCard
        status="done"
        steps={STEPS}
        durationMs={1000}
        expanded
        expandedStepIndex={null}
        onToggle={onToggle}
        onToggleStep={() => {}}
      />,
    )
    fireEvent.click(screen.getByTestId('tool-group-top-bar'))
    expect(onToggle).toHaveBeenCalled()
  })
})

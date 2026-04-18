import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { TurnSummaryBadge } from './TurnSummaryBadge'
import type { TurnSummary } from '@/stores/streamingStore'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

function buildSummary(overrides: Partial<TurnSummary> = {}): TurnSummary {
  return {
    outcome: 'Success',
    totalInputTokens: 900,
    totalOutputTokens: 100,
    totalCostUsd: 0.0123,
    completedAt: 1_713_000_000,
    ...overrides,
  }
}

describe('TurnSummaryBadge', () => {
  it('renders a compact success badge when cost is available', () => {
    render(<TurnSummaryBadge summary={buildSummary()} />)

    expect(screen.getByLabelText('turn summary')).toHaveTextContent('~$0.0123')
    expect(screen.getByLabelText('turn summary')).toHaveTextContent('1000 tokens')
  })

  it('renders nothing for successful turns without cost data', () => {
    const { container } = render(
      <TurnSummaryBadge summary={buildSummary({ totalCostUsd: null })} />,
    )

    expect(container.firstChild).toBeNull()
  })

  it('renders a warning badge for budget exceeded outcomes', () => {
    render(
      <TurnSummaryBadge
        summary={buildSummary({
          outcome: 'BudgetExceeded',
          totalCostUsd: 0.42,
        })}
      />,
    )

    expect(screen.getByLabelText('turn summary')).toHaveTextContent(
      'turnOutcome.budgetExceededTitle',
    )
  })

  it('renders nothing for cancelled turns', () => {
    const { container } = render(
      <TurnSummaryBadge
        summary={buildSummary({
          outcome: 'Cancelled',
          totalCostUsd: 0,
        })}
      />,
    )

    expect(container.firstChild).toBeNull()
  })
})

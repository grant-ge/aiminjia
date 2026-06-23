import { useTranslation } from 'react-i18next'

import type { TurnSummary } from '@/stores/streamingStore'

interface TurnSummaryBadgeProps {
  summary?: TurnSummary
}

export function TurnSummaryBadge({ summary }: TurnSummaryBadgeProps) {
  const { t } = useTranslation()

  if (!summary) {
    return null
  }

  const totalTokens = (summary.totalInputTokens ?? 0) + (summary.totalOutputTokens ?? 0)
  const showSuccessSummary =
    summary.outcome === 'Success' && summary.totalCostUsd != null && summary.totalCostUsd > 0

  if (summary.outcome === 'Success' && !showSuccessSummary) {
    return null
  }

  let label = ''
  let style: { backgroundColor: string; color: string; borderColor: string }

  switch (summary.outcome) {
    case 'Cancelled':
      label = t('turnOutcome.cancelledTitle')
      style = {
        backgroundColor: 'rgba(148, 163, 184, 0.18)',
        color: 'var(--color-text-secondary)',
        borderColor: 'rgba(148, 163, 184, 0.32)',
      }
      break
    case 'MaxIterationsReached':
      label = t('turnOutcome.maxIterationsTitle')
      style = {
        backgroundColor: 'rgba(245, 158, 11, 0.14)',
        color: '#b45309',
        borderColor: 'rgba(245, 158, 11, 0.28)',
      }
      break
    case 'BudgetExceeded':
      label = t('turnOutcome.budgetExceededTitle')
      style = {
        backgroundColor: 'rgba(245, 158, 11, 0.14)',
        color: '#b45309',
        borderColor: 'rgba(245, 158, 11, 0.28)',
      }
      break
    case 'ExecutionError':
      label = t('turnOutcome.executionErrorTitle')
      style = {
        backgroundColor: 'rgba(239, 68, 68, 0.12)',
        color: '#b91c1c',
        borderColor: 'rgba(239, 68, 68, 0.24)',
      }
      break
    case 'Success':
    default:
      label = `~$${(summary.totalCostUsd ?? 0).toFixed(4)}`
      style = {
        backgroundColor: 'rgba(148, 163, 184, 0.12)',
        color: 'var(--color-text-secondary)',
        borderColor: 'rgba(148, 163, 184, 0.22)',
      }
      break
  }

  return (
    <div className="mt-3 pl-9">
      <div
        aria-label="turn summary"
        className="inline-flex items-center gap-2 rounded-md border px-3 py-1 text-xs border-border"
        style={style}
      >
        <span>{label}</span>
        {showSuccessSummary && totalTokens > 0 ? <span>{totalTokens} tokens</span> : null}
      </div>
    </div>
  )
}

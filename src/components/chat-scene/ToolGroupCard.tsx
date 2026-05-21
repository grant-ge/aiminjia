import { useTranslation } from 'react-i18next'

import { ExecutionTraceCard } from '@/components/rich-content/ExecutionTraceCard'

import { ToolTraceDetails } from './ToolTraceDetails'
import type { ToolStep } from './ToolGroupStepRow'

interface ToolGroupCardProps {
  status: 'running' | 'done'
  steps: ToolStep[]
  /** Total tool execution duration in milliseconds. Optional. */
  durationMs?: number
}

export function ToolGroupCard({ status, steps, durationMs }: ToolGroupCardProps) {
  const { t } = useTranslation()
  const done = steps.filter((s) => s.status !== 'running').length

  let badge: string
  if (status === 'running') {
    badge = t('toolGroup.running', { done, total: steps.length })
  } else if (typeof durationMs === 'number' && durationMs > 0) {
    badge = t('toolGroup.doneCompact', { duration: formatDuration(durationMs, t) })
  } else {
    badge = t('toolGroup.done', { done })
  }

  return (
    <ExecutionTraceCard
      title={t('toolGroup.title')}
      badge={badge}
      headerCollapsible
      defaultHeaderExpanded={false}
    >
      <ToolTraceDetails steps={steps} />
    </ExecutionTraceCard>
  )
}

function formatDuration(ms: number, t: ReturnType<typeof useTranslation>['t']): string {
  const totalSec = Math.round(ms / 1000)
  if (totalSec < 60) return t('toolGroup.durationSec', { sec: totalSec })
  const min = Math.floor(totalSec / 60)
  const sec = totalSec % 60
  return t('toolGroup.durationMinSec', { min, sec })
}

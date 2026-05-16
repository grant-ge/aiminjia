import { useTranslation } from 'react-i18next'
import { ExecutionTraceCard } from '@/components/rich-content/ExecutionTraceCard'

import { ToolTraceDetails } from './ToolTraceDetails'
import type { ToolStep } from './ToolGroupStepRow'

interface ToolGroupCardProps {
  status: 'running' | 'done'
  steps: ToolStep[]
}

export function ToolGroupCard({ status, steps }: ToolGroupCardProps) {
  const { t } = useTranslation()
  const done = steps.filter((s) => s.status !== 'running').length
  const badge = status === 'running'
    ? t('toolGroup.running', { done, total: steps.length })
    : t('toolGroup.done', { done })
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

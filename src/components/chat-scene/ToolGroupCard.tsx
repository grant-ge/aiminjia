import { ExecutionTraceCard } from '@/components/rich-content/ExecutionTraceCard'

import { ToolTraceDetails } from './ToolTraceDetails'
import type { ToolStep } from './ToolGroupStepRow'

interface ToolGroupCardProps {
  status: 'running' | 'done'
  steps: ToolStep[]
}

function buildBadge(status: ToolGroupCardProps['status'], steps: ToolStep[]): string {
  const done = steps.filter((s) => s.status === 'done').length
  return status === 'running' ? `执行中 ${done} / ${steps.length}` : `已完成 ${done} 步`
}

export function ToolGroupCard({ status, steps }: ToolGroupCardProps) {
  return (
    <ExecutionTraceCard
      title="工具执行轨迹"
      badge={buildBadge(status, steps)}
      headerCollapsible
      defaultHeaderExpanded={false}
    >
      <ToolTraceDetails steps={steps} />
    </ExecutionTraceCard>
  )
}

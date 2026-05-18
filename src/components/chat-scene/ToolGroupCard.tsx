import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown } from 'lucide-react'

import { ToolTraceDetails } from './ToolTraceDetails'
import type { ToolStep } from './ToolGroupStepRow'

interface ToolGroupCardProps {
  status: 'running' | 'done'
  steps: ToolStep[]
  /** Total tool execution duration in milliseconds. Optional. */
  durationMs?: number
}

/**
 * Compact tool-trace block:
 *   collapsed → single low-emphasis chip "已完成 1分51秒 v" (no border / no
 *               card chrome), so a long chain of tool calls doesn't drown
 *               out the AI's actual answer in the bubble
 *   expanded  → the ToolTraceDetails list inside a lightweight border
 *               container
 *
 * Replaces the earlier ExecutionTraceCard wrapper which felt visually
 * heavy on the assistant turn (matches user feedback 2026-05-18).
 */
export function ToolGroupCard({ status, steps, durationMs }: ToolGroupCardProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const done = steps.filter((s) => s.status !== 'running').length

  let label: string
  if (status === 'running') {
    label = t('toolGroup.running', { done, total: steps.length })
  } else if (typeof durationMs === 'number' && durationMs > 0) {
    label = t('toolGroup.doneCompact', { duration: formatDuration(durationMs, t) })
  } else {
    label = t('toolGroup.done', { done })
  }

  return (
    <div data-testid="tool-group-card">
      <button
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        // chip 风格：纯文字 + 箭头，没框、没大背景。`hover:text-foreground` 给点
        // affordance，避免完全静态看不出可点。
        className="inline-flex items-center gap-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground"
        aria-expanded={expanded}
      >
        <span>{label}</span>
        <ChevronDown
          aria-hidden
          className={`h-3.5 w-3.5 transition-transform ${expanded ? 'rotate-180' : ''}`}
        />
      </button>
      {expanded ? (
        <div className="mt-2 overflow-hidden rounded-md border border-border bg-card">
          <ToolTraceDetails steps={steps} />
        </div>
      ) : null}
    </div>
  )
}

function formatDuration(ms: number, t: ReturnType<typeof useTranslation>['t']): string {
  const totalSec = Math.round(ms / 1000)
  if (totalSec < 60) return t('toolGroup.durationSec', { sec: totalSec })
  const min = Math.floor(totalSec / 60)
  const sec = totalSec % 60
  return t('toolGroup.durationMinSec', { min, sec })
}

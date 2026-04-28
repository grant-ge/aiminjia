/**
 * @designSource design.pen step rows
 * @sizing padding [10,14]
 */
import { CheckCircle2, ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import type { ReactNode } from 'react'

export interface ToolStep {
  index: number
  name: string
  status: 'running' | 'done' | 'error'
  durationMs?: number
  inputJson?: string
  output?: ReactNode
}

interface ToolGroupStepRowProps {
  step: ToolStep
  expanded: boolean
  onToggle: () => void
}

export function ToolGroupStepRow({ step, expanded, onToggle }: ToolGroupStepRowProps) {
  const seconds = step.durationMs ? (step.durationMs / 1000).toFixed(1) + 's' : '—'
  const StatusIcon =
    step.status === 'running' ? Loader2 : step.status === 'done' ? CheckCircle2 : ChevronRight
  return (
    <button
      type="button"
      onClick={onToggle}
      className="flex w-full items-center justify-between gap-2 px-3.5 py-2.5 text-left text-[0.8125rem] hover:bg-muted/50"
    >
      <div className="flex min-w-0 items-center gap-2">
        <StatusIcon
          className={
            step.status === 'running'
              ? 'h-3.5 w-3.5 animate-spin text-primary'
              : step.status === 'done'
                ? 'h-3.5 w-3.5'
                : 'h-3.5 w-3.5 text-muted-foreground'
          }
          style={step.status === 'done' ? { color: '#16A34A' } : undefined}
        />
        <span className="truncate font-mono text-[0.78125rem] text-foreground">{step.name}</span>
      </div>
      <div className="flex items-center gap-2 text-muted-foreground">
        <span className="text-xs">{seconds}</span>
        {expanded ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
      </div>
    </button>
  )
}

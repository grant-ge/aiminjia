/**
 * @designSource design.pen#yNouu/ECmej
 * @sizing r-12 border 1 bg background; topBar padding [12,14] bottom-border 1
 */
import { CheckCircle2, ChevronDown, ChevronUp, Sparkles } from 'lucide-react'

import { ToolGroupCodeBlock } from './ToolGroupCodeBlock'
import { ToolGroupStepRow, type ToolStep } from './ToolGroupStepRow'

interface ToolGroupCardProps {
  status: 'running' | 'done'
  steps: ToolStep[]
  durationMs: number
  expanded: boolean
  expandedStepIndex: number | null
  onToggle: () => void
  onToggleStep: (index: number) => void
}

export function ToolGroupCard({
  status,
  steps,
  durationMs,
  expanded,
  expandedStepIndex,
  onToggle,
  onToggleStep,
}: ToolGroupCardProps) {
  const done = steps.filter((s) => s.status === 'done').length
  const seconds = (durationMs / 1000).toFixed(1) + 's'

  return (
    <div className="overflow-hidden rounded-[12px] border border-border bg-background">
      <button
        type="button"
        data-testid="tool-group-top-bar"
        onClick={onToggle}
        className="flex w-full items-center justify-between gap-2 border-b border-border px-3.5 py-3 text-left"
      >
        <div className="flex items-center gap-2">
          {status === 'done' ? (
            <span
              className="flex h-6 w-6 items-center justify-center rounded-md"
              style={{ backgroundColor: '#DCFCE7' }}
            >
              <CheckCircle2 className="h-3.5 w-3.5" style={{ color: '#16A34A' }} />
            </span>
          ) : (
            <span className="flex h-6 w-6 items-center justify-center rounded-md bg-brand-primary-subtle">
              <Sparkles className="h-3.5 w-3.5 text-primary" />
            </span>
          )}
          <span className="text-sm font-semibold text-foreground">
            {status === 'done' ? `已完成 ${done} 步` : '正在执行任务步骤'}
          </span>
        </div>
        <div className="flex items-center gap-2 text-muted-foreground">
          {status === 'running' ? (
            <span className="text-[12px]">{done} / {steps.length}</span>
          ) : null}
          <span className="text-[12px]">{seconds}</span>
          {expanded ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
        </div>
      </button>
      {expanded ? (
        <div className="py-1">
          {steps.map((s) => {
            const isOpen = expandedStepIndex === s.index
            return (
              <div key={s.index} className={isOpen ? 'border-b border-t border-border' : ''}>
                <ToolGroupStepRow
                  step={s}
                  expanded={isOpen}
                  onToggle={() => onToggleStep(s.index)}
                />
                {isOpen ? (
                  <ToolGroupCodeBlock inputJson={s.inputJson} output={s.output} />
                ) : null}
              </div>
            )
          })}
        </div>
      ) : null}
    </div>
  )
}

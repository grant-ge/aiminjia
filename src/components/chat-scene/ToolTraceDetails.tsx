import { useState } from 'react'

import type { ToolStep } from './ToolGroupStepRow'
import { ToolTraceIO } from './ToolTraceIO'
import { ToolTraceStep } from './ToolTraceStep'

interface ToolTraceDetailsProps {
  steps: ToolStep[]
}

export function ToolTraceDetails({ steps }: ToolTraceDetailsProps) {
  // A running step with live progress is auto-expanded so the user sees the
  // "实时输出" panel without having to click the row.  Once it transitions to
  // done/error we leave the expansion state alone — the user's manual toggle
  // wins, and the auto-expanded set is keyed on `index` (stable per step).
  const autoExpanded = new Set(
    steps
      .filter((s) => s.status === 'running' && (s.progressTail ?? '').length > 0)
      .map((s) => s.index),
  )
  const [manual, setManual] = useState<Set<number>>(() => new Set())

  return (
    <div className="py-1">
      {steps.map((step) => {
        const isOpen = manual.has(step.index) || autoExpanded.has(step.index)
        return (
          <div key={`${step.index}-${step.name}`}>
            <ToolTraceStep
              step={step}
              expanded={isOpen}
              onToggle={() => setManual((current) => {
                const next = new Set(current)
                if (next.has(step.index)) {
                  next.delete(step.index)
                } else {
                  next.add(step.index)
                }
                return next
              })}
            />
            {isOpen ? (
              <ToolTraceIO
                toolName={step.name}
                inputJson={step.inputJson}
                output={step.output}
                progressTail={step.status === 'running' ? step.progressTail : undefined}
                progressTotalBytes={
                  step.status === 'running' ? step.progressTotalBytes : undefined
                }
              />
            ) : null}
          </div>
        )
      })}
    </div>
  )
}

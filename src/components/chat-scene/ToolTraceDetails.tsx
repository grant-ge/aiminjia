import { useState } from 'react'

import type { ToolStep } from './ToolGroupStepRow'
import { ToolTraceIO } from './ToolTraceIO'
import { ToolTraceStep } from './ToolTraceStep'

interface ToolTraceDetailsProps {
  steps: ToolStep[]
}

export function ToolTraceDetails({ steps }: ToolTraceDetailsProps) {
  const [expandedStepIndexes, setExpandedStepIndexes] = useState<Set<number>>(() => new Set())

  return (
    <div className="py-1">
      {steps.map((step) => {
        const isOpen = expandedStepIndexes.has(step.index)
        return (
          <div key={`${step.index}-${step.name}`}>
            <ToolTraceStep
              step={step}
              expanded={isOpen}
              onToggle={() => setExpandedStepIndexes((current) => {
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
              />
            ) : null}
          </div>
        )
      })}
    </div>
  )
}

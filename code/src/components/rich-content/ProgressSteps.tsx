/**
 * ProgressSteps — inline analysis progress bar with step labels.
 * Based on visual-prototype-zh.html .analysis-progress styles.
 */
import type { ProgressState } from '@/types/message'

interface ProgressStepsProps {
  progress: ProgressState
}

export function ProgressSteps({ progress }: ProgressStepsProps) {
  return (
    <div
      className="my-3 rounded-lg border p-3.5"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* Title */}
      <div className="mb-2.5 flex items-center gap-2">
        <span
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {progress.title}
        </span>
      </div>

      {/* Step pills */}
      <div className="flex flex-wrap gap-1.5">
        {progress.steps.map((step, i) => {
          const isDone = step.status === 'done'
          const isActive = step.status === 'active'

          return (
            <span
              key={i}
              className="flex items-center gap-1 rounded-xl px-2.5 py-1 text-xs font-medium"
              style={{
                background: isDone
                  ? 'var(--color-primary-subtle)'
                  : isActive
                    ? 'var(--color-semantic-blue-bg)'
                    : 'var(--color-bg-neutral-subtle)',
                color: isDone
                  ? 'var(--color-primary)'
                  : isActive
                    ? 'var(--color-semantic-blue)'
                    : 'var(--color-text-muted)',
              }}
            >
              {isDone && '\u2713 '}
              {isActive && '\u25CF '}
              {step.label}
            </span>
          )
        })}
      </div>
    </div>
  )
}

/**
 * StepDivider — "Step X / 5 — Title" separator between analysis steps.
 * Based on visual-prototype-zh.html .divider styles.
 */

interface StepDividerProps {
  stepNumber: number
  totalSteps?: number
  title: string
}

export function StepDivider({ stepNumber, totalSteps = 5, title }: StepDividerProps) {
  return (
    <div className="my-7 flex items-center gap-3">
      <span
        className="h-px flex-1"
        style={{ background: 'var(--color-border)' }}
      />
      <span
        className="whitespace-nowrap rounded-2xl border px-2.5 py-0.5 text-xs font-bold tracking-wide"
        style={{
          color: 'var(--color-text-muted)',
          background: 'var(--color-bg-card)',
          borderColor: 'var(--color-border)',
        }}
      >
        Step {stepNumber} / {totalSteps} — {title}
      </span>
      <span
        className="h-px flex-1"
        style={{ background: 'var(--color-border)' }}
      />
    </div>
  )
}

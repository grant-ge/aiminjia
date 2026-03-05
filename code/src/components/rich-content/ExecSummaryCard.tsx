/**
 * ExecSummaryCard — executive summary with KPI boxes.
 * Based on visual-prototype-zh.html .exec-card / .exec-box styles.
 */
import type { ExecSummary, ExecSummaryBox } from '@/types/message'

interface ExecSummaryCardProps {
  summary: ExecSummary
}

const VARIANT_COLOR: Record<string, string> = {
  danger: 'var(--color-semantic-red)',
  money: 'var(--color-accent)',
  good: 'var(--color-semantic-green)',
  neutral: 'var(--color-text-primary)',
}

export function ExecSummaryCard({ summary }: ExecSummaryCardProps) {
  return (
    <div
      className="my-3 rounded-lg border p-4"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* Title */}
      <div className="mb-3">
        <span
          className="text-base font-bold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {summary.title}
        </span>
      </div>

      {/* Boxes grid */}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        {summary.boxes.map((box, idx) => (
          <ExecBox key={idx} box={box} />
        ))}
      </div>
    </div>
  )
}

function ExecBox({ box }: { box: ExecSummaryBox }) {
  const valueColor = VARIANT_COLOR[box.variant ?? 'neutral']

  return (
    <div
      className="rounded-lg border p-3"
      style={{
        background: 'var(--color-bg-main)',
        borderColor: 'var(--color-border)',
      }}
    >
      <div
        className="mb-1 text-xs uppercase tracking-wide"
        style={{ color: 'var(--color-text-muted)' }}
      >
        {box.label}
      </div>
      <div
        className="text-lg font-bold"
        style={{ color: valueColor }}
      >
        {box.value}
      </div>
      {box.subtitle && (
        <div
          className="mt-0.5 text-xs"
          style={{ color: 'var(--color-text-muted)' }}
        >
          {box.subtitle}
        </div>
      )}
    </div>
  )
}

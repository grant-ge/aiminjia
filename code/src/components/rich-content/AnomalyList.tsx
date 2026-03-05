/**
 * AnomalyList — prioritized anomaly items with colored dots.
 * Based on visual-prototype-zh.html .ano styles.
 */
import type { AnomalyItem } from '@/types/message'

interface AnomalyListProps {
  anomalies: AnomalyItem[]
}

const PRIORITY_COLOR: Record<AnomalyItem['priority'], string> = {
  high: 'var(--color-semantic-red)',
  medium: 'var(--color-semantic-orange)',
  low: 'var(--color-text-muted)',
}

const PRIORITY_TEXT_COLOR: Record<AnomalyItem['priority'], string> = {
  high: 'var(--color-semantic-red)',
  medium: 'var(--color-semantic-orange)',
  low: 'var(--color-text-secondary)',
}

export function AnomalyList({ anomalies }: AnomalyListProps) {
  return (
    <div
      className="my-3 overflow-hidden rounded-lg border"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
      }}
    >
      {anomalies.map((a) => (
        <div
          key={a.id}
          className="flex items-start gap-2.5 px-4 py-3"
          style={{ borderBottom: '1px solid var(--color-border-subtle)' }}
        >
          {/* Priority dot */}
          <div
            className="mt-1.5 h-2.5 w-2.5 shrink-0 rounded-full"
            style={{ background: PRIORITY_COLOR[a.priority] }}
          />

          <div className="min-w-0 flex-1">
            <div
              className="text-sm font-semibold"
              style={{ color: PRIORITY_TEXT_COLOR[a.priority] }}
            >
              {a.title}
            </div>
            <div
              className="mt-0.5 text-sm leading-snug"
              style={{ color: 'var(--color-text-muted)' }}
            >
              {a.description}
            </div>
          </div>
        </div>
      ))}
    </div>
  )
}

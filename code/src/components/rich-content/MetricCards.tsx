/**
 * MetricCards — grid of KPI cards with state-based coloring.
 * Based on visual-prototype-zh.html .mc styles.
 */
import type { MetricCard } from '@/types/message'

interface MetricCardsProps {
  metrics: MetricCard[]
}

const STATE_COLOR: Record<MetricCard['state'], string> = {
  good: 'var(--color-semantic-green)',
  warn: 'var(--color-semantic-orange)',
  bad: 'var(--color-semantic-red)',
  neutral: 'var(--color-text-primary)',
}

export function MetricCards({ metrics }: MetricCardsProps) {
  const gridCols = metrics.length <= 2 ? 'grid-cols-2' : 'grid-cols-4'

  return (
    <div className={`my-3 grid gap-3 ${gridCols}`}>
      {metrics.map((m) => (
        <div
          key={m.id}
          className="rounded-lg border p-3.5 text-center"
          style={{
            background: 'var(--color-bg-card)',
            borderColor: 'var(--color-border)',
          }}
        >
          <div
            className="mb-1.5 text-xs"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {m.label}
          </div>
          <div
            className="text-2xl font-bold"
            style={{ color: STATE_COLOR[m.state] }}
          >
            {m.value}
          </div>
          {m.subtitle && (
            <div
              className="mt-0.5 text-xs"
              style={{ color: 'var(--color-text-muted)' }}
            >
              {m.subtitle}
            </div>
          )}
        </div>
      ))}
    </div>
  )
}

/**
 * InsightBlock — blue-tinted insight card.
 * Based on visual-prototype-zh.html .insight styles.
 */
import type { InsightBlock as InsightBlockType } from '@/types/message'

interface InsightBlockProps {
  insight: InsightBlockType
}

export function InsightBlock({ insight }: InsightBlockProps) {
  return (
    <div
      className="my-3 rounded-lg border p-3.5"
      style={{
        background: 'var(--color-semantic-blue-bg-light)',
        borderColor: 'var(--color-semantic-blue-border)',
      }}
    >
      <div
        className="mb-1.5 text-base font-semibold"
        style={{ color: 'var(--color-semantic-blue)' }}
      >
        {insight.title}
      </div>
      <p
        className="m-0 text-sm leading-relaxed"
        style={{ color: 'var(--color-text-secondary)' }}
      >
        {insight.content}
      </p>
    </div>
  )
}

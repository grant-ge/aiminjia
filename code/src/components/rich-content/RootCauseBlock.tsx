/**
 * RootCauseBlock — red-tinted root cause analysis card.
 * Based on visual-prototype-zh.html .rootcause styles.
 */
import type { RootCauseBlock as RootCauseBlockType } from '@/types/message'

interface RootCauseBlockProps {
  rootCause: RootCauseBlockType
}

export function RootCauseBlock({ rootCause }: RootCauseBlockProps) {
  return (
    <div
      className="my-3 rounded-lg border p-4"
      style={{
        background: 'var(--color-semantic-red-bg-light)',
        borderColor: 'var(--color-semantic-red-border)',
      }}
    >
      {/* Title */}
      <div
        className="mb-2.5 text-base font-semibold"
        style={{ color: 'var(--color-semantic-red)' }}
      >
        {rootCause.title}
      </div>

      {/* Items */}
      {rootCause.items.map((item, idx) => (
        <div
          key={idx}
          className="py-2.5"
          style={{
            borderBottom:
              idx < rootCause.items.length - 1
                ? '1px solid var(--color-border-subtle)'
                : 'none',
          }}
        >
          <div className="mb-1 flex items-center gap-2">
            <span
              className="rounded-xl px-2 py-0.5 text-xs font-bold"
              style={{
                background: 'var(--color-semantic-red-bg)',
                color: 'var(--color-semantic-red)',
              }}
            >
              {item.count}
            </span>
            <span
              className="text-sm font-semibold"
              style={{ color: 'var(--color-text-primary)' }}
            >
              {item.label}
            </span>
          </div>
          <div
            className="mt-1 text-sm leading-snug"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {item.detail}
          </div>
          <div
            className="mt-1 text-sm font-medium"
            style={{ color: 'var(--color-primary)' }}
          >
            {item.action}
          </div>
        </div>
      ))}
    </div>
  )
}

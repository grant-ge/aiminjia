import { useState, type ReactNode } from 'react'

export interface ExecutionTraceCardSection {
  title: string
  items: ReactNode[]
}

interface ExecutionTraceCardProps {
  title: string
  badge: string
  summary?: ReactNode
  sections?: ExecutionTraceCardSection[]
  children?: ReactNode
  expandLabel?: string
  collapseLabel?: string
  expandedContent?: ReactNode | (() => ReactNode)
  defaultExpanded?: boolean
  headerCollapsible?: boolean
  defaultHeaderExpanded?: boolean
}

export function ExecutionTraceCard({
  title,
  badge,
  summary,
  sections = [],
  children,
  expandLabel,
  collapseLabel,
  expandedContent,
  defaultExpanded = false,
  headerCollapsible = false,
  defaultHeaderExpanded = true,
}: ExecutionTraceCardProps) {
  const [expanded, setExpanded] = useState(defaultExpanded)
  const [headerExpanded, setHeaderExpanded] = useState(defaultHeaderExpanded)
  const canExpand = Boolean(expandLabel && expandedContent)
  const hasContentBeforeExpander = Boolean(summary) || sections.length > 0 || Boolean(children)
  const body = expanded && expandedContent
    ? typeof expandedContent === 'function'
      ? expandedContent()
      : expandedContent
    : null

  return (
    <div
      className="overflow-hidden rounded-lg border border-border"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
      }}
    >
      {headerCollapsible ? (
        <button
          type="button"
          aria-expanded={headerExpanded}
          onClick={() => setHeaderExpanded((prev) => !prev)}
          className={`flex w-full items-center justify-between px-4 py-2.5 text-left transition-colors hover:bg-[var(--color-bg-hover)] ${headerExpanded ? 'rounded-t-lg border-b border-border' : 'rounded-lg'}`}
          style={{
            background: 'var(--color-bg-elevated)',
            borderColor: 'var(--color-border)',
          }}
        >
          <span className="flex min-w-0 items-center gap-2">
            <svg
              width="12"
              height="12"
              viewBox="0 0 12 12"
              fill="none"
              style={{
                color: 'var(--color-text-muted)',
                transition: 'transform 0.15s ease',
                transform: headerExpanded ? 'rotate(90deg)' : 'rotate(0deg)',
                flexShrink: 0,
              }}
            >
              <path
                d="M4 2l4 4-4 4"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
            <span
              className="truncate text-xs font-semibold uppercase tracking-wide"
              style={{ color: 'var(--color-text-muted)' }}
            >
              {title}
            </span>
          </span>
          <span
            className="rounded-full px-2 py-0.5 text-xs font-medium"
            style={{
              background: 'var(--color-bg-neutral)',
              color: 'var(--color-text-muted)',
            }}
          >
            {badge}
          </span>
        </button>
      ) : (
        <div
          className="flex items-center justify-between rounded-t-lg border-b px-4 py-2.5 border-border"
          style={{
            background: 'var(--color-bg-elevated)',
            borderColor: 'var(--color-border)',
          }}
        >
          <span
            className="truncate text-xs font-semibold uppercase tracking-wide"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {title}
          </span>
          <span
            className="rounded-full px-2 py-0.5 text-xs font-medium"
            style={{
              background: 'var(--color-bg-neutral)',
              color: 'var(--color-text-muted)',
            }}
          >
            {badge}
          </span>
        </div>
      )}

      {headerExpanded && summary ? (
        <div className="px-4 py-3">
          <div
            className="text-sm leading-relaxed"
            style={{ color: 'var(--color-text-primary)' }}
          >
            {summary}
          </div>
        </div>
      ) : null}

      {headerExpanded && sections.map((section, sectionIndex) => (
        <div
          key={`${section.title}-${sectionIndex}`}
          className={`${summary || sectionIndex > 0 ? 'border-t border-border' : ''} px-4 py-2.5`}
          style={{ borderColor: 'var(--color-border)' }}
        >
          <div
            className="mb-1.5 text-xs font-semibold uppercase tracking-wide"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {section.title}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {section.items.map((item, index) => (
              <span
                key={index}
                className="rounded-md px-2 py-0.5 text-xs font-medium"
                style={{
                  background: 'var(--color-bg-neutral)',
                  color: 'var(--color-text-secondary)',
                  fontFamily: 'monospace',
                }}
              >
                {item}
              </span>
            ))}
          </div>
        </div>
      ))}

      {headerExpanded && children ? (
        <div
          className={summary || sections.length > 0 ? 'border-t border-border' : ''}
          style={{ borderColor: 'var(--color-border)' }}
        >
          {children}
        </div>
      ) : null}

      {headerExpanded && canExpand ? (
        <div
          className={hasContentBeforeExpander ? 'border-t border-border' : ''}
          style={{ borderColor: 'var(--color-border)' }}
        >
          <button
            type="button"
            onClick={() => setExpanded((prev) => !prev)}
            className="flex w-full items-center gap-2 px-4 py-2.5 text-left text-xs transition-colors hover:bg-[var(--color-bg-hover)]"
            style={{ color: 'var(--color-text-muted)' }}
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 12 12"
              fill="none"
              style={{
                transition: 'transform 0.15s ease',
                transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)',
                flexShrink: 0,
              }}
            >
              <path
                d="M4 2l4 4-4 4"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
            <span className="font-medium">{expanded ? collapseLabel ?? expandLabel : expandLabel}</span>
          </button>
          {body ? (
            <div className="border-t border-border" style={{ borderColor: 'var(--color-border)' }}>
              {body}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

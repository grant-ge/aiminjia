import type { ToolStep } from './ToolGroupStepRow'

interface ToolTraceStepProps {
  step: ToolStep
  expanded: boolean
  onToggle: () => void
}

function statusLabel(status: ToolStep['status']): string {
  switch (status) {
    case 'running': return '执行中'
    case 'error': return '失败'
    default: return '完成'
  }
}

function statusColor(status: ToolStep['status']): string {
  switch (status) {
    case 'running': return 'var(--color-semantic-blue)'
    case 'error': return 'var(--color-semantic-red)'
    default: return 'var(--color-text-muted)'
  }
}

export function ToolTraceStep({ step, expanded, onToggle }: ToolTraceStepProps) {
  const seconds = step.durationMs != null ? `${(step.durationMs / 1000).toFixed(1)}s` : '-'

  return (
    <button
      type="button"
      onClick={onToggle}
      className="flex w-full items-center justify-between gap-2 px-4 py-2.5 text-left text-xs transition-colors hover:bg-[var(--color-bg-hover)]"
      style={{ color: 'var(--color-text-muted)' }}
    >
      <div className="flex min-w-0 items-center gap-2">
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
        <span className="truncate font-mono text-[0.78125rem]" style={{ color: 'var(--color-text-primary)' }}>
          {step.name}
        </span>
        <span style={{ color: statusColor(step.status) }}>{statusLabel(step.status)}</span>
      </div>
      <span className="shrink-0" style={{ color: 'var(--color-text-muted)' }}>
        {seconds}
      </span>
    </button>
  )
}

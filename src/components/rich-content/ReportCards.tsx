/**
 * ReportCards — clickable report file cards with type icons.
 * Based on visual-prototype-zh.html .rcard styles.
 */
import type { ReportCard } from '@/types/message'

interface ReportCardsProps {
  reports: ReportCard[]
  onOpen?: (reportId: string) => void
}

const FILE_TYPE_ICON: Record<ReportCard['fileType'], { label: string; bg: string; color: string }> = {
  html: { label: 'HTML', bg: 'var(--color-filetype-blue-bg)', color: 'var(--color-semantic-blue)' },
  excel: { label: 'XLS', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  pdf: { label: 'PDF', bg: 'var(--color-filetype-red-bg)', color: 'var(--color-semantic-red)' },
}

export function ReportCards({ reports, onOpen }: ReportCardsProps) {
  return (
    <div className="my-3 grid grid-cols-2 gap-3">
      {reports.map((r) => {
        const icon = FILE_TYPE_ICON[r.fileType]

        return (
          <button
            key={r.id}
            type="button"
            className="flex cursor-pointer items-center gap-3.5 rounded-lg border p-4 text-left transition-all duration-200"
            style={{
              background: 'var(--color-bg-card)',
              borderColor: 'var(--color-border)',
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.borderColor = 'var(--color-primary)'
              e.currentTarget.style.background = 'var(--color-bg-card-hover)'
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.borderColor = 'var(--color-border)'
              e.currentTarget.style.background = 'var(--color-bg-card)'
            }}
            onClick={() => onOpen?.(r.id)}
          >
            <div
              className="flex h-[42px] w-[42px] shrink-0 items-center justify-center rounded-lg text-xs font-bold"
              style={{ background: icon.bg, color: icon.color }}
            >
              {icon.label}
            </div>
            <div className="min-w-0 flex-1">
              <h4
                className="mb-0.5 text-base font-semibold"
                style={{ color: 'var(--color-text-primary)' }}
              >
                {r.title}
              </h4>
              <p
                className="m-0 text-sm"
                style={{ color: 'var(--color-text-muted)' }}
              >
                {r.description}
              </p>
            </div>
          </button>
        )
      })}
    </div>
  )
}

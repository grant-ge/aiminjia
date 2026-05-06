import { useTranslation } from 'react-i18next'

interface UpdaterDialogProps {
  open: boolean
  version: string
  notes: string
  onLater: () => void
  onUpdateNow: () => void
}

/**
 * Modal for the auto-updater. Renders release notes as a scrollable bullet list
 * (one bullet per non-empty line of `notes`) with two clear actions: snooze
 * this version locally, or download + install + relaunch immediately. Sized
 * smaller than the generic Modal so it doesn't dominate the screen.
 */
export function UpdaterDialog({
  open,
  version,
  notes,
  onLater,
  onUpdateNow,
}: UpdaterDialogProps) {
  const { t } = useTranslation()
  if (!open) return null

  const bullets = notes
    .split(/\r?\n/)
    .map((line) => line.replace(/^[-•·]\s*/, '').trim())
    .filter(Boolean)

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: 'var(--color-overlay)' }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onLater()
      }}
    >
      <div
        className="flex flex-col rounded-lg border animate-[modalIn_0.2s_ease-out]"
        style={{
          width: '480px',
          maxWidth: 'calc(100vw - 48px)',
          maxHeight: '70vh',
          background: 'var(--color-bg-card)',
          borderColor: 'var(--color-border)',
          boxShadow: 'var(--shadow-modal)',
        }}
      >
        <div
          className="shrink-0 px-5 py-4 border-b"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <h3 className="text-base font-semibold">
            {t('updater.newVersionFound', { version })}
          </h3>
          <p className="mt-1 text-xs" style={{ color: 'var(--color-text-muted)' }}>
            {t('updater.releaseNotesHeader')}
          </p>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          {bullets.length > 0 ? (
            <ul
              className="space-y-2 pl-5 list-disc"
              style={{ color: 'var(--color-text-secondary)' }}
            >
              {bullets.map((line, i) => (
                <li key={i} className="text-sm leading-relaxed">
                  {line}
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm" style={{ color: 'var(--color-text-muted)' }}>
              {t('updater.updateAvailableDesc')}
            </p>
          )}
        </div>

        <div
          className="flex shrink-0 items-center justify-end gap-2 border-t px-5 py-3"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <button
            type="button"
            onClick={onLater}
            className="rounded-md border px-4 py-1.5 text-sm transition-colors"
            style={{
              borderColor: 'var(--color-border)',
              background: 'transparent',
              color: 'var(--color-text-secondary)',
            }}
          >
            {t('updater.updateLater')}
          </button>
          <button
            type="button"
            onClick={onUpdateNow}
            className="rounded-md px-4 py-1.5 text-sm font-medium transition-colors"
            style={{
              background: 'var(--color-primary)',
              color: 'var(--color-primary-contrast, #fff)',
            }}
          >
            {t('updater.updateNow')}
          </button>
        </div>
      </div>
    </div>
  )
}

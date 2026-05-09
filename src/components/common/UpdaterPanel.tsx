import { useTranslation } from 'react-i18next'
import { getVersion } from '@tauri-apps/api/app'
import { useEffect, useState } from 'react'
import { useUpdaterStore } from '@/lib/updaterStore'

function formatBytes(n: number): string {
  if (n <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toFixed(v >= 10 || i === 0 ? 0 : 1)} ${units[i]}`
}

/**
 * Non-modal panel shown when the user clicks the title-bar update link.
 * Displays release notes and two actions: dismiss (keeps the link) or install.
 * When offline (no live Update handle even though phase='ready'), the install
 * button is disabled with an explanatory tooltip.
 */
export function UpdaterPanel() {
  const { t } = useTranslation()
  const open = useUpdaterStore((s) => s.panelOpen)
  const phase = useUpdaterStore((s) => s.phase)
  const version = useUpdaterStore((s) => s.version)
  const notes = useUpdaterStore((s) => s.notes)
  const progress = useUpdaterStore((s) => s.progress)
  const hasHandle = useUpdaterStore((s) => s._update !== null)
  const closePanel = useUpdaterStore((s) => s.closePanel)
  const installNow = useUpdaterStore((s) => s.installNow)

  const [currentVersion, setCurrentVersion] = useState<string>('')
  useEffect(() => {
    if (open) {
      void getVersion().then(setCurrentVersion)
    }
  }, [open])

  if (!open || !version) return null

  const bullets = notes
    .split(/\r?\n/)
    .map((line) => line.replace(/^[-•·]\s*/, '').trim())
    .filter(Boolean)

  const installDisabled = !hasHandle || phase === 'installing'
  const disabledReason = !hasHandle
    ? t('updater.offlineHint')
    : phase === 'installing'
      ? t('updater.installing')
      : ''

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: 'var(--color-overlay)' }}
      onClick={(e) => {
        if (e.target === e.currentTarget) closePanel()
      }}
    >
      <div
        className="flex flex-col rounded-lg border animate-[modalIn_0.2s_ease-out]"
        style={{
          width: '520px',
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
            {t('updater.panelTitle', { version })}
          </h3>
          <p className="mt-1 text-xs" style={{ color: 'var(--color-text-muted)' }}>
            {currentVersion
              ? t('updater.versionLine', {
                  current: currentVersion,
                  next: version,
                  size: progress?.total ? formatBytes(progress.total) : '',
                })
              : t('updater.releaseNotesHeader')}
          </p>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          <p
            className="mb-3 text-xs font-medium uppercase tracking-wide"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {t('updater.releaseNotesHeader')}
          </p>
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
            onClick={closePanel}
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
            disabled={installDisabled}
            title={disabledReason || undefined}
            onClick={() => void installNow()}
            className="rounded-md px-4 py-1.5 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50"
            style={{
              background: 'var(--color-primary)',
              color: 'var(--color-primary-contrast, #fff)',
            }}
          >
            {phase === 'installing' ? t('updater.installing') : t('updater.installAndRestart')}
          </button>
        </div>
      </div>
    </div>
  )
}

import { useTranslation } from 'react-i18next'
import { useUpdaterStore } from '@/lib/updaterStore'

/**
 * Title-bar entry point for the updater. Visible when phase is 'ready' or
 * 'failed'. For 'ready', clicking opens the UpdaterPanel. For 'failed',
 * clicking retries via bootstrap() so the user has a visible recovery path.
 */
export function UpdateAvailableLink() {
  const { t } = useTranslation()
  const phase = useUpdaterStore((s) => s.phase)
  const version = useUpdaterStore((s) => s.version)
  const openPanel = useUpdaterStore((s) => s.openPanel)
  const bootstrap = useUpdaterStore((s) => s.bootstrap)

  if (phase === 'failed') {
    return (
      <button
        type="button"
        onClick={() => void bootstrap()}
        onMouseDown={(e) => e.stopPropagation()}
        title={t('updater.retryTooltip')}
        className="mr-2 flex h-6 shrink-0 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-primary-foreground/95 transition-colors hover:bg-white/10"
      >
        <span
          className="inline-block h-1.5 w-1.5 rounded-full ring-1 ring-white/75"
          style={{ background: '#f59e0b' }}
          aria-hidden
        />
        <span>{t('updater.retryLinkText')}</span>
      </button>
    )
  }

  if (phase !== 'ready' || !version) return null

  return (
    <button
      type="button"
      onClick={openPanel}
      onMouseDown={(e) => e.stopPropagation()}
      title={t('updater.linkTooltip')}
      className="mr-2 flex h-6 shrink-0 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-primary-foreground/95 transition-colors hover:bg-white/10"
    >
      <span
        className="inline-block h-1.5 w-1.5 rounded-full ring-1 ring-white/75"
        style={{ background: '#ef4444' }}
        aria-hidden
      />
      <span>{t('updater.linkText', { version })}</span>
    </button>
  )
}

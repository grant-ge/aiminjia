import { useTranslation } from 'react-i18next'
import { useUpdaterStore } from '@/lib/updaterStore'

/**
 * Title-bar entry point for the updater. Visible only when phase === 'ready'.
 * Clicking opens the UpdaterPanel where the user reads release notes and
 * decides whether to install + relaunch.
 */
export function UpdateAvailableLink() {
  const { t } = useTranslation()
  const phase = useUpdaterStore((s) => s.phase)
  const version = useUpdaterStore((s) => s.version)
  const openPanel = useUpdaterStore((s) => s.openPanel)

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

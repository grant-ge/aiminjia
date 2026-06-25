import { useTranslation } from 'react-i18next'
import { useUpdaterStore } from '@/lib/updaterStore'
import { Button } from '@/components/ui/button'

const READY_DOT_COLOR = '#22c55e'

export function UpdateAvailableLink() {
  const { t } = useTranslation()
  const phase = useUpdaterStore((s) => s.phase)
  const version = useUpdaterStore((s) => s.version)
  const openPanel = useUpdaterStore((s) => s.openPanel)

  if (phase !== 'ready' || !version) return null

  return (
    <Button unstyled
      data-aijia-updater-link
      data-aijia-updater-version={version}
      type="button"
      onClick={openPanel}
      onMouseDown={(e) => e.stopPropagation()}
      title={t('updater.linkReadyTooltip')}
      className="mr-2 flex h-6 shrink-0 items-center gap-1.5 rounded px-2 text-xs font-medium text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent/70 hover:text-sidebar-foreground"
    >
      <span
        className="inline-block h-1.5 w-1.5 rounded-md ring-1 ring-white/75"
        style={{ background: READY_DOT_COLOR }}
        aria-hidden
      />
      <span>{t('updater.linkReady', { version })}</span>
    </Button>
  )
}

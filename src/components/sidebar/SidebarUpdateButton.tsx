import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { useUpdaterStore } from '@/lib/updaterStore'
import { cn } from '@/lib/utils'

const VISIBLE_PHASES = new Set(['available', 'downloading', 'ready', 'failed', 'installing'])

function phaseClassName(phase: string): string {
  if (phase === 'failed') {
    return 'bg-[var(--color-updater-action-red)] text-[var(--color-updater-action-foreground)] hover:bg-[var(--color-updater-action-red-hover)]'
  }
  if (phase === 'ready') {
    return 'bg-[var(--color-updater-action-green)] text-[var(--color-updater-action-foreground)] hover:bg-[var(--color-updater-action-green-hover)]'
  }
  if (phase === 'installing') {
    return 'bg-[var(--color-updater-action-green)] text-[var(--color-updater-action-foreground)] hover:bg-[var(--color-updater-action-green-hover)]'
  }
  if (phase === 'downloading') {
    return 'bg-[var(--color-updater-action-blue)] text-[var(--color-updater-action-foreground)] hover:bg-[var(--color-updater-action-blue-hover)]'
  }
  return 'bg-[var(--color-updater-action-blue)] text-[var(--color-updater-action-foreground)] hover:bg-[var(--color-updater-action-blue-hover)]'
}

function phaseLabelKey(phase: string): string {
  if (phase === 'failed') return 'updater.sidebarButtonFailed'
  if (phase === 'ready') return 'updater.sidebarButtonReady'
  if (phase === 'installing') return 'updater.sidebarButtonInstalling'
  if (phase === 'downloading') return 'updater.sidebarButtonDownloading'
  return 'updater.sidebarButton'
}

export function SidebarUpdateButton() {
  const { t } = useTranslation()
  const phase = useUpdaterStore((s) => s.phase)
  const version = useUpdaterStore((s) => s.version)
  const openPanel = useUpdaterStore((s) => s.openPanel)

  if (!version || !VISIBLE_PHASES.has(phase)) return null

  return (
    <Button
      unstyled
      data-aijia-updater-sidebar-button
      data-aijia-updater-phase={phase}
      data-aijia-updater-version={version}
      type="button"
      className={cn(
        'flex h-6 shrink-0 -mr-2 items-center rounded-full px-2 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        phaseClassName(phase),
      )}
      title={t('updater.sidebarButtonTooltip', { version })}
      onPointerDown={(event) => event.stopPropagation()}
      onMouseDown={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation()
        openPanel()
      }}
    >
      {t(phaseLabelKey(phase))}
    </Button>
  )
}

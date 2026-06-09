import { useTranslation } from 'react-i18next'
import { getVersion } from '@tauri-apps/api/app'
import { useEffect, useState } from 'react'
import { CheckCircle2 } from 'lucide-react'
import { useUpdaterStore } from '@/lib/updaterStore'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

function formatBytes(n: number): string {
  if (n <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++ }
  return `${v.toFixed(v >= 10 || i === 0 ? 0 : 1)} ${units[i]}`
}

export function UpdaterPanel() {
  const { t } = useTranslation()
  const open = useUpdaterStore((s) => s.panelOpen)
  const phase = useUpdaterStore((s) => s.phase)
  const version = useUpdaterStore((s) => s.version)
  const notes = useUpdaterStore((s) => s.notes)
  const progress = useUpdaterStore((s) => s.progress)
  const installProgress = useUpdaterStore((s) => s.installProgress)
  const error = useUpdaterStore((s) => s.error)
  const online = useUpdaterStore((s) => s.online)
  const closePanel = useUpdaterStore((s) => s.closePanel)
  const startDownload = useUpdaterStore((s) => s.startDownload)
  const retryDownload = useUpdaterStore((s) => s.retryDownload)
  const installNow = useUpdaterStore((s) => s.installNow)

  const [currentVersion, setCurrentVersion] = useState('')
  useEffect(() => {
    if (open) void getVersion().then(setCurrentVersion)
  }, [open])

  if (!version) return null

  const pct = progress && progress.total > 0
    ? Math.round((progress.downloaded / progress.total) * 100)
    : 0
  const installPct = installProgress && installProgress.total > 0
    ? Math.round((installProgress.current / installProgress.total) * 100)
    : 0

  const bullets = notes
    .split(/\r?\n/)
    .map((line) => line.replace(/^[-•·]\s*/, '').trim())
    .filter(Boolean)

  const dialogTitle =
    phase === 'downloading' ? t('updater.dialogTitleDownloading', { version })
    : phase === 'ready' ? t('updater.dialogTitleReady', { version })
    : phase === 'failed' ? t('updater.dialogTitleFailed')
    : phase === 'installing' ? t('updater.dialogTitleInstalling', { version })
    : t('updater.dialogTitleAvailable', { version })

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) closePanel() }}>
      <DialogContent
        data-aijia-updater-panel
        data-aijia-updater-phase={phase}
        data-aijia-updater-version={version}
        className="max-w-md overflow-hidden"
        onOpenAutoFocus={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>{dialogTitle}</DialogTitle>
          {currentVersion && phase !== 'failed' && (
            <p className="text-sm text-muted-foreground">
              {t('updater.versionLine', { current: currentVersion, next: version })}
            </p>
          )}
        </DialogHeader>

        {/* Phase: available — release notes */}
        {phase === 'available' && (
          <div className="space-y-3">
            {bullets.length > 0 ? (
              <>
                <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  {t('updater.releaseNotesHeader')}
                </p>
                <ul className="space-y-1.5 pl-5 list-disc text-sm text-foreground/80">
                  {bullets.map((line, i) => <li key={i}>{line}</li>)}
                </ul>
              </>
            ) : (
              <p className="text-sm text-muted-foreground">{t('updater.updateAvailableDesc')}</p>
            )}
          </div>
        )}

        {/* Phase: downloading — progress bar */}
        {phase === 'downloading' && (
          <div className="space-y-3 py-2">
            <div className="h-2 w-full overflow-hidden rounded-md bg-muted">
              <div
                data-aijia-updater-progress
                data-aijia-updater-progress-percent={pct}
                className="h-full rounded-md bg-primary transition-all duration-300"
                style={{ width: `${pct}%` }}
              />
            </div>
            <p className="text-center text-sm text-muted-foreground">
              {t('updater.downloadProgress', {
                downloaded: formatBytes(progress?.downloaded ?? 0),
                total: formatBytes(progress?.total ?? 0),
              })}
            </p>
          </div>
        )}

        {/* Phase: ready — download complete, show release notes for context */}
        {phase === 'ready' && (
          <div className="space-y-4">
            <div className="flex items-center gap-3 rounded-md bg-[var(--color-semantic-green)]/8 px-4 py-3">
              <CheckCircle2
                className="h-5 w-5 shrink-0 text-[var(--color-semantic-green)]"
                strokeWidth={2.25}
              />
              <p className="text-sm font-medium text-foreground">
                {t('updater.downloadComplete', { size: formatBytes(progress?.total ?? 0) })}
              </p>
            </div>
            {bullets.length > 0 && (
              <div>
                <p className="mb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  {t('updater.releaseNotesHeader')}
                </p>
                <ul className="space-y-1.5 pl-5 list-disc text-sm text-foreground/80">
                  {bullets.map((line, i) => <li key={i}>{line}</li>)}
                </ul>
              </div>
            )}
          </div>
        )}

        {/* Phase: failed — error message */}
        {phase === 'failed' && (
          <div className="py-4 text-center">
            <p
              data-aijia-updater-error
              className="text-sm text-destructive"
            >
              {t('updater.downloadFailedMessage', { error: error ?? '' })}
            </p>
          </div>
        )}

        {/* Phase: installing — spinner */}
        {phase === 'installing' && (
          <div className="space-y-3 py-3">
            <div className="h-2 w-full overflow-hidden rounded-md bg-muted">
              <div
                data-testid="updater-install-progress"
                data-aijia-updater-install-progress
                data-aijia-updater-install-percent={installPct}
                className="h-full rounded-md bg-primary transition-all duration-300"
                style={{ width: `${installPct}%` }}
              />
            </div>
            <p className="text-center text-sm text-muted-foreground">
              {t(`updater.installStage.${installProgress?.stage ?? 'preparing'}`)}
            </p>
          </div>
        )}

        <DialogFooter>
          {phase === 'available' && (
            <>
              <Button
                data-aijia-updater-action="later"
                variant="outline"
                onClick={closePanel}
              >
                {t('updater.updateLater')}
              </Button>
              <Button
                data-aijia-updater-action="download"
                onClick={() => void startDownload()}
              >
                {t('updater.updateNow')}
              </Button>
            </>
          )}
          {phase === 'ready' && (
            <>
              <Button
                data-aijia-updater-action="later"
                variant="outline"
                onClick={closePanel}
              >
                {t('updater.updateLater')}
              </Button>
              <Button
                data-aijia-updater-action="install"
                onClick={() => void installNow()}
                disabled={!online}
                title={!online ? t('updater.offlineHint') : undefined}
              >
                {t('updater.installAndRestart')}
              </Button>
            </>
          )}
          {phase === 'failed' && (
            <Button
              data-aijia-updater-action="retry"
              onClick={() => void retryDownload()}
            >
              {t('updater.retry')}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

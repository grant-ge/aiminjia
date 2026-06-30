import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Switch } from '@/components/common/Switch'
import { getSettings, runtimeDiagnostics, type RuntimeDiagnostics, updateSettings } from '@/lib/tauri'
import { useSettingsStore } from '@/stores/settingsStore'

function useResolverLabel(): Record<RuntimeDiagnostics['activeResolver'], string> {
  const { t } = useTranslation()
  return {
    installed: t('settings.runtime.installed'),
    none: t('settings.runtime.unavailable'),
  }
}

function useFormatRelative() {
  const { t } = useTranslation()
  return (from: number, now: number): string => {
    const diffSec = Math.max(0, Math.round((now - from) / 1000))
    if (diffSec < 5) return t('settings.runtime.justNow')
    if (diffSec < 60) return t('settings.runtime.secondsAgo', { count: diffSec })
    const diffMin = Math.floor(diffSec / 60)
    if (diffMin < 60) return t('settings.runtime.minutesAgo', { count: diffMin })
    const diffHr = Math.floor(diffMin / 60)
    if (diffHr < 24) return t('settings.runtime.hoursAgo', { count: diffHr })
    return new Date(from).toLocaleString()
  }
}

export function RuntimePanel() {
  const { t } = useTranslation()
  const RESOLVER_LABEL = useResolverLabel()
  const formatRelative = useFormatRelative()
  const [data, setData] = useState<RuntimeDiagnostics | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [lastCheckedAt, setLastCheckedAt] = useState<number | null>(null)
  const [now, setNow] = useState(() => Date.now())
  const managedRuntimeEnabled = useSettingsStore((s) => s.managedRuntimeEnabled ?? true)
  const setManagedRuntimeEnabled = useSettingsStore((s) => s.setManagedRuntimeEnabled)

  const load = async () => {
    setLoading(true)
    setError(null)
    try {
      setData(await runtimeDiagnostics())
      setLastCheckedAt(Date.now())
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  useEffect(() => {
    if (lastCheckedAt == null) return
    const id = window.setInterval(() => setNow(Date.now()), 15_000)
    return () => window.clearInterval(id)
  }, [lastCheckedAt])

  const handleManagedRuntimeChange = async (enabled: boolean) => {
    const previous = managedRuntimeEnabled
    setManagedRuntimeEnabled(enabled)
    try {
      const current = await getSettings()
      await updateSettings({ ...current, managedRuntimeEnabled: enabled })
    } catch (e) {
      setManagedRuntimeEnabled(previous)
      setError(String(e))
    }
  }

  return (
    <section className="flex flex-col gap-4">
      <header>
        <h2 className="text-base font-semibold text-foreground">{t('settings.runtime.title')}</h2>
        <p className="mt-0.5 text-xs text-muted-foreground">
          {t('settings.runtime.description')}
        </p>
      </header>

      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {error}
        </div>
      )}

      <div className="flex items-center justify-between gap-4 rounded-md border border-border bg-card px-4 py-3">
        <div className="min-w-0">
          <div className="text-sm font-semibold text-foreground">
            {t('settings.runtime.managedRuntime')}
          </div>
          <div className="mt-1 text-xs leading-5 text-muted-foreground">
            {t('settings.runtime.managedRuntimeDesc')}
          </div>
        </div>
        <Switch
          checked={managedRuntimeEnabled}
          onCheckedChange={(checked) => void handleManagedRuntimeChange(checked)}
          aria-label={t('settings.runtime.managedRuntime')}
        />
      </div>

      {data && (
        <dl className="grid grid-cols-[120px_1fr] gap-x-4 gap-y-2 rounded-md border border-border bg-muted/30 p-4 text-sm">
          <dt className="text-muted-foreground">{t('settings.runtime.source')}</dt>
          <dd className="font-mono text-foreground">{RESOLVER_LABEL[data.activeResolver]}</dd>

          <dt className="text-muted-foreground">{t('settings.runtime.installedVersion')}</dt>
          <dd className="font-mono text-foreground">{data.installedVersion ?? '—'}</dd>

          <dt className="text-muted-foreground">Node</dt>
          <dd className="font-mono text-foreground">{data.node}</dd>

          <dt className="text-muted-foreground">Python</dt>
          <dd className="font-mono text-foreground">{data.python}</dd>

          <dt className="text-muted-foreground">uv</dt>
          <dd className="font-mono text-foreground">{data.uv}</dd>
        </dl>
      )}

      <div className="flex items-center gap-3">
        <Button variant="outline" onClick={() => void load()} disabled={loading}>
          {loading ? t('settings.runtime.checking') : t('settings.runtime.recheck')}
        </Button>
        {lastCheckedAt != null && (
          <span className="text-xs text-muted-foreground">
            {t('settings.runtime.lastChecked')}{formatRelative(lastCheckedAt, now)}
          </span>
        )}
      </div>
    </section>
  )
}

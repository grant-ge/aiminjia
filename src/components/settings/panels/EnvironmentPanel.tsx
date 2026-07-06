import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { TitleBarEnvSwitcher } from '@/components/layout/TitleBarEnvSwitcher'
import type { DevEnvironmentState } from '@/lib/tauri'

export function EnvironmentPanel() {
  const { t } = useTranslation()
  const [environment, setEnvironment] = useState<DevEnvironmentState | null>(null)

  const currentPreset = environment?.presets.find((preset) => preset.tenant === environment.currentTenant)
  const currentLabel = environment
    ? currentPreset
      ? t(`settings.environment.env.${currentPreset.key}`, currentPreset.key)
      : t('settings.environment.custom')
    : '...'

  return (
    <section className="flex flex-col gap-4">
      <header>
        <h3 className="text-base font-semibold text-foreground">{t('settings.environment.title')}</h3>
        <p className="mt-0.5 text-xs leading-5 text-muted-foreground">
          {t('settings.environment.customHint')}
        </p>
      </header>

      <div className="flex items-center justify-between gap-4 rounded-md border border-border bg-card px-4 py-3">
        <div className="min-w-0">
          <div className="text-sm font-semibold text-foreground">
            {t('settings.environment.current')}
          </div>
          <div className="mt-1 text-xs text-muted-foreground">{currentLabel}</div>
        </div>
        <TitleBarEnvSwitcher className="mr-0 shrink-0" onStateChange={setEnvironment} />
      </div>

      <dl className="grid grid-cols-[120px_1fr] gap-x-4 gap-y-2 rounded-md border border-border bg-[rgba(var(--muted-rgb),0.30)] p-4 text-sm">
        <dt className="text-muted-foreground">{t('settings.environment.tenantLabel')}</dt>
        <dd className="min-w-0 break-all font-mono text-foreground">
          {environment?.currentTenant ?? '...'}
        </dd>

        <dt className="text-muted-foreground">{t('settings.environment.opsLabel')}</dt>
        <dd className="min-w-0 break-all font-mono text-foreground">
          {environment?.currentOps ?? '...'}
        </dd>
      </dl>
    </section>
  )
}

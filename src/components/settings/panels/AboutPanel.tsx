/**
 * @designSource copied from Wukong about settings page, adapted to AI 小家 branding.
 */
import { useEffect, useState } from 'react'
import type { ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { getLogLevel, setLogLevel } from '@/lib/tauri'
import { cn } from '@/lib/utils'
import type { AppLogLevel } from '@/types/settings'

interface AboutPanelLinks {
  customerService: () => void
  productSuggestion: () => void
  privacyPolicy: () => void
  terms: () => void
}

interface AboutPanelProps {
  appName: string
  version: string
  logoUrl: string
  checkingUpdate?: boolean
  onCheckUpdate: () => void
  onUploadLogs: () => void | Promise<void>
  onResetData: () => void
  links: AboutPanelLinks
}

type PillButtonProps = Omit<ComponentProps<typeof Button>, 'type' | 'variant' | 'className' | 'children'> & {
  children: string
  onClick: () => void
  danger?: boolean
  disabled?: boolean
}

function PillButton({
  children,
  onClick,
  danger = false,
  disabled = false,
  ...buttonProps
}: PillButtonProps) {
  return (
    <Button
      {...buttonProps}
      type="button"
      onClick={onClick}
      disabled={disabled}
      variant={danger ? 'destructive' : 'outline'}
      className={cn(
        'h-9 rounded-md px-5 text-sm font-semibold',
        disabled && 'cursor-not-allowed opacity-60',
      )}
    >
      {children}
    </Button>
  )
}

export function AboutPanel({
  appName,
  version,
  logoUrl,
  checkingUpdate = false,
  onCheckUpdate,
  onUploadLogs,
  links,
}: AboutPanelProps) {
  const { t } = useTranslation()
  const [uploadingLogs, setUploadingLogs] = useState(false)
  const [logLevel, setLogLevelState] = useState('info')

  useEffect(() => {
    getLogLevel().then(setLogLevelState).catch(() => {})
  }, [])

  const handleLogLevelChange = (level: string) => {
    setLogLevelState(level)
    setLogLevel(level).catch(() => {})
  }

  const LOG_LEVEL_OPTIONS: Array<{ value: AppLogLevel; labelKey: string }> = [
    { value: 'error', labelKey: 'settings.about.logLevelError' },
    { value: 'warn', labelKey: 'settings.about.logLevelWarn' },
    { value: 'info', labelKey: 'settings.about.logLevelInfo' },
    { value: 'debug', labelKey: 'settings.about.logLevelDebug' },
  ]

  const handleUploadLogs = async () => {
    if (uploadingLogs) return
    setUploadingLogs(true)
    try {
      await onUploadLogs()
    } finally {
      setUploadingLogs(false)
    }
  }

  return (
    <div className="flex flex-col gap-4 text-foreground">
      <section className="flex items-center justify-between gap-6">
        <div className="flex min-w-0 items-center justify-center gap-4">
          <img
            src={logoUrl}
            alt={`${appName} ${t('settings.about.icon')}`}
            className="h-16 w-16 shrink-0 rounded-md border-border bg-card object-cover"
          />
          <div className="flex min-w-0 flex-col gap-1.5 pt-1">
            <div className="text-base font-bold leading-none text-foreground">{appName}</div>
            <div className="text-sm leading-none text-muted-foreground">{t('settings.about.version')} {version}</div>
          </div>
        </div>
        <PillButton
          onClick={onCheckUpdate}
          disabled={checkingUpdate}
          data-aijia-settings-action="check-update"
        >
          {checkingUpdate ? t('settings.about.checkingUpdate') : t('settings.about.checkUpdate')}
        </PillButton>
      </section>

      <div className="h-px bg-border mb-2" />

      <section className="flex flex-col gap-3">
        <div className="text-xl font-bold text-foreground">{t('settings.about.policiesTitle')}</div>

        <div className="flex flex-wrap gap-2">
          <PillButton onClick={links.terms}>{t('settings.about.terms')}</PillButton>
          <PillButton onClick={links.privacyPolicy}>{t('settings.about.privacyPolicy')}</PillButton>
        </div>
      </section>

      <div className="mb-2 h-px bg-border" />

      <section className="flex flex-col gap-3 pb-2">
        <div className="text-xl font-bold text-foreground">{t('settings.about.devMode')}</div>

        <div className="flex items-center justify-between gap-6">
          <div className="flex flex-col gap-1">
            <span className="text-base font-semibold text-foreground">{t('settings.about.logUpload')}</span>
            <div className="text-sm text-muted-foreground">{t('settings.about.logUploadDesc')}</div>
          </div>
          <PillButton onClick={handleUploadLogs} disabled={uploadingLogs}>
            {uploadingLogs ? t('settings.about.uploading') : t('settings.about.uploadLogs')}
          </PillButton>
        </div>

        <div className="flex items-center justify-between gap-6">
          <div className="flex min-w-0 flex-col gap-1">
            <span className="text-base font-semibold text-foreground">{t('settings.about.logLevel')}</span>
            <div className="text-sm text-muted-foreground">{t('settings.about.logLevelDesc')}</div>
          </div>
          <div
            className="inline-flex shrink-0 rounded-lg bg-muted p-1"
            role="radiogroup"
            aria-label={t('settings.about.logLevel')}
          >
            {LOG_LEVEL_OPTIONS.map((option) => {
              const selected = logLevel === option.value
              const label = t(option.labelKey)
              return (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={label}
                  onClick={() => handleLogLevelChange(option.value)}
                  className={
                    selected
                      ? 'rounded-md bg-card px-3 py-1.5 text-sm font-semibold text-foreground shadow-sm'
                      : 'rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground'
                  }
                >
                  {label}
                </button>
              )
            })}
          </div>
        </div>
      </section>
    </div>
  )
}

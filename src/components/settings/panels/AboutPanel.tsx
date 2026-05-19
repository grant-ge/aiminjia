/**
 * @designSource copied from Wukong about settings page, adapted to AI 小家 branding.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { DataMaskingLevel } from '@/types/settings'

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
  onCheckUpdate: () => void
  onUploadLogs: () => void | Promise<void>
  onResetData: () => void
  dataMaskingLevel: DataMaskingLevel
  onDataMaskingChange: (level: DataMaskingLevel) => void
  links: AboutPanelLinks
}

function PillButton({
  children,
  onClick,
  danger = false,
  disabled = false,
}: {
  children: string
  onClick: () => void
  danger?: boolean
  disabled?: boolean
}) {
  return (
    <Button
      type="button"
      onClick={onClick}
      disabled={disabled}
      variant={danger ? 'destructive' : 'outline'}
      className={cn(
        'h-9 rounded-lg px-5 text-sm font-semibold',
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
  onCheckUpdate,
  onUploadLogs,
  links,
}: AboutPanelProps) {
  const { t } = useTranslation()
  const [uploadingLogs, setUploadingLogs] = useState(false)

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
        <div className="flex min-w-0 items-start gap-4">
          <img
            src={logoUrl}
            alt={`${appName} ${t('settings.about.icon')}`}
            className="h-16 w-16 shrink-0 rounded-lg border-border bg-card object-cover"
          />
          <div className="flex min-w-0 flex-col gap-1.5 pt-1">
            <div className="text-base font-bold leading-none text-foreground">{appName}</div>
            <div className="text-sm leading-none text-muted-foreground">{t('settings.about.version')} {version}</div>
          </div>
        </div>
        <PillButton onClick={onCheckUpdate}>{t('settings.about.checkUpdate')}</PillButton>
      </section>

      <div className="h-px bg-border mb-2" />

      {/*
      <section className="flex flex-col gap-4">
        <div className="text-xl font-bold tracking-tight text-foreground">隐私</div>
        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">隐私保护增强</div>
            <div className="text-sm text-muted-foreground">
              开启后，发送给模型前会自动隐藏部分敏感信息。关闭后可获得更完整的上下文体验。
            </div>
          </div>
          <Switch
            aria-label="隐私保护增强"
            checked={dataMaskingLevel !== 'relaxed'}
            onCheckedChange={(checked) => onDataMaskingChange(checked ? 'strict' : 'relaxed')}
          />
        </div>
      </section>

      <div className="h-px bg-border mb-2" />
      */}

      <section className="flex flex-col gap-3">
        <div className="text-xl font-bold tracking-tight text-foreground">{t('settings.about.policiesTitle')}</div>

        <div className="flex flex-wrap gap-2">
          <PillButton onClick={links.terms}>{t('settings.about.terms')}</PillButton>
          <PillButton onClick={links.privacyPolicy}>{t('settings.about.privacyPolicy')}</PillButton>
        </div>
      </section>

      <div className="mb-2 h-px bg-border" />

      <section className="flex flex-col gap-3 pb-2">
        <div className="text-xl font-bold tracking-tight text-foreground">{t('settings.about.devMode')}</div>

        <div className="flex items-center justify-between gap-6">
          <div className="flex flex-col gap-1">
            <span className="text-base font-semibold text-foreground">{t('settings.about.logUpload')}</span>
            <div className="text-sm text-muted-foreground">{t('settings.about.logUploadDesc')}</div>
          </div>
          <PillButton onClick={handleUploadLogs} disabled={uploadingLogs}>
            {uploadingLogs ? t('settings.about.uploading') : t('settings.about.uploadLogs')}
          </PillButton>
        </div>
      </section>
    </div>
  )
}

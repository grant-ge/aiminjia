/**
 * @designSource design.pen#S3D6p / 1MCFZ / az6ZY
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { message } from '@tauri-apps/plugin-dialog'

import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { LegalDocumentDialog } from '@/components/legal/LegalDocumentDialog'
import { LEGAL_DOCUMENTS, type LegalDocumentKey } from '@/components/legal/legalDocuments'
import { getSettings, updateSettings } from '@/lib/tauri'
import { useUpdaterStore } from '@/lib/updaterStore'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { useUiStore } from '@/stores/uiStore'

import { SettingsContentBody } from './SettingsContentBody'
import { SettingsMenu } from './SettingsMenu'
import { SettingsShell } from './SettingsShell'
import { AboutPanel } from './panels/AboutPanel'
import { ArchivedPanel } from './panels/ArchivedPanel'
import { GeneralPanel } from './panels/GeneralPanel'
import { RuntimePanel } from './panels/RuntimePanel'

export function SettingsModal() {
  const { t } = useTranslation()
  const settingsModal = useUiStore((s) => s.settingsModal)
  const closeSettings = useUiStore((s) => s.closeSettings)
  const openSettings = useUiStore((s) => s.openSettings)
  const user = useAuthStore((s) => s.user)
  const tenant = useAuthStore((s) => s.tenant)
  const logout = useAuthStore((s) => s.logout)
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const dataMaskingLevel = useSettingsStore((s) => s.dataMaskingLevel ?? 'relaxed')
  const setDataMaskingLevel = useSettingsStore((s) => s.setDataMaskingLevel)
  const [pendingLogout, setPendingLogout] = useState(false)
  const [appVersion, setAppVersion] = useState(t('settings.loadingVersion'))
  const [activeLegalDocument, setActiveLegalDocument] = useState<LegalDocumentKey | null>(null)

  useEffect(() => {
    if (settingsModal !== 'about') return

    let cancelled = false
    import('@tauri-apps/api/app')
      .then(({ getVersion }) => getVersion())
      .then((version) => {
        if (!cancelled) setAppVersion(version)
      })
      .catch(() => {
        if (!cancelled) setAppVersion(t('settings.unknown'))
      })

    return () => {
      cancelled = true
    }
  }, [settingsModal])

  if (!settingsModal) return null

  const legalDocument = activeLegalDocument ? LEGAL_DOCUMENTS[activeLegalDocument] : null

  const onLogout = async () => {
    if (pendingLogout) return
    setPendingLogout(true)
    try {
      await logout()
      closeSettings()
    } finally {
      setPendingLogout(false)
    }
  }

  const openExternalLink = async (url: string) => {
    try {
      const { open } = await import('@tauri-apps/plugin-shell')
      await open(url)
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), { title: productName, kind: 'error' })
    }
  }

  const onCheckUpdate = async () => {
    try {
      const store = useUpdaterStore.getState()
      // Always re-check via bootstrap() — it deduplicates concurrent calls
      // and handles the full check → download → ready pipeline (#1).
      await store.bootstrap()
      const phase = useUpdaterStore.getState().phase
      if (phase === 'idle') {
        await message(t('settings.alreadyLatestVersion'), { title: productName, kind: 'info' })
      } else if (phase !== 'failed') {
        // downloading / ready / installing → show the panel
        store.openPanel()
      }
      // 'failed' → download-failure toast already shown by bootstrap()
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), { title: productName, kind: 'error' })
    }
  }

  const onUploadLogs = async () => {
    try {
      const { uploadDiagnosticLogs } = await import('@/lib/tauri')
      const result = await uploadDiagnosticLogs()
      const badNote = result.bad_metrics_lines > 0
        ? `\n${t('settings.badMetricsLines')}: ${result.bad_metrics_lines}`
        : ''
      await message(
        `${t('settings.logsUploaded')}\n${t('settings.uploadId')}: ${result.session_id}\n${t('settings.chunks')}: ${result.chunks_uploaded}/${result.chunks_total}\n${t('settings.appLogLines')}: ${result.app_log_lines_uploaded}\n${t('settings.diagnosticEvents')}: ${result.events_uploaded}${badNote}`,
        { title: productName, kind: 'info' },
      )
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), { title: productName, kind: 'error' })
    }
  }

  const onResetData = async () => {
    const confirmed = await requestConfirm({
      title: productName,
      description: t('settings.resetDescription'),
      confirmLabel: t('settings.reset'),
      variant: 'destructive',
    })
    if (!confirmed) return

    localStorage.clear()
    useBrandingStore.getState().reset()
    await message(t('settings.localCacheCleared'), { title: productName, kind: 'info' })
  }

  return (
    <>
      <SettingsShell
        open
        onClose={closeSettings}
        height={720}
        menu={
          <SettingsMenu
            activeKey={settingsModal}
            onSelect={(k) => openSettings(k)}
          />
        }
        content={
          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            <SettingsContentBody>
              {settingsModal === 'account' ? (
                <GeneralPanel
                  user={{
                    name: user?.name ?? user?.username ?? t('settings.notLoggedIn'),
                    tenantName: tenant?.name ?? '',
                    avatarUrl: '',
                  }}
                  onLogout={() => void onLogout()}
                />
              ) : null}
              {settingsModal === 'about' ? (
                <AboutPanel
                  appName={productName}
                  version={appVersion}
                  logoUrl={logoUrl}
                  onCheckUpdate={() => void onCheckUpdate()}
                  onUploadLogs={() => void onUploadLogs()}
                  onResetData={() => void onResetData()}
                  dataMaskingLevel={dataMaskingLevel}
                  onDataMaskingChange={async (level) => {
                    setDataMaskingLevel(level)
                    try {
                      const current = await getSettings()
                      await updateSettings({ ...current, dataMaskingLevel: level })
                    } catch (err) {
                      console.error('Failed to persist dataMaskingLevel:', err)
                    }
                  }}
                  links={{
                    customerService: () => void openExternalLink('https://www.renlijia.com/support'),
                    productSuggestion: () => void openExternalLink('https://www.renlijia.com/feedback'),
                    privacyPolicy: () => setActiveLegalDocument('privacy'),
                    terms: () => setActiveLegalDocument('terms'),
                  }}
                />
              ) : null}
              {settingsModal === 'runtime' ? <RuntimePanel /> : null}
              {settingsModal === 'archived' ? <ArchivedPanel /> : null}
            </SettingsContentBody>
          </div>
        }
      />
      <LegalDocumentDialog
        document={legalDocument}
        open={legalDocument !== null}
        onOpenChange={(open) => {
          if (!open) setActiveLegalDocument(null)
        }}
      />
    </>
  )
}

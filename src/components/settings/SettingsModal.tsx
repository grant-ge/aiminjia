/**
 * @designSource design.pen#S3D6p / 1MCFZ / az6ZY
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { message } from '@tauri-apps/plugin-dialog'

import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { LegalDocumentDialog } from '@/components/legal/LegalDocumentDialog'
import { getLegalDocument, type LegalDocumentKey } from '@/components/legal/legalDocuments'
import { useUpdaterStore } from '@/lib/updaterStore'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore, type SettingsModalKey } from '@/stores/uiStore'

import { SettingsContentBody } from './SettingsContentBody'
import { SettingsMenu } from './SettingsMenu'
import { SettingsShell } from './SettingsShell'
import { AboutPanel } from './panels/AboutPanel'
import { AccountBillingPanel } from './panels/AccountBillingPanel'
import { ArchivedPanel } from './panels/ArchivedPanel'
import { EnvironmentPanel } from './panels/EnvironmentPanel'
import { GeneralPanel } from './panels/GeneralPanel'
import { PermissionsPanel } from './panels/PermissionsPanel'
import { RuntimePanel } from './panels/RuntimePanel'

export function SettingsModal() {
  const { t, i18n } = useTranslation()
  const settingsModal = useUiStore((s) => s.settingsModal)
  const closeSettings = useUiStore((s) => s.closeSettings)
  const openSettings = useUiStore((s) => s.openSettings)
  const user = useAuthStore((s) => s.user)
  const tenant = useAuthStore((s) => s.tenant)
  const logout = useAuthStore((s) => s.logout)
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const showAccountBilling = tenant?.tenantType !== 'enterprise'
  const showDevEnvironmentSettings = import.meta.env.DEV
  const hiddenSettingsKeys: SettingsModalKey[] = [
    ...(showAccountBilling ? [] : (['account-billing'] satisfies SettingsModalKey[])),
    ...(showDevEnvironmentSettings ? [] : (['environment'] satisfies SettingsModalKey[])),
  ]
  const [pendingLogout, setPendingLogout] = useState(false)
  const [appVersion, setAppVersion] = useState(t('settings.loadingVersion'))
  const [checkingUpdate, setCheckingUpdate] = useState(false)
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

  useEffect(() => {
    if (settingsModal === 'account-billing' && !showAccountBilling) {
      openSettings('account')
    }
  }, [openSettings, settingsModal, showAccountBilling])

  useEffect(() => {
    if (settingsModal === 'environment' && !showDevEnvironmentSettings) {
      openSettings('account')
    }
  }, [openSettings, settingsModal, showDevEnvironmentSettings])

  if (!settingsModal) return null

  const legalDocument = activeLegalDocument
    ? getLegalDocument(activeLegalDocument, i18n.language)
    : null

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
    if (checkingUpdate) return
    setCheckingUpdate(true)
    try {
      const store = useUpdaterStore.getState()
      // If an update is already known (any non-idle phase), just open the panel
      // — re-running bootstrap could race with an in-flight download.
      const phase = store.phase
      if (phase !== 'idle' && phase !== 'checking') {
        store.openPanel()
        return
      }
      // Manual mode: bootstrap stays in `available` if a new version is found
      // (instead of auto-starting the download), so the user explicitly
      // confirms in the dialog.
      await store.bootstrap({ triggeredBy: 'manual' })
      const newPhase = useUpdaterStore.getState().phase
      if (newPhase === 'idle' || newPhase === 'checking') {
        await message(t('settings.about.alreadyLatestVersion'), { title: productName, kind: 'info' })
      } else {
        store.openPanel()
      }
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), { title: productName, kind: 'error' })
    } finally {
      setCheckingUpdate(false)
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
            hiddenKeys={hiddenSettingsKeys}
          />
        }
        content={
          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            <SettingsContentBody>
              {settingsModal === 'account' ? (
                <GeneralPanel
                  section="profile"
                  user={{
                    name: user?.name ?? user?.username ?? t('settings.notLoggedIn'),
                    accountName: user?.username ?? '',
                    tenantName: tenant?.name ?? '',
                    avatarUrl: '',
                  }}
                  onLogout={() => void onLogout()}
                />
              ) : null}
              {settingsModal === 'system' ? (
                <>
                  <GeneralPanel
                    section="system"
                    user={{
                      name: user?.name ?? user?.username ?? t('settings.notLoggedIn'),
                      accountName: user?.username ?? '',
                      tenantName: tenant?.name ?? '',
                      avatarUrl: '',
                    }}
                    onLogout={() => void onLogout()}
                  />
                  <PermissionsPanel />
                </>
              ) : null}
              {settingsModal === 'account-billing' && showAccountBilling ? <AccountBillingPanel /> : null}
              {settingsModal === 'environment' && showDevEnvironmentSettings ? <EnvironmentPanel /> : null}
              {settingsModal === 'about' ? (
                <AboutPanel
                  appName={productName}
                  version={appVersion}
                  logoUrl={logoUrl}
                  tenantName={tenant?.name ?? ''}
                  checkingUpdate={checkingUpdate}
                  onCheckUpdate={() => void onCheckUpdate()}
                  onUploadLogs={() => void onUploadLogs()}
                  onResetData={() => void onResetData()}
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

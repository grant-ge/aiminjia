/**
 * @designSource design.pen#S3D6p / 1MCFZ / az6ZY
 */
import { useEffect, useState } from 'react'
import { message } from '@tauri-apps/plugin-dialog'

import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { getSettings, updateSettings } from '@/lib/tauri'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { useUiStore } from '@/stores/uiStore'

import { SettingsContentBody } from './SettingsContentBody'
import { SettingsMenu } from './SettingsMenu'
import { SettingsShell } from './SettingsShell'
import { AboutPanel } from './panels/AboutPanel'
import { GeneralPanel } from './panels/GeneralPanel'
import { ArchivedPanel } from './panels/ArchivedPanel'

export function SettingsModal() {
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
  const [appVersion, setAppVersion] = useState('读取中')

  useEffect(() => {
    if (settingsModal !== 'about') return

    let cancelled = false
    import('@tauri-apps/api/app')
      .then(({ getVersion }) => getVersion())
      .then((version) => {
        if (!cancelled) setAppVersion(version)
      })
      .catch(() => {
        if (!cancelled) setAppVersion('未知')
      })

    return () => {
      cancelled = true
    }
  }, [settingsModal])

  if (!settingsModal) return null

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
      const [{ check }, { getVersion }, { relaunch }] = await Promise.all([
        import('@tauri-apps/plugin-updater'),
        import('@tauri-apps/api/app'),
        import('@tauri-apps/plugin-process'),
      ])
      const update = await check()
      if (!update) {
        await message('当前已是最新版本。', { title: productName, kind: 'info' })
        return
      }

      const currentVersion = await getVersion()
      if (update.version === currentVersion) {
        await message('当前已是最新版本。', { title: productName, kind: 'info' })
        return
      }

      const confirmed = await requestConfirm({
        title: `发现新版本 v${update.version}`,
        description: update.body ?? `发现新版本 v${update.version}，是否立即更新？`,
        confirmLabel: '立即更新',
        cancelLabel: '稍后再说',
      })
      if (!confirmed) return

      await update.downloadAndInstall()
      await relaunch()
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), { title: productName, kind: 'error' })
    }
  }

  const onUploadLogs = async () => {
    try {
      const { uploadDiagnosticLogs } = await import('@/lib/tauri')
      const result = await uploadDiagnosticLogs()
      const badNote = result.bad_metrics_lines > 0
        ? `\n损坏行数: ${result.bad_metrics_lines}（已作为文本上传）`
        : ''
      await message(
        `日志已上传\n上传 ID: ${result.session_id}\n分块: ${result.chunks_uploaded}/${result.chunks_total}\n应用日志行数: ${result.app_log_lines_uploaded}\n诊断事件数: ${result.events_uploaded}${badNote}`,
        { title: productName, kind: 'info' },
      )
    } catch (e) {
      await message(e instanceof Error ? e.message : String(e), { title: productName, kind: 'error' })
    }
  }

  const onResetData = async () => {
    const confirmed = await requestConfirm({
      title: productName,
      description: '将清除本地缓存并恢复默认设置，是否继续？',
      confirmLabel: '重置',
      variant: 'destructive',
    })
    if (!confirmed) return

    localStorage.clear()
    useBrandingStore.getState().reset()
    await message('本地缓存已清除，部分设置会在重启后恢复默认。', { title: productName, kind: 'info' })
  }

  return (
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
                  name: user?.name ?? user?.username ?? '未登录',
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
                copyright="仁励家网络科技(杭州)有限公司 版权所有"
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
                  privacyPolicy: () => void openExternalLink('https://www.renlijia.com/privacy'),
                  terms: () => void openExternalLink('https://www.renlijia.com/terms'),
                }}
              />
            ) : null}
            {settingsModal === 'archived' ? <ArchivedPanel /> : null}
          </SettingsContentBody>
        </div>
      }
    />
  )
}

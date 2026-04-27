/**
 * @designSource design.pen#S3D6p / 1MCFZ / az6ZY
 */
import { useState } from 'react'

import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore } from '@/stores/uiStore'

import { SettingsContentBody } from './SettingsContentBody'
import { SettingsContentTop } from './SettingsContentTop'
import { SettingsMenu, SETTINGS_MENU_ITEMS } from './SettingsMenu'
import { SettingsShell } from './SettingsShell'
import { AboutPanel } from './panels/AboutPanel'
import { GeneralPanel } from './panels/GeneralPanel'
import { ArchivedPanel } from './panels/ArchivedPanel'
import { PlaceholderPanel } from './panels/PlaceholderPanel'
import { UsagePanel } from './panels/UsagePanel'

export function SettingsModal() {
  const settingsModal = useUiStore((s) => s.settingsModal)
  const closeSettings = useUiStore((s) => s.closeSettings)
  const openSettings = useUiStore((s) => s.openSettings)
  const user = useAuthStore((s) => s.user)
  const tenant = useAuthStore((s) => s.tenant)
  const logout = useAuthStore((s) => s.logout)
  const productName = useBrandingStore((s) => s.productName)
  const [pendingLogout, setPendingLogout] = useState(false)

  if (!settingsModal) return null

  const activeLabel =
    SETTINGS_MENU_ITEMS.find((m) => m.key === settingsModal)?.label || '设置'

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
          <SettingsContentTop title={activeLabel} onClose={closeSettings} />
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
                version="0.9.30"
                tenantName="仁励家网络科技(杭州)有限公司"
                helpLinks={[
                  { label: '使用手册', onClick: () => {} },
                  { label: '反馈问题', onClick: () => {} },
                ]}
                devInfo={[
                  { label: '架构', value: 'Tauri 2.x · React' },
                  { label: '更新通道', value: '稳定版' },
                ]}
              />
            ) : null}
            {settingsModal === 'usage' ? (
              <UsagePanel
                planName="标准版"
                planRenewLabel="按企业账号自动续期"
                quota={[
                  { label: '会话次数', used: 142, total: 500 },
                  { label: '模型调用 tokens', used: 234000, total: 1000000 },
                ]}
                detail={[
                  { label: '本月会话', value: '142 次' },
                  { label: '本月技能调用', value: '38 次' },
                ]}
              />
            ) : null}
            {settingsModal === 'permissions' ? <PlaceholderPanel title="系统权限" /> : null}
            {settingsModal === 'mcp' ? <PlaceholderPanel title="MCP 服务" /> : null}
            {settingsModal === 'sso' ? <PlaceholderPanel title="SSO 集成" /> : null}
            {settingsModal === 'shortcuts' ? <PlaceholderPanel title="快捷键" /> : null}
            {settingsModal === 'archived' ? <ArchivedPanel /> : null}
          </SettingsContentBody>
        </div>
      }
    />
  )
}

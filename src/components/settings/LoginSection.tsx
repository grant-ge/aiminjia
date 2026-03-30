/**
 * LoginSection — Login form and cloud account info display.
 * Displayed inside SettingsModal when in cloud mode or to trigger login.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { open } from '@tauri-apps/plugin-shell'
import { Button } from '@/components/common/Button'
import { useAuthStore } from '@/stores/authStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { cloudLogin, cloudLogout, updateSettings, getSettings, cloudChangePassword } from '@/lib/tauri'
import { useSettingsStore } from '@/stores/settingsStore'
import { useBrandingStore } from '@/stores/brandingStore'

interface LoginSectionProps {
  onLoginSuccess?: () => void
}

export function LoginSection({ onLoginSuccess }: LoginSectionProps) {
  const { t } = useTranslation()
  const auth = useAuthStore()
  const notifications = useNotificationStore()
  const useCloud = useSettingsStore((s) => s.useCloud)

  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  // Change password state
  const [showChangePassword, setShowChangePassword] = useState(false)
  const [oldPassword, setOldPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [changingPassword, setChangingPassword] = useState(false)
  const [changePasswordError, setChangePasswordError] = useState('')

  const handleLogin = async () => {
    if (!username.trim() || !password) {
      setError(t('login.fillAllFields'))
      return
    }

    setLoading(true)
    setError('')
    try {
      const result = await cloudLogin(username.trim(), password)
      auth.setAuth(result)
      useBrandingStore.getState().applyBranding(result.tenant ?? null)

      // Persist the selected cloud model to settings (only if not already set)
      if (result.models.length > 0) {
        const settings = await getSettings()
        const firstModel = result.models[0]
        if (!settings.cloudModel) {
          await updateSettings({
            ...settings,
            useCloud: true,
            cloudModel: firstModel.id,
            cloudModelType: firstModel.modelType || 'chat'
          })
          useSettingsStore.getState().setSettings({
            useCloud: true,
            cloudModel: firstModel.id,
            cloudModelType: firstModel.modelType || 'chat'
          })
        } else {
          // Restore previously selected model + enable cloud
          await updateSettings({ ...settings, useCloud: true })
          useSettingsStore.getState().setSettings({ useCloud: true })
          const prev = result.models.find((m) => m.id === settings.cloudModel)
          auth.setSelectedCloudModel(prev ? settings.cloudModel : firstModel.id)
        }
      } else {
        // No models but still enable cloud
        const settings = await getSettings()
        await updateSettings({ ...settings, useCloud: true })
        useSettingsStore.getState().setSettings({ useCloud: true })
      }

      setUsername('')
      setPassword('')
      notifications.push({
        level: 'success',
        title: t('login.loginSuccess'),
        message: t('login.welcome', { name: result.user?.name ?? result.user?.username }),
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
      onLoginSuccess?.()
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      setError(msg)
    } finally {
      setLoading(false)
    }
  }

  const handleLogout = async () => {
    try {
      await cloudLogout()
    } catch (err) {
      console.error('Failed to logout:', err)
    }
    // Always clear frontend state regardless of IPC result
    auth.clearAuth()
    useBrandingStore.getState().reset()
    // Disable cloud mode
    try {
      const settings = await getSettings()
      await updateSettings({ ...settings, useCloud: false })
      useSettingsStore.getState().setSettings({ useCloud: false })
    } catch (err) {
      console.error('Failed to update useCloud:', err)
    }
    useSettingsStore.getState().setSettings({ useCloud: false })
    notifications.push({
      level: 'info',
      title: t('login.loggedOut'),
      message: t('login.switchedToLocal'),
      actions: [],
      dismissible: true,
      autoHide: 3,
      context: 'toast',
    })
  }

  const handleChangePassword = async () => {
    if (!oldPassword || !newPassword || !confirmPassword) {
      setChangePasswordError(t('login.fillAllFields'))
      return
    }
    if (newPassword.length < 8) {
      setChangePasswordError(t('login.newPasswordMinLength'))
      return
    }
    if (newPassword !== confirmPassword) {
      setChangePasswordError(t('login.passwordMismatch'))
      return
    }

    setChangingPassword(true)
    setChangePasswordError('')
    try {
      await cloudChangePassword(oldPassword, newPassword)
      // Server-side logout already happened, clear frontend state
      auth.clearAuth()
      useBrandingStore.getState().reset()
      try {
        const settings = await getSettings()
        await updateSettings({ ...settings, useCloud: false })
        useSettingsStore.getState().setSettings({ useCloud: false })
      } catch (err) {
        console.error('Failed to update useCloud:', err)
      }
      notifications.push({
        level: 'success',
        title: t('login.passwordChanged'),
        message: t('login.pleaseRelogin'),
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
      // Reset form
      setShowChangePassword(false)
      setOldPassword('')
      setNewPassword('')
      setConfirmPassword('')
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      setChangePasswordError(msg)
    } finally {
      setChangingPassword(false)
    }
  }

  // --- Logged in state ---
  if (auth.isLoggedIn) {
    const handleToggleCloud = async (value: boolean) => {
      try {
        const settings = await getSettings()
        await updateSettings({ ...settings, useCloud: value })
        useSettingsStore.getState().setSettings({ useCloud: value })
      } catch (err) {
        console.error('Failed to toggle useCloud:', err)
      }
    }

    return (
      <div>
        {/* Account info card */}
        <div
          className="mb-4 rounded-lg border p-4"
          style={{
            background: 'var(--color-bg-main)',
            borderColor: 'var(--color-border)',
          }}
        >
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div
                className="flex h-8 w-8 items-center justify-center rounded-full text-sm font-semibold"
                style={{
                  background: 'var(--color-primary-subtle)',
                  color: 'var(--color-primary)',
                }}
              >
                {(auth.user?.name ?? auth.user?.username ?? '?')[0].toUpperCase()}
              </div>
              <div>
                <div
                  className="text-sm font-medium"
                  style={{ color: 'var(--color-text-primary)' }}
                >
                  {auth.user?.name ?? auth.user?.username}
                </div>
                <div
                  className="text-xs"
                  style={{ color: 'var(--color-text-muted)' }}
                >
                  {auth.tenant?.name}
                </div>
              </div>
            </div>
            <Button variant="secondary" onClick={handleLogout}>
              {t('login.logout')}
            </Button>
          </div>

        </div>

        {/* Change password section */}
        <div className="mb-4 rounded-md border p-3" style={{ borderColor: 'var(--color-border)' }}>
          <button
            className="mb-2 text-sm font-medium transition-colors hover:opacity-80"
            style={{ color: 'var(--color-primary)', background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}
            onClick={() => {
              setShowChangePassword(!showChangePassword)
              setChangePasswordError('')
              setOldPassword('')
              setNewPassword('')
              setConfirmPassword('')
            }}
          >
            {showChangePassword ? `▼ ${t('login.cancelChangePassword')}` : `▶ ${t('login.changePassword')}`}
          </button>

          {showChangePassword && (
            <div className="mt-3 space-y-3">
              <input
                type="password"
                className="h-9 w-full rounded-md border px-3 text-sm outline-none"
                style={{
                  background: 'var(--color-bg-main)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
                placeholder={t('login.oldPassword')}
                value={oldPassword}
                onChange={(e) => setOldPassword(e.target.value)}
              />
              <input
                type="password"
                className="h-9 w-full rounded-md border px-3 text-sm outline-none"
                style={{
                  background: 'var(--color-bg-main)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
                placeholder={t('login.newPassword')}
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
              />
              <input
                type="password"
                className="h-9 w-full rounded-md border px-3 text-sm outline-none"
                style={{
                  background: 'var(--color-bg-main)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
                placeholder={t('login.confirmNewPassword')}
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
              />
              {changePasswordError && (
                <div
                  className="text-xs"
                  style={{ color: 'var(--color-semantic-red)' }}
                >
                  {changePasswordError}
                </div>
              )}
              <Button
                variant="primary"
                onClick={handleChangePassword}
                disabled={changingPassword}
              >
                {changingPassword ? t('login.changing') : t('login.confirmChange')}
              </Button>
            </div>
          )}
        </div>

        {/* Model mode toggle */}
        <div className="mb-4">
          <label
            className="mb-1.5 block text-sm font-semibold"
            style={{ color: 'var(--color-text-secondary)' }}
          >
            {t('login.modelMode')}
          </label>
          <div
            className="inline-flex rounded-md border"
            style={{ borderColor: 'var(--color-border)' }}
          >
            <button
              className="rounded-l-md px-4 py-1.5 text-sm font-medium transition-colors"
              style={{
                background: useCloud ? 'var(--color-primary-subtle)' : 'transparent',
                color: useCloud ? 'var(--color-primary)' : 'var(--color-text-muted)',
                border: 'none',
                cursor: 'pointer',
              }}
              onClick={() => handleToggleCloud(true)}
            >
              {t('login.cloudModel')}
            </button>
            <button
              className="rounded-r-md px-4 py-1.5 text-sm font-medium transition-colors"
              style={{
                background: !useCloud ? 'var(--color-primary-subtle)' : 'transparent',
                color: !useCloud ? 'var(--color-primary)' : 'var(--color-text-muted)',
                border: 'none',
                borderLeft: '1px solid var(--color-border)',
                cursor: 'pointer',
              }}
              onClick={() => handleToggleCloud(false)}
            >
              {t('login.localModel')}
            </button>
          </div>
          <div
            className="mt-1 text-xs"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {useCloud ? t('login.cloudEnabled') : t('login.localEnabled')}
          </div>
        </div>

        {/* Cloud model is auto-selected on login, hidden from user */}

        <div
          className="rounded-md px-3 py-2 text-xs"
          style={{
            background: 'var(--color-primary-subtle)',
            color: 'var(--color-primary)',
          }}
        >
          {useCloud
            ? t('login.cloudEnabledInfo')
            : t('login.localEnabledInfo')}
        </div>
      </div>
    )
  }

  // --- Login form ---
  return (
    <div>
      <div
        className="mb-4 rounded-lg border p-4"
        style={{
          background: 'var(--color-bg-main)',
          borderColor: 'var(--color-border)',
        }}
      >
        <div
          className="mb-2 text-sm font-semibold"
          style={{ color: 'var(--color-text-secondary)' }}
        >
          {t('login.loginTitle')}
        </div>
        <div
          className="mb-3 text-xs"
          style={{ color: 'var(--color-text-muted)' }}
        >
          {t('login.loginDesc')}
          {t('login.adminAssigned')}
          <a
            href="#"
            onClick={(e) => {
              e.preventDefault()
              open('https://ai-tenant.renlijia.com/')
            }}
            style={{ color: 'var(--color-primary)' }}
          >
            {t('login.registerEnterprise')}
          </a>
        </div>

        <div className="mb-3">
          <input
            type="text"
            className="mb-2 h-9 w-full rounded-md border px-3 text-sm outline-none"
            style={{
              background: 'var(--color-bg-main)',
              borderColor: 'var(--color-border)',
              color: 'var(--color-text-primary)',
            }}
            placeholder={t('login.usernamePlaceholder')}
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleLogin()}
          />
          <input
            type="password"
            className="h-9 w-full rounded-md border px-3 text-sm outline-none"
            style={{
              background: 'var(--color-bg-main)',
              borderColor: 'var(--color-border)',
              color: 'var(--color-text-primary)',
            }}
            placeholder={t('login.passwordPlaceholder')}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleLogin()}
          />
        </div>

        {error && (
          <div
            className="mb-3 text-xs"
            style={{ color: 'var(--color-semantic-red)' }}
          >
            {error}
          </div>
        )}

        <Button
          variant="primary"
          onClick={handleLogin}
          disabled={loading}
        >
          {loading ? t('login.loggingIn') : t('login.loginButton')}
        </Button>
      </div>
    </div>
  )
}

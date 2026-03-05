/**
 * LoginSection — Login form and cloud account info display.
 * Displayed inside SettingsModal when in cloud mode or to trigger login.
 */
import { useState } from 'react'
import { open } from '@tauri-apps/plugin-shell'
import { Button } from '@/components/common/Button'
import { useAuthStore } from '@/stores/authStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { cloudLogin, cloudLogout, updateSettings, getSettings, cloudChangePassword } from '@/lib/tauri'
import { useSettingsStore } from '@/stores/settingsStore'

interface LoginSectionProps {
  onLoginSuccess?: () => void
}

export function LoginSection({ onLoginSuccess }: LoginSectionProps) {
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
      setError('请输入用户名和密码')
      return
    }

    setLoading(true)
    setError('')
    try {
      const result = await cloudLogin(username.trim(), password)
      auth.setAuth(result)

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
        title: '登录成功',
        message: `欢迎，${result.user?.name ?? result.user?.username}`,
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
      title: '已退出登录',
      message: '已切换到本地模式',
      actions: [],
      dismissible: true,
      autoHide: 3,
      context: 'toast',
    })
  }

  const handleChangePassword = async () => {
    if (!oldPassword || !newPassword || !confirmPassword) {
      setChangePasswordError('请填写所有字段')
      return
    }
    if (newPassword.length < 8) {
      setChangePasswordError('新密码长度至少 8 个字符')
      return
    }
    if (newPassword !== confirmPassword) {
      setChangePasswordError('两次输入的新密码不一致')
      return
    }

    setChangingPassword(true)
    setChangePasswordError('')
    try {
      await cloudChangePassword(oldPassword, newPassword)
      // Server-side logout already happened, clear frontend state
      auth.clearAuth()
      try {
        const settings = await getSettings()
        await updateSettings({ ...settings, useCloud: false })
        useSettingsStore.getState().setSettings({ useCloud: false })
      } catch (err) {
        console.error('Failed to update useCloud:', err)
      }
      notifications.push({
        level: 'success',
        title: '密码修改成功',
        message: '请重新登录',
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
              退出登录
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
            {showChangePassword ? '▼ 取消修改密码' : '▶ 修改密码'}
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
                placeholder="旧密码"
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
                placeholder="新密码（至少 8 个字符）"
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
                placeholder="确认新密码"
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
                {changingPassword ? '修改中...' : '确认修改'}
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
            模型模式
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
              云端模型
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
              本地模型
            </button>
          </div>
          <div
            className="mt-1 text-xs"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {useCloud ? '使用企业云端模型，无需 API Key' : '使用本地配置的 API Key 调用模型'}
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
            ? '云端模式已启用，大模型和搜索请求通过服务端处理。'
            : '本地模式，使用你配置的 API Key 直接调用模型。'}
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
          登录企业账号
        </div>
        <div
          className="mb-3 text-xs"
          style={{ color: 'var(--color-text-muted)' }}
        >
          登录后可直接使用云端大模型和联网搜索，无需配置 API Key。
          企业账号由管理员分配，
          <a
            href="#"
            onClick={(e) => {
              e.preventDefault()
              open('https://ai-tenant.renlijia.com/')
            }}
            style={{ color: 'var(--color-primary)' }}
          >
            注册企业 →
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
            placeholder="用户名@企业编码 如 zhangsan@001"
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
            placeholder="密码"
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
          {loading ? '登录中...' : '登录'}
        </Button>
      </div>
    </div>
  )
}

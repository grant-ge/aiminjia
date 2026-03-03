/**
 * LoginSection — Login form and cloud account info display.
 * Displayed inside SettingsModal when in cloud mode or to trigger login.
 */
import { useState } from 'react'
import { Button } from '@/components/common/Button'
import { useAuthStore } from '@/stores/authStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { cloudLogin, cloudLogout, getCloudModels, updateSettings, getSettings } from '@/lib/tauri'
import { useSettingsStore } from '@/stores/settingsStore'

interface LoginSectionProps {
  onLoginSuccess?: () => void
}

export function LoginSection({ onLoginSuccess }: LoginSectionProps) {
  const auth = useAuthStore()
  const notifications = useNotificationStore()

  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const handleLogin = async () => {
    if (!username.trim() || !password) {
      setError('Please enter username and password')
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
        if (!settings.cloudModel) {
          await updateSettings({ ...settings, cloudModel: result.models[0].id })
          useSettingsStore.getState().setSettings({ cloudModel: result.models[0].id })
        } else {
          // Restore previously selected model
          const prev = result.models.find((m) => m.id === settings.cloudModel)
          auth.setSelectedCloudModel(prev ? settings.cloudModel : result.models[0].id)
        }
      }

      setUsername('')
      setPassword('')
      notifications.push({
        level: 'success',
        title: 'Login successful',
        message: `Welcome, ${result.user?.name ?? result.user?.username}`,
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
    notifications.push({
      level: 'info',
      title: 'Logged out',
      message: 'Switched to local mode',
      actions: [],
      dismissible: true,
      autoHide: 3,
      context: 'toast',
    })
  }

  const handleRefreshModels = async () => {
    try {
      const models = await getCloudModels()
      auth.setCloudModels(models)
    } catch (err) {
      console.error('Failed to refresh models:', err)
      notifications.push({
        level: 'error',
        title: '刷新模型列表失败',
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    }
  }

  // --- Logged in state ---
  if (auth.isLoggedIn) {
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
              Logout
            </Button>
          </div>

          {auth.tenant?.balance && (
            <div
              className="flex items-center gap-1.5 text-xs"
              style={{ color: 'var(--color-text-muted)' }}
            >
              Balance: <span style={{ color: 'var(--color-text-primary)' }}>{auth.tenant.balance}</span>
            </div>
          )}
        </div>

        {/* Cloud model selector */}
        <div className="mb-4">
          <label
            className="mb-1.5 block text-sm font-semibold"
            style={{ color: 'var(--color-text-secondary)' }}
          >
            Cloud Model
          </label>
          <div className="flex items-center gap-2">
            <select
              className="h-9 flex-1 rounded-md border px-3 py-2 text-base outline-none"
              style={{
                background: 'var(--color-bg-main)',
                borderColor: 'var(--color-border)',
                color: 'var(--color-text-primary)',
              }}
              value={auth.selectedCloudModel}
              onChange={async (e) => {
                auth.setSelectedCloudModel(e.target.value)
                // Persist to settings
                try {
                  const settings = await getSettings()
                  await updateSettings({ ...settings, cloudModel: e.target.value })
                  useSettingsStore.getState().setSettings({ cloudModel: e.target.value })
                } catch (err) {
                  console.error('Failed to save cloud model:', err)
                }
              }}
            >
              {auth.cloudModels.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
            <Button variant="secondary" onClick={handleRefreshModels}>
              Refresh
            </Button>
          </div>
        </div>

        <div
          className="rounded-md px-3 py-2 text-xs"
          style={{
            background: 'var(--color-primary-subtle)',
            color: 'var(--color-primary)',
          }}
        >
          Cloud mode active. LLM and search requests are routed through the server.
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
          className="mb-3 text-sm font-semibold"
          style={{ color: 'var(--color-text-secondary)' }}
        >
          Login to Cloud
        </div>
        <div
          className="mb-3 text-xs"
          style={{ color: 'var(--color-text-muted)' }}
        >
          Login to use cloud LLM and search without configuring API keys.
        </div>

        <div className="mb-3">
          <input
            type="text"
            className="mb-2 h-9 w-full rounded-md border px-3 py-2 text-base outline-none"
            style={{
              background: 'var(--color-bg-main)',
              borderColor: 'var(--color-border)',
              color: 'var(--color-text-primary)',
            }}
            placeholder="Username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleLogin()}
          />
          <input
            type="password"
            className="h-9 w-full rounded-md border px-3 py-2 text-base outline-none"
            style={{
              background: 'var(--color-bg-main)',
              borderColor: 'var(--color-border)',
              color: 'var(--color-text-primary)',
            }}
            placeholder="Password"
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
          disabled={loading || !username.trim() || !password}
        >
          {loading ? 'Logging in...' : 'Login'}
        </Button>
      </div>
    </div>
  )
}

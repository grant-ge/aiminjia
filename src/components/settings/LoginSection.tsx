import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/common/Button'
import { useAuthStore } from '@/stores/authStore'
// TODO: 租户皮肤下发暂时禁用，改用前端强调色选择器
// import { useBrandingStore } from '@/stores/brandingStore'
import { useNotificationStore } from '@/stores/notificationStore'

interface LoginSectionProps {
  onLoginSuccess?: () => void
}

export function LoginSection({ onLoginSuccess }: LoginSectionProps) {
  const { t } = useTranslation()
  const auth = useAuthStore()
  const notifications = useNotificationStore()

  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')

  const handleLogin = async () => {
    if (!username.trim() || !password) {
      setError(t('login.fillAllFields'))
      return
    }

    setError('')
    try {
      await auth.login(username.trim(), password)
      // TODO: 租户皮肤下发暂时禁用，改用前端强调色选择器
      // useBrandingStore.getState().applyBranding(useAuthStore.getState().tenant ?? null)
      setUsername('')
      setPassword('')
      notifications.push({
        level: 'success',
        title: t('login.loginSuccess'),
        message: t('login.welcome', { name: useAuthStore.getState().user?.name ?? useAuthStore.getState().user?.username }),
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
      onLoginSuccess?.()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  const handleLogout = async () => {
    await auth.logout()
    // TODO: 租户皮肤重置暂时禁用，改用前端强调色选择器
    // useBrandingStore.getState().reset()
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

  if (auth.isLoggedIn) {
    return (
      <div className="space-y-4">
        <div
          className="rounded-lg border p-4"
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
                <div className="text-sm font-medium" style={{ color: 'var(--color-text-primary)' }}>
                  {auth.user?.name ?? auth.user?.username}
                </div>
                <div className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
                  {auth.tenant?.name}
                </div>
              </div>
            </div>
            <Button variant="secondary" onClick={handleLogout}>
              {t('login.logout')}
            </Button>
          </div>
          <div className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
            当前已使用企业账号登录，模型与租户皮肤将按登录态自动恢复。
          </div>
        </div>
      </div>
    )
  }

  return (
    <div>
      <div
        className="rounded-lg border p-4"
        style={{
          background: 'var(--color-bg-main)',
          borderColor: 'var(--color-border)',
        }}
      >
        <div className="mb-2 text-sm font-semibold" style={{ color: 'var(--color-text-secondary)' }}>
          {t('login.loginTitle')}
        </div>
        <div className="mb-3 text-xs" style={{ color: 'var(--color-text-muted)' }}>
          {t('login.loginDesc')}
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

        {error ? (
          <div className="mb-3 text-xs" style={{ color: 'var(--color-semantic-red)' }}>
            {error}
          </div>
        ) : null}

        <Button variant="primary" onClick={handleLogin} disabled={auth.isAuthPending}>
          {auth.isAuthPending ? t('login.loggingIn') : t('login.loginButton')}
        </Button>
      </div>
    </div>
  )
}

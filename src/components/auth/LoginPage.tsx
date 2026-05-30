import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { LoginCard } from '@/components/auth/LoginCard'
import { LoginFooter } from '@/components/auth/LoginFooter'
import { LoginLanguageSwitch } from '@/components/auth/LoginLanguageSwitch'
import { LoginLogoStack } from '@/components/auth/LoginLogoStack'
import { LoginOptionsRow } from '@/components/auth/LoginOptionsRow'
import { RegisterCard } from '@/components/auth/RegisterCard'
import { ResetPasswordCard } from '@/components/auth/ResetPasswordCard'
import { LegalDocumentDialog } from '@/components/legal/LegalDocumentDialog'
import { getLegalDocument, type LegalDocumentKey } from '@/components/legal/legalDocuments'
import { TitleBar } from '@/components/layout/TitleBar'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'

const REMEMBER_KEY = 'login_remembered_username'
const PHONE_LIKE_REGEX = /^\+?[1-9]\d{1,14}$/

function getSavedUsername() {
  return localStorage.getItem(REMEMBER_KEY) ?? ''
}

export function LoginPage() {
  const { t, i18n } = useTranslation()
  const login = useAuthStore((s) => s.login)
  const isAuthPending = useAuthStore((s) => s.isAuthPending)
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const [mode, setMode] = useState<'login' | 'register' | 'resetPassword'>('login')
  const [account, setAccount] = useState(getSavedUsername())
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(() => !!localStorage.getItem(REMEMBER_KEY))
  const [error, setError] = useState('')
  const [info, setInfo] = useState('')
  const [activeLegalDocument, setActiveLegalDocument] = useState<LegalDocumentKey | null>(null)
  const [appVersion, setAppVersion] = useState('')

  useEffect(() => {
    let cancelled = false
    import('@tauri-apps/api/app')
      .then(({ getVersion }) => getVersion())
      .then((v) => {
        if (!cancelled) setAppVersion(v)
      })
      .catch(() => {
        // Outside Tauri (vitest/jsdom) — leave blank, footer will fall back gracefully.
      })
    return () => {
      cancelled = true
    }
  }, [])

  async function handleSubmit(event: { preventDefault: () => void }) {
    event.preventDefault()
    const username = account.trim()
    if (!username) {
      setError(t('login.fillAllFields'))
      return
    }
    try {
      setError('')
      setInfo('')
      await login(username, password)
      if (remember) {
        localStorage.setItem(REMEMBER_KEY, username)
      } else {
        localStorage.removeItem(REMEMBER_KEY)
      }
    } catch (err) {
      setPassword('')
      const message = err instanceof Error ? err.message : t('login.loginFailed')
      setError(PHONE_LIKE_REGEX.test(username) ? t('login.phoneLoginFailedHint') : message)
    }
  }

  function handleRegisterSuccess(identifier: string, pwd: string) {
    setAccount(identifier)
    setPassword(pwd)
    setMode('login')
    setError('')
    setInfo('')
    // Auto-login with the freshly registered credentials.
    void (async () => {
      try {
        await login(identifier, pwd)
        if (remember) {
          localStorage.setItem(REMEMBER_KEY, identifier)
        }
      } catch (err) {
        setPassword('')
        setError(err instanceof Error ? err.message : t('login.loginFailed'))
      }
    })()
  }

  function handleResetPasswordSuccess(identifier: string) {
    setAccount(identifier)
    setPassword('')
    setMode('login')
    setError('')
    setInfo(t('resetPassword.resetSuccess'))
  }

  const legalDocument = activeLegalDocument
    ? getLegalDocument(activeLegalDocument, i18n.language)
    : null
  const footerText = appVersion
    ? `${productName} ${t('login.version', { version: appVersion })} · ${t('login.footerCopyright')}`
    : `${productName} · ${t('login.footerCopyright')}`
  const accountHint = PHONE_LIKE_REGEX.test(account.trim())
    ? t('login.phoneLoginHint')
    : t('login.accountHint')

  return (
    <div
      className="relative flex h-screen w-full flex-col overflow-hidden"
      style={{
        background:
          'linear-gradient(135deg, var(--background) 0%, var(--brand-primary-subtle) 46%, var(--primary-on-bg-10) 100%)',
      }}
    >
      {/* Window chrome: drag region + (Windows) minimize/maximize/close controls.
          Pre-auth the main app shell (which owns the only other TitleBar) is not
          mounted, and native window decorations are disabled on Windows — without
          this the login/register screen has no way to close or minimize. */}
      <TitleBar />
      {/* Pre-auth language toggle: pinned top-right under the title bar so it
          stays in place for both the login and register cards. */}
      <div className="absolute right-4 top-11 z-10">
        <LoginLanguageSwitch />
      </div>
      <div className="relative flex min-h-0 w-full flex-1 flex-col items-center justify-center gap-6 overflow-y-auto px-6">
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -left-32 -top-32 h-[500px] w-[500px] rounded-full opacity-30" style={{ background: 'radial-gradient(circle, var(--primary) 0%, transparent 70%)', filter: 'blur(80px)' }} />
        <div className="absolute -bottom-40 -right-40 h-[600px] w-[600px] rounded-full opacity-20" style={{ background: 'radial-gradient(circle, var(--primary-on-bg-72) 0%, transparent 70%)', filter: 'blur(100px)' }} />
        <div className="absolute left-1/3 top-1/2 h-[400px] w-[400px] -translate-y-1/2 rounded-full opacity-15" style={{ background: 'radial-gradient(circle, var(--primary-on-bg-24) 0%, transparent 70%)', filter: 'blur(90px)' }} />
      </div>
      <LoginLogoStack logoUrl={logoUrl} brandName={productName} />
      {mode === 'login' ? (
        <LoginCard>
          <div className="flex flex-col gap-1.5">
            <div className="text-xl font-semibold text-foreground">{t('login.loginTo', { name: productName })}</div>
            <div className="text-sm text-muted-foreground">{t('login.continueWithAccount')}</div>
          </div>
          <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="account">{t('login.account')}</Label>
              <Input
                id="account"
                aria-label={t('login.account')}
                placeholder={t('login.accountPlaceholder')}
                value={account}
                onChange={(e) => setAccount(e.target.value)}
                autoComplete="username"
              />
              <div className="text-xs text-muted-foreground">{accountHint}</div>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="password">{t('login.password')}</Label>
              <Input
                id="password"
                type="password"
                placeholder={t('login.enterPassword')}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="current-password"
              />
            </div>
            <LoginOptionsRow
              rememberSlot={
                <label className="flex items-center gap-2 text-sm text-foreground">
                  <input
                    type="checkbox"
                    checked={remember}
                    onChange={(e) => setRemember(e.target.checked)}
                    className="h-4 w-4 shrink-0 cursor-pointer appearance-none rounded border border-border bg-background transition-colors checked:border-primary checked:bg-primary"
                    ref={(el) => {
                      if (!el) return
                      const update = () => {
                        el.style.backgroundImage = el.checked
                          ? `url("data:image/svg+xml,%3Csvg viewBox='0 0 16 16' fill='white' xmlns='http://www.w3.org/2000/svg'%3E%3Cpath d='M12.207 4.793a1 1 0 0 1 0 1.414l-5 5a1 1 0 0 1-1.414 0l-2-2a1 1 0 0 1 1.414-1.414L6.5 9.086l4.293-4.293a1 1 0 0 1 1.414 0z'/%3E%3C/svg%3E")`
                          : 'none'
                      }
                      update()
                      el.addEventListener('change', update)
                    }}
                  />
                  {t('login.rememberMe')}
                </label>
              }
              onForget={() => {
                setMode('resetPassword')
                setError('')
                setInfo('')
              }}
            />
            {info ? (
              <div className="text-sm text-primary">{info}</div>
            ) : null}
            {error ? (
              <div data-aijia-login-error className="text-sm text-destructive">{error}</div>
            ) : null}
            <Button
              type="submit"
              size="lg"
              disabled={isAuthPending}
              className="w-full"
            >
              {isAuthPending ? (
                <span className="flex items-center justify-center gap-2">
                  <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none">
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                  </svg>
                  {t('login.loggingIn')}
                </span>
              ) : t('login.loginButtonLabel')}
            </Button>
            <div className="flex items-center justify-center gap-1 text-sm text-muted-foreground">
              <span>{t('login.noAccount')}</span>
              <button
                type="button"
                className="font-medium text-primary underline-offset-4 hover:underline"
                onClick={() => {
                  setMode('register')
                  setError('')
                  setInfo('')
                }}
              >
                {t('login.registerNow')}
              </button>
            </div>
            <div className="text-center text-xs text-muted-foreground">
              {t('login.agreeByLogin')}
              <button
                type="button"
                className="mx-0.5 font-medium text-primary underline-offset-4 hover:underline"
                onClick={() => setActiveLegalDocument('terms')}
              >
                {t('login.termsOfService')}
              </button>
              {t('login.and')}
              <button
                type="button"
                className="mx-0.5 font-medium text-primary underline-offset-4 hover:underline"
                onClick={() => setActiveLegalDocument('privacy')}
              >
                {t('login.privacyPolicy')}
              </button>
            </div>
          </form>
        </LoginCard>
      ) : mode === 'register' ? (
        <RegisterCard
          productName={productName}
          onBack={() => {
            setMode('login')
            setError('')
            setInfo('')
          }}
          onSuccess={handleRegisterSuccess}
        />
      ) : (
        <ResetPasswordCard
          onBack={() => {
            setMode('login')
            setError('')
            setInfo('')
          }}
          onSuccess={handleResetPasswordSuccess}
        />
      )}
        <LoginFooter text={footerText} />
      </div>
      <LegalDocumentDialog
        document={legalDocument}
        open={legalDocument !== null}
        onOpenChange={(open) => {
          if (!open) setActiveLegalDocument(null)
        }}
      />
    </div>
  )
}

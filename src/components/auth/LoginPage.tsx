import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { LoginCard } from '@/components/auth/LoginCard'
import { LoginFooter } from '@/components/auth/LoginFooter'
import { LoginLogoStack } from '@/components/auth/LoginLogoStack'
import { LoginOptionsRow } from '@/components/auth/LoginOptionsRow'
import { LegalDocumentDialog } from '@/components/legal/LegalDocumentDialog'
import { LEGAL_DOCUMENTS, type LegalDocumentKey } from '@/components/legal/legalDocuments'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'

const REMEMBER_KEY = 'login_remembered_username'

function getSavedUsername() {
  return localStorage.getItem(REMEMBER_KEY) ?? ''
}

function splitUsername(full: string): [string, string] {
  const idx = full.indexOf('@')
  if (idx === -1) return [full, '']
  return [full.slice(0, idx), full.slice(idx + 1)]
}

export function LoginPage() {
  const { t } = useTranslation()
  const login = useAuthStore((s) => s.login)
  const isAuthPending = useAuthStore((s) => s.isAuthPending)
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const [userPrefix, userSuffix] = splitUsername(getSavedUsername())
  const [usernamePrefix, setUsernamePrefix] = useState(userPrefix)
  const [usernameSuffix, setUsernameSuffix] = useState(userSuffix)
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(() => !!localStorage.getItem(REMEMBER_KEY))
  const [error, setError] = useState('')
  const [activeLegalDocument, setActiveLegalDocument] = useState<LegalDocumentKey | null>(null)

  async function handleSubmit(event: { preventDefault: () => void }) {
    event.preventDefault()
    const username = `${usernamePrefix.trim()}@${usernameSuffix.trim()}`
    try {
      setError('')
      await login(username, password)
      if (remember) {
        localStorage.setItem(REMEMBER_KEY, username)
      } else {
        localStorage.removeItem(REMEMBER_KEY)
      }
    } catch (err) {
      setPassword('')
      setError(err instanceof Error ? err.message : t('login.loginFailed'))
    }
  }

  const legalDocument = activeLegalDocument ? LEGAL_DOCUMENTS[activeLegalDocument] : null

  return (
    <div
      className="relative flex min-h-screen w-full flex-col items-center justify-center gap-6 overflow-hidden px-6"
      style={{
        background:
          'linear-gradient(135deg, var(--background) 0%, var(--brand-primary-subtle) 46%, color-mix(in srgb, var(--primary) 10%, var(--background)) 100%)',
      }}
    >
      <div data-tauri-drag-region className="absolute inset-x-0 top-0 h-8 z-10" />
      {/* Background glow */}
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -left-32 -top-32 h-[500px] w-[500px] rounded-full opacity-30" style={{ background: 'radial-gradient(circle, var(--primary) 0%, transparent 70%)', filter: 'blur(80px)' }} />
        <div className="absolute -bottom-40 -right-40 h-[600px] w-[600px] rounded-full opacity-20" style={{ background: 'radial-gradient(circle, color-mix(in srgb, var(--primary) 72%, var(--background)) 0%, transparent 70%)', filter: 'blur(100px)' }} />
        <div className="absolute left-1/3 top-1/2 h-[400px] w-[400px] -translate-y-1/2 rounded-full opacity-15" style={{ background: 'radial-gradient(circle, color-mix(in srgb, var(--primary) 24%, var(--background)) 0%, transparent 70%)', filter: 'blur(90px)' }} />
      </div>
      <LoginLogoStack logoUrl={logoUrl} brandName={productName} />
      <LoginCard>
        <div className="flex flex-col gap-1.5">
          <div className="text-xl font-semibold text-foreground">{t('login.loginTo', { name: productName })}</div>
          <div className="text-sm text-muted-foreground">{t('login.continueWithEnterprise')}</div>
        </div>
        <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="username-prefix">{t('login.account')}</Label>
            <div className="flex items-center gap-1.5">
              <Input
                id="username-prefix"
                aria-label={t('login.account')}
                placeholder={t('login.username')}
                value={usernamePrefix}
                onChange={(e) => setUsernamePrefix(e.target.value)}
                autoComplete="username"
              />
              <span className="shrink-0 text-sm font-medium text-muted-foreground">@</span>
              <Input
                id="username-suffix"
                aria-label={t('login.orgCode')}
                placeholder={t('login.orgCode')}
                value={usernameSuffix}
                onChange={(e) => setUsernameSuffix(e.target.value)}
                autoComplete="off"
              />
            </div>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="password">{t('login.password')}</Label>
            <Input
              id="password"
              type="password"
              placeholder={t('login.enterPassword')}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
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
            onForget={() => {}}
          />
          {error ? (
            <div className="text-sm text-destructive">{error}</div>
          ) : null}
          <Button
            type="submit"
            disabled={isAuthPending}
            className="w-full rounded-full py-3 text-md font-semibold"
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
      <LoginFooter text="AI 小家 v0.9.30 · © 仁励家网络科技(杭州)有限公司" />
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

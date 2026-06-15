import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { LoginCard } from '@/components/auth/LoginCard'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cloudResetPassword, cloudSendEmailCode, cloudSendSmsCode } from '@/lib/tauri'
import { Button } from '@/components/ui/button'

const PHONE_REGEX = /^1[3-9]\d{9}$/
const EMAIL_REGEX = /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/
const COUNTDOWN_SECONDS = 60

type Method = 'phone' | 'email'

interface ResetPasswordCardProps {
  onBack: () => void
  onSuccess: (identifier: string) => void
}

export function ResetPasswordCard({ onBack, onSuccess }: ResetPasswordCardProps) {
  const { t } = useTranslation()
  const [method, setMethod] = useState<Method>('phone')
  const [phone, setPhone] = useState('')
  const [email, setEmail] = useState('')
  const [code, setCode] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [info, setInfo] = useState('')
  const [countdown, setCountdown] = useState(0)
  const [sending, setSending] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null)

  useEffect(() => {
    return () => {
      if (timerRef.current) clearInterval(timerRef.current)
    }
  }, [])

  function startCountdown() {
    setCountdown(COUNTDOWN_SECONDS)
    if (timerRef.current) clearInterval(timerRef.current)
    timerRef.current = setInterval(() => {
      setCountdown((c) => {
        if (c <= 1) {
          if (timerRef.current) clearInterval(timerRef.current)
          return 0
        }
        return c - 1
      })
    }, 1000)
  }

  function currentIdentifier() {
    return method === 'phone' ? phone.trim() : email.trim()
  }

  function validateIdentifier(identifier: string) {
    if (method === 'phone' && !PHONE_REGEX.test(identifier)) {
      setError(t('resetPassword.invalidPhone'))
      return false
    }
    if (method === 'email' && !EMAIL_REGEX.test(identifier)) {
      setError(t('resetPassword.invalidEmail'))
      return false
    }
    return true
  }

  async function handleSendCode() {
    setError('')
    setInfo('')
    const identifier = currentIdentifier()
    if (!validateIdentifier(identifier)) return

    setSending(true)
    try {
      if (method === 'phone') {
        await cloudSendSmsCode(identifier)
      } else {
        await cloudSendEmailCode(identifier)
      }
      setInfo(t('resetPassword.codeSent'))
      startCountdown()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSending(false)
    }
  }

  async function handleSubmit(event: { preventDefault: () => void }) {
    event.preventDefault()
    setError('')
    setInfo('')

    const identifier = currentIdentifier()
    if (!validateIdentifier(identifier)) return
    if (!code.trim()) {
      setError(t('resetPassword.codeRequired'))
      return
    }
    if (password.length < 8) {
      setError(t('resetPassword.passwordTooShort'))
      return
    }

    setSubmitting(true)
    try {
      await cloudResetPassword({
        method,
        phone: method === 'phone' ? identifier : '',
        email: method === 'email' ? identifier : '',
        code: code.trim(),
        password,
      })
      onSuccess(identifier)
    } catch (err) {
      setError(err instanceof Error ? err.message : t('resetPassword.resetFailed'))
    } finally {
      setSubmitting(false)
    }
  }

  function switchMethod(next: Method) {
    if (next === method) return
    setMethod(next)
    setCode('')
    setError('')
    setInfo('')
  }

  const sendBtnLabel = countdown > 0
    ? t('resetPassword.resendIn', { seconds: countdown })
    : sending
      ? t('resetPassword.sending')
      : t('resetPassword.sendCode')

  return (
    <LoginCard>
      <div className="flex flex-col gap-1.5">
        <div className="text-xl font-semibold text-foreground">{t('resetPassword.title')}</div>
        <div className="text-sm text-muted-foreground">{t('resetPassword.subtitle')}</div>
      </div>
      <div className="flex w-full gap-2 rounded-md bg-muted p-1">
        <Button unstyled
          type="button"
          onClick={() => switchMethod('phone')}
          className={`flex-1 rounded-md py-1.5 text-sm font-medium transition-colors ${
            method === 'phone' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          {t('resetPassword.methodPhone')}
        </Button>
        <Button unstyled
          type="button"
          onClick={() => switchMethod('email')}
          className={`flex-1 rounded-md py-1.5 text-sm font-medium transition-colors ${
            method === 'email' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          {t('resetPassword.methodEmail')}
        </Button>
      </div>
      <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
        {method === 'phone' ? (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="reset-phone">{t('resetPassword.phone')}</Label>
            <Input
              id="reset-phone"
              type="tel"
              autoComplete="tel"
              placeholder={t('resetPassword.phonePlaceholder')}
              value={phone}
              onChange={(e) => setPhone(e.target.value)}
            />
          </div>
        ) : (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="reset-email">{t('resetPassword.email')}</Label>
            <Input
              id="reset-email"
              type="email"
              autoComplete="email"
              placeholder={t('resetPassword.emailPlaceholder')}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </div>
        )}
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="reset-code">{t('resetPassword.code')}</Label>
          <div className="flex items-center gap-2">
            <Input
              id="reset-code"
              autoComplete="one-time-code"
              placeholder={t('resetPassword.codePlaceholder')}
              value={code}
              onChange={(e) => setCode(e.target.value)}
            />
            <Button
              type="button"
              variant="outline"
              disabled={sending || countdown > 0}
              onClick={() => void handleSendCode()}
              className="shrink-0 whitespace-nowrap"
            >
              {sendBtnLabel}
            </Button>
          </div>
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="reset-password">{t('resetPassword.newPassword')}</Label>
          <Input
            id="reset-password"
            type="password"
            autoComplete="new-password"
            placeholder={t('resetPassword.passwordPlaceholder')}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        </div>
        {info ? <div className="text-sm text-primary">{info}</div> : null}
        {error ? <div className="text-sm text-destructive">{error}</div> : null}
        <Button type="submit" size="lg" disabled={submitting} className="w-full">
          {submitting ? t('resetPassword.resetting') : t('resetPassword.reset')}
        </Button>
      </form>
      <div className="flex items-center justify-center gap-1 text-sm text-muted-foreground">
        <span>{t('login.haveAccount')}</span>
        <Button unstyled
          type="button"
          className="font-medium text-primary underline-offset-4 hover:underline"
          onClick={onBack}
        >
          {t('login.backToLogin')}
        </Button>
      </div>
    </LoginCard>
  )
}

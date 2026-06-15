import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { LoginCard } from '@/components/auth/LoginCard'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cloudRegister, cloudSendEmailCode, cloudSendSmsCode } from '@/lib/tauri'
import { Button } from '@/components/ui/button'

const PHONE_REGEX = /^1[3-9]\d{9}$/
const EMAIL_REGEX = /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/
const COUNTDOWN_SECONDS = 60

type Method = 'phone' | 'email'

interface RegisterCardProps {
  productName: string
  onBack: () => void
  onSuccess: (identifier: string, password: string) => void
}

export function RegisterCard({ productName, onBack, onSuccess }: RegisterCardProps) {
  const { t } = useTranslation()
  const [method, setMethod] = useState<Method>('phone')
  const [phone, setPhone] = useState('')
  const [email, setEmail] = useState('')
  const [code, setCode] = useState('')
  const [name, setName] = useState('')
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

  async function handleSendCode() {
    setError('')
    setInfo('')
    if (method === 'phone') {
      if (!PHONE_REGEX.test(phone.trim())) {
        setError(t('register.invalidPhone'))
        return
      }
    } else {
      if (!EMAIL_REGEX.test(email.trim())) {
        setError(t('register.invalidEmail'))
        return
      }
    }
    setSending(true)
    try {
      if (method === 'phone') {
        await cloudSendSmsCode(phone.trim())
      } else {
        await cloudSendEmailCode(email.trim())
      }
      setInfo(t('register.codeSent'))
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

    const identifier = method === 'phone' ? phone.trim() : email.trim()
    if (method === 'phone' && !PHONE_REGEX.test(identifier)) {
      setError(t('register.invalidPhone'))
      return
    }
    if (method === 'email' && !EMAIL_REGEX.test(identifier)) {
      setError(t('register.invalidEmail'))
      return
    }
    if (!code.trim()) {
      setError(t('register.codeRequired'))
      return
    }
    if (password.length < 8) {
      setError(t('register.passwordTooShort'))
      return
    }

    setSubmitting(true)
    try {
      await cloudRegister({
        method,
        phone: method === 'phone' ? identifier : '',
        email: method === 'email' ? identifier : '',
        code: code.trim(),
        password,
        name: name.trim() || undefined,
      })
      onSuccess(identifier, password)
    } catch (err) {
      setError(err instanceof Error ? err.message : t('register.registerFailed'))
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
    ? t('register.resendIn', { seconds: countdown })
    : sending
      ? t('register.sending')
      : t('register.sendCode')

  return (
    <LoginCard>
      <div className="flex flex-col gap-1.5">
        <div className="text-xl font-semibold text-foreground">{t('register.title', { name: productName })}</div>
        <div className="text-sm text-muted-foreground">{t('register.subtitle')}</div>
      </div>
      <div className="flex w-full gap-2 rounded-md bg-muted p-1">
        <Button unstyled
          type="button"
          onClick={() => switchMethod('phone')}
          className={`flex-1 rounded-md py-1.5 text-sm font-medium transition-colors ${
            method === 'phone' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          {t('register.methodPhone')}
        </Button>
        <Button unstyled
          type="button"
          onClick={() => switchMethod('email')}
          className={`flex-1 rounded-md py-1.5 text-sm font-medium transition-colors ${
            method === 'email' ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          {t('register.methodEmail')}
        </Button>
      </div>
      <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
        {method === 'phone' ? (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="reg-phone">{t('register.phone')}</Label>
            <Input
              id="reg-phone"
              type="tel"
              autoComplete="tel"
              placeholder={t('register.phonePlaceholder')}
              value={phone}
              onChange={(e) => setPhone(e.target.value)}
            />
          </div>
        ) : (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="reg-email">{t('register.email')}</Label>
            <Input
              id="reg-email"
              type="email"
              autoComplete="email"
              placeholder={t('register.emailPlaceholder')}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </div>
        )}
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="reg-code">{t('register.code')}</Label>
          <div className="flex items-center gap-2">
            <Input
              id="reg-code"
              autoComplete="one-time-code"
              placeholder={t('register.codePlaceholder')}
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
          <Label htmlFor="reg-name">{t('register.name')}</Label>
          <Input
            id="reg-name"
            placeholder={t('register.namePlaceholder')}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="reg-password">{t('register.password')}</Label>
          <Input
            id="reg-password"
            type="password"
            autoComplete="new-password"
            placeholder={t('register.passwordPlaceholder')}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        </div>
        {info ? <div className="text-sm text-primary">{info}</div> : null}
        {error ? <div className="text-sm text-destructive">{error}</div> : null}
        <Button
          type="submit"
          size="lg"
          disabled={submitting}
          className="w-full"
        >
          {submitting ? (
            <span className="flex items-center justify-center gap-2">
              <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              {t('register.registering')}
            </span>
          ) : t('register.register')}
        </Button>
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
        <div className="text-center text-xs text-muted-foreground">
          {t('register.personalRegisterOnly')}
        </div>
      </form>
    </LoginCard>
  )
}

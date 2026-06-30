import { useEffect, useRef, useState } from 'react'
import QRCode from 'qrcode'
import { useTranslation } from 'react-i18next'
import { CheckCircle2, ExternalLink, RefreshCw } from 'lucide-react'
import { type ChannelConfigView, type ChannelRegistrationBeginResult } from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'

interface FeishuChannelConfigProps {
  onSaved?: () => void
  onClose?: () => void
}

type RegistrationStatus = 'idle' | 'opening' | 'waiting' | 'success' | 'error'

interface RegisteredCredentials {
  config: ChannelConfigView
}

const REGISTRATION_POLL_GRACE_MS = 3000

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

function CredentialRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-[rgba(var(--muted-rgb),0.25)] px-4 py-3">
      <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{value}</div>
    </div>
  )
}

function QrCodePanel({ value, loading, qrAlt }: { value: string | null; loading: boolean; qrAlt: string }) {
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null)

  useEffect(() => {
    if (!value) {
      setQrDataUrl(null)
      return
    }
    let cancelled = false
    QRCode.toDataURL(value, {
      errorCorrectionLevel: 'M',
      margin: 1,
      width: 224,
      color: {
        dark: '#111111',
        light: '#ffffff',
      },
    })
      .then((url) => {
        if (!cancelled) setQrDataUrl(url)
      })
      .catch(() => {
        if (!cancelled) setQrDataUrl(null)
      })
    return () => {
      cancelled = true
    }
  }, [value])

  return (
    // QR 容器固定白底：保证扫码相机/飞书客户端可识别，不随主题切换
    <div className="relative flex h-60 w-60 items-center justify-center rounded-md border border-border bg-white p-4">
      {qrDataUrl ? (
        <img src={qrDataUrl} alt={qrAlt} className="h-full w-full" />
      ) : (
        // 占位 QR pattern：和外层一致保持白底，黑点是结构性占位
        <div aria-label={qrAlt} className="grid h-full w-full grid-cols-7 grid-rows-7 gap-1 rounded-md bg-white p-2">
          {Array.from({ length: 49 }).map((_, index) => (
            <span
              key={index}
              className={`rounded-md ${[0, 1, 2, 7, 14, 42, 43, 44, 48, 34, 24, 18, 12, 31, 39, 5, 10, 29, 36, 46].includes(index) ? 'bg-black' : 'bg-zinc-100'}`}
            />
          ))}
        </div>
      )}
      {loading && (
        <div className="absolute inset-4 flex items-center justify-center rounded-md bg-[rgba(var(--background-rgb),0.75)] backdrop-blur-[1px]">
          <Spinner size="lg" className="text-primary" />
        </div>
      )}
    </div>
  )
}

export function FeishuChannelConfig({ onSaved, onClose }: FeishuChannelConfigProps) {
  const { t } = useTranslation()
  const [error, setError] = useState<string | null>(null)
  const [registrationStatus, setRegistrationStatus] = useState<RegistrationStatus>('idle')
  const [registrationUrl, setRegistrationUrl] = useState<string | null>(null)
  const [hasRegistrationUrl, setHasRegistrationUrl] = useState(false)
  const [registrationMessage, setRegistrationMessage] = useState<string | null>(null)
  const [credentials, setCredentials] = useState<RegisteredCredentials | null>(null)
  const pollRunIdRef = useRef(0)
  const beginRegistration = useChannelStore((s) => s.beginRegistration)
  const pollRegistrationAction = useChannelStore((s) => s.pollRegistration)
  const setPlatformState = useChannelStore((s) => s.setPlatformState)

  useEffect(() => {
    return () => {
      pollRunIdRef.current += 1
    }
  }, [])

  const pollRegistration = async (begin: ChannelRegistrationBeginResult, runId: number) => {
    const intervalMs = Math.max(1, begin.intervalSeconds || 2) * 1000
    const deadline = Date.now() + Math.max(1, begin.expiresInSeconds || 7200) * 1000 + REGISTRATION_POLL_GRACE_MS

    while (Date.now() <= deadline) {
      if (pollRunIdRef.current !== runId) return
      const result = await pollRegistrationAction('feishu', begin.deviceCode)
      if (pollRunIdRef.current !== runId) return
      if (result.state === 'waiting') {
        setRegistrationMessage(t('channel.feishu.config.waitingHint'))
        await sleep(Math.min(intervalMs, Math.max(0, deadline - Date.now())))
        continue
      }
      if (result.state === 'success') {
        const config = result.config ?? result.platformState?.config
        if (!config) {
          throw new Error(t('channel.feishu.config.errorNoConfig'))
        }
        if (result.platformState) {
          setPlatformState(result.platformState)
        }
        setRegistrationStatus('success')
        setCredentials({ config })
        setRegistrationMessage(t('channel.feishu.config.connected'))
        onSaved?.()
        return
      }
      if (result.state === 'expired') {
        throw new Error(t('channel.feishu.config.errorExpired'))
      }
      if (result.state === 'fail') {
        throw new Error(result.failReason || t('channel.feishu.config.errorScanFailed'))
      }
      throw new Error(t('channel.feishu.config.errorUnknownState'))
    }
    throw new Error(t('channel.feishu.config.errorExpired'))
  }

  const handleStartRegistration = async () => {
    const runId = pollRunIdRef.current + 1
    pollRunIdRef.current = runId
    setError(null)
    setCredentials(null)
    setRegistrationMessage(null)
    setRegistrationStatus('opening')
    setRegistrationUrl(null)
    setHasRegistrationUrl(false)
    try {
      const begin = await beginRegistration('feishu')
      if (pollRunIdRef.current !== runId) return
      setRegistrationUrl(begin.verificationUriComplete)
      setHasRegistrationUrl(true)
      setRegistrationStatus('waiting')
      setRegistrationMessage(t('channel.feishu.config.waitingHint'))
      await pollRegistration(begin, runId)
    } catch (e) {
      if (pollRunIdRef.current !== runId) return
      setRegistrationStatus('error')
      const message = e instanceof Error ? e.message : t('channel.feishu.config.errorBeginFailed')
      setRegistrationMessage(message)
      setError(message)
    }
  }

  useEffect(() => {
    void handleStartRegistration()
    // 组件打开后立即创建注册二维码，只执行一次。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const registrationBusy = registrationStatus === 'opening' || registrationStatus === 'waiting'
  const registrationDone = registrationStatus === 'success'

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold text-foreground">{t('channel.feishu.config.title')}</h2>
        <p className="mt-3 text-sm font-medium text-muted-foreground">{t('channel.feishu.config.subtitle')}</p>
      </div>

      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-5">
          {registrationDone && credentials ? (
            <div className="flex w-full flex-col items-center gap-5">
              <div className="flex w-64 flex-col items-center rounded-md bg-emerald-50 px-8 py-5 text-emerald-500">
                <CheckCircle2 className="h-8 w-8" />
                <div className="mt-3 text-xl font-bold">{t('channel.feishu.config.scanSuccess')}</div>
                <div className="mt-1 text-sm font-semibold">{t('channel.feishu.config.appCreated')}</div>
              </div>
              <div className="grid w-full gap-3 rounded-md border border-border bg-card p-4 text-left">
                <CredentialRow label="AppID" value={credentials.config.appKey} />
                <CredentialRow label="AppSecret" value={credentials.config.appSecretMasked} />
              </div>
              <Button size="sm" variant="secondary" onClick={handleStartRegistration}>
                {t('channel.feishu.config.rescanConfig')}
              </Button>
            </div>
          ) : (
            <div className="flex w-full flex-col items-center gap-4">
              <QrCodePanel value={registrationUrl} loading={registrationStatus === 'opening'} qrAlt={t('channel.feishu.config.qrAlt')} />
              {registrationStatus === 'error' && (
                <Button
                  onClick={handleStartRegistration}
                  size="lg"
                >
                  {t('channel.feishu.config.retryQr')}
                </Button>
              )}
              {hasRegistrationUrl && registrationUrl && registrationBusy && (
                <a
                  href={registrationUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
                >
                  {t('channel.feishu.config.openLinkHint')} <ExternalLink className="h-3 w-3" />
                </a>
              )}
            </div>
          )}

          {registrationMessage && (
            <div
              className={`flex items-center gap-2 rounded-md px-5 py-3 text-sm font-semibold ${
                registrationStatus === 'error'
                  ? 'bg-red-50 text-red-500'
                  : registrationDone
                    ? 'bg-emerald-50 text-emerald-500'
                    : 'bg-muted text-muted-foreground'
              }`}
            >
              {registrationBusy && <RefreshCw className="h-4 w-4 animate-spin" />}
              {registrationMessage}
            </div>
          )}
          {error && !registrationMessage && <p className="text-sm text-red-500">{error}</p>}
        </div>
      </div>

      {registrationDone && (
        <div className="border-t border-border bg-background px-10 py-4">
          <Button
            size="lg"
            block
            onClick={() => {
              onSaved?.()
              onClose?.()
            }}
          >
            {t('channel.actions.done')}
          </Button>
        </div>
      )}
    </div>
  )
}

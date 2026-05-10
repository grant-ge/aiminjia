import { useEffect, useRef, useState } from 'react'
import QRCode from 'qrcode'
import { CheckCircle2, ExternalLink, Loader2, RefreshCw } from 'lucide-react'
import { type ChannelConfigView, type ChannelRegistrationBeginResult } from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'
import { Button } from '@/components/ui/button'

interface ChannelConfigProps {
  onSaved?: () => void
  onClose?: () => void
}

type RegistrationStatus = 'idle' | 'opening' | 'waiting' | 'success' | 'error'

interface RegisteredCredentials {
  config: ChannelConfigView
}

const REGISTRATION_POLL_GRACE_MS = 3000
const QR_PLACEHOLDER_URL = 'https://open-dev.dingtalk.com/openapp/registration/openClaw?source=OPEN_CLAW'

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

function CredentialRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-border bg-muted/25 px-4 py-3">
      <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{value}</div>
    </div>
  )
}

function QrCodePanel({ value, loading }: { value: string; loading: boolean }) {
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null)

  useEffect(() => {
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
    // QR 容器固定白底：保证扫码相机/钉钉客户端可识别，不随主题切换
    <div className="relative flex h-60 w-60 items-center justify-center rounded-3xl border border-border bg-white p-4">
      {qrDataUrl ? (
        <img src={qrDataUrl} alt="钉钉扫码二维码" className="h-full w-full" />
      ) : (
        // 占位 QR pattern：和外层一致保持白底，黑点是结构性占位
        <div aria-label="钉钉扫码二维码" className="grid h-full w-full grid-cols-7 grid-rows-7 gap-1 rounded bg-white p-2">
          {Array.from({ length: 49 }).map((_, index) => (
            <span
              key={index}
              className={`rounded-[2px] ${[0, 1, 2, 7, 14, 42, 43, 44, 48, 34, 24, 18, 12, 31, 39, 5, 10, 29, 36, 46].includes(index) ? 'bg-black' : 'bg-zinc-100'}`}
            />
          ))}
        </div>
      )}
      {loading && (
        <div className="absolute inset-4 flex items-center justify-center rounded-xl bg-background/75 backdrop-blur-[1px]">
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
        </div>
      )}
    </div>
  )
}

export function ChannelConfig({ onSaved, onClose }: ChannelConfigProps) {
  const [error, setError] = useState<string | null>(null)
  const [registrationStatus, setRegistrationStatus] = useState<RegistrationStatus>('idle')
  const [registrationUrl, setRegistrationUrl] = useState<string>(QR_PLACEHOLDER_URL)
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
      const result = await pollRegistrationAction('dingtalk', begin.deviceCode)
      if (pollRunIdRef.current !== runId) return
      if (result.state === 'waiting') {
        setRegistrationMessage('等待你在钉钉页面完成创建，完成后会自动连接。')
        await sleep(Math.min(intervalMs, Math.max(0, deadline - Date.now())))
        continue
      }
      if (result.state === 'success') {
        const config = result.config ?? result.platformState?.config
        if (!config) {
          throw new Error('钉钉已授权，但未返回频道配置')
        }
        if (result.platformState) {
          setPlatformState(result.platformState)
        }
        setRegistrationStatus('success')
        setCredentials({ config })
        setRegistrationMessage('钉钉频道已连接')
        onSaved?.()
        return
      }
      if (result.state === 'expired') {
        throw new Error('扫码开通已过期，请重新发起')
      }
      if (result.state === 'fail') {
        throw new Error(result.failReason || '钉钉扫码开通失败')
      }
      throw new Error('钉钉返回未知开通状态，请重试')
    }
    throw new Error('扫码开通已过期，请重新发起')
  }

  const handleStartRegistration = async () => {
    const runId = pollRunIdRef.current + 1
    pollRunIdRef.current = runId
    setError(null)
    setCredentials(null)
    setRegistrationMessage(null)
    setRegistrationStatus('opening')
    try {
      const begin = await beginRegistration('dingtalk')
      if (pollRunIdRef.current !== runId) return
      setRegistrationUrl(begin.verificationUriComplete)
      setHasRegistrationUrl(true)
      setRegistrationStatus('waiting')
      setRegistrationMessage('等待你在钉钉页面完成创建，完成后会自动连接。')
      await pollRegistration(begin, runId)
    } catch (e) {
      if (pollRunIdRef.current !== runId) return
      setRegistrationStatus('error')
      const message = e instanceof Error ? e.message : '钉钉扫码开通失败，请重试'
      setRegistrationMessage(message)
      setError(message)
    }
  }

  useEffect(() => {
    void handleStartRegistration()
    // 组件打开后立即创建 OPEN_CLAW 二维码，只执行一次。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const registrationBusy = registrationStatus === 'opening' || registrationStatus === 'waiting'
  const registrationDone = registrationStatus === 'success'

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">配置钉钉</h2>
        <p className="mt-3 text-sm font-medium text-muted-foreground">使用钉钉扫码创建应用和机器人，完成后自动读取凭证。</p>
      </div>

      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-5">
          {registrationDone && credentials ? (
            <div className="flex w-full flex-col items-center gap-5">
              <div className="flex w-64 flex-col items-center rounded-xl bg-emerald-50 px-8 py-5 text-emerald-500">
                <CheckCircle2 className="h-8 w-8" />
                <div className="mt-3 text-xl font-bold">扫码开通成功</div>
                <div className="mt-1 text-sm font-semibold">应用已创建</div>
              </div>
              <div className="grid w-full gap-3 rounded-xl border border-border bg-card p-4 text-left">
                <CredentialRow label="AppKey" value={credentials.config.appKey} />
                <CredentialRow label="AppSecret" value={credentials.config.appSecretMasked} />
                <CredentialRow label="RobotCode" value={credentials.config.robotCode} />
              </div>
              <Button size="sm" variant="secondary" onClick={handleStartRegistration}>
                重新扫码更换配置
              </Button>
            </div>
          ) : (
            <div className="flex w-full flex-col items-center gap-4">
              <QrCodePanel value={registrationUrl} loading={registrationStatus === 'opening'} />
              {registrationStatus === 'error' && (
                <Button
                  onClick={handleStartRegistration}
                  className="h-10 w-64 rounded-full"
                >
                  重新生成二维码
                </Button>
              )}
              {hasRegistrationUrl && registrationBusy && (
                <a
                  href={registrationUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
                >
                  页面未自动打开？点击继续 <ExternalLink className="h-3 w-3" />
                </a>
              )}
            </div>
          )}

          {registrationMessage && (
            <div
              className={`flex items-center gap-2 rounded-xl px-5 py-3 text-sm font-semibold ${
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
            className="h-10 w-full rounded-full"
            onClick={() => {
              onSaved?.()
              onClose?.()
            }}
          >
            完成
          </Button>
        </div>
      )}
    </div>
  )
}

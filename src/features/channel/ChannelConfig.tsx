import { useEffect, useRef, useState } from 'react'
import { CheckCircle2 } from 'lucide-react'
import { type ChannelConfigView, type ChannelRegistrationBeginResult } from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'
import { Button } from '@/components/ui/button'
import {
  RegistrationModal,
  type RegistrationPollState,
} from '@/components/registration/RegistrationModal'

interface ChannelConfigProps {
  onSaved?: () => void
  onClose?: () => void
}

interface RegisteredCredentials {
  config: ChannelConfigView
}

function CredentialRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-border bg-muted/25 px-4 py-3">
      <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{value}</div>
    </div>
  )
}

export function ChannelConfig({ onSaved, onClose }: ChannelConfigProps) {
  const [error, setError] = useState<string | null>(null)
  const [begin, setBegin] = useState<ChannelRegistrationBeginResult | null>(null)
  const [credentials, setCredentials] = useState<RegisteredCredentials | null>(null)
  const [attempt, setAttempt] = useState(0)
  const beginRegistration = useChannelStore((s) => s.beginRegistration)
  const pollRegistrationAction = useChannelStore((s) => s.pollRegistration)
  const setPlatformState = useChannelStore((s) => s.setPlatformState)

  // Snapshot deviceCode so RegistrationModal's pollState closure doesn't capture
  // stale begin state across retries.
  const deviceCodeRef = useRef<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setError(null)
    setBegin(null)
    setCredentials(null)
    const run = async () => {
      try {
        const result = await beginRegistration('dingtalk')
        if (cancelled) return
        deviceCodeRef.current = result.deviceCode
        setBegin(result)
      } catch (e) {
        if (cancelled) return
        setError(e instanceof Error ? e.message : '钉钉扫码开通失败，请重试')
      }
    }
    void run()
    return () => {
      cancelled = true
    }
  }, [attempt, beginRegistration])

  // Adapter: backend's pollRegistration -> RegistrationModal's RegistrationPollState
  const pollState = async (): Promise<RegistrationPollState> => {
    const deviceCode = deviceCodeRef.current
    if (!deviceCode) return 'waiting'
    const result = await pollRegistrationAction('dingtalk', deviceCode)
    if (result.state === 'success') {
      const config = result.config ?? result.platformState?.config
      if (!config) {
        setError('钉钉已授权，但未返回频道配置')
        return 'cancelled'
      }
      if (result.platformState) setPlatformState(result.platformState)
      setCredentials({ config })
      onSaved?.()
      return 'confirmed'
    }
    if (result.state === 'expired') return 'expired'
    if (result.state === 'fail') {
      setError(result.failReason || '钉钉扫码开通失败')
      return 'cancelled'
    }
    return 'waiting'
  }

  const handleRetry = () => setAttempt((n) => n + 1)

  if (credentials) {
    return (
      <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
        <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
          <h2 className="text-2xl font-bold tracking-tight text-foreground">配置钉钉</h2>
        </div>
        <div className="flex-1 overflow-y-auto px-10 pb-6">
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
            <Button size="sm" variant="secondary" onClick={handleRetry}>
              重新扫码更换配置
            </Button>
          </div>
        </div>
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
      </div>
    )
  }

  if (error && !begin) {
    return (
      <div className="flex max-h-[78vh] w-full flex-col items-center justify-center bg-background p-10">
        <p className="text-sm text-red-500">{error}</p>
        <Button className="mt-4 h-10 w-64 rounded-full" onClick={handleRetry}>
          重新生成二维码
        </Button>
      </div>
    )
  }

  if (!begin) {
    return (
      <div className="flex max-h-[78vh] w-full flex-col items-center justify-center bg-background p-10 text-sm text-muted-foreground">
        正在准备扫码…
      </div>
    )
  }

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <RegistrationModal
        mode="url"
        title="配置钉钉"
        url={begin.verificationUriComplete}
        userCode={begin.userCode}
        expireSeconds={begin.expiresInSeconds || 7200}
        pollIntervalMs={Math.max(1, begin.intervalSeconds || 2) * 1000}
        pollState={pollState}
        onConfirmed={() => {
          /* credentials already set inside pollState() */
        }}
        onCancel={handleRetry}
      />
      <p className="px-10 pb-4 text-center text-xs font-medium text-muted-foreground">
        等待你在钉钉页面完成创建，完成后会自动连接。
      </p>
    </div>
  )
}

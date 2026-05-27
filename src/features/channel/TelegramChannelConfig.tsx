import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { open as openExternal } from '@tauri-apps/plugin-shell'
import { CheckCircle2, ExternalLink, Loader2, X } from 'lucide-react'
import { QrCodeCanvas } from '@/components/registration/QrCodeCanvas'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  channelTelegramApprovePairing,
  channelTelegramBeginPairing,
  channelTelegramListPairedUsers,
  channelTelegramListPendingPairings,
  channelTelegramRejectPairing,
  channelTelegramRemove,
  channelTelegramRevokeUser,
  channelTelegramSave,
  type TelegramPairedUser,
  type TelegramPairingBeginResult,
  type TelegramPendingPairing,
} from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'
import { useNotificationStore } from '@/stores/notificationStore'

interface TelegramChannelConfigProps {
  onSaved?: () => void
  onClose?: () => void
}

const POLL_INTERVAL_MS = 2000

export function TelegramChannelConfig({ onSaved, onClose }: TelegramChannelConfigProps) {
  const { t } = useTranslation()
  const tgState = useChannelStore((s) => s.platforms.telegram)
  const setPlatformState = useChannelStore((s) => s.setPlatformState)
  const pushNotification = useNotificationStore((s) => s.push)

  const alreadyConfigured = tgState?.configured ?? false

  const [step, setStep] = useState<'token' | 'pairing'>(alreadyConfigured ? 'pairing' : 'token')
  const [token, setToken] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const [begin, setBegin] = useState<TelegramPairingBeginResult | null>(null)
  const [pending, setPending] = useState<TelegramPendingPairing[]>([])
  const [paired, setPaired] = useState<TelegramPairedUser[]>([])
  const [remaining, setRemaining] = useState(0)
  const pollTimerRef = useRef<number | null>(null)
  const expireTimerRef = useRef<number | null>(null)

  const handleSaveToken = async () => {
    const trimmed = token.trim()
    if (!trimmed) {
      setError(t('channel.telegram.config.errorEmptyToken'))
      return
    }
    setSaving(true)
    setError(null)
    try {
      const state = await channelTelegramSave(trimmed)
      setPlatformState(state)
      setStep('pairing')
      onSaved?.()
    } catch (e) {
      setError(e instanceof Error ? e.message : t('channel.telegram.config.errorTokenFailed'))
    } finally {
      setSaving(false)
    }
  }

  const refreshQr = async () => {
    try {
      const r = await channelTelegramBeginPairing()
      setBegin(r)
      setRemaining(r.expiresInSeconds)
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('channel.telegram.config.errorPairingFailed'),
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    }
  }

  const pollPending = async () => {
    try {
      const list = await channelTelegramListPendingPairings()
      setPending(list)
    } catch {
      // 静默；下次轮询继续
    }
  }

  const pollPaired = async () => {
    try {
      const list = await channelTelegramListPairedUsers()
      setPaired(list)
    } catch {
      // 静默；下次轮询继续
    }
  }

  useEffect(() => {
    if (step !== 'pairing') return
    void refreshQr()
    void pollPaired()
    pollTimerRef.current = window.setInterval(() => {
      void pollPending()
      void pollPaired()
    }, POLL_INTERVAL_MS)
    expireTimerRef.current = window.setInterval(() => {
      setRemaining((r) => {
        if (r <= 1) {
          void refreshQr()
          return 0
        }
        return r - 1
      })
    }, 1000)
    return () => {
      if (pollTimerRef.current !== null) window.clearInterval(pollTimerRef.current)
      if (expireTimerRef.current !== null) window.clearInterval(expireTimerRef.current)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step])

  const handleApprove = async (code: string) => {
    try {
      await channelTelegramApprovePairing(code)
      await pollPending()
      await pollPaired()
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('channel.telegram.config.approveFailed'),
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    }
  }

  const handleReject = async (code: string) => {
    try {
      await channelTelegramRejectPairing(code)
      await pollPending()
    } catch {
      // 静默
    }
  }

  const handleRevokeUser = async (userId: number) => {
    const confirmed = await requestConfirm({
      title: t('channel.telegram.config.revokeUserTitle'),
      description: t('channel.telegram.config.revokeUserDesc'),
      confirmLabel: t('channel.actions.confirmRemove'),
      cancelLabel: t('channel.actions.cancel'),
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      const state = await channelTelegramRevokeUser(userId)
      setPlatformState(state)
      await pollPaired()
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('channel.telegram.config.revokeFailed'),
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    }
  }

  const handleRemove = async () => {
    const confirmed = await requestConfirm({
      title: t('channel.remove.telegram.title'),
      description: t('channel.remove.telegram.description'),
      confirmLabel: t('channel.actions.confirmRemove'),
      cancelLabel: t('channel.actions.cancel'),
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      const state = await channelTelegramRemove()
      setPlatformState(state)
      onClose?.()
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('channel.telegram.config.removeFailed'),
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    }
  }

  // ---- render ---------------------------------------------------------------

  if (step === 'token') {
    return (
      <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
        <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
          <h2 className="text-2xl font-bold tracking-tight text-foreground">{t('channel.telegram.config.title')}</h2>
          <p className="mt-3 text-sm font-medium text-muted-foreground">
            {t('channel.telegram.config.tokenSubtitle')}
          </p>
        </div>
        <div className="flex-1 overflow-y-auto px-10 pb-6">
          <div className="flex flex-col gap-3">
            <Button
              type="button"
              variant="secondary"
              className="w-full"
              onClick={() => void openExternal('https://t.me/BotFather')}
            >
              <ExternalLink className="mr-2 h-4 w-4" />
              {t('channel.telegram.config.openBotFather')}
            </Button>
            <label className="text-xs font-semibold text-foreground" htmlFor="tg-token">
              {t('channel.telegram.config.botTokenLabel')} <span className="text-destructive">*</span>
            </label>
            <Input
              id="tg-token"
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="123456789:ABCdef..."
              autoComplete="new-password"
            />
            {error && <p className="text-sm text-destructive">{error}</p>}
            <details className="text-xs text-muted-foreground">
              <summary className="cursor-pointer font-semibold text-foreground">
                {t('channel.telegram.config.botFatherSteps')}
              </summary>
              <ol className="ml-4 mt-2 list-decimal space-y-1">
                <li>{t('channel.telegram.config.step1').split('@BotFather')[0]}<span className="font-mono">@BotFather</span>{t('channel.telegram.config.step1').split('@BotFather')[1]}</li>
                <li>{t('channel.telegram.config.step2').split('/newbot')[0]}<span className="font-mono">/newbot</span>{t('channel.telegram.config.step2').split('/newbot')[1]}</li>
                <li>{t('channel.telegram.config.step3')}</li>
                <li>{t('channel.telegram.config.step4Prefix')}<span className="font-mono">123456:ABC...</span>{t('channel.telegram.config.step4Suffix')}</li>
              </ol>
            </details>
          </div>
        </div>
        <div className="flex gap-3 border-t border-border bg-background px-10 py-4">
          <Button variant="ghost" className="flex-1 rounded-full" onClick={onClose}>
            {t('channel.actions.cancel')}
          </Button>
          <Button
            className="flex-1 rounded-full"
            disabled={!token.trim() || saving}
            onClick={() => void handleSaveToken()}
          >
            {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {t('channel.actions.next')}
          </Button>
        </div>
      </div>
    )
  }

  // step === 'pairing'
  const m = Math.floor(remaining / 60)
  const s = remaining % 60

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">{t('channel.telegram.config.pairingTitle')}</h2>
        <p className="mt-3 text-sm font-medium text-muted-foreground">
          {tgState?.config ? `@${tgState.config.appKey}` : 'Telegram bot'}
        </p>
        <p className="mt-2 text-xs text-muted-foreground">
          {t('channel.telegram.config.pairingScanHint', { methods: t('channel.telegram.config.pairingScanMethods') })}
        </p>
      </div>
      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-4">
          <QrCodeCanvas value={begin?.deepLink ?? ''} loading={false} alt={t('channel.telegram.config.qrAlt')} />
          <div className="text-xs text-muted-foreground">
            {t('channel.telegram.config.qrExpiry', { time: `${m}:${s.toString().padStart(2, '0')}` })}
          </div>
          {begin?.deepLink && (
            <button
              type="button"
              onClick={() => void openExternal(begin.deepLink)}
              className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
            >
              {t('channel.telegram.config.openTelegramDirect')} <ExternalLink className="h-3 w-3" />
            </button>
          )}

          {pending.length > 0 && (
            <div className="flex w-full flex-col gap-2 rounded-xl border border-border bg-card p-3">
              <div className="text-xs font-semibold text-foreground">{t('channel.telegram.config.pendingTitle')}</div>
              {pending.map((p) => (
                <div key={p.code} className="flex items-center justify-between gap-2 rounded-lg bg-muted px-3 py-2 text-sm">
                  <span className="font-semibold text-foreground">
                    {p.firstName}
                    {p.username && <span className="ml-1 text-muted-foreground">@{p.username}</span>}
                  </span>
                  <div className="flex gap-2">
                    <Button size="sm" className="rounded-full" onClick={() => void handleApprove(p.code)}>
                      <CheckCircle2 className="mr-1 h-4 w-4" />
                      {t('channel.telegram.config.approve')}
                    </Button>
                    <Button size="sm" variant="ghost" className="rounded-full" onClick={() => void handleReject(p.code)}>
                      <X className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}

          {paired.length > 0 && (
            <div className="flex w-full flex-col gap-2 rounded-xl border border-border bg-card p-3">
              <div className="text-xs font-semibold text-foreground">{t('channel.telegram.config.pairedUsersTitle')}</div>
              {paired.map((u) => (
                <div key={u.userId} className="flex items-center justify-between gap-2 rounded-lg bg-muted px-3 py-2 text-sm">
                  <span className="font-semibold text-foreground">{u.firstName}</span>
                  <Button size="sm" variant="ghost" className="rounded-full" onClick={() => void handleRevokeUser(u.userId)}>
                    {t('channel.actions.remove')}
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
      <div className="flex gap-3 border-t border-border bg-background px-10 py-4">
        {alreadyConfigured && (
          <Button variant="destructive" className="flex-1 rounded-full" onClick={() => void handleRemove()}>
            {t('channel.actions.removeChannel')}
          </Button>
        )}
        <Button className={`rounded-full ${alreadyConfigured ? 'flex-1' : 'w-full'}`} onClick={onClose}>
          {t('channel.actions.done')}
        </Button>
      </div>
    </div>
  )
}

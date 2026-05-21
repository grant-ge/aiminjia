import { useEffect, useRef, useState } from 'react'
import QRCode from 'qrcode'
import { open as openExternal } from '@tauri-apps/plugin-shell'
import { CheckCircle2, ExternalLink, Loader2, X } from 'lucide-react'
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

function QrPanel({ value }: { value: string | null }) {
  const [dataUrl, setDataUrl] = useState<string | null>(null)
  useEffect(() => {
    if (!value) {
      setDataUrl(null)
      return
    }
    let cancelled = false
    QRCode.toDataURL(value, {
      errorCorrectionLevel: 'M',
      margin: 1,
      width: 224,
      color: { dark: '#111111', light: '#ffffff' },
    })
      .then((url) => {
        if (!cancelled) setDataUrl(url)
      })
      .catch(() => {
        if (!cancelled) setDataUrl(null)
      })
    return () => {
      cancelled = true
    }
  }, [value])

  return (
    // QR scanner requires literal white background; cannot use bg-background (would be invisible in dark mode + QRCode lib already bakes light:#fff into the PNG). Same pattern as WecomChannelConfig QR panel.
    <div className="flex h-60 w-60 items-center justify-center rounded-3xl border border-border bg-white p-4">
      {dataUrl ? (
        <img src={dataUrl} alt="Telegram 扫码配对" className="h-full w-full" />
      ) : (
        <Loader2 className="h-8 w-8 animate-spin text-primary" />
      )}
    </div>
  )
}

export function TelegramChannelConfig({ onSaved, onClose }: TelegramChannelConfigProps) {
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
      setError('请输入 bot token')
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
      setError(e instanceof Error ? e.message : 'token 验证失败')
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
        title: '生成配对码失败',
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
        title: '批准失败',
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
      title: '移除该 Telegram 用户？',
      description: '用户将不再能与你的 bot 对话；需要重新扫码才能恢复连接。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
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
        title: '移除失败',
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
      title: '移除 Telegram 频道？',
      description: '会删除本地保存的 bot token 和已配对用户列表。已有聊天历史保留。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
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
        title: '移除失败',
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
          <h2 className="text-2xl font-bold tracking-tight text-foreground">配置 Telegram</h2>
          <p className="mt-3 text-sm font-medium text-muted-foreground">
            在 Telegram 中找到 @BotFather 创建 bot，将拿到的 token 粘贴到下面。
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
              打开 BotFather
            </Button>
            <label className="text-xs font-semibold text-foreground" htmlFor="tg-token">
              Bot Token <span className="text-destructive">*</span>
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
                BotFather 使用步骤
              </summary>
              <ol className="ml-4 mt-2 list-decimal space-y-1">
                <li>在手机或桌面 Telegram 搜索 <span className="font-mono">@BotFather</span> 并打开对话</li>
                <li>发送 <span className="font-mono">/newbot</span></li>
                <li>按提示输入 bot 名称（可中文）和 username（必须以 _bot 结尾）</li>
                <li>把返回的 token（形如 <span className="font-mono">123456:ABC...</span>）复制到上面</li>
              </ol>
            </details>
          </div>
        </div>
        <div className="flex gap-3 border-t border-border bg-background px-10 py-4">
          <Button variant="ghost" className="flex-1 rounded-full" onClick={onClose}>
            取消
          </Button>
          <Button
            className="flex-1 rounded-full"
            disabled={!token.trim() || saving}
            onClick={() => void handleSaveToken()}
          >
            {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            下一步
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
        <h2 className="text-2xl font-bold tracking-tight text-foreground">扫码配对</h2>
        <p className="mt-3 text-sm font-medium text-muted-foreground">
          {tgState?.config ? `@${tgState.config.appKey}` : 'Telegram bot'}
        </p>
        <p className="mt-2 text-xs text-muted-foreground">
          用 <span className="font-semibold text-foreground">手机相机 / 微信扫一扫 / 支付宝扫一扫</span> 扫描下方二维码（Telegram App 自身不支持扫这种码）
        </p>
      </div>
      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-4">
          <QrPanel value={begin?.deepLink ?? null} />
          <div className="text-xs text-muted-foreground">
            二维码 {m}:{s.toString().padStart(2, '0')} 后过期
          </div>
          {begin?.deepLink && (
            <button
              type="button"
              onClick={() => void openExternal(begin.deepLink)}
              className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
            >
              本机已装 Telegram？直接打开 <ExternalLink className="h-3 w-3" />
            </button>
          )}

          {pending.length > 0 && (
            <div className="flex w-full flex-col gap-2 rounded-xl border border-border bg-card p-3">
              <div className="text-xs font-semibold text-foreground">待批准</div>
              {pending.map((p) => (
                <div key={p.code} className="flex items-center justify-between gap-2 rounded-lg bg-muted px-3 py-2 text-sm">
                  <span className="font-semibold text-foreground">
                    {p.firstName}
                    {p.username && <span className="ml-1 text-muted-foreground">@{p.username}</span>}
                  </span>
                  <div className="flex gap-2">
                    <Button size="sm" className="rounded-full" onClick={() => void handleApprove(p.code)}>
                      <CheckCircle2 className="mr-1 h-4 w-4" />
                      批准
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
              <div className="text-xs font-semibold text-foreground">已连接用户</div>
              {paired.map((u) => (
                <div key={u.userId} className="flex items-center justify-between gap-2 rounded-lg bg-muted px-3 py-2 text-sm">
                  <span className="font-semibold text-foreground">{u.firstName}</span>
                  <Button size="sm" variant="ghost" className="rounded-full" onClick={() => void handleRevokeUser(u.userId)}>
                    移除
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
            移除整个频道
          </Button>
        )}
        <Button className={`rounded-full ${alreadyConfigured ? 'flex-1' : 'w-full'}`} onClick={onClose}>
          完成
        </Button>
      </div>
    </div>
  )
}

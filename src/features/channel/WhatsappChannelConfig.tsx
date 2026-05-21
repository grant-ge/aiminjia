import { useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  RegistrationModal,
  type RegistrationPollState,
} from '@/components/registration/RegistrationModal'
import { WhatsappRiskBanner } from './WhatsappRiskBanner'
import { useNotificationStore } from '@/stores/notificationStore'
import {
  channelBeginRegistration,
  channelPollRegistration,
  channelWhatsappUpdateAllowFrom,
} from '@/lib/tauri'

interface Props {
  onSaved?: () => void
  onClose?: () => void
  /** Pass true when WhatsApp is already paired (connection === 'connected' | 'reconnecting'). */
  connected?: boolean
}

type Phase = 'idle' | 'risk_banner' | 'scanning'

interface QrPayload {
  kind: 'qr'
  qr_url: string
  expires_in_seconds: number
}

interface SuccessPayload {
  kind: 'whatsapp_success'
  jid: string
  push_name: string
}

function parseAllowFrom(text: string): { ok: string[]; bad: string[] } {
  const lines = text.split('\n').map((l) => l.trim()).filter((l) => l.length > 0)
  const ok: string[] = []
  const bad: string[] = []
  for (const raw of lines) {
    const cleaned = raw.replace(/[\s-]/g, '')
    const normalized = cleaned.startsWith('+') ? cleaned : `+${cleaned}`
    if (/^\+[1-9]\d{6,14}$/.test(normalized)) ok.push(normalized)
    else bad.push(raw)
  }
  return { ok, bad }
}

/**
 * WhatsApp 频道配置入口（spec v3 §3.6 + §3.8 + §9.1 + §3.10）。
 *
 * 三阶段：
 *   idle        — 显示"添加 WhatsApp 账号"按钮（或已连接时的管理界面）
 *   risk_banner — spec §9.1 一次性风险确认弹窗
 *   scanning    — RegistrationModal mode='qr_url' 扫码 + 倒计时
 *
 * QR string 通过 backend `failReason` JSON envelope 传递（同 wechat 约定）。
 *
 * 当 connected=true 时显示"允许的发送人"管理区域（spec §3.10）。
 */
export function WhatsappChannelConfig({ onSaved, onClose, connected }: Props) {
  const [phase, setPhase] = useState<Phase>('idle')
  const [qrUrl, setQrUrl] = useState<string>('')
  const [expireSec, setExpireSec] = useState<number>(60)
  const [allowFromText, setAllowFromText] = useState('')
  const [saving, setSaving] = useState(false)
  const pushNotification = useNotificationStore((s) => s.push)

  function handleAdd() {
    setPhase('risk_banner')
  }

  async function handleRiskAccepted() {
    setPhase('scanning')
    setQrUrl('')
    try {
      const begin = await channelBeginRegistration('whatsapp')
      setExpireSec(begin.expiresInSeconds)
      // begin.verificationUriComplete is empty by design for whatsapp —
      // first poll returns the real QR via failReason JSON envelope.
    } catch (e) {
      pushNotification({
        level: 'error',
        title: '添加 WhatsApp 失败',
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
      setPhase('idle')
    }
  }

  async function pollOnce(): Promise<RegistrationPollState> {
    try {
      const result = await channelPollRegistration('whatsapp', 'whatsapp')
      if (result.state === 'success') {
        if (result.failReason) {
          try {
            const payload = JSON.parse(result.failReason) as SuccessPayload
            pushNotification({
              level: 'success',
              title: 'WhatsApp 已连接',
              message: `已连接：${payload.push_name}`,
              actions: [],
              dismissible: true,
              autoHide: 4,
              context: 'toast',
            })
          } catch {
            pushNotification({
              level: 'success',
              title: 'WhatsApp 已连接',
              message: '账号已成功接入',
              actions: [],
              dismissible: true,
              autoHide: 4,
              context: 'toast',
            })
          }
        }
        return 'confirmed'
      }
      if (result.state === 'expired') {
        return 'expired'
      }
      // waiting：尝试解析 failReason 拿 QR
      if (result.failReason) {
        try {
          const payload = JSON.parse(result.failReason) as QrPayload
          if (payload.kind === 'qr') {
            setQrUrl(payload.qr_url)
            setExpireSec(payload.expires_in_seconds)
          }
        } catch {
          // failReason 不是 JSON，是错误描述；忽略，等下次 poll
        }
      }
      return 'waiting'
    } catch (e) {
      console.error('[whatsapp] poll failed:', e)
      return 'waiting'
    }
  }

  function handleConfirmed() {
    setPhase('idle')
    setQrUrl('')
    onSaved?.()
    onClose?.()
  }

  function handleCancel() {
    setPhase('idle')
    setQrUrl('')
    onClose?.()
  }

  async function handleSaveAllowFrom() {
    const { ok, bad } = parseAllowFrom(allowFromText)
    if (bad.length > 0) {
      pushNotification({
        level: 'error',
        title: '号码格式无效',
        message: `${bad.length} 个号码格式无效，请使用 E.164 格式`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
      return
    }
    setSaving(true)
    try {
      await channelWhatsappUpdateAllowFrom(ok)
      pushNotification({
        level: 'success',
        title: '已更新',
        message: `已更新允许列表（${ok.length} 项）`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (e) {
      pushNotification({
        level: 'error',
        title: '保存失败',
        message: `保存失败：${String(e)}`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="p-6 space-y-4">
      {phase === 'idle' && (
        <div className="space-y-3">
          <h2 className="text-lg font-semibold text-foreground">添加 WhatsApp 账号</h2>
          <p className="text-sm text-muted-foreground">
            通过手机 WhatsApp 扫码登录，AIjia 将作为已链接设备接入。
          </p>
          <Button onClick={handleAdd}>添加 WhatsApp 账号</Button>
        </div>
      )}

      <WhatsappRiskBanner
        open={phase === 'risk_banner'}
        onAccept={() => void handleRiskAccepted()}
        onCancel={handleCancel}
      />

      {phase === 'scanning' && (
        <RegistrationModal
          mode="qr_url"
          title="扫码登录 WhatsApp"
          qrUrl={qrUrl}
          expireSeconds={expireSec}
          pollState={pollOnce}
          onConfirmed={handleConfirmed}
          onCancel={handleCancel}
        />
      )}

      {phase === 'idle' && (
        <section className="space-y-2 border-t border-border pt-4">
          <h4 className="text-sm font-semibold">允许的发送人（可选）</h4>
          <p className="text-xs text-muted-foreground">
            一行一个 E.164 号码（例如 +8613912345678）。留空表示接收所有联系人。
          </p>
          <textarea
            className="w-full min-h-24 rounded-md border border-input bg-background px-3 py-2 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            placeholder="+8613912345678"
            value={allowFromText}
            onChange={(e) => setAllowFromText(e.target.value)}
            disabled={!connected}
          />
          {!connected && (
            <p className="text-xs text-muted-foreground">未连接时无法保存</p>
          )}
          <div className="flex justify-end">
            <Button size="sm" onClick={() => void handleSaveAllowFrom()} disabled={saving || !connected}>
              {saving ? '保存中...' : '保存'}
            </Button>
          </div>
        </section>
      )}
    </div>
  )
}

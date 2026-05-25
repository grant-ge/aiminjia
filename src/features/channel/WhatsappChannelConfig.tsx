import { useEffect, useMemo, useState } from 'react'
import { ChevronDown, Loader2, Plus, RefreshCcw, Trash2 } from 'lucide-react'
import { AppDropdown } from '@/components/common/AppDropdown'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  RegistrationModal,
  type RegistrationPollState,
} from '@/components/registration/RegistrationModal'
import { WhatsappRiskBanner } from './WhatsappRiskBanner'
import { useChannelStore } from '@/stores/channelStore'
import { useNotificationStore } from '@/stores/notificationStore'
import {
  channelBeginRegistration,
  channelPollRegistration,
  channelWhatsappGetAllowFrom,
  channelWhatsappUpdateAllowFrom,
} from '@/lib/tauri'

interface Props {
  onSaved?: () => void
  onClose?: () => void
  /** Pass true when WhatsApp is already paired (connection === 'connected' | 'reconnecting'). */
  connected?: boolean
}

type Phase = 'idle' | 'risk_banner' | 'scanning'
type AllowMode = 'all' | 'specific'

interface PhoneEntry {
  /** 行 id,用于 React key + 增删稳定。 */
  id: string
  /** E.164 区号,带前导 +,如 `+86`。 */
  country: string
  /** 国家码后的本地段(用户输入,不带 +)。 */
  number: string
}

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

/**
 * 国家码下拉的预置列表。覆盖 AIjia 客户常见地区;不在列表里的用户可手动改
 * Input,所以这不是闭合枚举,只是引导。顺序按国内 → 港澳台 → 海外华语圈 →
 * 主要英语国家 → 欧亚其它的常用度排,不按字母。
 */
const COMMON_COUNTRIES: ReadonlyArray<{ code: string; label: string }> = [
  { code: '+86', label: '中国大陆 (+86)' },
  { code: '+852', label: '中国香港 (+852)' },
  { code: '+853', label: '中国澳门 (+853)' },
  { code: '+886', label: '中国台湾 (+886)' },
  { code: '+65', label: '新加坡 (+65)' },
  { code: '+60', label: '马来西亚 (+60)' },
  { code: '+1', label: '美国 / 加拿大 (+1)' },
  { code: '+44', label: '英国 (+44)' },
  { code: '+81', label: '日本 (+81)' },
  { code: '+82', label: '韩国 (+82)' },
  { code: '+61', label: '澳大利亚 (+61)' },
  { code: '+49', label: '德国 (+49)' },
  { code: '+33', label: '法国 (+33)' },
]

function uid(): string {
  return Math.random().toString(36).slice(2, 10)
}

function emptyRow(country = '+86'): PhoneEntry {
  return { id: uid(), country, number: '' }
}

/**
 * 把后端存的 E.164 字符串(`+8613912345678`)拆成 country + number。
 * 策略:优先 longest-match COMMON_COUNTRIES;失败时把前 1-3 位数字当区号,
 * 兜底也不报错(用户可手动改区号)。
 */
function parseE164(raw: string): PhoneEntry {
  const s = raw.startsWith('+') ? raw : `+${raw}`
  const known = [...COMMON_COUNTRIES]
    .sort((a, b) => b.code.length - a.code.length)
    .find((c) => s.startsWith(c.code))
  if (known) {
    return { id: uid(), country: known.code, number: s.slice(known.code.length) }
  }
  // 未知区号:截前 1-3 位数字,把剩下当 number。
  const m = /^\+(\d{1,3})(\d*)$/.exec(s)
  if (m) {
    return { id: uid(), country: `+${m[1]}`, number: m[2] }
  }
  return { id: uid(), country: '+86', number: s.replace(/^\+/, '') }
}

interface ParsedRows {
  ok: string[]
  errors: Array<{ id: string; reason: string }>
}

/**
 * 把 UI 行拼成 E.164 数组。空行跳过(允许有占位空白行不算错)。
 * 校验:country 必须 `+\d{1,4}`,number 必须 7-14 位纯数字(E.164 总长 8-15)。
 */
function rowsToE164(rows: PhoneEntry[]): ParsedRows {
  const ok: string[] = []
  const errors: ParsedRows['errors'] = []
  for (const row of rows) {
    const country = row.country.trim()
    const number = row.number.replace(/[\s-]/g, '')
    if (!country && !number) continue // 整行空,跳过
    if (!/^\+\d{1,4}$/.test(country)) {
      errors.push({ id: row.id, reason: '区号格式错误(应为 + 后跟 1-4 位数字)' })
      continue
    }
    if (!/^\d{7,14}$/.test(number)) {
      errors.push({ id: row.id, reason: '号码格式错误(应为 7-14 位数字)' })
      continue
    }
    const full = `${country}${number}`
    // E.164 总长 8-15(含 +)
    if (full.length < 8 || full.length > 16) {
      errors.push({ id: row.id, reason: '号码总长不符合 E.164 (8-15 位)' })
      continue
    }
    ok.push(full)
  }
  return { ok, errors }
}

/** Country code popover trigger + 选项菜单。 */
function CountryCodePicker({
  value,
  onChange,
}: {
  value: string
  onChange: (next: string) => void
}) {
  return (
    <AppDropdown
      ariaLabel="选择国家或地区区号"
      contentClassName="max-h-72 overflow-y-auto"
      trigger={
        <button
          type="button"
          className="inline-flex h-9 w-[88px] shrink-0 items-center justify-between gap-1 rounded-md border border-input bg-background px-2 text-sm font-medium text-foreground hover:bg-muted"
        >
          <span>{value}</span>
          <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
        </button>
      }
      items={COMMON_COUNTRIES.map((c) => ({
        id: c.code,
        label: c.label,
        onSelect: () => onChange(c.code),
      }))}
    />
  )
}

/** "● 接收所有" / "○ 仅指定号码" 自定义 radio(项目无 RadioGroup 组件,用按钮组实现)。 */
function ModeRadio({
  value,
  onChange,
}: {
  value: AllowMode
  onChange: (next: AllowMode) => void
}) {
  const Option = ({ kind, label, hint }: { kind: AllowMode; label: string; hint: string }) => {
    const active = value === kind
    return (
      <button
        type="button"
        role="radio"
        aria-checked={active}
        onClick={() => onChange(kind)}
        className="group flex w-full items-start gap-3 rounded-lg border border-border bg-card p-3 text-left transition hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span
          className={`mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full border-2 ${
            active ? 'border-primary' : 'border-muted-foreground/50'
          }`}
        >
          {active && <span className="h-2 w-2 rounded-full bg-primary" />}
        </span>
        <span className="flex flex-col gap-0.5">
          <span className="text-sm font-semibold text-foreground">{label}</span>
          <span className="text-xs text-muted-foreground">{hint}</span>
        </span>
      </button>
    )
  }
  return (
    <div role="radiogroup" className="flex flex-col gap-2">
      <Option
        kind="all"
        label="接收所有联系人"
        hint="任何能发消息给你的人都会触发 AI 回复(默认)"
      />
      <Option
        kind="specific"
        label="仅接收指定号码"
        hint="只对下方列出的号码触发 AI 回复,其它来源不打扰"
      />
    </div>
  )
}

/**
 * WhatsApp 频道配置 / 二次编辑入口。
 *
 * 三阶段:
 *   idle        — 已配对则显示"允许的发送人"管理 UI + 失效时显示重连提示
 *                 未配对则显示"添加 WhatsApp 账号"按钮
 *   risk_banner — spec §9.1 一次性风险确认弹窗
 *   scanning    — RegistrationModal mode='qr_url' 扫码 + 倒计时
 *
 * spec v3 §3.6 / §3.8 / §3.10 / §9.1。
 */
export function WhatsappChannelConfig({ onSaved, onClose, connected }: Props) {
  const [phase, setPhase] = useState<Phase>('idle')
  const [qrUrl, setQrUrl] = useState<string>('')
  const [expireSec, setExpireSec] = useState<number>(60)
  const pushNotification = useNotificationStore((s) => s.push)

  const waState = useChannelStore((s) => s.platforms.whatsapp)
  const setPlatformState = useChannelStore((s) => s.setPlatformState)
  const removePlatform = useChannelStore((s) => s.removePlatform)
  const isConfigured = waState?.configured ?? false
  const needsReauth = waState?.connection === 'needsReauth'

  // ---- allow-list state ----------------------------------------------------
  const [allowMode, setAllowMode] = useState<AllowMode>('all')
  const [rows, setRows] = useState<PhoneEntry[]>([emptyRow()])
  const [allowLoading, setAllowLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [removing, setRemoving] = useState(false)

  useEffect(() => {
    if (!isConfigured) return
    let cancelled = false
    setAllowLoading(true)
    void channelWhatsappGetAllowFrom()
      .then((list) => {
        if (cancelled) return
        if (list === null) {
          // 未配对(并发被移除)。保持默认空行,UI 给"添加账号"流程兜底。
          setAllowMode('all')
          setRows([emptyRow()])
          return
        }
        if (list.length === 0) {
          setAllowMode('all')
          setRows([emptyRow()])
        } else {
          setAllowMode('specific')
          setRows(list.map(parseE164))
        }
      })
      .catch(() => {
        if (!cancelled) {
          // 读不到的话不阻塞 UI,默认"接收所有"。
          setAllowMode('all')
          setRows([emptyRow()])
        }
      })
      .finally(() => {
        if (!cancelled) setAllowLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [isConfigured])

  // ---- pairing flow --------------------------------------------------------

  function handleAddOrRescan() {
    setPhase('risk_banner')
  }

  async function handleRiskAccepted() {
    setPhase('scanning')
    setQrUrl('')
    try {
      const begin = await channelBeginRegistration('whatsapp')
      setExpireSec(begin.expiresInSeconds)
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
              message: `已连接:${payload.push_name}`,
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
      if (result.state === 'expired') return 'expired'
      if (result.failReason) {
        try {
          const payload = JSON.parse(result.failReason) as QrPayload
          if (payload.kind === 'qr') {
            setQrUrl(payload.qr_url)
            setExpireSec(payload.expires_in_seconds)
          }
        } catch {
          // failReason 不是 JSON,可能是错误描述;忽略,等下次 poll。
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
    // 扫码成功不关弹窗,让用户接着配 allow-list。
  }

  function handleScanCancel() {
    setPhase('idle')
    setQrUrl('')
  }

  // ---- row ops -------------------------------------------------------------

  function addRow() {
    // 新增一行继承上一行的区号(常见场景是同国号码批量加)。
    const last = rows[rows.length - 1]
    setRows((rs) => [...rs, emptyRow(last?.country ?? '+86')])
  }

  function removeRow(id: string) {
    setRows((rs) => (rs.length <= 1 ? [emptyRow()] : rs.filter((r) => r.id !== id)))
  }

  function setRowCountry(id: string, country: string) {
    setRows((rs) => rs.map((r) => (r.id === id ? { ...r, country } : r)))
  }

  function setRowNumber(id: string, number: string) {
    // 只允许数字 + 空格 + 短横(粘贴时用户可能带分隔)。保存时再 strip。
    const cleaned = number.replace(/[^\d\s-]/g, '')
    setRows((rs) => rs.map((r) => (r.id === id ? { ...r, number: cleaned } : r)))
  }

  // ---- save / remove -------------------------------------------------------

  const parsed = useMemo<ParsedRows>(() => {
    if (allowMode === 'all') return { ok: [], errors: [] }
    return rowsToE164(rows)
  }, [allowMode, rows])

  async function handleSave() {
    if (allowMode === 'specific' && parsed.errors.length > 0) {
      pushNotification({
        level: 'error',
        title: '号码格式无效',
        message: `${parsed.errors.length} 行号码格式有误,请检查后再保存`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
      return
    }
    setSaving(true)
    try {
      await channelWhatsappUpdateAllowFrom(parsed.ok)
      pushNotification({
        level: 'success',
        title: '已保存',
        message:
          allowMode === 'all'
            ? '已设置为接收所有联系人'
            : `已限制为 ${parsed.ok.length} 个指定号码`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
      onSaved?.()
      onClose?.()
    } catch (e) {
      pushNotification({
        level: 'error',
        title: '保存失败',
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } finally {
      setSaving(false)
    }
  }

  async function handleRemove() {
    const confirmed = await requestConfirm({
      title: '移除 WhatsApp 频道?',
      description: '会删除本地保存的 WhatsApp 会话和允许列表。已有聊天历史保留。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    setRemoving(true)
    try {
      const state = await removePlatform('whatsapp')
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
    } finally {
      setRemoving(false)
    }
  }

  // ---- render --------------------------------------------------------------

  if (phase === 'risk_banner') {
    return (
      <WhatsappRiskBanner
        open
        onAccept={() => void handleRiskAccepted()}
        onCancel={handleScanCancel}
      />
    )
  }

  if (phase === 'scanning') {
    return (
      <RegistrationModal
        mode="qr_url"
        title="扫码登录 WhatsApp"
        qrUrl={qrUrl}
        expireSeconds={expireSec}
        pollState={pollOnce}
        onConfirmed={handleConfirmed}
        onCancel={handleScanCancel}
      />
    )
  }

  // phase === 'idle'
  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">
          {isConfigured ? 'WhatsApp 配置' : '配置 WhatsApp'}
        </h2>
        <p className="mt-3 text-sm font-medium text-muted-foreground">
          {isConfigured
            ? connected
              ? '设置哪些联系人发来的消息会触发 AI 回复'
              : '管理允许列表;会话失效时可重新扫码'
            : '通过手机 WhatsApp 扫码登录,AIjia 将作为已链接设备接入'}
        </p>
      </div>

      <div className="flex-1 space-y-6 overflow-y-auto px-10 pb-6">
        {!isConfigured && (
          <div className="flex justify-center">
            <Button onClick={handleAddOrRescan} className="rounded-full px-6">
              添加 WhatsApp 账号
            </Button>
          </div>
        )}

        {isConfigured && needsReauth && (
          <div className="flex items-start gap-3 rounded-xl border border-border bg-card p-4">
            <div className="flex-1">
              <div className="text-sm font-semibold text-destructive">会话已失效</div>
              <div className="mt-1 text-xs text-muted-foreground">
                手机端可能退出了登录或长时间离线,需要重新扫码配对。
              </div>
            </div>
            <Button
              size="sm"
              variant="secondary"
              className="rounded-full"
              onClick={handleAddOrRescan}
            >
              <RefreshCcw className="mr-1.5 h-3.5 w-3.5" />
              重新扫码
            </Button>
          </div>
        )}

        {isConfigured && (
          <section className="space-y-4">
            <h3 className="text-sm font-semibold text-foreground">允许的发送人</h3>

            {allowLoading ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                读取允许列表…
              </div>
            ) : (
              <>
                <ModeRadio value={allowMode} onChange={setAllowMode} />

                {allowMode === 'specific' && (
                  <div className="space-y-2">
                    {rows.map((row) => {
                      const err = parsed.errors.find((e) => e.id === row.id)
                      return (
                        <div key={row.id} className="space-y-1">
                          <div className="flex items-center gap-2">
                            <CountryCodePicker
                              value={row.country}
                              onChange={(c) => setRowCountry(row.id, c)}
                            />
                            <Input
                              value={row.number}
                              onChange={(e) => setRowNumber(row.id, e.target.value)}
                              placeholder="13912345678"
                              inputMode="tel"
                              className="h-9 flex-1"
                              aria-invalid={err ? true : undefined}
                            />
                            <button
                              type="button"
                              aria-label="删除此行"
                              onClick={() => removeRow(row.id)}
                              className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-destructive"
                            >
                              <Trash2 className="h-4 w-4" />
                            </button>
                          </div>
                          {err && (
                            <p className="px-1 text-xs text-destructive">{err.reason}</p>
                          )}
                        </div>
                      )
                    })}

                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-8 rounded-md px-2 text-primary hover:bg-primary/10 hover:text-primary"
                      onClick={addRow}
                    >
                      <Plus className="mr-1 h-3.5 w-3.5" />
                      添加号码
                    </Button>
                  </div>
                )}
              </>
            )}
          </section>
        )}
      </div>

      <div className="flex gap-3 border-t border-border bg-background px-10 py-4">
        {isConfigured ? (
          <>
            <Button
              variant="destructive"
              className="flex-1 rounded-full"
              onClick={() => void handleRemove()}
              disabled={removing || saving}
            >
              {removing && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              移除整个频道
            </Button>
            <Button
              className="flex-1 rounded-full"
              onClick={() => void handleSave()}
              disabled={saving || removing || allowLoading}
            >
              {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              保存
            </Button>
          </>
        ) : (
          <Button variant="ghost" className="w-full rounded-full" onClick={onClose}>
            取消
          </Button>
        )}
      </div>
    </div>
  )
}

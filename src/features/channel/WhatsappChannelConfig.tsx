import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { ChevronDown, Plus, RefreshCcw, Trash2 } from 'lucide-react'
import { AppDropdown } from '@/components/common/AppDropdown'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { Input } from '@/components/ui/input'
import { Spinner } from '@/components/ui/spinner'
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
import { Button } from '@/components/ui/button'

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
const COUNTRY_CODES = ['+86', '+852', '+853', '+886', '+65', '+60', '+1', '+44', '+81', '+82', '+61', '+49', '+33'] as const

function getCommonCountries(t: TFunction): ReadonlyArray<{ code: string; label: string }> {
  return COUNTRY_CODES.map((code) => ({ code, label: t(`channel.whatsapp.countryCodes.${code}`) }))
}

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
  const known = [...COUNTRY_CODES]
    .sort((a, b) => b.length - a.length)
    .find((c) => s.startsWith(c))
  if (known) {
    return { id: uid(), country: known, number: s.slice(known.length) }
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
      errors.push({ id: row.id, reason: 'channel.whatsapp.allowlist.errorCountryFormat' })
      continue
    }
    if (!/^\d{7,14}$/.test(number)) {
      errors.push({ id: row.id, reason: 'channel.whatsapp.allowlist.errorNumberFormat' })
      continue
    }
    const full = `${country}${number}`
    // E.164 总长 8-15(含 +)
    if (full.length < 8 || full.length > 16) {
      errors.push({ id: row.id, reason: 'channel.whatsapp.allowlist.errorE164Length' })
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
  const { t } = useTranslation()
  const commonCountries = getCommonCountries(t)
  return (
    <AppDropdown
      ariaLabel={t('channel.whatsapp.allowlist.selectCountry')}
      contentClassName="max-h-72 overflow-y-auto"
      trigger={
        <Button unstyled
          type="button"
          className="inline-flex h-9 w-[88px] shrink-0 items-center justify-between gap-1 rounded-md border border-input bg-background px-2 text-sm font-medium text-foreground hover:bg-muted"
        >
          <span>{value}</span>
          <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
        </Button>
      }
      items={commonCountries.map((c) => ({
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
  const { t } = useTranslation()
  const Option = ({ kind, label, hint }: { kind: AllowMode; label: string; hint: string }) => {
    const active = value === kind
    return (
      <Button unstyled
        type="button"
        role="radio"
        aria-checked={active}
        onClick={() => onChange(kind)}
        className="group flex w-full items-start gap-3 rounded-md border border-border bg-card p-3 text-left transition hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span
          className={`mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-md border-2 ${
            active ? 'border-primary' : 'border-[rgba(var(--muted-foreground-rgb),0.50)]'
          }`}
        >
          {active && <span className="h-2 w-2 rounded-md bg-primary" />}
        </span>
        <span className="flex flex-col gap-0.5">
          <span className="text-sm font-semibold text-foreground">{label}</span>
          <span className="text-xs text-muted-foreground">{hint}</span>
        </span>
      </Button>
    )
  }
  return (
    <div role="radiogroup" className="flex flex-col gap-2">
      <Option
        kind="all"
        label={t('channel.whatsapp.allowlist.modeAll')}
        hint={t('channel.whatsapp.allowlist.modeAllHint')}
      />
      <Option
        kind="specific"
        label={t('channel.whatsapp.allowlist.modeSpecific')}
        hint={t('channel.whatsapp.allowlist.modeSpecificHint')}
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
  const { t } = useTranslation()
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
        title: t('channel.whatsapp.config.addFailed'),
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
              title: t('channel.whatsapp.config.connectedTitle'),
              message: t('channel.whatsapp.config.connectedMessage', { name: payload.push_name }),
              actions: [],
              dismissible: true,
              autoHide: 4,
              context: 'toast',
            })
          } catch {
            pushNotification({
              level: 'success',
              title: t('channel.whatsapp.config.connectedTitle'),
              message: t('channel.whatsapp.config.connectedGeneric'),
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
        title: t('channel.whatsapp.allowlist.invalidTitle'),
        message: t('channel.whatsapp.allowlist.invalidMessage', { count: parsed.errors.length }),
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
        title: t('channel.whatsapp.allowlist.savedTitle'),
        message:
          allowMode === 'all'
            ? t('channel.whatsapp.allowlist.savedAll')
            : t('channel.whatsapp.allowlist.savedSpecific', { count: parsed.ok.length }),
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
        title: t('channel.whatsapp.allowlist.saveFailed'),
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
      title: t('channel.remove.whatsapp.title'),
      description: t('channel.remove.whatsapp.description'),
      confirmLabel: t('channel.actions.confirmRemove'),
      cancelLabel: t('channel.actions.cancel'),
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
        title: t('channel.whatsapp.config.removeFailed'),
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
        title={t('channel.whatsapp.config.scanTitle')}
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
        <h2 className="text-2xl font-bold text-foreground">
          {isConfigured ? t('channel.whatsapp.config.titleConfigured') : t('channel.whatsapp.config.titleNew')}
        </h2>
        <p className="mt-3 text-sm font-medium text-muted-foreground">
          {isConfigured
            ? connected
              ? t('channel.whatsapp.config.subtitleConnected')
              : t('channel.whatsapp.config.subtitleDisconnected')
            : t('channel.whatsapp.config.subtitleNew')}
        </p>
      </div>

      <div className="flex-1 space-y-6 overflow-y-auto px-10 pb-6">
        {!isConfigured && (
          <div className="flex justify-center">
            <Button onClick={handleAddOrRescan}>
              {t('channel.whatsapp.config.addAccount')}
            </Button>
          </div>
        )}

        {isConfigured && needsReauth && (
          <div className="flex items-start gap-3 rounded-md border border-border bg-card p-4">
            <div className="flex-1">
              <div className="text-sm font-semibold text-destructive">{t('channel.whatsapp.config.sessionExpired')}</div>
              <div className="mt-1 text-xs text-muted-foreground">
                {t('channel.whatsapp.config.sessionExpiredHint')}
              </div>
            </div>
            <Button
              size="sm"
              variant="secondary"
              icon={<RefreshCcw className="h-3.5 w-3.5" />}
              onClick={handleAddOrRescan}
            >
              {t('channel.whatsapp.config.rescan')}
            </Button>
          </div>
        )}

        {isConfigured && (
          <section className="space-y-4">
            <h3 className="text-sm font-semibold text-foreground">{t('channel.whatsapp.allowlist.title')}</h3>

            {allowLoading ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Spinner />
                {t('channel.whatsapp.allowlist.loading')}
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
                              placeholder={t('channel.whatsapp.allowlist.placeholder')}
                              inputMode="tel"
                              className="h-9 flex-1"
                              aria-invalid={err ? true : undefined}
                            />
                            <Button unstyled
                              type="button"
                              aria-label={t('channel.actions.deleteRow')}
                              onClick={() => removeRow(row.id)}
                              className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-destructive"
                            >
                              <Trash2 className="h-4 w-4" />
                            </Button>
                          </div>
                          {err && (
                            <p className="px-1 text-xs text-destructive">{t(err.reason)}</p>
                          )}
                        </div>
                      )
                    })}

                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      link
                      icon={<Plus className="h-3.5 w-3.5" />}
                      onClick={addRow}
                    >
                      {t('channel.actions.addNumber')}
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
              danger
              className="flex-1"
              loading={removing}
              onClick={() => void handleRemove()}
              disabled={removing || saving}
            >
              {t('channel.actions.removeChannel')}
            </Button>
            <Button
              className="flex-1"
              loading={saving}
              onClick={() => void handleSave()}
              disabled={saving || removing || allowLoading}
            >
              {t('channel.actions.save')}
            </Button>
          </>
        ) : (
          <Button variant="ghost" block onClick={onClose}>
            {t('channel.actions.cancel')}
          </Button>
        )}
      </div>
    </div>
  )
}

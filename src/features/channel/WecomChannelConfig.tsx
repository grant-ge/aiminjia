import { useEffect, useRef, useState } from 'react'
import QRCode from 'qrcode'
import { useTranslation } from 'react-i18next'
import { open as openExternal } from '@tauri-apps/plugin-shell'
import { CheckCircle2, ExternalLink, HelpCircle, Loader2, RefreshCw } from 'lucide-react'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { Input } from '@/components/ui/input'
import {
  channelWecomBeginRegistration,
  channelWecomPollRegistration,
  channelWecomRemove,
  channelWecomSave,
  type WecomBeginResult,
} from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { Button } from '@/components/ui/button'

interface WecomChannelConfigProps {
  onSaved?: () => void
  onClose?: () => void
}

type RegistrationStatus = 'idle' | 'opening' | 'waiting' | 'success' | 'error'

const REGISTRATION_POLL_GRACE_MS = 3000

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
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
    // QR 容器固定白底：保证扫码相机/企微客户端可识别，不随主题切换
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
        <div className="absolute inset-4 flex items-center justify-center rounded-md bg-background/75 backdrop-blur-[1px]">
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
        </div>
      )}
    </div>
  )
}

/**
 * 使用帮助面板：解释「企业微信智能机器人 + 长连接 + 扫码授权」的前提条件、入口
 * 位置和常见 troubleshooting，并提供官方文档跳转入口。
 *
 * 决定折叠在 `<details>` 里而不是默认展开：用户首次配置很可能就能扫上，把这块
 * 默认展开会喧宾夺主；扫不上时再来翻这里也来得及。
 */
function HelpPanel() {
  const { t } = useTranslation()
  const officialDocs: Array<{ label: string; url: string; hint?: string }> = [
    {
      label: t('channel.wecom.help.doc1Label'),
      url: 'https://open.work.weixin.qq.com/help2/pc/21663',
      hint: t('channel.wecom.help.doc1Hint'),
    },
    {
      label: t('channel.wecom.help.doc2Label'),
      url: 'https://open.work.weixin.qq.com/help2/pc/21657',
      hint: t('channel.wecom.help.doc2Hint'),
    },
    {
      label: t('channel.wecom.help.doc3Label'),
      url: 'https://developer.work.weixin.qq.com/document/path/101463',
      hint: t('channel.wecom.help.doc3Hint'),
    },
  ]

  return (
    <details className="w-full rounded-md border border-border bg-muted/30 px-4 py-3 text-sm">
      <summary className="flex cursor-pointer items-center gap-2 font-semibold text-foreground">
        <HelpCircle className="h-4 w-4 text-primary" />
        {t('channel.wecom.help.summary')}
      </summary>

      <div className="mt-3 flex flex-col gap-4 text-xs leading-relaxed text-muted-foreground">
        <div>
          <div className="mb-1 text-[11px] font-bold uppercase tracking-wide text-foreground">
            {t('channel.wecom.help.whoCanScan')}
          </div>
          <p>
            {t('channel.wecom.help.whoCanScanDesc', { authorized: t('channel.wecom.help.whoCanScanAuthorized') })}
            <span className="font-mono"> {t('channel.wecom.help.whoCanScanPath')} </span>
            {t('channel.wecom.help.whoCanScanSuffix')}
          </p>
          <p className="mt-1">
            {t('channel.wecom.help.memberLimit', { memberMax: 20, orgMax: 300 })}
          </p>
        </div>

        <div>
          <div className="mb-1 text-[11px] font-bold uppercase tracking-wide text-foreground">
            {t('channel.wecom.help.preScanCheck')}
          </div>
          <ol className="ml-4 list-decimal space-y-1">
            <li>{t('channel.wecom.help.preScanStep1')}</li>
            <li>{t('channel.wecom.help.preScanStep2')}</li>
            <li>{t('channel.wecom.help.preScanStep3')}</li>
            <li>{t('channel.wecom.help.preScanStep4')}</li>
          </ol>
        </div>

        <div>
          <div className="mb-1 text-[11px] font-bold uppercase tracking-wide text-foreground">
            {t('channel.wecom.help.troubleshoot')}
          </div>
          <ul className="ml-4 list-disc space-y-1">
            <li>{t('channel.wecom.help.troubleshootItem1')}</li>
            <li>{t('channel.wecom.help.troubleshootItem2')}</li>
            <li>{t('channel.wecom.help.troubleshootItem3')}</li>
          </ul>
        </div>

        <div>
          <div className="mb-1 text-[11px] font-bold uppercase tracking-wide text-foreground">
            {t('channel.wecom.help.officialDocs')}
          </div>
          <ul className="space-y-1.5">
            {officialDocs.map((d) => (
              <li key={d.url}>
                <Button unstyled
                  type="button"
                  onClick={() => void openExternal(d.url)}
                  className="group inline-flex items-start gap-1.5 text-left text-primary underline-offset-4 hover:underline"
                >
                  <ExternalLink className="mt-[2px] h-3 w-3 shrink-0" />
                  <span>
                    <span className="font-semibold">{d.label}</span>
                    {d.hint && (
                      <span className="ml-1 text-muted-foreground"> — {d.hint}</span>
                    )}
                  </span>
                </Button>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </details>
  )
}

export function WecomChannelConfig({ onSaved, onClose }: WecomChannelConfigProps) {
  const { t } = useTranslation()
  const wecomState = useChannelStore((s) => s.platforms.wecom)
  const setPlatformState = useChannelStore((s) => s.setPlatformState)
  const pushNotification = useNotificationStore((s) => s.push)

  const alreadyConfigured = wecomState?.configured ?? false

  const [registrationStatus, setRegistrationStatus] = useState<RegistrationStatus>('idle')
  const [registrationMessage, setRegistrationMessage] = useState<string | null>(null)
  const [begin, setBegin] = useState<WecomBeginResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [savedBotId, setSavedBotId] = useState<string | null>(null)
  const pollRunIdRef = useRef(0)

  // ---- Manual fallback state (高级选项：手填 botId/secret) -----------------
  const [manualOpen, setManualOpen] = useState(false)
  const [manualBotId, setManualBotId] = useState('')
  const [manualSecret, setManualSecret] = useState('')
  const [manualDisplayName, setManualDisplayName] = useState('')
  const [manualSaving, setManualSaving] = useState(false)

  useEffect(() => {
    return () => {
      pollRunIdRef.current += 1
    }
  }, [])

  const persistBotInfo = async (botId: string, secret: string, displayName?: string) => {
    const state = await channelWecomSave(botId, secret, displayName)
    setPlatformState(state)
    setSavedBotId(botId)
    return state
  }

  const pollUntilDoneOrTimeout = async (resp: WecomBeginResult, runId: number) => {
    const intervalMs = Math.max(1, resp.intervalSeconds || 3) * 1000
    const deadline =
      Date.now() + Math.max(1, resp.expiresInSeconds || 300) * 1000 + REGISTRATION_POLL_GRACE_MS

    while (Date.now() <= deadline) {
      if (pollRunIdRef.current !== runId) return
      const result = await channelWecomPollRegistration(resp.scode)
      if (pollRunIdRef.current !== runId) return
      if (result.state === 'success') {
        if (!result.botId || !result.secret) {
          throw new Error(t('channel.wecom.config.scanSuccessNoBotInfo'))
        }
        setRegistrationStatus('success')
        setRegistrationMessage(t('channel.wecom.config.scanSuccessSaving'))
        await persistBotInfo(result.botId, result.secret)
        setRegistrationMessage(t('channel.wecom.config.connected'))
        onSaved?.()
        return
      }
      // 其他状态一律 Waiting；按 interval 继续轮询。
      setRegistrationMessage(t('channel.wecom.config.waitingConfirm'))
      await sleep(Math.min(intervalMs, Math.max(0, deadline - Date.now())))
    }
    throw new Error(t('channel.wecom.config.errorTimeout'))
  }

  const handleStartRegistration = async () => {
    const runId = pollRunIdRef.current + 1
    pollRunIdRef.current = runId
    setError(null)
    setRegistrationStatus('opening')
    setBegin(null)
    setRegistrationMessage(null)
    setSavedBotId(null)
    try {
      const resp = await channelWecomBeginRegistration()
      if (pollRunIdRef.current !== runId) return
      setBegin(resp)
      setRegistrationStatus('waiting')
      setRegistrationMessage(t('channel.wecom.config.waitingConfirm'))
      await pollUntilDoneOrTimeout(resp, runId)
    } catch (e) {
      if (pollRunIdRef.current !== runId) return
      setRegistrationStatus('error')
      const msg = e instanceof Error ? e.message : t('channel.wecom.config.errorScanFailed')
      setError(msg)
      setRegistrationMessage(msg)
    }
  }

  useEffect(() => {
    // 已配置过则不自动启动扫码（避免误覆盖现有凭证）
    if (alreadyConfigured) return
    void handleStartRegistration()
    // 仅初次 mount 时启动
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const handleManualSave = async () => {
    if (!manualBotId.trim() || !manualSecret.trim()) {
      pushNotification({
        level: 'error',
        title: t('channel.wecom.config.missingFieldsTitle'),
        message: t('channel.wecom.config.missingFieldsMessage'),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
      return
    }
    setManualSaving(true)
    try {
      await persistBotInfo(
        manualBotId.trim(),
        manualSecret.trim(),
        manualDisplayName.trim() || undefined,
      )
      setRegistrationStatus('success')
      setRegistrationMessage(t('channel.wecom.config.connected'))
      onSaved?.()
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('channel.wecom.config.saveFailedTitle'),
        message: e instanceof Error ? e.message : t('channel.wecom.config.saveFailedMessage'),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setManualSaving(false)
    }
  }

  const handleRemove = async () => {
    const confirmed = await requestConfirm({
      title: t('channel.remove.wecom.title'),
      description: t('channel.remove.wecom.description'),
      confirmLabel: t('channel.actions.confirmRemove'),
      cancelLabel: t('channel.actions.cancel'),
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      const state = await channelWecomRemove()
      setPlatformState(state)
      setSavedBotId(null)
      setRegistrationStatus('idle')
      setRegistrationMessage(null)
      pushNotification({
        level: 'success',
        title: t('channel.wecom.config.removedTitle'),
        message: t('channel.wecom.config.removedMessage'),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('channel.wecom.config.removeFailedTitle'),
        message: e instanceof Error ? e.message : t('channel.wecom.config.removeFailedMessage'),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    }
  }

  const busy = registrationStatus === 'opening' || registrationStatus === 'waiting'
  const done = registrationStatus === 'success'

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold text-foreground">{t('channel.wecom.config.title')}</h2>
        <p className="mt-3 text-sm font-medium text-muted-foreground">
          {t('channel.wecom.config.subtitle')}
        </p>
      </div>

      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-5">
          {done ? (
            <div className="flex w-full flex-col items-center gap-5">
              <div className="flex w-64 flex-col items-center rounded-md bg-emerald-50 px-8 py-5 text-emerald-500">
                <CheckCircle2 className="h-8 w-8" />
                <div className="mt-3 text-xl font-bold">{t('channel.wecom.config.botCreated')}</div>
                <div className="mt-1 text-sm font-semibold">{t('channel.wecom.config.connected')}</div>
              </div>
              {savedBotId && (
                <div className="w-full rounded-md border border-border bg-card px-4 py-3 text-left">
                  <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">Bot ID</div>
                  <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{savedBotId}</div>
                </div>
              )}
              <Button size="sm" variant="secondary" onClick={() => void handleStartRegistration()}>
                {t('channel.wecom.config.rescanBot')}
              </Button>
            </div>
          ) : (
            <div className="flex w-full flex-col items-center gap-4">
              <QrCodePanel value={begin?.authUrl ?? null} loading={registrationStatus === 'opening'} qrAlt={t('channel.wecom.config.qrAlt')} />
              {registrationStatus === 'error' && (
                <Button size="lg" onClick={() => void handleStartRegistration()} className="w-64">
                  {t('channel.wecom.config.retryQr')}
                </Button>
              )}
              {begin?.fallbackUrl && busy && (
                <Button unstyled
                  type="button"
                  onClick={() => void openExternal(begin.fallbackUrl)}
                  className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
                >
                  {t('channel.wecom.config.openInBrowser')} <ExternalLink className="h-3 w-3" />
                </Button>
              )}
            </div>
          )}

          {registrationMessage && (
            <div
              className={`flex items-center gap-2 rounded-md px-5 py-3 text-sm font-semibold ${
                registrationStatus === 'error'
                  ? 'bg-red-50 text-red-500'
                  : done
                    ? 'bg-emerald-50 text-emerald-500'
                    : 'bg-muted text-muted-foreground'
              }`}
            >
              {busy && <RefreshCw className="h-4 w-4 animate-spin" />}
              {registrationMessage}
            </div>
          )}
          {error && !registrationMessage && <p className="text-sm text-red-500">{error}</p>}

          {!done && <HelpPanel />}

          {!done && (
            <details
              className="w-full"
              open={manualOpen}
              onToggle={(e) => setManualOpen((e.target as HTMLDetailsElement).open)}
            >
              <summary className="cursor-pointer text-xs font-medium text-muted-foreground hover:text-foreground">
                {t('channel.wecom.config.manualTitle')}
              </summary>
              <div className="mt-3 flex flex-col gap-3 rounded-md border border-border bg-card p-4">
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-semibold text-foreground" htmlFor="wecom-manual-bot-id">
                    Bot ID <span className="text-destructive">*</span>
                  </label>
                  <Input
                    id="wecom-manual-bot-id"
                    value={manualBotId}
                    onChange={(e) => setManualBotId(e.target.value)}
                    placeholder={t('channel.wecom.config.manualBotIdPlaceholder')}
                    autoComplete="off"
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-semibold text-foreground" htmlFor="wecom-manual-secret">
                    Secret <span className="text-destructive">*</span>
                  </label>
                  <Input
                    id="wecom-manual-secret"
                    type="password"
                    value={manualSecret}
                    onChange={(e) => setManualSecret(e.target.value)}
                    placeholder={t('channel.wecom.config.manualSecretPlaceholder')}
                    autoComplete="new-password"
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-semibold text-foreground" htmlFor="wecom-manual-display">
                    {t('channel.wecom.config.manualDisplayNameLabel')} <span className="text-[10px] font-normal text-muted-foreground">{t('channel.wecom.config.manualDisplayNameOptional')}</span>
                  </label>
                  <Input
                    id="wecom-manual-display"
                    value={manualDisplayName}
                    onChange={(e) => setManualDisplayName(e.target.value)}
                    placeholder={t('channel.wecom.config.manualDisplayNamePlaceholder')}
                    autoComplete="off"
                  />
                </div>
                <Button
                  size="sm"
                  loading={manualSaving}
                  onClick={() => void handleManualSave()}
                  disabled={manualSaving || !manualBotId.trim() || !manualSecret.trim()}
                >
                  {t('channel.wecom.config.manualSave')}
                </Button>
              </div>
            </details>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-3 border-t border-border bg-background px-10 py-4">
        {done ? (
          <Button size="lg" block onClick={onClose}>
            {t('channel.actions.done')}
          </Button>
        ) : (
          <div className="flex gap-3">
            {alreadyConfigured && (
              <Button danger className="flex-1" onClick={() => void handleRemove()}>
                {t('channel.actions.remove')}
              </Button>
            )}
            <Button
              variant="ghost"
              className={alreadyConfigured ? 'flex-1' : undefined}
              block={!alreadyConfigured}
              onClick={onClose}
            >
              {t('channel.actions.close')}
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}

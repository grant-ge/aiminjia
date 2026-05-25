import { useEffect, useRef, useState } from 'react'
import QRCode from 'qrcode'
import { open as openExternal } from '@tauri-apps/plugin-shell'
import { CheckCircle2, ExternalLink, HelpCircle, Loader2, RefreshCw } from 'lucide-react'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { Button } from '@/components/ui/button'
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

interface WecomChannelConfigProps {
  onSaved?: () => void
  onClose?: () => void
}

type RegistrationStatus = 'idle' | 'opening' | 'waiting' | 'success' | 'error'

const REGISTRATION_POLL_GRACE_MS = 3000

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

function QrCodePanel({ value, loading }: { value: string | null; loading: boolean }) {
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
    <div className="relative flex h-60 w-60 items-center justify-center rounded-3xl border border-border bg-white p-4">
      {qrDataUrl ? (
        <img src={qrDataUrl} alt="企业微信扫码二维码" className="h-full w-full" />
      ) : (
        // 占位 QR pattern：和外层一致保持白底，黑点是结构性占位
        <div aria-label="企业微信扫码二维码" className="grid h-full w-full grid-cols-7 grid-rows-7 gap-1 rounded bg-white p-2">
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

/**
 * 使用帮助面板：解释「企业微信智能机器人 + 长连接 + 扫码授权」的前提条件、入口
 * 位置和常见 troubleshooting，并提供官方文档跳转入口。
 *
 * 决定折叠在 `<details>` 里而不是默认展开：用户首次配置很可能就能扫上，把这块
 * 默认展开会喧宾夺主；扫不上时再来翻这里也来得及。
 */
function HelpPanel() {
  const officialDocs: Array<{ label: string; url: string; hint?: string }> = [
    {
      label: '企业微信 · 如何使用智能机器人',
      url: 'https://open.work.weixin.qq.com/help2/pc/21663',
      hint: '创建权限规则 / 数量上限 / 入口位置',
    },
    {
      label: '企业微信 · 接入 OpenClaw 智能机器人指引',
      url: 'https://open.work.weixin.qq.com/help2/pc/21657',
      hint: 'API 模式 + 长连接的官方接入说明',
    },
    {
      label: '企业微信开发者中心 · 智能机器人长连接协议',
      url: 'https://developer.work.weixin.qq.com/document/path/101463',
      hint: '协议细节 / 心跳 / 频控限制（一般用不到）',
    },
  ]

  return (
    <details className="w-full rounded-xl border border-border bg-muted/30 px-4 py-3 text-sm">
      <summary className="flex cursor-pointer items-center gap-2 font-semibold text-foreground">
        <HelpCircle className="h-4 w-4 text-primary" />
        看不到二维码 / 扫不上？查看使用说明
      </summary>

      <div className="mt-3 flex flex-col gap-4 text-xs leading-relaxed text-muted-foreground">
        <div>
          <div className="mb-1 text-[11px] font-bold uppercase tracking-wide text-foreground">
            谁能扫码创建机器人
          </div>
          <p>
            企业超级管理员 + <span className="font-semibold text-foreground">被授权的企业成员</span>。
            默认「全员开放」，但管理员可以在
            <span className="font-mono"> 管理后台 → 安全与管理 → 管理工具 → 智能机器人 → 管理 → 机器人创建权限 </span>
            里改为白名单 / 部门 / 标签。
          </p>
          <p className="mt-1">
            单个普通成员最多可建 <span className="font-semibold text-foreground">20 个</span>，
            单企业最多 <span className="font-semibold text-foreground">300 个</span>。
          </p>
        </div>

        <div>
          <div className="mb-1 text-[11px] font-bold uppercase tracking-wide text-foreground">
            扫码前自查
          </div>
          <ol className="ml-4 list-decimal space-y-1">
            <li>用手机企业微信 App 打开「工作台」</li>
            <li>找到「智能机器人」应用 — 如果连入口都没有，说明管理员关闭了该能力，需要联系管理员开放</li>
            <li>能进入「智能机器人」首页 → 看到「创建智能机器人」按钮可点 → 你就有权限</li>
            <li>回到本弹窗扫码：在企业微信 App 内打开扫一扫即可</li>
          </ol>
        </div>

        <div>
          <div className="mb-1 text-[11px] font-bold uppercase tracking-wide text-foreground">
            扫码后没反应 / 一直「等待中」
          </div>
          <ul className="ml-4 list-disc space-y-1">
            <li>没有点确认创建？回到企业微信 App 看是否有待确认页面</li>
            <li>二维码过期了？5 分钟内有效，点「重新生成二维码」拿新的</li>
            <li>仍然失败？展开下方「手动填写 Bot ID 和 Secret」，让管理员在管理后台建好机器人后把凭证给你</li>
          </ul>
        </div>

        <div>
          <div className="mb-1 text-[11px] font-bold uppercase tracking-wide text-foreground">
            官方文档
          </div>
          <ul className="space-y-1.5">
            {officialDocs.map((d) => (
              <li key={d.url}>
                <button
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
                </button>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </details>
  )
}

export function WecomChannelConfig({ onSaved, onClose }: WecomChannelConfigProps) {
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
          throw new Error('扫码成功但未返回 Bot 信息')
        }
        setRegistrationStatus('success')
        setRegistrationMessage('扫码成功，正在保存配置…')
        await persistBotInfo(result.botId, result.secret)
        setRegistrationMessage('企业微信频道已连接')
        onSaved?.()
        return
      }
      // 其他状态一律 Waiting；按 interval 继续轮询。
      setRegistrationMessage('等待你在企业微信里确认创建机器人…')
      await sleep(Math.min(intervalMs, Math.max(0, deadline - Date.now())))
    }
    throw new Error('扫码超时（5 分钟），请重新生成二维码')
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
      setRegistrationMessage('等待你在企业微信里确认创建机器人…')
      await pollUntilDoneOrTimeout(resp, runId)
    } catch (e) {
      if (pollRunIdRef.current !== runId) return
      setRegistrationStatus('error')
      const msg = e instanceof Error ? e.message : '扫码注册失败，请重试'
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
        title: '缺少必填项',
        message: 'Bot ID 和 Secret 不能为空',
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
      setRegistrationMessage('企业微信频道已连接')
      onSaved?.()
    } catch (e) {
      pushNotification({
        level: 'error',
        title: '保存失败',
        message: e instanceof Error ? e.message : '保存企业微信配置失败，请重试',
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
      title: '移除企业微信频道？',
      description: '这会断开企业微信频道，并删除本地保存的 Bot ID 和 Secret。已有聊天历史会保留。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
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
        title: '企业微信已移除',
        message: '频道配置已删除',
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (e) {
      pushNotification({
        level: 'error',
        title: '移除失败',
        message: e instanceof Error ? e.message : '移除企业微信配置失败，请重试',
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
        <h2 className="text-2xl font-bold tracking-tight text-foreground">配置企业微信</h2>
        <p className="mt-3 text-sm font-medium text-muted-foreground">
          用企业微信扫码即可一键创建机器人，无需进入管理后台。
        </p>
      </div>

      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-5">
          {done ? (
            <div className="flex w-full flex-col items-center gap-5">
              <div className="flex w-64 flex-col items-center rounded-xl bg-emerald-50 px-8 py-5 text-emerald-500">
                <CheckCircle2 className="h-8 w-8" />
                <div className="mt-3 text-xl font-bold">机器人已创建</div>
                <div className="mt-1 text-sm font-semibold">企业微信频道已连接</div>
              </div>
              {savedBotId && (
                <div className="w-full rounded-xl border border-border bg-card px-4 py-3 text-left">
                  <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">Bot ID</div>
                  <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{savedBotId}</div>
                </div>
              )}
              <Button size="sm" variant="secondary" onClick={() => void handleStartRegistration()}>
                重新扫码更换机器人
              </Button>
            </div>
          ) : (
            <div className="flex w-full flex-col items-center gap-4">
              <QrCodePanel value={begin?.authUrl ?? null} loading={registrationStatus === 'opening'} />
              {registrationStatus === 'error' && (
                <Button onClick={() => void handleStartRegistration()} className="h-10 w-64 rounded-full">
                  重新生成二维码
                </Button>
              )}
              {begin?.fallbackUrl && busy && (
                <button
                  type="button"
                  onClick={() => void openExternal(begin.fallbackUrl)}
                  className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
                >
                  无法扫码？在浏览器中打开 <ExternalLink className="h-3 w-3" />
                </button>
              )}
            </div>
          )}

          {registrationMessage && (
            <div
              className={`flex items-center gap-2 rounded-xl px-5 py-3 text-sm font-semibold ${
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
                高级：手动填写 Bot ID 和 Secret
              </summary>
              <div className="mt-3 flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-semibold text-foreground" htmlFor="wecom-manual-bot-id">
                    Bot ID <span className="text-destructive">*</span>
                  </label>
                  <Input
                    id="wecom-manual-bot-id"
                    value={manualBotId}
                    onChange={(e) => setManualBotId(e.target.value)}
                    placeholder="企业微信后台 → 智能机器人 → 详情页"
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
                    placeholder="同上"
                    autoComplete="new-password"
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-semibold text-foreground" htmlFor="wecom-manual-display">
                    显示名称 <span className="text-[10px] font-normal text-muted-foreground">（可选）</span>
                  </label>
                  <Input
                    id="wecom-manual-display"
                    value={manualDisplayName}
                    onChange={(e) => setManualDisplayName(e.target.value)}
                    placeholder="例如：销售群机器人"
                    autoComplete="off"
                  />
                </div>
                <Button
                  size="sm"
                  className="rounded-full"
                  onClick={() => void handleManualSave()}
                  disabled={manualSaving || !manualBotId.trim() || !manualSecret.trim()}
                >
                  {manualSaving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                  保存手动配置
                </Button>
              </div>
            </details>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-3 border-t border-border bg-background px-10 py-4">
        {done ? (
          <Button className="h-10 w-full rounded-full" onClick={onClose}>
            完成
          </Button>
        ) : (
          <div className="flex gap-3">
            {alreadyConfigured && (
              <Button
                variant="destructive"
                className="flex-1 rounded-full"
                onClick={() => void handleRemove()}
              >
                移除
              </Button>
            )}
            <Button
              variant="ghost"
              className={`rounded-full ${alreadyConfigured ? 'flex-1' : 'w-full'}`}
              onClick={onClose}
            >
              关闭
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}

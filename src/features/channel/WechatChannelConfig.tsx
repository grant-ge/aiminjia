import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CheckCircle2 } from 'lucide-react'
import { useChannelStore } from '@/stores/channelStore'
import { Button } from '@/components/ui/button'
import {
  RegistrationModal,
  type RegistrationPollState,
} from '@/components/registration/RegistrationModal'
import type { ChannelRegistrationBeginResult } from '@/lib/tauri'

interface WechatChannelConfigProps {
  onSaved?: () => void
  onClose?: () => void
}

interface WechatLoginInfo {
  ilink_bot_id: string
  ilink_user_id: string
  baseurl: string
}

// ---------------------------------------------------------------------------
// Module-level begin_registration dedup cache.
//
// Why this exists:
//   - React 18 StrictMode mounts WechatChannelConfig twice in dev. Without
//     deduping, every modal open fires 2 backend `begin_registration` calls.
//   - Even outside StrictMode, users routinely click 配置 → close (without
//     scanning) → click again. The first call's await is in flight; closing
//     unmounts the component and our cleanup sets `cancelled=true`, so even
//     when iLink replies the result is dropped. The next mount fires a fresh
//     GET — but if the user closes/reopens fast again, that one also gets
//     dropped. End result: 30 GETs fired, only 4 succeed before the modal sees
//     a `setBegin(result)` call.
//
// Fix: share one in-flight promise across mounts. The first mount kicks off
// the GET; subsequent mounts within ~25s reuse the same promise (and even the
// already-resolved value when it's done). Closing the modal does NOT abort the
// shared promise — its result is held in the cache so the next open can read
// it immediately.
//
// Cache is invalidated when registration succeeds (we have credentials, no
// further begin needed) or when the modal explicitly retries (`handleRetry`).
type WechatBeginPromise = Promise<ChannelRegistrationBeginResult>
let beginCache: {
  promise: WechatBeginPromise
  // resolved value, populated when the promise settles.
  resolved?: ChannelRegistrationBeginResult
  // monotonic id so per-mount waiters can detect "the cache was invalidated
  // while I was awaiting" — they should silently bail.
  generation: number
} | null = null
let beginGeneration = 0

function invalidateBeginCache() {
  beginCache = null
  beginGeneration += 1
}

async function getOrCreateBegin(
  beginFn: (platform: 'wechat') => Promise<ChannelRegistrationBeginResult>,
): Promise<{ result: ChannelRegistrationBeginResult; generation: number }> {
  if (beginCache?.resolved) {
    return { result: beginCache.resolved, generation: beginCache.generation }
  }
  if (beginCache) {
    const cached = beginCache
    const result = await cached.promise
    return { result, generation: cached.generation }
  }
  const generation = beginGeneration
  const promise = beginFn('wechat').then((r) => {
    if (beginCache && beginCache.generation === generation) {
      beginCache.resolved = r
    }
    return r
  })
  beginCache = { promise, generation }
  const result = await promise
  return { result, generation }
}

/**
 * MVP scope (Phase 5 backend hasn't persisted credentials yet): we drive the
 * scan-to-login flow end-to-end and show the captured iLink bot_id / user_id /
 * baseurl when polling reports Success. Real persistence + auto-connect lands
 * in Phase 5 PR3.
 */
export function WechatChannelConfig({ onSaved, onClose }: WechatChannelConfigProps) {
  const { t } = useTranslation()
  const beginRegistration = useChannelStore((s) => s.beginRegistration)
  const pollRegistrationAction = useChannelStore((s) => s.pollRegistration)

  const [begin, setBegin] = useState<ChannelRegistrationBeginResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [attempt, setAttempt] = useState(0)
  const [loginInfo, setLoginInfo] = useState<WechatLoginInfo | null>(null)
  // Captures the iLink "phone scanned, awaiting confirm on device" signal so
  // the modal can show a clear status banner instead of leaving the user
  // staring at the same QR.
  const [scanned, setScanned] = useState(false)

  // Snapshot the current device_code in a ref so the polling closure inside
  // RegistrationModal always sees the latest value when iLink rotates the QR
  // mid-flight (server signals via `kind=qr_refresh` payload in fail_reason).
  const deviceCodeRef = useRef<string | null>(null)
  // Same idea for the qr_url: when iLink rotates, we update this and the
  // RegistrationModal re-renders with the new image. We avoid stomping over
  // `begin.verificationUriComplete` itself so the underlying state object
  // stays consistent with backend.
  const [overrideQrUrl, setOverrideQrUrl] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setError(null)
    setBegin(null)
    setLoginInfo(null)
    setOverrideQrUrl(null)
    setScanned(false)
    // 25s 兜底超时：如果 iLink 真的卡了那么久就给用户重试出口。正常路径下
    // begin <= 3s 返回，cache 命中时是同步的（resolved value 立即可用）。
    const timeoutHandle = window.setTimeout(() => {
      if (cancelled) return
      setError(t('channel.wechat.config.errorTimeout'))
    }, 25_000)
    const run = async () => {
      try {
        const { result } = await getOrCreateBegin(beginRegistration)
        if (cancelled) return
        deviceCodeRef.current = result.deviceCode
        setBegin(result)
      } catch (e) {
        if (cancelled) return
        // 失败时把 cache 清掉，下次 mount 重新发请求。
        invalidateBeginCache()
        setError(e instanceof Error ? e.message : t('channel.wechat.config.errorBeginFailed'))
      } finally {
        window.clearTimeout(timeoutHandle)
      }
    }
    void run()
    return () => {
      cancelled = true
      window.clearTimeout(timeoutHandle)
    }
  }, [attempt, beginRegistration])

  const handleRetry = () => {
    // 用户主动「重试」按钮：强制丢弃当前 cache，重新拉新二维码。
    invalidateBeginCache()
    setAttempt((n) => n + 1)
  }

  const pollState = async (): Promise<RegistrationPollState> => {
    const deviceCode = deviceCodeRef.current
    if (!deviceCode) return 'waiting'
    const result = await pollRegistrationAction('wechat', deviceCode)
    // Backend signals out-of-band changes (QR refreshed, login confirmed) via
    // a JSON envelope in `fail_reason`. Parse defensively — non-JSON payloads
    // fall through as plain failure messages.
    const payload = result.failReason
      ? safeParseJson(result.failReason)
      : null

    if (result.state === 'success' && payload && payload.kind === 'wechat_success') {
      setLoginInfo({
        ilink_bot_id: String(payload.ilink_bot_id),
        ilink_user_id: String(payload.ilink_user_id),
        baseurl: String(payload.baseurl),
      })
      // 已登录成功，旧的 begin device_code 不再有用，下次重新打开应当走新一轮
      invalidateBeginCache()
      onSaved?.()
      return 'confirmed'
    }
    if (result.state === 'waiting' && payload && payload.kind === 'qr_refresh') {
      // 二维码被服务端轮换，cache 里那个 device_code 也失效了
      invalidateBeginCache()
      deviceCodeRef.current = String(payload.device_code)
      setOverrideQrUrl(String(payload.qr_url))
      setScanned(false)
      return 'waiting'
    }
    if (result.state === 'waiting' && payload && payload.kind === 'scanned') {
      setScanned(true)
      return 'waiting'
    }
    if (result.state === 'expired') {
      invalidateBeginCache()
      return 'expired'
    }
    if (result.state === 'fail') {
      invalidateBeginCache()
      setError(result.failReason || t('channel.wechat.config.errorBeginFailed'))
      return 'cancelled'
    }
    return 'waiting'
  }

  if (loginInfo) {
    return (
      <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
        <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
          <h2 className="text-2xl font-bold text-foreground">{t('channel.wechat.config.title')}</h2>
        </div>
        <div className="flex-1 overflow-y-auto px-10 pb-6">
          <div className="flex w-full flex-col items-center gap-5">
            <div className="flex w-72 flex-col items-center rounded-xl bg-emerald-50 px-8 py-5 text-emerald-500">
              <CheckCircle2 className="h-8 w-8" />
              <div className="mt-3 text-xl font-bold">{t('channel.wechat.config.scanSuccess')}</div>
              <div className="mt-1 text-sm font-semibold">{t('channel.wechat.config.accountConfirmed')}</div>
            </div>
            <div className="grid w-full gap-3 rounded-xl border border-border bg-card p-4 text-left">
              <CredentialRow label="ilink_bot_id" value={loginInfo.ilink_bot_id} />
              <CredentialRow label="ilink_user_id" value={loginInfo.ilink_user_id} />
              <CredentialRow label="baseurl" value={loginInfo.baseurl} />
            </div>
            <p className="text-xs text-muted-foreground">
              {t('channel.wechat.config.mvpNote')}
            </p>
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
            {t('channel.actions.done')}
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
          {t('channel.wechat.config.retryQr')}
        </Button>
      </div>
    )
  }

  if (!begin) {
    return (
      <div className="flex max-h-[78vh] w-full flex-col items-center justify-center bg-background p-10 text-sm text-muted-foreground">
        {t('channel.wechat.config.preparingQr')}
      </div>
    )
  }

  const qrUrl = overrideQrUrl ?? begin.verificationUriComplete

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <RegistrationModal
        mode="qr_url"
        title={t('channel.wechat.config.title')}
        qrUrl={qrUrl}
        expireSeconds={begin.expiresInSeconds || 600}
        pollIntervalMs={Math.max(1, begin.intervalSeconds || 2) * 1000}
        pollState={pollState}
        onConfirmed={() => {
          /* loginInfo state already set inside pollState() */
        }}
        onCancel={handleRetry}
      />
      {scanned ? (
        <div className="mx-10 mb-4 flex items-center justify-center gap-2 rounded-xl bg-emerald-50 px-5 py-3 text-sm font-semibold text-emerald-600">
          <CheckCircle2 className="h-4 w-4" />
          {t('channel.wechat.config.scannedHint')}
        </div>
      ) : (
        <p className="px-10 pb-4 text-center text-xs font-medium text-muted-foreground">
          {t('channel.wechat.config.scanHint')}
        </p>
      )}
    </div>
  )
}

function CredentialRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-border bg-muted/25 px-4 py-3">
      <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{value}</div>
    </div>
  )
}

function safeParseJson(raw: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(raw)
    return typeof parsed === 'object' && parsed !== null ? (parsed as Record<string, unknown>) : null
  } catch {
    return null
  }
}

import { useEffect, useRef, useState } from 'react'
import { ExternalLink } from 'lucide-react'
import { QrCodeCanvas } from './QrCodeCanvas'

export type RegistrationPollState = 'waiting' | 'confirmed' | 'cancelled' | 'expired'

interface CommonProps {
  title: string
  /** Total time before the registration session expires, in seconds. */
  expireSeconds: number
  /** Caller-provided polling function. Called repeatedly until it returns
   *  a non-`waiting` state OR the deadline is reached. */
  pollState: () => Promise<RegistrationPollState>
  /** Interval between polls in ms. Default 2000. */
  pollIntervalMs?: number
  onConfirmed: () => void
  onCancel: () => void
}

interface UrlModeProps extends CommonProps {
  mode: 'url'
  url: string
  /** Optional user-visible code (DingTalk OPEN_CLAW shows e.g. "ABCD-EFGH"). */
  userCode?: string
  qrUrl?: never
}

interface QrUrlModeProps extends CommonProps {
  mode: 'qr_url'
  /** The raw URL string returned by the platform; will be rendered into a QR
   *  image client-side via `qrcode` lib. NOT a base64 PNG. */
  qrUrl: string
  url?: never
  userCode?: never
}

export type RegistrationModalProps = UrlModeProps | QrUrlModeProps

type LocalState = 'polling' | 'confirmed' | 'cancelled' | 'expired'

export function RegistrationModal(props: RegistrationModalProps) {
  const [remainingSec, setRemainingSec] = useState(props.expireSeconds)
  const [localState, setLocalState] = useState<LocalState>('polling')
  const pollIntervalMs = props.pollIntervalMs ?? 2000

  // Snapshot callbacks in refs so the polling loop doesn't restart on every
  // parent re-render. The loop reads the latest values via the ref.
  const pollStateRef = useRef(props.pollState)
  const onConfirmedRef = useRef(props.onConfirmed)
  const onCancelRef = useRef(props.onCancel)
  useEffect(() => {
    pollStateRef.current = props.pollState
    onConfirmedRef.current = props.onConfirmed
    onCancelRef.current = props.onCancel
  })

  // Countdown
  useEffect(() => {
    if (localState !== 'polling' || remainingSec <= 0) return
    const interval = window.setInterval(() => {
      setRemainingSec((s) => {
        if (s <= 1) {
          setLocalState('expired')
          onCancelRef.current()
          return 0
        }
        return s - 1
      })
    }, 1000)
    return () => window.clearInterval(interval)
  }, [localState, remainingSec])

  // Polling loop
  useEffect(() => {
    if (localState !== 'polling') return
    let cancelled = false
    const loop = async () => {
      while (!cancelled) {
        let result: RegistrationPollState
        try {
          result = await pollStateRef.current()
        } catch {
          // Network blip; let the countdown tick down and let the next iteration retry.
          result = 'waiting'
        }
        if (cancelled) return
        if (result === 'confirmed') {
          setLocalState('confirmed')
          onConfirmedRef.current()
          return
        }
        if (result === 'cancelled') {
          setLocalState('cancelled')
          onCancelRef.current()
          return
        }
        if (result === 'expired') {
          setLocalState('expired')
          onCancelRef.current()
          return
        }
        // waiting → sleep then poll again
        await new Promise((r) => window.setTimeout(r, pollIntervalMs))
      }
    }
    void loop()
    return () => {
      cancelled = true
    }
  }, [localState, pollIntervalMs])

  const mm = String(Math.floor(remainingSec / 60)).padStart(2, '0')
  const ss = String(remainingSec % 60).padStart(2, '0')

  const qrPayload = props.mode === 'url' ? props.url : props.qrUrl

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">{props.title}</h2>
        <p data-testid="registration-countdown" className="mt-2 text-xs font-medium text-muted-foreground">
          剩余 {mm}:{ss}
        </p>
      </div>

      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-4">
          {localState === 'expired' ? (
            <div className="rounded-xl bg-red-50 px-5 py-3 text-sm font-semibold text-red-500">
              二维码已过期，请重新发起
            </div>
          ) : localState === 'cancelled' ? (
            <div className="rounded-xl bg-muted px-5 py-3 text-sm font-semibold text-muted-foreground">
              扫码已取消
            </div>
          ) : localState === 'confirmed' ? (
            <div className="rounded-xl bg-emerald-50 px-5 py-3 text-sm font-semibold text-emerald-500">
              扫码成功，正在完成配置…
            </div>
          ) : (
            <>
              <QrCodeCanvas value={qrPayload} loading={false} />

              {props.mode === 'url' && (
                <>
                  {props.userCode && (
                    <div className="rounded-xl border border-border bg-muted/25 px-4 py-3 text-center">
                      <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">用户码</div>
                      <div className="mt-1 font-mono text-lg font-semibold text-foreground">{props.userCode}</div>
                    </div>
                  )}
                  <a
                    href={props.url}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
                  >
                    页面未自动打开？点击继续 <ExternalLink className="h-3 w-3" />
                  </a>
                </>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  )
}

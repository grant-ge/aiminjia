import { AlertCircle } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useNetworkStore } from '@/stores/networkStore'
import { Button } from '@/components/ui/button'

export function NetworkStatusIndicator() {
  const { t } = useTranslation()
  const status = useNetworkStore((s) => s.status)
  const lastCheckAt = useNetworkStore((s) => s.lastCheckAt)
  const forceProbe = useNetworkStore((s) => s.forceProbe)

  const [retrying, setRetrying] = useState(false)
  const triggeredAtRef = useRef<number | null>(null)

  // Clear retrying once a new probe completes (lastCheckAt advances past the
  // trigger time). When status doesn't change, the backend dedupes the event
  // and lastCheckAt doesn't advance — the fallback timeout below handles that.
  useEffect(() => {
    if (
      retrying &&
      triggeredAtRef.current !== null &&
      lastCheckAt !== null &&
      lastCheckAt > triggeredAtRef.current
    ) {
      setRetrying(false)
    }
  }, [lastCheckAt, retrying])

  // Fallback: clear retrying after 5.5s (HEAD timeout 5s + buffer).
  useEffect(() => {
    if (!retrying) return
    const timer = setTimeout(() => setRetrying(false), 5500)
    return () => clearTimeout(timer)
  }, [retrying])

  if (status === 'unknown' || status === 'online') {
    return null
  }

  const isOffline = status === 'offline'
  const bannerText = retrying
    ? t('network.bannerRetryingText')
    : isOffline
      ? t('network.bannerOfflineText')
      : t('network.bannerDegradedText')

  const handleRetry = () => {
    if (retrying) return
    triggeredAtRef.current = Date.now()
    setRetrying(true)
    void forceProbe().catch((err) => {
      console.warn('[NetworkStatusIndicator] forceProbe failed:', err)
      setRetrying(false)
    })
  }

  return (
    <div
      role="alert"
      className="flex shrink-0 items-center gap-2 border-b border-border bg-[rgba(var(--color-semantic-orange-rgb),0.10)] px-4 py-2 text-sm text-destructive"
    >
      <AlertCircle className="h-4 w-4 shrink-0 text-destructive" />
      <span className="flex-1">{bannerText}</span>
      <Button
        type="button"
        size="sm"
        danger
        loading={retrying}
        disabled={retrying}
        onClick={handleRetry}
      >
        {t('network.retryNow')}
      </Button>
    </div>
  )
}

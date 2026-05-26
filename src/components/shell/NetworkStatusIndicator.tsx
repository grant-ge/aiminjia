import { CloudOff, WifiOff } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { useNetworkStore } from '@/stores/networkStore'

export function NetworkStatusIndicator() {
  const { t } = useTranslation()
  const status = useNetworkStore((s) => s.status)
  const lastOnlineAt = useNetworkStore((s) => s.lastOnlineAt)
  const forceProbe = useNetworkStore((s) => s.forceProbe)
  const [open, setOpen] = useState(false)

  if (status === 'unknown' || status === 'online') {
    return null
  }

  const isOffline = status === 'offline'
  const Icon = isOffline ? WifiOff : CloudOff
  const wrapperCls = isOffline
    ? 'bg-destructive/12 text-destructive'
    : 'bg-muted text-muted-foreground'
  const badgeLabel = isOffline ? t('network.offlineBadge') : t('network.degradedBadge')
  const popTitle = isOffline
    ? t('network.popoverOfflineTitle')
    : t('network.popoverDegradedTitle')
  const popDesc = isOffline
    ? t('network.popoverOfflineDesc')
    : t('network.popoverDegradedDesc')

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={badgeLabel}
          className={`inline-flex items-center justify-center rounded-full p-[6px] ${wrapperCls}`}
        >
          <Icon className="h-3.5 w-3.5" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="w-72 space-y-3 shadow-[var(--shadow-popover)]"
      >
        <div className="space-y-1">
          <div className="text-sm font-medium text-foreground">{popTitle}</div>
          <div className="text-xs text-muted-foreground">{popDesc}</div>
        </div>
        {lastOnlineAt ? (
          <div className="text-xs text-muted-foreground">
            {t('network.lastOnline', {
              time: new Date(lastOnlineAt).toLocaleString(),
            })}
          </div>
        ) : null}
        <div className="flex justify-end">
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={() => {
              void forceProbe().catch((err) => {
                console.warn('[NetworkStatusIndicator] forceProbe failed:', err)
              })
            }}
          >
            {t('network.retryNow')}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}

import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { Eye, EyeOff } from 'lucide-react'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { ChannelConfigView } from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'
import { Button } from '@/components/ui/button'

interface ChannelConfigDetailsProps {
  config: ChannelConfigView
  open: boolean
  onOpenChange: (open: boolean) => void
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-[rgba(var(--muted-rgb),0.25)] px-4 py-3">
      <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{value}</div>
    </div>
  )
}

interface PlatformCopy {
  title: string
  appKeyLabel: string
  secretLabel: string
  secretConfirmTitle: string
  secretConfirmDescription: string
  secretRevealButton: string
  // 是否需要单独展示 RobotCode（仅钉钉有）
  showRobotCode: boolean
}

function copyForPlatform(platform: ChannelConfigView['platform'], t: TFunction): PlatformCopy {
  switch (platform) {
    case 'feishu':
      return {
        title: t('channel.details.titleByPlatform.feishu'),
        appKeyLabel: 'AppID',
        secretLabel: 'AppSecret',
        secretConfirmTitle: t('channel.details.secretConfirm.feishu.title'),
        secretConfirmDescription: t('channel.details.secretConfirm.feishu.description'),
        secretRevealButton: t('channel.details.secretConfirm.feishu.revealButton'),
        showRobotCode: false,
      }
    case 'wecom':
      return {
        title: t('channel.details.titleByPlatform.wecom'),
        appKeyLabel: 'Bot ID',
        secretLabel: 'Secret',
        secretConfirmTitle: t('channel.details.secretConfirm.wecom.title'),
        secretConfirmDescription: t('channel.details.secretConfirm.wecom.description'),
        secretRevealButton: t('channel.details.secretConfirm.wecom.revealButton'),
        showRobotCode: false,
      }
    case 'wechat':
      return {
        title: t('channel.details.titleByPlatform.wechat'),
        // 后端 wechat_config_view 把 ilink_user_id 放进 appKey、ilink_bot_id 放进
        // robotCode，bot_token mask 走 appSecretMasked。详见
        // src-tauri/.../shared/config_store.rs::wechat_config_view。
        appKeyLabel: t('channel.details.fields.wechatAccountId'),
        secretLabel: 'Bot Token',
        secretConfirmTitle: t('channel.details.secretConfirm.wechat.title'),
        secretConfirmDescription: t('channel.details.secretConfirm.wechat.description'),
        secretRevealButton: t('channel.details.secretConfirm.wechat.revealButton'),
        showRobotCode: false,
      }
    case 'dingtalk':
    default:
      return {
        title: t('channel.details.titleByPlatform.dingtalk'),
        appKeyLabel: 'AppKey',
        secretLabel: 'AppSecret',
        secretConfirmTitle: t('channel.details.secretConfirm.dingtalk.title'),
        secretConfirmDescription: t('channel.details.secretConfirm.dingtalk.description'),
        secretRevealButton: t('channel.details.secretConfirm.dingtalk.revealButton'),
        showRobotCode: true,
      }
  }
}

export function ChannelConfigDetails({ config, open, onOpenChange }: ChannelConfigDetailsProps) {
  const { t } = useTranslation()
  const revealSecret = useChannelStore((s) => s.revealSecret)
  const openRef = useRef(open)
  const [revealedSecret, setRevealedSecret] = useState<string | null>(null)
  const [revealing, setRevealing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const revealRunIdRef = useRef(0)
  const copy = copyForPlatform(config.platform, t)

  useEffect(() => {
    openRef.current = open
    if (!open) {
      revealRunIdRef.current += 1
      setRevealedSecret(null)
      setRevealing(false)
      setError(null)
    }
  }, [open])

  const handleReveal = async () => {
    const runId = revealRunIdRef.current + 1
    revealRunIdRef.current = runId
    const confirmed = await requestConfirm({
      title: copy.secretConfirmTitle,
      description: copy.secretConfirmDescription,
      confirmLabel: t('channel.actions.confirmReveal'),
      cancelLabel: t('channel.actions.cancel'),
      variant: 'destructive',
    })
    if (revealRunIdRef.current !== runId || !openRef.current) return
    if (!confirmed) return

    setRevealing(true)
    setError(null)
    try {
      const secret = await revealSecret(config.platform)
      if (revealRunIdRef.current !== runId) return
      setRevealedSecret(secret)
    } catch (err) {
      if (revealRunIdRef.current !== runId) return
      setError(err instanceof Error ? err.message : t('channel.details.secretReadFailed', { label: copy.secretLabel }))
    } finally {
      if (revealRunIdRef.current === runId) {
        setRevealing(false)
      }
    }
  }

  const secretValue = revealedSecret ?? config.appSecretMasked

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl rounded-md border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
        <DialogHeader className="px-8 pt-8 text-left">
          <DialogTitle className="text-2xl font-bold">{copy.title}</DialogTitle>
          <DialogDescription>{t('channel.details.readonly')}</DialogDescription>
        </DialogHeader>

        <div className="grid gap-3 px-8 pb-8 pt-4">
          <DetailRow label={copy.appKeyLabel} value={config.appKey} />
          <div className="rounded-md border border-border bg-[rgba(var(--muted-rgb),0.25)] px-4 py-3">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 flex-1">
                <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">{copy.secretLabel}</div>
                <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{secretValue}</div>
              </div>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                icon={revealedSecret ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                onClick={handleReveal}
                disabled={revealing}
              >
                {revealedSecret ? t('channel.details.secretRevealed') : revealing ? t('channel.details.secretReading') : copy.secretRevealButton}
              </Button>
            </div>
          </div>
          {copy.showRobotCode && <DetailRow label="RobotCode" value={config.robotCode} />}
          {config.platform === 'wechat' && <DetailRow label={t('channel.details.fields.wechatBotId')} value={config.robotCode} />}
          <DetailRow label="Source" value={config.source} />
          <DetailRow label={t('channel.details.createdAt')} value={config.createdAt} />
          <DetailRow label={t('channel.details.updatedAt')} value={config.updatedAt} />
          {error && <div className="rounded-md bg-red-50 px-4 py-3 text-sm font-semibold text-red-500">{error}</div>}
        </div>
      </DialogContent>
    </Dialog>
  )
}

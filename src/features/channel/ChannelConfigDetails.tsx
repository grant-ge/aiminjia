import { useEffect, useRef, useState } from 'react'
import { Eye, EyeOff } from 'lucide-react'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { ChannelConfigView } from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'

interface ChannelConfigDetailsProps {
  config: ChannelConfigView
  open: boolean
  onOpenChange: (open: boolean) => void
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-border bg-muted/25 px-4 py-3">
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

function copyForPlatform(platform: ChannelConfigView['platform']): PlatformCopy {
  switch (platform) {
    case 'feishu':
      return {
        title: '飞书配置',
        appKeyLabel: 'AppID',
        secretLabel: 'AppSecret',
        secretConfirmTitle: '显示 AppSecret？',
        secretConfirmDescription: 'AppSecret 是敏感凭证。确认后会在当前弹窗中显示，关闭弹窗后会自动清除。',
        secretRevealButton: '显示 AppSecret',
        showRobotCode: false,
      }
    case 'wecom':
      return {
        title: '企业微信配置',
        appKeyLabel: 'Bot ID',
        secretLabel: 'Secret',
        secretConfirmTitle: '显示 Secret？',
        secretConfirmDescription: 'Secret 是敏感凭证。确认后会在当前弹窗中显示，关闭弹窗后会自动清除。',
        secretRevealButton: '显示 Secret',
        showRobotCode: false,
      }
    case 'wechat':
      return {
        title: '个人微信配置',
        // 后端 wechat_config_view 把 ilink_user_id 放进 appKey、ilink_bot_id 放进
        // robotCode，bot_token mask 走 appSecretMasked。详见
        // src-tauri/.../shared/config_store.rs::wechat_config_view。
        appKeyLabel: '账号 ID (ilink_user_id)',
        secretLabel: 'Bot Token',
        secretConfirmTitle: '显示 Bot Token？',
        secretConfirmDescription: 'Bot Token 是敏感凭证。确认后会在当前弹窗中显示，关闭弹窗后会自动清除。',
        secretRevealButton: '显示 Bot Token',
        showRobotCode: false,
      }
    case 'dingtalk':
    default:
      return {
        title: '钉钉配置',
        appKeyLabel: 'AppKey',
        secretLabel: 'AppSecret',
        secretConfirmTitle: '显示 AppSecret？',
        secretConfirmDescription: 'AppSecret 是敏感凭证。确认后会在当前弹窗中显示，关闭弹窗后会自动清除。',
        secretRevealButton: '显示 AppSecret',
        showRobotCode: true,
      }
  }
}

export function ChannelConfigDetails({ config, open, onOpenChange }: ChannelConfigDetailsProps) {
  const revealSecret = useChannelStore((s) => s.revealSecret)
  const openRef = useRef(open)
  const [revealedSecret, setRevealedSecret] = useState<string | null>(null)
  const [revealing, setRevealing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const revealRunIdRef = useRef(0)
  const copy = copyForPlatform(config.platform)

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
      confirmLabel: '确认显示',
      cancelLabel: '取消',
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
      setError(err instanceof Error ? err.message : `读取 ${copy.secretLabel} 失败`)
    } finally {
      if (revealRunIdRef.current === runId) {
        setRevealing(false)
      }
    }
  }

  const secretValue = revealedSecret ?? config.appSecretMasked

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl rounded-xl border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
        <DialogHeader className="px-8 pt-8 text-left">
          <DialogTitle className="text-2xl font-bold">{copy.title}</DialogTitle>
          <DialogDescription>当前配置为只读。需要更换凭证时，请移除后重新扫码配置。</DialogDescription>
        </DialogHeader>

        <div className="grid gap-3 px-8 pb-8 pt-4">
          <DetailRow label={copy.appKeyLabel} value={config.appKey} />
          <div className="rounded-xl border border-border bg-muted/25 px-4 py-3">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 flex-1">
                <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">{copy.secretLabel}</div>
                <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{secretValue}</div>
              </div>
              <Button type="button" variant="secondary" size="sm" onClick={handleReveal} disabled={revealing}>
                {revealedSecret ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                {revealedSecret ? '已显示' : revealing ? '读取中...' : copy.secretRevealButton}
              </Button>
            </div>
          </div>
          {copy.showRobotCode && <DetailRow label="RobotCode" value={config.robotCode} />}
          {config.platform === 'wechat' && <DetailRow label="iLink Bot ID" value={config.robotCode} />}
          <DetailRow label="Source" value={config.source} />
          <DetailRow label="创建时间" value={config.createdAt} />
          <DetailRow label="更新时间" value={config.updatedAt} />
          {error && <div className="rounded-xl bg-red-50 px-4 py-3 text-sm font-semibold text-red-500">{error}</div>}
        </div>
      </DialogContent>
    </Dialog>
  )
}

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

export function ChannelConfigDetails({ config, open, onOpenChange }: ChannelConfigDetailsProps) {
  const revealSecret = useChannelStore((s) => s.revealSecret)
  const openRef = useRef(open)
  const [revealedSecret, setRevealedSecret] = useState<string | null>(null)
  const [revealing, setRevealing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const revealRunIdRef = useRef(0)

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
      title: '显示 AppSecret？',
      description: 'AppSecret 是敏感凭证。确认后会在当前弹窗中显示，关闭弹窗后会自动清除。',
      confirmLabel: '确认显示',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (revealRunIdRef.current !== runId || !openRef.current) return
    if (!confirmed) return

    setRevealing(true)
    setError(null)
    try {
      const secret = await revealSecret('dingtalk')
      if (revealRunIdRef.current !== runId) return
      setRevealedSecret(secret)
    } catch (err) {
      if (revealRunIdRef.current !== runId) return
      setError(err instanceof Error ? err.message : '读取 AppSecret 失败')
    } finally {
      if (revealRunIdRef.current === runId) {
        setRevealing(false)
      }
    }
  }

  const secretValue = revealedSecret ?? config.appSecretMasked

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl rounded-[28px] border border-border bg-background p-0 shadow-2xl">
        <DialogHeader className="px-8 pt-8 text-left">
          <DialogTitle className="text-2xl font-bold">钉钉配置</DialogTitle>
          <DialogDescription>当前配置为只读。需要更换凭证时，请移除后重新扫码配置。</DialogDescription>
        </DialogHeader>

        <div className="grid gap-3 px-8 pb-8 pt-4">
          <DetailRow label="AppKey" value={config.appKey} />
          <div className="rounded-xl border border-border bg-muted/25 px-4 py-3">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 flex-1">
                <div className="text-xs font-bold uppercase tracking-wide text-muted-foreground">AppSecret</div>
                <div className="mt-1 break-all font-mono text-sm font-semibold text-foreground">{secretValue}</div>
              </div>
              <Button type="button" variant="secondary" size="sm" onClick={handleReveal} disabled={revealing}>
                {revealedSecret ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                {revealedSecret ? '已显示' : revealing ? '读取中...' : '显示 AppSecret'}
              </Button>
            </div>
          </div>
          <DetailRow label="RobotCode" value={config.robotCode} />
          <DetailRow label="Source" value={config.source} />
          <DetailRow label="创建时间" value={config.createdAt} />
          <DetailRow label="更新时间" value={config.updatedAt} />
          {error && <div className="rounded-xl bg-red-50 px-4 py-3 text-sm font-semibold text-red-500">{error}</div>}
        </div>
      </DialogContent>
    </Dialog>
  )
}

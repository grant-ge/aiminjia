import { useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

interface Props {
  open: boolean
  onAccept: () => void
  onCancel: () => void
}

/**
 * 首次扫码风险 banner。spec §9.1。
 *
 * 用户必须勾选"我已了解上述风险"才能点继续。每次扫码弹一次，不持久化
 * （重新扫码会再弹）。
 */
export function WhatsappRiskBanner({ open, onAccept, onCancel }: Props) {
  const [acknowledged, setAcknowledged] = useState(false)

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onCancel()
      }}
    >
      <DialogContent className="max-w-lg overflow-hidden">
        <DialogHeader>
          <DialogTitle>关于 AIjia 接入 WhatsApp 的说明</DialogTitle>
        </DialogHeader>
        <DialogDescription asChild>
          <div className="space-y-3 text-sm text-foreground">
            <p>
              AIjia 接入 WhatsApp 当前采用与开源项目{' '}
              <a
                href="https://docs.openclaw.ai/channels/whatsapp"
                target="_blank"
                rel="noreferrer"
                className="underline text-primary"
              >
                OpenClaw
              </a>
              {' '}相同的方案：通过 <b>WhatsApp Web 多设备协议</b>把 AIjia
              作为一台"已链接设备"接入你的 WhatsApp
              账号（与你手机上的"已链接的设备 → 链接设备"是同一套机制）。
              我们正在申请 WhatsApp Business 官方接入资质，待批复后会迁移到官方
              API；在此之前先用 OpenClaw 的方式保证可用。
            </p>
            <h4 className="font-semibold mt-2">
              这个方案的风险（与 OpenClaw 完全相同）
            </h4>
            <ul className="list-disc list-inside space-y-1">
              <li>
                <b>WhatsApp 官方未授权</b>这种接入方式，属于 TOS 灰区
              </li>
              <li>
                账号有被 WhatsApp <b>限速 / 封禁</b>的风险，
                <b>风险由你自行承担</b>
              </li>
              <li>
                实测中，<b>虚拟号</b>（Google Voice
                等）被封概率显著高于真实手机号
              </li>
              <li>群发、频繁主动外呼、异常高频回复都会增加封号风险</li>
            </ul>
            <h4 className="font-semibold mt-2">强烈建议</h4>
            <ul className="list-disc list-inside space-y-1">
              <li>
                使用<b>真实手机号</b>，不要用虚拟号
              </li>
              <li>
                不在 AIjia 中<b>群发</b>或频繁主动外呼
              </li>
              <li>用于 AI 辅助对话场景，不用于营销 / 推广</li>
              <li>
                重要号码请勿接入，建议用<b>专门的工作号</b>
              </li>
            </ul>
          </div>
        </DialogDescription>
        <label className="flex items-center gap-2 text-sm text-foreground mt-4 cursor-pointer">
          <input
            type="checkbox"
            checked={acknowledged}
            onChange={(e) => setAcknowledged(e.target.checked)}
          />
          我已了解上述风险，继续扫码
        </label>
        <DialogFooter className="mt-4">
          <Button variant="ghost" onClick={onCancel}>
            取消
          </Button>
          <Button onClick={onAccept} disabled={!acknowledged}>
            继续
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

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
  const { t } = useTranslation()
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
          <DialogTitle>{t('channel.whatsapp.risk.dialogTitle')}</DialogTitle>
        </DialogHeader>
        <DialogBody data-testid="whatsapp-risk-dialog-body" className="flex flex-col gap-4">
          <DialogDescription asChild>
            <div className="space-y-3 text-sm text-foreground">
              <p>
                {t('channel.whatsapp.risk.intro')}{' '}
                <a
                  href="https://docs.openclaw.ai/channels/whatsapp"
                  target="_blank"
                  rel="noreferrer"
                  className="underline text-primary"
                >
                  {t('channel.whatsapp.risk.openClawLink')}
                </a>
                {' '}{t('channel.whatsapp.risk.introDesc', { protocol: t('channel.whatsapp.risk.introProtocol') })}
                {t('channel.whatsapp.risk.introMigration')}
              </p>
              <h4 className="font-semibold mt-2">
                {t('channel.whatsapp.risk.risksHeading')}
              </h4>
              <ul className="list-disc list-inside space-y-1">
                <li>
                  <b>{t('channel.whatsapp.risk.riskUnauthorized')}</b>{t('channel.whatsapp.risk.riskUnauthorizedSuffix')}
                </li>
                <li>
                  {t('channel.whatsapp.risk.riskBanPrefix')} <b>{t('channel.whatsapp.risk.riskBan')}</b>{t('channel.whatsapp.risk.riskBanSuffix')}
                  <b>{t('channel.whatsapp.risk.riskBanOwn')}</b>
                </li>
                <li>
                  {t('channel.whatsapp.risk.riskVirtualNumberDesc', { virtualNumber: t('channel.whatsapp.risk.riskVirtualNumber') })}
                </li>
                <li>{t('channel.whatsapp.risk.riskBroadcast')}</li>
              </ul>
              <h4 className="font-semibold mt-2">{t('channel.whatsapp.risk.suggestionsHeading')}</h4>
              <ul className="list-disc list-inside space-y-1">
                <li>
                  {t('channel.whatsapp.risk.suggestRealPhoneDesc', { phone: t('channel.whatsapp.risk.suggestRealPhone') })}
                </li>
                <li>
                  {t('channel.whatsapp.risk.suggestNoBroadcastDesc', { broadcast: t('channel.whatsapp.risk.suggestNoBroadcast') })}
                </li>
                <li>{t('channel.whatsapp.risk.suggestAiOnly')}</li>
                <li>
                  {t('channel.whatsapp.risk.suggestWorkPhoneDesc', { phone: t('channel.whatsapp.risk.suggestWorkPhone') })}
                </li>
              </ul>
            </div>
          </DialogDescription>
          <label className="flex cursor-pointer items-center gap-2 text-sm text-foreground">
            <input
              type="checkbox"
              checked={acknowledged}
              onChange={(e) => setAcknowledged(e.target.checked)}
            />
            {t('channel.whatsapp.risk.acknowledge')}
          </label>
        </DialogBody>
        <DialogFooter data-testid="whatsapp-risk-dialog-footer">
          <Button variant="ghost" onClick={onCancel}>
            {t('channel.actions.cancel')}
          </Button>
          <Button onClick={onAccept} disabled={!acknowledged}>
            {t('channel.whatsapp.risk.continue')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

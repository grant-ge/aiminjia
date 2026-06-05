import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { normalizeGatewayHost } from '@/lib/gatewayHost'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'

interface CustomGatewayDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Host currently in effect, used to disable a no-op switch. */
  currentHost: string
  /** Seed value when the dialog opens (e.g. the current custom host). */
  initialHost?: string
  /** Called with the normalized origin when the user confirms. */
  onConfirm: (host: string) => void
}

/**
 * Dev-only modal for entering a custom gateway origin. Shared by the settings
 * environment switcher and the login-page switcher. The modal itself is the
 * confirmation step (the target origin is shown live), so callers switch
 * immediately on `onConfirm`.
 */
export function CustomGatewayDialog({
  open,
  onOpenChange,
  currentHost,
  initialHost,
  onConfirm,
}: CustomGatewayDialogProps) {
  const { t } = useTranslation()
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[420px] overflow-hidden">
        <DialogHeader>
          <DialogTitle>{t('settings.devGateway.customTitle')}</DialogTitle>
        </DialogHeader>
        {/* Body lives in a child so it remounts each time the dialog opens
            (Radix unmounts content when closed), seeding the input from
            `initialHost` without a state-syncing effect. */}
        <CustomGatewayBody
          currentHost={currentHost}
          initialHost={initialHost ?? ''}
          onCancel={() => onOpenChange(false)}
          onConfirm={(host) => {
            onOpenChange(false)
            onConfirm(host)
          }}
        />
      </DialogContent>
    </Dialog>
  )
}

interface CustomGatewayBodyProps {
  currentHost: string
  initialHost: string
  onCancel: () => void
  onConfirm: (host: string) => void
}

function CustomGatewayBody({
  currentHost,
  initialHost,
  onCancel,
  onConfirm,
}: CustomGatewayBodyProps) {
  const { t } = useTranslation()
  const [draft, setDraft] = useState(initialHost)
  const target = normalizeGatewayHost(draft)
  const disabled = !target || target === currentHost

  return (
    <>
      <p className="text-sm text-muted-foreground">{t('settings.devGateway.customHint')}</p>
      <Input
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        placeholder={t('settings.devGateway.customPlaceholder')}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        autoFocus
      />
      {draft.trim() ? (
        target ? (
          <p className="text-xs text-muted-foreground">
            {t('settings.devGateway.switchTarget')}：
            <span className="font-mono text-foreground">{target}</span>
          </p>
        ) : (
          <p className="text-xs text-destructive">{t('settings.devGateway.invalidHost')}</p>
        )
      ) : null}
      <DialogFooter>
        <Button variant="outline" onClick={onCancel}>
          {t('common.cancel')}
        </Button>
        <Button variant="destructive" disabled={disabled} onClick={() => target && onConfirm(target)}>
          {t('settings.devGateway.confirmButton')}
        </Button>
      </DialogFooter>
    </>
  )
}

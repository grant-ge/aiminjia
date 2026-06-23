import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { normalizeOrigin } from '@/lib/environment'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Button } from '@/components/ui/button'

interface CustomEnvironmentDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Environment currently in effect, used to disable a no-op switch. */
  current: { tenant: string; ops: string }
  /** Seed values when the dialog opens (e.g. the current custom environment). */
  initial?: { tenant: string; ops: string }
  /** Called with the normalized origins when the user confirms. */
  onConfirm: (tenant: string, ops: string) => void
}

/**
 * Dev-only modal for entering a custom environment (tenant + ops origins).
 * Shared by the title-bar environment switcher. The modal itself is the
 * confirmation step (the target origins are shown live), so callers switch
 * immediately on `onConfirm`.
 */
export function CustomEnvironmentDialog({
  open,
  onOpenChange,
  current,
  initial,
  onConfirm,
}: CustomEnvironmentDialogProps) {
  const { t } = useTranslation()
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[460px] overflow-hidden">
        <DialogHeader>
          <DialogTitle>{t('settings.environment.customTitle')}</DialogTitle>
        </DialogHeader>
        {/* Body lives in a child so it remounts each time the dialog opens
            (Radix unmounts content when closed), seeding the inputs from
            `initial` without a state-syncing effect. */}
        <CustomEnvironmentBody
          current={current}
          initial={initial ?? { tenant: '', ops: '' }}
          onCancel={() => onOpenChange(false)}
          onConfirm={(tenant, ops) => {
            onOpenChange(false)
            onConfirm(tenant, ops)
          }}
        />
      </DialogContent>
    </Dialog>
  )
}

interface CustomEnvironmentBodyProps {
  current: { tenant: string; ops: string }
  initial: { tenant: string; ops: string }
  onCancel: () => void
  onConfirm: (tenant: string, ops: string) => void
}

function CustomEnvironmentBody({ current, initial, onCancel, onConfirm }: CustomEnvironmentBodyProps) {
  const { t } = useTranslation()
  const [tenantDraft, setTenantDraft] = useState(initial.tenant)
  const [opsDraft, setOpsDraft] = useState(initial.ops)

  const tenantTarget = normalizeOrigin(tenantDraft)
  const opsTarget = normalizeOrigin(opsDraft)
  const bothValid = !!tenantTarget && !!opsTarget
  const unchanged = tenantTarget === current.tenant && opsTarget === current.ops
  const disabled = !bothValid || unchanged

  return (
    <>
      <p className="text-sm text-muted-foreground">{t('settings.environment.customHint')}</p>

      <div className="flex flex-col gap-1.5">
        <Label htmlFor="env-tenant">{t('settings.environment.tenantLabel')}</Label>
        <Input
          id="env-tenant"
          value={tenantDraft}
          onChange={(e) => setTenantDraft(e.target.value)}
          placeholder={t('settings.environment.tenantPlaceholder')}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          autoFocus
        />
        {tenantDraft.trim() && !tenantTarget ? (
          <p className="text-xs text-destructive">{t('settings.environment.invalidHost')}</p>
        ) : null}
      </div>

      <div className="flex flex-col gap-1.5">
        <Label htmlFor="env-ops">{t('settings.environment.opsLabel')}</Label>
        <Input
          id="env-ops"
          value={opsDraft}
          onChange={(e) => setOpsDraft(e.target.value)}
          placeholder={t('settings.environment.opsPlaceholder')}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
        {opsDraft.trim() && !opsTarget ? (
          <p className="text-xs text-destructive">{t('settings.environment.invalidHost')}</p>
        ) : null}
      </div>

      <DialogFooter>
        <Button variant="outline" onClick={onCancel}>
          {t('common.cancel')}
        </Button>
        <Button
          variant="destructive"
          disabled={disabled}
          onClick={() => bothValid && onConfirm(tenantTarget, opsTarget)}
        >
          {t('settings.environment.confirmButton')}
        </Button>
      </DialogFooter>
    </>
  )
}

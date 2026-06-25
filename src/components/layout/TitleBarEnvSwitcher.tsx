import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, ChevronDown } from 'lucide-react'

import { setEnvironmentCache } from '@/lib/environment'
import { getDevEnvironment, setDevEnvironment, type DevEnvironmentState } from '@/lib/tauri'
import { AppDropdown } from '@/components/common/AppDropdown'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { CustomEnvironmentDialog } from '@/components/common/CustomEnvironmentDialog'
import { useAuthStore } from '@/stores/authStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useUiStore } from '@/stores/uiStore'
import { Button } from '@/components/ui/button'

/**
 * Dev-only environment switcher living in the title bar (the green badge).
 * Always visible in debug builds regardless of login state. Switching while
 * logged in logs the user out and bounces to login. Stripped from release
 * builds by the title bar's `isDev` guard.
 */
export function TitleBarEnvSwitcher() {
  const { t } = useTranslation()
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)
  const logout = useAuthStore((s) => s.logout)
  const closeSettings = useUiStore((s) => s.closeSettings)
  const pushNotification = useNotificationStore((s) => s.push)
  const [state, setState] = useState<DevEnvironmentState | null>(null)
  const [customOpen, setCustomOpen] = useState(false)

  useEffect(() => {
    void getDevEnvironment()
      .then((s) => {
        setEnvironmentCache({ tenant: s.currentTenant, ops: s.currentOps })
        setState(s)
      })
      .catch((e) => {
        // Outside Tauri (vitest/jsdom) the command is unavailable; in a dev
        // build a failure here is a real bug (e.g. command not registered),
        // so surface it instead of swallowing it.
        console.error('[TitleBarEnvSwitcher] getDevEnvironment failed:', e)
      })
  }, [])

  if (!state) return null

  // Preset labels come from a stable backend key (test/pre/prod); translate
  // here so the switcher follows the UI language.
  const presetLabel = (key: string) => t(`settings.environment.env.${key}`, key)

  // An environment is identified by its tenant origin; ops follows it.
  const currentPreset = state.presets.find((p) => p.tenant === state.currentTenant)
  const isCustom = !currentPreset
  const currentLabel = currentPreset ? presetLabel(currentPreset.key) : t('settings.environment.custom')

  const applySwitch = async (tenant: string, ops: string) => {
    if (tenant === state.currentTenant && ops === state.currentOps) return
    try {
      const next = await setDevEnvironment(tenant, ops)
      setEnvironmentCache({ tenant: next.currentTenant, ops: next.currentOps })
      if (isLoggedIn) {
        // Close settings before logout: logout() doesn't reset uiStore, so a
        // lingering settingsModal would reopen after re-login.
        closeSettings()
        void logout()
      } else {
        setState(next)
      }
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('settings.environment.switchFailed'),
        message: String(e),
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    }
  }

  const switchToPreset = async (tenant: string, ops: string, label: string) => {
    if (tenant === state.currentTenant && ops === state.currentOps) return
    const confirmed = await requestConfirm({
      title: t('settings.environment.confirmTitle'),
      description: t('settings.environment.confirmRelogin', { env: label }),
      confirmLabel: t('settings.environment.confirmButton'),
      variant: 'destructive',
    })
    if (!confirmed) return
    void applySwitch(tenant, ops)
  }

  return (
    <span className="mr-2" onMouseDown={(e) => e.stopPropagation()}>
      <AppDropdown
        ariaLabel={t('settings.environment.title')}
        align="end"
        trigger={
          <Button unstyled
            type="button"
            className="inline-flex items-center gap-0.5 rounded bg-[var(--color-semantic-green)] px-1.5 py-0.5 text-[11px] font-semibold tracking-wide text-primary-foreground shadow-[var(--shadow-sm)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-semantic-green)_92%,black)]"
          >
            {currentLabel}
            <ChevronDown className="h-3 w-3" />
          </Button>
        }
        items={[
          ...state.presets.map((p) => {
            const selected = state.currentTenant === p.tenant && state.currentOps === p.ops
            return {
              id: p.tenant,
              className: selected ? 'bg-accent' : undefined,
              label: (
                <span className="flex w-full items-center justify-between gap-2">
                  <span className="flex flex-col">
                    <span>{presetLabel(p.key)}</span>
                    <span className="font-mono text-xs text-muted-foreground">{p.tenant}</span>
                  </span>
                  {selected ? <Check className="h-3.5 w-3.5 text-primary" /> : null}
                </span>
              ),
              onSelect: () => void switchToPreset(p.tenant, p.ops, presetLabel(p.key)),
            }
          }),
          {
            id: '__custom__',
            className: isCustom ? 'bg-accent' : undefined,
            label: (
              <span className="flex w-full items-center justify-between gap-2">
                <span className="flex flex-col">
                  <span>{t('settings.environment.custom')}</span>
                  {isCustom ? (
                    <span className="font-mono text-xs text-muted-foreground">
                      {state.currentTenant}
                    </span>
                  ) : null}
                </span>
                {isCustom ? <Check className="h-3.5 w-3.5 text-primary" /> : null}
              </span>
            ),
            onSelect: () => setCustomOpen(true),
          },
        ]}
      />

      <CustomEnvironmentDialog
        open={customOpen}
        onOpenChange={setCustomOpen}
        current={{ tenant: state.currentTenant, ops: state.currentOps }}
        initial={isCustom ? { tenant: state.currentTenant, ops: state.currentOps } : undefined}
        onConfirm={(tenant, ops) => void applySwitch(tenant, ops)}
      />
    </span>
  )
}

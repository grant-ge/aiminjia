import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, ChevronDown } from 'lucide-react'

import { getDevGateway, setDevGateway, type DevGatewayState } from '@/lib/tauri'
import { AppDropdown } from '@/components/common/AppDropdown'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { CustomGatewayDialog } from '@/components/common/CustomGatewayDialog'
import { useAuthStore } from '@/stores/authStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useUiStore } from '@/stores/uiStore'

/**
 * Dev-only environment switcher living in the title bar (the green badge). The
 * single in-app entry for switching gateways — there is no settings panel for
 * it. Behavior depends on auth state:
 *
 * - Logged in: shown only off-production (production switches happen from the
 *   login screen). Switching logs the user out and bounces to login.
 * - Login screen: always shown, listing every environment including
 *   production, so the user can pick any. Switching just repoints the host.
 *
 * Rendered behind the title bar's `isDev` guard, so it's stripped from
 * production builds.
 */
export function TitleBarEnvSwitcher() {
  const { t } = useTranslation()
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)
  const logout = useAuthStore((s) => s.logout)
  const closeSettings = useUiStore((s) => s.closeSettings)
  const pushNotification = useNotificationStore((s) => s.push)
  const [state, setState] = useState<DevGatewayState | null>(null)
  const [customOpen, setCustomOpen] = useState(false)

  useEffect(() => {
    void getDevGateway()
      .then(setState)
      .catch(() => {
        // Outside Tauri (vitest/jsdom) the command is unavailable.
      })
  }, [])

  // Not loaded → nothing. Once logged in, hide on production (switch it from
  // the login screen). On the login screen, always show so production can be
  // picked again.
  if (!state) return null
  if (isLoggedIn && !state.isOverride) return null

  const isCustomHost = !state.presets.some((p) => p.host === state.currentHost)
  const currentLabel =
    state.presets.find((p) => p.host === state.currentHost)?.label ??
    t('settings.devGateway.custom')

  const applySwitch = async (host: string) => {
    if (host === state.currentHost) return
    try {
      const next = await setDevGateway(host)
      if (isLoggedIn) {
        // Close settings before logout: logout() doesn't reset uiStore, so a
        // lingering settingsModal would reopen after re-login.
        closeSettings()
        void logout()
      } else {
        // Login screen: already logged out, just repoint and refresh the badge.
        setState(next)
      }
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('settings.devGateway.switchFailed'),
        message: String(e),
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    }
  }

  const switchToPreset = async (host: string, label: string) => {
    if (host === state.currentHost) return
    const confirmed = await requestConfirm({
      title: t('settings.devGateway.confirmTitle'),
      description: t('settings.devGateway.confirmRelogin', { env: label, host }),
      confirmLabel: t('settings.devGateway.confirmButton'),
      variant: 'destructive',
    })
    if (!confirmed) return
    void applySwitch(host)
  }

  return (
    <span className="mr-2" onMouseDown={(e) => e.stopPropagation()}>
      <AppDropdown
        ariaLabel={t('settings.devGateway.title')}
        align="end"
        trigger={
          <button
            type="button"
            className="inline-flex items-center gap-0.5 rounded-sm bg-[var(--color-semantic-green)] px-1.5 py-0.5 text-[11px] font-semibold tracking-wide text-primary-foreground shadow-[var(--shadow-sm)] transition-[filter] hover:brightness-95"
          >
            {currentLabel}
            <ChevronDown className="h-3 w-3" />
          </button>
        }
        items={[
          ...state.presets.map((p) => {
            const selected = state.currentHost === p.host
            return {
              id: p.host,
              className: selected ? 'bg-accent' : undefined,
              label: (
                <span className="flex w-full items-center justify-between gap-2">
                  <span className="flex flex-col">
                    <span>{p.label}</span>
                    <span className="font-mono text-xs text-muted-foreground">{p.host}</span>
                  </span>
                  {selected ? <Check className="h-3.5 w-3.5 text-primary" /> : null}
                </span>
              ),
              onSelect: () => void switchToPreset(p.host, p.label),
            }
          }),
          {
            id: '__custom__',
            className: isCustomHost ? 'bg-accent' : undefined,
            label: (
              <span className="flex w-full items-center justify-between gap-2">
                <span className="flex flex-col">
                  <span>{t('settings.devGateway.custom')}</span>
                  {isCustomHost ? (
                    <span className="font-mono text-xs text-muted-foreground">
                      {state.currentHost}
                    </span>
                  ) : null}
                </span>
                {isCustomHost ? <Check className="h-3.5 w-3.5 text-primary" /> : null}
              </span>
            ),
            onSelect: () => setCustomOpen(true),
          },
        ]}
      />

      <CustomGatewayDialog
        open={customOpen}
        onOpenChange={setCustomOpen}
        currentHost={state.currentHost}
        initialHost={isCustomHost ? state.currentHost : ''}
        onConfirm={(host) => void applySwitch(host)}
      />
    </span>
  )
}

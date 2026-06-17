/**
 * @designSource design.pen#YboA7/Z9asD/r95Aa
 * @sizing 220 width, bg secondary, top-left radius 18; row r-10 padding [10,12]
 */
import { useTranslation } from 'react-i18next'
import type { SettingsModalKey } from '@/stores/uiStore'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'

export interface SettingsMenuItem {
  key: SettingsModalKey
  /**
   * i18n key under `settings.tabs.*`. Resolved at render time so the
   * label follows the active language. `SETTINGS_MENU_ITEMS` is exported
   * as `const` for other modules that need the menu order; consumers
   * outside React should use the labelKey directly.
   */
  labelKey: string
  disabled?: boolean
}

// eslint-disable-next-line react-refresh/only-export-components
export const SETTINGS_MENU_ITEMS: SettingsMenuItem[] = [
  { key: 'account', labelKey: 'settings.tabs.general' },
  { key: 'account-billing', labelKey: 'settings.billing.title' },
  { key: 'usage', labelKey: 'settings.tabs.usage', disabled: true },
  { key: 'permissions', labelKey: 'settings.tabs.permissions' },
  { key: 'mcp', labelKey: 'settings.tabs.mcp', disabled: true },
  { key: 'sso', labelKey: 'settings.tabs.sso', disabled: true },
  { key: 'shortcuts', labelKey: 'settings.tabs.shortcuts', disabled: true },
  { key: 'archived', labelKey: 'settings.tabs.archived' },
  { key: 'runtime', labelKey: 'settings.tabs.runtime' },
  { key: 'about', labelKey: 'settings.tabs.about' },
]

interface SettingsMenuProps {
  activeKey: SettingsModalKey
  onSelect: (key: SettingsModalKey) => void
}

export function SettingsMenu({ activeKey, onSelect }: SettingsMenuProps) {
  const { t } = useTranslation()
  return (
    <aside className="flex min-h-0 flex-col rounded-l-md bg-secondary px-4 py-6">
      <div className="mb-2 shrink-0 text-lg font-bold text-foreground">{t('settings.tabs.title')}</div>
      <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto pr-1">
        {SETTINGS_MENU_ITEMS
          .filter((it) => !it.disabled)
          .map((it) => {
          const active = it.key === activeKey
          const label = t(it.labelKey)
          return (
            <Button unstyled
              key={it.key}
              type="button"
              data-aijia-settings-panel={it.key}
              aria-label={label}
              onClick={() => {
                onSelect(it.key)
              }}
              className={cn(
                'flex items-center rounded-md px-3 py-2.5 text-left text-sm',
                active
                  ? 'bg-card font-semibold text-foreground'
                  : 'font-medium text-muted-foreground transition-colors hover:bg-card/60',
              )}
            >
              <span className="min-w-0 flex-1 truncate">{label}</span>
            </Button>
          )
        })}
      </div>
    </aside>
  )
}

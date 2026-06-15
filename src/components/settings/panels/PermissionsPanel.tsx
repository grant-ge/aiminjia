import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { getSettings, updateSettings } from '@/lib/tauri'
import { cn } from '@/lib/utils'
import { useSettingsStore } from '@/stores/settingsStore'
import type { DefaultPermissionMode } from '@/types/settings'

export function PermissionsPanel() {
  const { t } = useTranslation()
  const defaultPermissionMode = useSettingsStore((s) => s.defaultPermissionMode ?? 'default')
  const setDefaultPermissionMode = useSettingsStore((s) => s.setDefaultPermissionMode)

  const persistToBackend = async (mode: DefaultPermissionMode) => {
    try {
      const current = await getSettings()
      await updateSettings({ ...current, defaultPermissionMode: mode })
    } catch (err) {
      console.error('Failed to persist default permission mode:', err)
    }
  }

  const handleChange = (mode: DefaultPermissionMode) => {
    setDefaultPermissionMode(mode)
    void persistToBackend(mode)
  }

  const options: Array<{
    value: DefaultPermissionMode
    titleKey: string
    descriptionKey: string
  }> = [
    {
      value: 'default',
      titleKey: 'settings.permissions.defaultAccess',
      descriptionKey: 'settings.permissions.defaultAccessDesc',
    },
    {
      value: 'fullAccess',
      titleKey: 'settings.permissions.fullAccess',
      descriptionKey: 'settings.permissions.fullAccessDesc',
    },
  ]

  return (
    <div className="flex flex-col gap-5 text-foreground">
      <section className="flex flex-col gap-2">
        <div className="text-xl font-bold text-foreground">{t('settings.permissions.title')}</div>
        <div className="max-w-2xl text-sm leading-6 text-muted-foreground">
          {t('settings.permissions.description')}
        </div>
      </section>

      <section className="grid gap-3">
        {options.map((option) => {
          const selected = defaultPermissionMode === option.value
          return (
            <Button
              unstyled
              key={option.value}
              type="button"
              role="radio"
              aria-checked={selected}
              onClick={() => handleChange(option.value)}
              className={cn(
                'flex w-full items-start justify-between gap-4 rounded-md border border-border bg-card px-4 py-3 text-left transition-[border-color,box-shadow,background-color]',
                selected
                  ? 'border-primary/60 bg-[rgba(var(--primary-rgb),0.06)] shadow-[inset_0_0_0_1px_rgba(var(--primary-rgb),0.12)]'
                  : 'hover:border-border/90 hover:bg-muted/40',
              )}
            >
              <span className="flex min-w-0 flex-col gap-1">
                <span className="text-sm font-semibold text-foreground">{t(option.titleKey)}</span>
                <span className="text-xs leading-5 text-muted-foreground">{t(option.descriptionKey)}</span>
              </span>
              <span
                className={cn(
                  'mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full border',
                  selected ? 'border-primary bg-primary' : 'border-border bg-background',
                )}
                aria-hidden="true"
              >
                {selected ? <span className="h-1.5 w-1.5 rounded-full bg-primary-foreground" /> : null}
              </span>
            </Button>
          )
        })}
      </section>
    </div>
  )
}

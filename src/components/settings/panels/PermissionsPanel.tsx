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
    <div className="rounded-md border border-border bg-card text-foreground">
      <section className="border-b border-border bg-[rgba(var(--muted-rgb),0.25)] px-4 py-3">
        <div className="flex gap-3">
          <span className="mt-1 h-8 w-1 shrink-0 rounded-full bg-[rgba(var(--primary-rgb),0.70)]" aria-hidden="true" />
          <div className="min-w-0">
            <h3 className="text-sm font-bold leading-5 text-foreground">
              {t('settings.permissions.title')}
            </h3>
            <div className="mt-0.5 max-w-2xl text-sm leading-5 text-muted-foreground">
              {t('settings.permissions.description')}
            </div>
          </div>
        </div>
      </section>

      <section className="divide-y divide-border">
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
                'flex w-full items-start justify-between gap-4 px-4 py-3 text-left transition-colors',
                selected ? 'bg-[rgba(var(--primary-rgb),0.06)]' : 'hover:bg-[rgba(var(--muted-rgb),0.40)]',
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

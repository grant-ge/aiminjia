import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { getSettings, updateSettings } from '@/lib/tauri'
import { useSettingsStore } from '@/stores/settingsStore'
import type { FontScale } from '@/types/settings'
import type { AppLanguage } from '@/i18n'

interface GeneralPanelProps {
  user: { name: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
}

export function GeneralPanel({ user, onLogout }: GeneralPanelProps) {
  const { t } = useTranslation()
  const fontScale = useSettingsStore((s) => s.fontScale ?? 'medium')
  const setFontScale = useSettingsStore((s) => s.setFontScale)
  const appLanguage = useSettingsStore((s) => s.appLanguage ?? 'zh-CN')
  const setAppLanguage = useSettingsStore((s) => s.setAppLanguage)

  const persistToBackend = async (patch: { fontScale?: FontScale; appLanguage?: AppLanguage }) => {
    try {
      const current = await getSettings()
      await updateSettings({ ...current, ...patch })
    } catch (err) {
      console.error('Failed to persist appearance settings:', err)
    }
  }

  const handleFontScaleChange = (value: FontScale) => {
    setFontScale(value)
    void persistToBackend({ fontScale: value })
  }

  const handleLanguageChange = (value: AppLanguage) => {
    setAppLanguage(value)
    void persistToBackend({ appLanguage: value })
  }

  const FONT_SCALE_OPTIONS: Array<{ value: FontScale; description: string; labelKey: string }> = [
    { value: 'small', description: '14px', labelKey: 'settings.general.fontSmall' },
    { value: 'medium', description: '16px', labelKey: 'settings.general.fontMedium' },
    { value: 'large', description: '18px', labelKey: 'settings.general.fontLarge' },
  ]

  const LANGUAGE_OPTIONS: Array<{ value: AppLanguage; label: string }> = [
    { value: 'zh-CN', label: t('settings.general.languageZh') },
    { value: 'en-US', label: t('settings.general.languageEn') },
  ]

  return (
    <div className="flex flex-col gap-5 text-foreground">
      <section className="flex items-center justify-between gap-8">
        <div className="flex min-w-0 items-center gap-4">
          <div className="h-12 w-12 shrink-0 overflow-hidden rounded-lg bg-primary">
            {user.avatarUrl ? (
              <img src={user.avatarUrl} alt="" className="h-full w-full object-cover" />
            ) : (
              <span className="flex h-full w-full items-center justify-center text-2xl font-semibold text-primary-foreground">
                {(user.name.charAt(0) || '?').toUpperCase()}
              </span>
            )}
          </div>
          <div className="flex min-w-0 flex-col gap-2">
            <div className="text-base font-bold leading-none text-foreground">{user.name}</div>
            <div className="truncate text-sm leading-none text-muted-foreground">{user.tenantName}</div>
          </div>
        </div>
        <Button variant="outline" data-aijia-logout-button className="h-9 rounded-lg px-5 text-sm font-semibold" onClick={onLogout}>
          {t('settings.general.logout')}
        </Button>
      </section>

      <div className="h-px bg-border mb-2" />

      <section className="flex flex-col gap-4 pb-2">
        <div className="text-xl font-bold tracking-tight text-foreground">{t('settings.general.appearance')}</div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">{t('settings.general.fontSize')}</div>
            <div className="text-sm text-muted-foreground">{t('settings.general.fontSizeDesc')}</div>
          </div>
          <div
            className="inline-flex rounded-lg bg-muted p-1"
            role="radiogroup"
            aria-label={t('settings.general.fontSize')}
          >
            {FONT_SCALE_OPTIONS.map((option) => {
              const selected = fontScale === option.value
              const label = t(option.labelKey)
              return (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={label}
                  title={option.description}
                  onClick={() => handleFontScaleChange(option.value)}
                  className={
                    selected
                      ? 'rounded-md bg-card px-3 py-1.5 text-sm font-semibold text-foreground shadow-sm'
                      : 'rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground'
                  }
                >
                  {label}
                </button>
              )
            })}
          </div>
        </div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">
              {t('settings.general.language')}
            </div>
            <div className="text-sm text-muted-foreground">
              {t('settings.general.languageDesc')}
            </div>
          </div>
          <div
            className="inline-flex rounded-lg bg-muted p-1"
            role="radiogroup"
            aria-label={t('settings.general.language')}
            data-testid="settings-language-switch"
          >
            {LANGUAGE_OPTIONS.map((option) => {
              const selected = appLanguage === option.value
              return (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={option.label}
                  onClick={() => handleLanguageChange(option.value)}
                  className={
                    selected
                      ? 'rounded-md bg-card px-3 py-1.5 text-sm font-semibold text-foreground shadow-sm'
                      : 'rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground'
                  }
                >
                  {option.label}
                </button>
              )
            })}
          </div>
        </div>
      </section>
    </div>
  )
}

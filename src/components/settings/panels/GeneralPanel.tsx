import { useTranslation } from 'react-i18next'
import { getSettings, updateSettings } from '@/lib/tauri'
import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import type { ChatWidthMode, FontScale } from '@/types/settings'
import type { AppLanguage } from '@/i18n'
import { Button } from '@/components/ui/button'

interface GeneralPanelProps {
  user: { name: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
}

export function GeneralPanel({ user, onLogout }: GeneralPanelProps) {
  const { t, i18n } = useTranslation()
  const fontScale = useSettingsStore((s) => s.fontScale ?? 'medium')
  const setFontScale = useSettingsStore((s) => s.setFontScale)
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? 'full')
  const setChatWidthMode = useSettingsStore((s) => s.setChatWidthMode)
  const productName = useBrandingStore((s) => s.productName)
  const appLanguage: AppLanguage = i18n.language === 'en-US' ? 'en-US' : 'zh-CN'
  const setAppLanguage = useSettingsStore((s) => s.setAppLanguage)
  const accountSubtitle = productName.trim() || user.tenantName

  const persistToBackend = async (patch: {
    fontScale?: FontScale
    appLanguage?: AppLanguage
    chatWidthMode?: ChatWidthMode
  }) => {
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

  const handleChatWidthModeChange = (value: ChatWidthMode) => {
    setChatWidthMode(value)
    void persistToBackend({ chatWidthMode: value })
  }

  const FONT_SCALE_OPTIONS: Array<{ value: FontScale; description: string; labelKey: string }> = [
    { value: 'small', description: '12px', labelKey: 'settings.general.fontSmall' },
    { value: 'medium', description: '13px', labelKey: 'settings.general.fontMedium' },
    { value: 'large', description: '14px', labelKey: 'settings.general.fontLarge' },
  ]

  const LANGUAGE_OPTIONS: Array<{ value: AppLanguage; label: string }> = [
    { value: 'zh-CN', label: t('settings.general.languageZh') },
    { value: 'en-US', label: t('settings.general.languageEn') },
  ]

  const CHAT_WIDTH_OPTIONS: Array<{ value: ChatWidthMode; labelKey: string }> = [
    { value: 'centered', labelKey: 'settings.general.chatWidthCentered' },
    { value: 'full', labelKey: 'settings.general.chatWidthFull' },
  ]

  return (
    <div className="flex flex-col gap-5 text-foreground">
      <section className="flex items-center justify-between gap-8">
        <div className="flex min-w-0 items-center gap-4">
          <div className="h-12 w-12 shrink-0 overflow-hidden rounded-md bg-primary">
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
            <div className="truncate text-sm leading-none text-muted-foreground">{accountSubtitle}</div>
          </div>
        </div>
        <Button variant="outline" data-aijia-logout-button onClick={onLogout}>
          {t('settings.general.logout')}
        </Button>
      </section>

      <div className="h-px bg-border mb-2" />

      <section className="flex flex-col gap-4 pb-2">
        <div className="text-xl font-bold text-foreground">{t('settings.general.appearance')}</div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">{t('settings.general.fontSize')}</div>
            <div className="text-sm text-muted-foreground">{t('settings.general.fontSizeDesc')}</div>
          </div>
          <div
            className="inline-flex rounded-md bg-muted p-1"
            role="radiogroup"
            aria-label={t('settings.general.fontSize')}
          >
            {FONT_SCALE_OPTIONS.map((option) => {
              const selected = fontScale === option.value
              const label = t(option.labelKey)
              return (
                <Button unstyled
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
                </Button>
              )
            })}
          </div>
        </div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">{t('settings.general.chatWidth')}</div>
            <div className="text-sm text-muted-foreground">{t('settings.general.chatWidthDesc')}</div>
          </div>
          <div
            className="inline-flex rounded-md bg-muted p-1"
            role="radiogroup"
            aria-label={t('settings.general.chatWidth')}
          >
            {CHAT_WIDTH_OPTIONS.map((option) => {
              const selected = chatWidthMode === option.value
              const label = t(option.labelKey)
              return (
                <Button unstyled
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={label}
                  onClick={() => handleChatWidthModeChange(option.value)}
                  className={
                    selected
                      ? 'rounded-md bg-card px-3 py-1.5 text-sm font-semibold text-foreground shadow-sm'
                      : 'rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground'
                  }
                >
                  {label}
                </Button>
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
            className="inline-flex rounded-md bg-muted p-1"
            role="radiogroup"
            aria-label={t('settings.general.language')}
            data-testid="settings-language-switch"
          >
            {LANGUAGE_OPTIONS.map((option) => {
              const selected = appLanguage === option.value
              return (
                <Button unstyled
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
                </Button>
              )
            })}
          </div>
        </div>
      </section>
    </div>
  )
}
